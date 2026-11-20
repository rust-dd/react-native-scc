#pragma once

#include "scc_kv_ffi.h"

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace margelo::nitro::scckv::common {

inline const uint8_t* bytes(const std::string& text) {
  return reinterpret_cast<const uint8_t*>(text.data());
}

inline std::string lastError(const char* op) {
  char* err = scc_kv_last_error();
  std::string message = err != nullptr ? err : "unknown error";
  if (err != nullptr) scc_kv_free_cstring(err);
  return std::string(op) + " failed: " + message;
}

[[noreturn]] inline void throwLastError(const char* op) {
  throw std::runtime_error(lastError(op));
}

inline bool isPositiveSafeInteger(double value) {
  constexpr double maxSafeInteger = 9007199254740991.0;
  return std::isfinite(value) && value > 0 && value <= maxSafeInteger &&
         std::floor(value) == value;
}

inline uint32_t readU32Le(const uint8_t* data) {
  return static_cast<uint32_t>(data[0]) | (static_cast<uint32_t>(data[1]) << 8) |
         (static_cast<uint32_t>(data[2]) << 16) | (static_cast<uint32_t>(data[3]) << 24);
}

constexpr size_t maxRetainedCapacity = 256 * 1024;

// Retaining a rare multi-megabyte read on every thread would inflate memory, so only modest scratch buffers stay cached.
template <typename OnError, typename Make>
auto readStringLike(SccKvStore* handle, const std::string& key, uint8_t tag, OnError&& onError,
                    Make&& make) -> std::optional<decltype(make(bytes(key), size_t{0}))> {
  static thread_local std::vector<uint8_t> scratch(4096);
  std::vector<uint8_t> oversized;
  std::vector<uint8_t>* buffer = &scratch;
  while (true) {
    size_t needed = 0;
    int rc = scc_kv_get_raw(handle, bytes(key), key.size(), tag, buffer->data(), buffer->size(),
                            &needed);
    if (rc < 0) onError("get");
    if (rc == 0) return std::nullopt;
    if (needed <= buffer->size()) return make(buffer->data(), needed);
    if (needed <= maxRetainedCapacity) {
      scratch.resize(needed);
      buffer = &scratch;
    } else {
      oversized.resize(needed);
      buffer = &oversized;
    }
  }
}

// Throws on a malformed buffer instead of returning a silently truncated list.
inline std::vector<std::string> unpackKeys(const uint8_t* data, size_t len, const char* op) {
  std::vector<std::string> keys;
  size_t off = 0;
  while (off < len) {
    if (len - off < 4) throw std::runtime_error(std::string(op) + ": malformed key buffer");
    uint32_t n = readU32Le(data + off);
    off += 4;
    if (len - off < n) throw std::runtime_error(std::string(op) + ": malformed key buffer");
    keys.emplace_back(reinterpret_cast<const char*>(data + off), n);
    off += n;
  }
  return keys;
}

} // namespace margelo::nitro::scckv::common
