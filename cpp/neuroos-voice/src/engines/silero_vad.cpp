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

class SileroVad final : public Vad {
 public:
  explicit SileroVad(std::unique_ptr<Ort::Session> session)
      : session_(std::move(session)),
        memory_info_(Ort::MemoryInfo::CreateCpu(OrtArenaAllocator, OrtMemTypeDefault)) {
    Ort::AllocatorWithDefaultOptions allocator;

    const std::size_t input_count = session_->GetInputCount();
    input_names_storage_.reserve(input_count);
    input_names_.reserve(input_count);
    for (std::size_t i = 0; i < input_count; ++i) {
      auto name = session_->GetInputNameAllocated(i, allocator);
      input_names_storage_.emplace_back(name.get());
      input_names_.push_back(input_names_storage_.back().c_str());
    }

    const std::size_t output_count = session_->GetOutputCount();
    output_names_storage_.reserve(output_count);
    output_names_.reserve(output_count);
    for (std::size_t i = 0; i < output_count; ++i) {
      auto name = session_->GetOutputNameAllocated(i, allocator);
      output_names_storage_.emplace_back(name.get());
      output_names_.push_back(output_names_storage_.back().c_str());
    }

    for (std::size_t i = 0; i < output_names_storage_.size(); ++i) {
      if (output_names_storage_[i] == "output") {
        output_prob_idx_ = i;
      } else if (output_names_storage_[i] == "stateN") {
        output_state_idx_ = i;
      }
    }

    Reset();
  }

  float Process(std::span<const std::int16_t> frame) override {
    if (frame.size() != 512) {
      return 0.0F;
    }

    try {
      // Input is 64 context samples followed by current 512 samples
      std::copy_n(context_.data(), 64, input_data_.data());
      for (std::size_t i = 0; i < 512; ++i) {
        input_data_[64 + i] = static_cast<float>(frame[i]) / 32768.0F;
      }

      std::array<int64_t, 2> input_shape = {1, 576};
      std::array<int64_t, 3> state_shape = {2, 1, 128};

      Ort::Value input_tensor =
          Ort::Value::CreateTensor<float>(memory_info_, input_data_.data(), input_data_.size(),
                                          input_shape.data(), input_shape.size());

      Ort::Value state_tensor =
          Ort::Value::CreateTensor<float>(memory_info_, state_data_.data(), state_data_.size(),
                                          state_shape.data(), state_shape.size());

      Ort::Value sr_tensor =
          Ort::Value::CreateTensor<int64_t>(memory_info_, &sr_val_, 1, nullptr, 0);

      std::vector<Ort::Value> input_tensors;
      input_tensors.reserve(input_names_storage_.size());
      for (const auto& name : input_names_storage_) {
        if (name == "input") {
          input_tensors.push_back(std::move(input_tensor));
        } else if (name == "state") {
          input_tensors.push_back(std::move(state_tensor));
        } else if (name == "sr") {
          input_tensors.push_back(std::move(sr_tensor));
        }
      }

      auto outputs =
          session_->Run(Ort::RunOptions{nullptr}, input_names_.data(), input_tensors.data(),
                        input_tensors.size(), output_names_.data(), output_names_.size());

      const float prob = outputs[output_prob_idx_].GetTensorData<float>()[0];
      const float* new_state = outputs[output_state_idx_].GetTensorData<float>();
      std::copy_n(new_state, 256, state_data_.data());

      for (std::size_t i = 0; i < 64; ++i) {
        context_[i] = static_cast<float>(frame[512 - 64 + i]) / 32768.0F;
      }

      return prob;
    } catch (...) {
      return 0.0F;
    }
  }

  void Reset() override {
    try {
      state_data_.fill(0.0F);
      context_.fill(0.0F);
    } catch (...) {
    }
  }

 private:
  std::unique_ptr<Ort::Session> session_;
  Ort::MemoryInfo memory_info_;
  std::vector<std::string> input_names_storage_;
  std::vector<const char*> input_names_;
  std::vector<std::string> output_names_storage_;
  std::vector<const char*> output_names_;

  std::size_t output_prob_idx_ = 0;
  std::size_t output_state_idx_ = 1;

  std::array<float, 576> input_data_{};
  std::array<float, 256> state_data_{};
  std::array<float, 64> context_{};
  int64_t sr_val_ = 16000;
};

}  // namespace engines

Result<std::unique_ptr<Vad>> MakeSileroVad(const std::string& model_path) {
  try {
    auto session = std::make_unique<Ort::Session>(engines::OrtEnv(), model_path.c_str(),
                                                  engines::SessionOptions(1));
    std::unique_ptr<Vad> vad = std::make_unique<engines::SileroVad>(std::move(session));
    return vad;
  } catch (const Ort::Exception& ex) {
    return Error{.message = ex.what(), .retryable = false, .code = 7};
  } catch (const std::exception& ex) {
    return Error{.message = ex.what(), .retryable = false, .code = 7};
  } catch (...) {
    return Error{.message = "Failed to load Silero VAD model", .retryable = false, .code = 7};
  }
}

}  // namespace neuroos::voice
