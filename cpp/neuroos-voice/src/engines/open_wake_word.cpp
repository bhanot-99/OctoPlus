#include <onnxruntime_cxx_api.h>

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <span>
#include <string>
#include <utility>
#include <vector>

#include "neuroos/result.hpp"
#include "neuroos/voice/engines.hpp"
#include "neuroos/voice/real_engines.hpp"
#include "ort_session.hpp"

namespace neuroos::voice {
namespace engines {

class OpenWakeWord final : public WakeWord {
 public:
  OpenWakeWord(std::unique_ptr<Ort::Session> mel_session, std::unique_ptr<Ort::Session> emb_session,
               std::unique_ptr<Ort::Session> cls_session, float threshold)
      : mel_session_(std::move(mel_session)),
        emb_session_(std::move(emb_session)),
        cls_session_(std::move(cls_session)),
        threshold_(threshold),
        memory_info_(Ort::MemoryInfo::CreateCpu(OrtArenaAllocator, OrtMemTypeDefault)) {
    Ort::AllocatorWithDefaultOptions const allocator;

    mel_in_name_ = mel_session_->GetInputNameAllocated(0, allocator).get();
    mel_out_name_ = mel_session_->GetOutputNameAllocated(0, allocator).get();

    emb_in_name_ = emb_session_->GetInputNameAllocated(0, allocator).get();
    emb_out_name_ = emb_session_->GetOutputNameAllocated(0, allocator).get();

    cls_in_name_ = cls_session_->GetInputNameAllocated(0, allocator).get();
    cls_out_name_ = cls_session_->GetOutputNameAllocated(0, allocator).get();

    accumulator_.reserve(2048);
    raw_ring_.reserve(kMaxRawRingSamples);
    mel_buffer_.reserve(kMaxMelRows * 32);
    feature_buffer_.reserve(kMaxFeatureRows * 96);

    Reset();
  }

  bool Process(std::span<const std::int16_t> frame) override {
    if (frame.empty()) {
      return false;
    }

    try {
      accumulator_.insert(accumulator_.end(), frame.begin(), frame.end());

      bool detected = false;
      while (accumulator_.size() >= kChunkSize) {
        // 1. Append this chunk's samples to raw_ring as float32 int16 values
        for (std::size_t i = 0; i < kChunkSize; ++i) {
          raw_ring_.push_back(static_cast<float>(accumulator_[i]));
        }
        if (raw_ring_.size() > kMaxRawRingSamples) {
          const std::size_t excess = raw_ring_.size() - kMaxRawRingSamples;
          raw_ring_.erase(raw_ring_.begin(),
                          raw_ring_.begin() + static_cast<std::ptrdiff_t>(excess));
        }

        // 2. Run melspectrogram on the last min(raw_ring.size(), 1760) samples
        const std::size_t m_samples = std::min(raw_ring_.size(), kMelInputWindow);
        const float* m_data = raw_ring_.data() + (raw_ring_.size() - m_samples);

        std::array<int64_t, 2> mel_shape = {1, static_cast<int64_t>(m_samples)};
        Ort::Value const mel_in_tensor =
            Ort::Value::CreateTensor<float>(memory_info_, const_cast<float*>(m_data), m_samples,
                                            mel_shape.data(), mel_shape.size());

        const char* mel_in_names[] = {mel_in_name_.c_str()};
        const char* mel_out_names[] = {mel_out_name_.c_str()};
        auto mel_outputs = mel_session_->Run(Ort::RunOptions{nullptr}, mel_in_names, &mel_in_tensor,
                                             1, mel_out_names, 1);

        auto mel_out_info = mel_outputs[0].GetTensorTypeAndShapeInfo();
        auto mel_out_shape = mel_out_info.GetShape();
        // Shape is [1, 1, F, 32]
        const auto frames = static_cast<std::size_t>(mel_out_shape[2]);
        const std::size_t mel_count = frames * 32;
        const auto* mel_out_data = mel_outputs[0].GetTensorData<float>();

        for (std::size_t i = 0; i < mel_count; ++i) {
          mel_buffer_.push_back(mel_out_data[i] / 10.0F + 2.0F);
        }
        if (mel_buffer_.size() > kMaxMelRows * 32) {
          const std::size_t excess = mel_buffer_.size() - (kMaxMelRows * 32);
          mel_buffer_.erase(mel_buffer_.begin(),
                            mel_buffer_.begin() + static_cast<std::ptrdiff_t>(excess));
        }

        // 3. Run embedding model on last 76 rows of mel buffer [1, 76, 32, 1]
        constexpr std::size_t kEmbInputFloats = 76 * 32;
        const float* emb_in_data = mel_buffer_.data() + (mel_buffer_.size() - kEmbInputFloats);
        std::array<int64_t, 4> emb_shape = {1, 76, 32, 1};
        Ort::Value const emb_in_tensor =
            Ort::Value::CreateTensor<float>(memory_info_, const_cast<float*>(emb_in_data),
                                            kEmbInputFloats, emb_shape.data(), emb_shape.size());

        const char* emb_in_names[] = {emb_in_name_.c_str()};
        const char* emb_out_names[] = {emb_out_name_.c_str()};
        auto emb_outputs = emb_session_->Run(Ort::RunOptions{nullptr}, emb_in_names, &emb_in_tensor,
                                             1, emb_out_names, 1);

        const auto* emb_out_data = emb_outputs[0].GetTensorData<float>();
        for (std::size_t i = 0; i < 96; ++i) {
          feature_buffer_.push_back(emb_out_data[i]);
        }
        if (feature_buffer_.size() > kMaxFeatureRows * 96) {
          const std::size_t excess = feature_buffer_.size() - (kMaxFeatureRows * 96);
          feature_buffer_.erase(feature_buffer_.begin(),
                                feature_buffer_.begin() + static_cast<std::ptrdiff_t>(excess));
        }

        // 4. Run classifier on last 16 rows of feature buffer [1, 16, 96]
        constexpr std::size_t kClsInputFloats = 16 * 96;
        const float* cls_in_data =
            feature_buffer_.data() + (feature_buffer_.size() - kClsInputFloats);
        std::array<int64_t, 3> cls_shape = {1, 16, 96};
        Ort::Value const cls_in_tensor =
            Ort::Value::CreateTensor<float>(memory_info_, const_cast<float*>(cls_in_data),
                                            kClsInputFloats, cls_shape.data(), cls_shape.size());

        const char* cls_in_names[] = {cls_in_name_.c_str()};
        const char* cls_out_names[] = {cls_out_name_.c_str()};
        auto cls_outputs = cls_session_->Run(Ort::RunOptions{nullptr}, cls_in_names, &cls_in_tensor,
                                             1, cls_out_names, 1);

        const float raw_score = cls_outputs[0].GetTensorData<float>()[0];

        // 5. Warmup check
        if (chunks_processed_ < kWarmupChunks) {
          last_score_ = 0.0F;
        } else {
          last_score_ = raw_score;
          if (raw_score >= threshold_) {
            detected = true;
          }
        }
        ++chunks_processed_;

        // Remove the processed 1280 samples from accumulator
        accumulator_.erase(accumulator_.begin(),
                           accumulator_.begin() + static_cast<std::ptrdiff_t>(kChunkSize));
      }

      return detected;
    } catch (...) {
      return false;
    }
  }

