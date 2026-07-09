#pragma once

#include "HybridSccKvSpec.hpp"
#include "HybridSccKvInstance.hpp"
#include "scc_kv_ffi.h"

#include <cmath>
#include <memory>
#include <optional>
#include <stdexcept>
#include <string>

namespace margelo::nitro::scckv {

class HybridSccKv : public HybridSccKvSpec {
public:
  HybridSccKv() : HybridObject(TAG) {}

  std::shared_ptr<HybridSccKvInstanceSpec> open(const std::string& dir,
                                                const std::string& id,
                                                bool strictDurability,
                                                bool recreate,
                                                const std::optional<std::string>& encryptionKey,
                                                std::optional<double> maxEntries,
                                                std::optional<double> ttlSweepIntervalMs) override {
    const uint8_t* keyPtr = nullptr;
    size_t keyLen = 0;
    if (encryptionKey.has_value() && !encryptionKey->empty()) {
      keyPtr = reinterpret_cast<const uint8_t*>(encryptionKey->data());
      keyLen = encryptionKey->size();
    }
    size_t maxEntriesValue = optionalPositiveInteger(maxEntries, "maxEntries");
    uint64_t sweepValue = static_cast<uint64_t>(
        optionalPositiveInteger(ttlSweepIntervalMs, "ttlSweepIntervalMs"));
    SccKvStore* handle =
        scc_kv_open(dir.c_str(), id.c_str(), strictDurability, recreate, keyPtr, keyLen,
                    maxEntriesValue, sweepValue);
    if (handle == nullptr) throwLastError("open");
    return std::make_shared<HybridSccKvInstance>(handle);
  }

  std::shared_ptr<HybridSccKvInstanceSpec> inMemory(const std::string& id,
                                                    std::optional<double> maxEntries,
                                                    std::optional<double> ttlSweepIntervalMs) override {
    size_t maxEntriesValue = optionalPositiveInteger(maxEntries, "maxEntries");
    uint64_t sweepValue = static_cast<uint64_t>(
        optionalPositiveInteger(ttlSweepIntervalMs, "ttlSweepIntervalMs"));
    SccKvStore* handle = scc_kv_in_memory(id.c_str(), maxEntriesValue, sweepValue);
    if (handle == nullptr) throwLastError("inMemory");
    return std::make_shared<HybridSccKvInstance>(handle);
  }

private:
  static size_t optionalPositiveInteger(const std::optional<double>& value, const char* name) {
    if (!value.has_value()) return 0;
    constexpr double maxSafeInteger = 9007199254740991.0;
    double v = *value;
    if (!std::isfinite(v) || v <= 0 || v > maxSafeInteger || std::floor(v) != v) {
      throw std::runtime_error(std::string(name) + " must be a positive safe integer");
    }
    return static_cast<size_t>(v);
  }

  [[noreturn]] static void throwLastError(const char* op) {
    char* err = scc_kv_last_error();
    std::string msg = err != nullptr ? err : "unknown error";
    if (err != nullptr) scc_kv_free_cstring(err);
    throw std::runtime_error(std::string(op) + " failed: " + msg);
  }
};

} // namespace margelo::nitro::scckv
