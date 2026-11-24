// Contract round-trip tool (P0-S02, phases.md §3.3 CT).
//
//   octoplus_contract_tool generate   writes one fully populated Envelope per body
//                                    variant (and per nested oneof choice) to stdout
//   octoplus_contract_tool echo       reads Envelopes from stdin, parses them, drops
//                                    unknown fields, re-serialises them to stdout
//
// Frames are a 4-byte little-endian length followed by the Envelope bytes, the
// same framing as the sockets (Architecture §5.1). Population is driven by the
// descriptors, so a new message or field is covered without editing this file.

#include <google/protobuf/descriptor.h>
#include <google/protobuf/io/coded_stream.h>
#include <google/protobuf/io/zero_copy_stream_impl_lite.h>
#include <google/protobuf/message.h>

#include <algorithm>
#include <array>
#include <cstdint>
#include <cstdio>
#include <iostream>
#include <string>
#include <string_view>

#include "octoplus/v1/envelope.pb.h"

namespace {

using google::protobuf::Descriptor;
using google::protobuf::FieldDescriptor;
using google::protobuf::Message;
using google::protobuf::Reflection;

constexpr int kMaxDepth = 4;
constexpr std::uint32_t kMaxFrame = 16U * 1024U * 1024U;

// Picks which member of each oneof to set. `choice` rotates through options so that
// repeated calls with increasing choice cover every member of every nested oneof.
struct Filler {
  int choice = 0;

  [[nodiscard]] static std::uint64_t Seed(const FieldDescriptor* f, int i) {
    return static_cast<std::uint64_t>(f->number()) * 7U + static_cast<std::uint64_t>(i) + 1U;
  }