  void Reset() override {
    accumulator_.clear();
    raw_ring_.clear();
    mel_buffer_.assign(76 * 32, 1.0F);
    feature_buffer_.assign(16 * 96, 0.0F);
    chunks_processed_ = 0;
    last_score_ = 0.0F;
  }

  [[nodiscard]] float last_score() const noexcept { return last_score_; }

 private:
  static constexpr std::size_t kChunkSize = 1280;             // 80 ms at 16 kHz
  static constexpr std::size_t kMelInputWindow = 1280 + 480;  // 1760 samples
  static constexpr std::size_t kMaxRawRingSamples = 160000;   // 10 seconds at 16 kHz
  static constexpr std::size_t kMaxMelRows = 970;
  static constexpr std::size_t kMaxFeatureRows = 120;
  static constexpr std::uint64_t kWarmupChunks = 5;

  std::unique_ptr<Ort::Session> mel_session_;
  std::unique_ptr<Ort::Session> emb_session_;
  std::unique_ptr<Ort::Session> cls_session_;
  float threshold_;

  Ort::MemoryInfo memory_info_;

  std::string mel_in_name_;
  std::string mel_out_name_;
  std::string emb_in_name_;
  std::string emb_out_name_;
  std::string cls_in_name_;
  std::string cls_out_name_;

  std::vector<std::int16_t> accumulator_;
  std::vector<float> raw_ring_;
  std::vector<float> mel_buffer_;
  std::vector<float> feature_buffer_;

  std::uint64_t chunks_processed_ = 0;
  float last_score_ = 0.0F;
};

}  // namespace engines

Result<std::unique_ptr<WakeWord>> MakeOpenWakeWord(const WakeWordPaths& paths) {
  try {
    auto mel_sess = std::make_unique<Ort::Session>(engines::OrtEnv(), paths.melspectrogram.c_str(),
                                                   engines::SessionOptions(1));
    auto emb_sess = std::make_unique<Ort::Session>(engines::OrtEnv(), paths.embedding.c_str(),
                                                   engines::SessionOptions(1));
    auto cls_sess = std::make_unique<Ort::Session>(engines::OrtEnv(), paths.classifier.c_str(),
                                                   engines::SessionOptions(1));
    std::unique_ptr<WakeWord> ww = std::make_unique<engines::OpenWakeWord>(
        std::move(mel_sess), std::move(emb_sess), std::move(cls_sess), paths.threshold);
    return ww;
  } catch (const Ort::Exception& ex) {
    return Error{.message = ex.what(), .retryable = false, .code = 7};
  } catch (const std::exception& ex) {
    return Error{.message = ex.what(), .retryable = false, .code = 7};
  } catch (...) {
    return Error{.message = "Failed to load OpenWakeWord models", .retryable = false, .code = 7};
  }
}

}  // namespace neuroos::voice
