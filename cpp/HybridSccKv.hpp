#pragma once

#include "HybridSccKvSpec.hpp"
#include "HybridSccKvInstance.hpp"
#include "scc_kv_ffi.h"

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
                                                const std::optional<std::string>& encryptionKey) override {
    const uint8_t* keyPtr = nullptr;
    size_t keyLen = 0;
    if (encryptionKey.has_value() && !encryptionKey->empty()) {
      keyPtr = reinterpret_cast<const uint8_t*>(encryptionKey->data());
      keyLen = encryptionKey->size();
    }
    SccKvStore* handle =
        scc_kv_open(dir.c_str(), id.c_str(), strictDurability, recreate, keyPtr, keyLen);
    if (handle == nullptr) throwLastError("open");
    return std::make_shared<HybridSccKvInstance>(handle);
  }

  std::shared_ptr<HybridSccKvInstanceSpec> inMemory(const std::string& id) override {
    SccKvStore* handle = scc_kv_in_memory(id.c_str());
    if (handle == nullptr) throwLastError("inMemory");
    return std::make_shared<HybridSccKvInstance>(handle);
  }

private:
  [[noreturn]] static void throwLastError(const char* op) {
    char* err = scc_kv_last_error();
    std::string msg = err != nullptr ? err : "unknown error";
    if (err != nullptr) scc_kv_free_cstring(err);
    throw std::runtime_error(std::string(op) + " failed: " + msg);
  }
};

} // namespace margelo::nitro::scckv