  static void SetScalar(Message* m, const FieldDescriptor* f, int i) {
    const Reflection* r = m->GetReflection();
    const std::uint64_t s = Seed(f, i);
    const bool rep = f->is_repeated();
    switch (f->cpp_type()) {
      case FieldDescriptor::CPPTYPE_INT32: {
        const auto v = static_cast<std::int32_t>(s);
        rep ? r->AddInt32(m, f, v) : r->SetInt32(m, f, v);
        break;
      }
      case FieldDescriptor::CPPTYPE_INT64: {
        const auto v = static_cast<std::int64_t>(s * 1'000'000'007ULL);
        rep ? r->AddInt64(m, f, v) : r->SetInt64(m, f, v);
        break;
      }
      case FieldDescriptor::CPPTYPE_UINT32: {
        const auto v = static_cast<std::uint32_t>(s * 65'537U);
        rep ? r->AddUInt32(m, f, v) : r->SetUInt32(m, f, v);
        break;
      }
      case FieldDescriptor::CPPTYPE_UINT64: {
        const std::uint64_t v = s * 1'000'000'000'039ULL;
        rep ? r->AddUInt64(m, f, v) : r->SetUInt64(m, f, v);
        break;
      }
      case FieldDescriptor::CPPTYPE_DOUBLE: {
        const double v = static_cast<double>(s) + 0.25;
        rep ? r->AddDouble(m, f, v) : r->SetDouble(m, f, v);
        break;
      }
      case FieldDescriptor::CPPTYPE_FLOAT: {
        const float v = static_cast<float>(s) + 0.5F;
        rep ? r->AddFloat(m, f, v) : r->SetFloat(m, f, v);
        break;
      }
      case FieldDescriptor::CPPTYPE_BOOL:
        rep ? r->AddBool(m, f, true) : r->SetBool(m, f, true);
        break;
      case FieldDescriptor::CPPTYPE_ENUM: {
        const auto* e = f->enum_type();
        const auto* v = e->value(e->value_count() - 1);  // last value, never 0
        rep ? r->AddEnum(m, f, v) : r->SetEnum(m, f, v);
        break;
      }
      case FieldDescriptor::CPPTYPE_STRING: {
        std::string v = std::string(f->name()) + "-" + std::to_string(s) + "-é✓";
        if (f->type() == FieldDescriptor::TYPE_BYTES) {
          v.push_back('\0');
          v.push_back('\xff');
        }
        rep ? r->AddString(m, f, v) : r->SetString(m, f, v);
        break;
      }
      case FieldDescriptor::CPPTYPE_MESSAGE:
        break;  // handled by the caller
    }
  }

  // Recursion is bounded by kMaxDepth.
  void Fill(Message* m, int depth) const {  // NOLINT(misc-no-recursion)
    if (depth > kMaxDepth) {
      return;
    }
    const Descriptor* d = m->GetDescriptor();
    const Reflection* r = m->GetReflection();
    for (int i = 0; i < d->field_count(); ++i) {
      const FieldDescriptor* f = d->field(i);
      if (const auto* o = f->containing_oneof(); o != nullptr) {
        if (o->field(choice % o->field_count()) != f) {
          continue;
        }
      }
      const int copies = f->is_repeated() ? 2 : 1;
      for (int k = 0; k < copies; ++k) {
        if (f->cpp_type() == FieldDescriptor::CPPTYPE_MESSAGE) {
          Message* child = f->is_repeated() ? r->AddMessage(m, f) : r->MutableMessage(m, f);
          Fill(child, depth + 1);
        } else {
          SetScalar(m, f, k);
        }
      }
    }
  }
};

// Largest oneof among a message and its nested messages, so we know how many
// rotations cover every nested choice.
// Recursion is bounded by kMaxDepth.
// NOLINTNEXTLINE(misc-no-recursion)
int MaxOneofWidth(const Descriptor* d, int depth) {
  if (depth > kMaxDepth) {
    return 1;
  }
  int width = 1;
  for (int i = 0; i < d->oneof_decl_count(); ++i) {
    width = std::max(width, d->oneof_decl(i)->field_count());
  }
  for (int i = 0; i < d->field_count(); ++i) {
    if (const auto* f = d->field(i); f->cpp_type() == FieldDescriptor::CPPTYPE_MESSAGE) {
      width = std::max(width, MaxOneofWidth(f->message_type(), depth + 1));
    }
  }
  return width;
}

bool WriteFrame(const std::string& bytes) {
  const auto n = static_cast<std::uint32_t>(bytes.size());
  const std::array<char, 4> len = {
      static_cast<char>(n & 0xFFU), static_cast<char>((n >> 8U) & 0xFFU),
      static_cast<char>((n >> 16U) & 0xFFU), static_cast<char>((n >> 24U) & 0xFFU)};
  std::cout.write(len.data(), len.size());
  std::cout.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
  return static_cast<bool>(std::cout);
}

bool Serialize(const Message& m, std::string* out) {
  out->clear();
  google::protobuf::io::StringOutputStream raw(out);
  google::protobuf::io::CodedOutputStream coded(&raw);
  coded.SetSerializationDeterministic(true);
  return m.SerializeToCodedStream(&coded) && !coded.HadError();
}

int Generate() {
  const Descriptor* env = octoplus::v1::Envelope::descriptor();
  const auto* body = env->FindOneofByName("body");
  if (body == nullptr) {
    std::cerr << "Envelope has no oneof body\n";
    return 2;
  }
  std::string bytes;
  for (int i = 0; i < body->field_count(); ++i) {
    const FieldDescriptor* f = body->field(i);
    const int rotations = MaxOneofWidth(f->message_type(), 0);
    for (int c = 0; c < rotations; ++c) {
      octoplus::v1::Envelope e;
      e.set_schema_version(1);
      e.set_trace_id("trace-" + std::to_string(f->number()) + "-" + std::to_string(c));
      e.set_request_id(static_cast<std::uint64_t>(f->number()) * 1000U +
                       static_cast<std::uint64_t>(c));
      e.set_sent_at_ns(1'794'659'696'123'456'789ULL);
      e.set_end_of_stream(c % 2 == 0);
      const Filler filler{c};
      filler.Fill(octoplus::v1::Envelope::GetReflection()->MutableMessage(&e, f), 1);
      if (!Serialize(e, &bytes) || !WriteFrame(bytes)) {
        return 3;
      }
    }
  }
  std::cerr << "body_variants=" << body->field_count() << "\n";
  return 0;
}

int Echo() {
  std::string bytes;
  std::string out;
  for (;;) {
    std::array<char, 4> raw{};
    std::cin.read(raw.data(), raw.size());
    if (std::cin.gcount() == 0 && std::cin.eof()) {
      return 0;
    }
    if (std::cin.gcount() != static_cast<std::streamsize>(raw.size())) {
      std::cerr << "truncated length prefix\n";
      return 4;
    }
    std::uint32_t n = 0;
    for (std::size_t k = 0; k < raw.size(); ++k) {
      n |= static_cast<std::uint32_t>(static_cast<unsigned char>(raw.at(k))) << (8U * k);
    }
    if (n > kMaxFrame) {
      std::cerr << "frame too large\n";
      return 5;
    }
    bytes.resize(n);
    std::cin.read(bytes.data(), static_cast<std::streamsize>(n));
    if (std::cin.gcount() != static_cast<std::streamsize>(n)) {
      std::cerr << "truncated frame\n";
      return 6;
    }
    octoplus::v1::Envelope e;
    if (!e.ParseFromString(bytes)) {
      std::cerr << "parse failed\n";
      return 7;
    }
    e.DiscardUnknownFields();  // a field C++ does not know must make the bytes differ
    if (!Serialize(e, &out) || !WriteFrame(out)) {
      return 8;
    }
  }
}

}  // namespace

int main(int argc, char** argv) {
  GOOGLE_PROTOBUF_VERIFY_VERSION;
  std::ios::sync_with_stdio(false);
  const std::string_view mode =
      argc == 2 ? std::string_view(argv[1])
                : std::string_view();  // NOLINT(cppcoreguidelines-pro-bounds-pointer-arithmetic)
  if (mode == "generate") {
    return Generate();
  }
  if (mode == "echo") {
    return Echo();
  }
  std::cerr << "usage: octoplus_contract_tool generate|echo\n";
  return 1;
}
