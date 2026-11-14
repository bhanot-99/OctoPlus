// Expected-style results (rules.md §5: no exceptions across module boundaries).
#pragma once

#include <string>
#include <utility>
#include <variant>

namespace neuroos {

// Why an operation failed. `message` never contains user content (rules.md §6.3).
// `code` is a neuroos.v1.ErrorCode value (0 = unspecified).
// Every field has a default; the analyzer cannot see through std::variant copies
// and reports them as uninitialised, hence the suppression.
struct Error {  // NOLINT(clang-analyzer-core.uninitialized.Assign)
  std::string message;
  bool retryable = false;
  int code = 0;
};

// Either a value or an Error. Minimal on purpose; C++20 has no std::expected.
template <class T>
class Result {
 public:
  Result(T value) : v_(std::move(value)) {}      // NOLINT(google-explicit-constructor)
  Result(Error error) : v_(std::move(error)) {}  // NOLINT(google-explicit-constructor)

  [[nodiscard]] bool ok() const noexcept { return std::holds_alternative<T>(v_); }
  explicit operator bool() const noexcept { return ok(); }

  // Precondition: ok().
  [[nodiscard]] const T& value() const& { return std::get<T>(v_); }
  [[nodiscard]] T& value() & { return std::get<T>(v_); }
  [[nodiscard]] T&& value() && { return std::get<T>(std::move(v_)); }
  // Precondition: !ok().
  [[nodiscard]] const Error& error() const& { return std::get<Error>(v_); }

 private:
  std::variant<T, Error> v_;
};

// Result for operations that return nothing on success.
struct Unit {};
using Status = Result<Unit>;
inline Status ok_status() { return Unit{}; }

}  // namespace neuroos
