#pragma once

#include "HybridSccKvInstanceSpec.hpp"
#include "scc_kv_ffi.h"

#include <NitroModules/ArrayBuffer.hpp>
#include <NitroModules/Null.hpp>
#include <NitroModules/Promise.hpp>

#include <cmath>
#include <cstring>
#include <functional>
#include <limits>
#include <memory>
#include <optional>
#include <stdexcept>
#include <string>
#include <unordered_map>
#include <variant>
#include <vector>

namespace margelo::nitro::scckv {

class HybridSccKvInstance : public HybridSccKvInstanceSpec {
public:
  explicit HybridSccKvInstance(SccKvStore* handle)
      : HybridObject(TAG), _handle(handle) {}

  ~HybridSccKvInstance() override {
    if (_handle != nullptr) {
      for (auto& [id, box] : _listeners) {
        scc_kv_unsubscribe(_handle, id);
      }
      _listeners.clear();
      scc_kv_release(_handle);
      _handle = nullptr;
    }
  }

  void setString(const std::string& key, const std::string& value) override {
    if (scc_kv_set_str(_handle, kptr(key), key.size(),
                       reinterpret_cast<const uint8_t*>(value.data()),
                       value.size()) != 0) {
      throwLastError("set");
    }
  }

  void setNumber(const std::string& key, double value) override {
    if (scc_kv_set_f64(_handle, kptr(key), key.size(), value) != 0) throwLastError("set");
  }

  void setBoolean(const std::string& key, bool value) override {
    if (scc_kv_set_bool(_handle, kptr(key), key.size(), value) != 0) throwLastError("set");
  }

  void setBuffer(const std::string& key, const std::shared_ptr<ArrayBuffer>& value) override {
    setRaw(key, 3, value->data(), value->size());
  }

  void setJson(const std::string& key, const std::string& json) override {
    setRaw(key, 4, reinterpret_cast<const uint8_t*>(json.data()), json.size());
  }

  void applyBatch(const std::shared_ptr<ArrayBuffer>& packed) override {
    if (scc_kv_apply_batch(_handle, packed->data(), packed->size()) != 0) {
      throwLastError("applyBatch");
    }
  }

  std::optional<std::string> getString(const std::string& key) override {
    return getStringLike(key, 0);
  }

  std::optional<double> getNumber(const std::string& key) override {
    double value = 0;
    int rc = scc_kv_get_f64(_handle, kptr(key), key.size(), &value);
    if (rc < 0) throwLastError("get");
    if (rc == 0) return std::nullopt;
    return value;
  }

  std::optional<bool> getBoolean(const std::string& key) override {
    bool value = false;
    int rc = scc_kv_get_bool(_handle, kptr(key), key.size(), &value);
    if (rc < 0) throwLastError("get");
    if (rc == 0) return std::nullopt;
    return value;
  }

  std::optional<std::shared_ptr<ArrayBuffer>> getBuffer(const std::string& key) override {
    uint8_t tag = 0;
    uint8_t* data = nullptr;
    size_t len = 0;
    int rc = scc_kv_get(_handle, kptr(key), key.size(), &tag, &data, &len);
    if (rc < 0) throwLastError("get");
    if (rc == 0) return std::nullopt;
    if (tag != 3) {
      scc_kv_free(data, len);
      return std::nullopt;
    }
    if (data == nullptr || len == 0) {
      scc_kv_free(data, len);
      return ArrayBuffer::allocate(0);
    }
    // Hand the Rust allocation to JS without another copy; the deleter
    // returns it to the Rust allocator when the JS buffer is collected.
    return ArrayBuffer::wrap(data, len, [data, len]() { scc_kv_free(data, len); });
  }

  std::optional<std::string> getJson(const std::string& key) override {
    return getStringLike(key, 4);
  }

  bool contains(const std::string& key) override {
    int rc = scc_kv_contains(_handle, kptr(key), key.size());
    if (rc < 0) throwLastError("contains");
    return rc == 1;
  }

  bool remove(const std::string& key) override {
    int rc = scc_kv_remove(_handle, kptr(key), key.size());
    if (rc < 0) throwLastError("remove");
    return rc == 1;
  }

  std::vector<std::string> getAllKeys() override {
    size_t len = 0;
    uint8_t* data = scc_kv_keys(_handle, &len);
    if (data == nullptr) {
      if (len == 1) throwLastError("getAllKeys");
      return {};
    }
    std::vector<std::string> keys;
    size_t off = 0;
    while (off + 4 <= len) {
      uint32_t n;
      std::memcpy(&n, data + off, 4);
      off += 4;
      if (off + n > len) break;
      keys.emplace_back(reinterpret_cast<const char*>(data + off), n);
      off += n;
    }
    scc_kv_free(data, len);
    return keys;
  }

  void clearAll() override {
    if (scc_kv_clear(_handle) != 0) throwLastError("clearAll");
  }

  void flush() override {
    if (scc_kv_flush(_handle) != 0) throwLastError("flush");
  }

  double size() override {
    return static_cast<double>(scc_kv_len(_handle));
  }

  void close() override {
    if (scc_kv_close(_handle) != 0) throwLastError("close");
  }

  // Nitro JS callbacks are safely invokable from any thread — they dispatch
  // onto the JS runtime. The trampoline may fire on the JS thread (sync
  // mutations) or an async pool thread; both funnel through that dispatch.
  double addListener(
      const std::function<void(const std::optional<std::string>&)>& listener) override {
    auto box = std::make_unique<ListenerBox>();
    box->fn = listener;
    uint64_t id = scc_kv_subscribe(_handle, &listenerTrampoline, box.get());
    if (id == 0) throwLastError("addListener");
    _listeners[id] = std::move(box);
    return static_cast<double>(id);
  }

  bool removeListener(double id) override {
    auto native = static_cast<uint64_t>(id);
    int rc = scc_kv_unsubscribe(_handle, native);
    if (rc < 0) throwLastError("removeListener");
    _listeners.erase(native);
    return rc == 1;
  }

  void setStringTtl(const std::string& key, const std::string& value, double ttlMs) override {
    setRawTtl(key, 0, reinterpret_cast<const uint8_t*>(value.data()), value.size(), ttlMs);
  }

  void setNumberTtl(const std::string& key, double value, double ttlMs) override {
    uint8_t buf[8];
    std::memcpy(buf, &value, 8);
    setRawTtl(key, 1, buf, 8, ttlMs);
  }

  void setBooleanTtl(const std::string& key, bool value, double ttlMs) override {
    uint8_t b = value ? 1 : 0;
    setRawTtl(key, 2, &b, 1, ttlMs);
  }

  void setBufferTtl(const std::string& key, const std::shared_ptr<ArrayBuffer>& value,
                    double ttlMs) override {
    setRawTtl(key, 3, value->data(), value->size(), ttlMs);
  }

  void setJsonTtl(const std::string& key, const std::string& json, double ttlMs) override {
    setRawTtl(key, 4, reinterpret_cast<const uint8_t*>(json.data()), json.size(), ttlMs);
  }

  void setManyString(const std::vector<std::string>& keys,
                     const std::vector<std::string>& values) override {
    if (keys.size() != values.size()) {
      throw std::runtime_error("setManyString: keys and values length mismatch");
    }
    if (keys.empty()) return;
    size_t total = 0;
    auto checkedAdd = [&total](size_t part) {
      if (part > std::numeric_limits<size_t>::max() - total) {
        throw std::runtime_error("setManyString: packed size overflow");
      }
      total += part;
    };
    for (size_t i = 0; i < keys.size(); i++) {
      if (keys[i].size() > std::numeric_limits<uint32_t>::max() ||
          values[i].size() > std::numeric_limits<uint32_t>::max()) {
        throw std::runtime_error("setManyString: entry exceeds the wire-format limit");
      }
      checkedAdd(8);
      checkedAdd(keys[i].size());
      checkedAdd(values[i].size());
    }
    std::vector<uint8_t> packed(total);
    size_t offset = 0;
    auto appendU32 = [&packed, &offset](uint32_t value) {
      writeU32Le(packed.data() + offset, value);
      offset += 4;
    };
    for (size_t i = 0; i < keys.size(); i++) {
      appendU32(static_cast<uint32_t>(keys[i].size()));
      if (!keys[i].empty()) {
        std::memcpy(packed.data() + offset, keys[i].data(), keys[i].size());
        offset += keys[i].size();
      }
      appendU32(static_cast<uint32_t>(values[i].size()));
      if (!values[i].empty()) {
        std::memcpy(packed.data() + offset, values[i].data(), values[i].size());
        offset += values[i].size();
      }
    }
    if (scc_kv_set_many_str(_handle, packed.data(), packed.size(), keys.size()) != 0) {
      throwLastError("setMany");
    }
  }

  std::vector<std::variant<nitro::NullType, std::string>>
  getManyString(const std::vector<std::string>& keys) override {
    if (keys.empty()) return {};
    size_t packedSize = 0;
    for (const auto& key : keys) {
      if (key.size() > std::numeric_limits<uint32_t>::max()) {
        throw std::runtime_error("getManyString: key exceeds the wire-format limit");
      }
      if (packedSize > std::numeric_limits<size_t>::max() - 4 ||
          key.size() > std::numeric_limits<size_t>::max() - packedSize - 4) {
        throw std::runtime_error("getManyString: packed size overflow");
      }
      packedSize += 4 + key.size();
    }

    std::vector<uint8_t> packed(packedSize);
    size_t packedOffset = 0;
    for (const auto& key : keys) {
      writeU32Le(packed.data() + packedOffset, static_cast<uint32_t>(key.size()));
      packedOffset += 4;
      if (!key.empty()) {
        std::memcpy(packed.data() + packedOffset, key.data(), key.size());
        packedOffset += key.size();
      }
    }

    struct RustBuffer {
      uint8_t* data = nullptr;
      size_t len = 0;
      ~RustBuffer() { scc_kv_free(data, len); }
    } result;
    if (scc_kv_get_many_str(_handle, packed.data(), packed.size(), keys.size(),
                            &result.data, &result.len) != 0) {
      throwLastError("getMany");
    }
    if (result.data == nullptr) {
      throw std::runtime_error("getMany failed: malformed packed output");
    }

    std::vector<std::variant<nitro::NullType, std::string>> out;
    out.reserve(keys.size());
    size_t offset = 0;
    for (size_t i = 0; i < keys.size(); i++) {
      if (offset > result.len || result.len - offset < 4) {
        throw std::runtime_error("getMany failed: malformed packed output");
      }
      uint32_t valueLen = readU32Le(result.data + offset);
      offset += 4;
      if (valueLen == std::numeric_limits<uint32_t>::max()) {
        out.emplace_back(nitro::NullType{});
        continue;
      }
      if (static_cast<size_t>(valueLen) > result.len - offset) {
        throw std::runtime_error("getMany failed: malformed packed output");
      }
      out.emplace_back(
          std::string(reinterpret_cast<const char*>(result.data + offset), valueLen));
      offset += valueLen;
    }
    if (offset != result.len) {
      throw std::runtime_error("getMany failed: malformed packed output");
    }
    return out;
  }

  std::shared_ptr<Promise<void>> setStringAsync(const std::string& key,
                                                const std::string& value) override {
    auto self = shared(this);
    return Promise<void>::async([self, key, value] { self->setString(key, value); });
  }

  std::shared_ptr<Promise<void>> setNumberAsync(const std::string& key, double value) override {
    auto self = shared(this);
    return Promise<void>::async([self, key, value] { self->setNumber(key, value); });
  }

  std::shared_ptr<Promise<void>> setBooleanAsync(const std::string& key, bool value) override {
    auto self = shared(this);
    return Promise<void>::async([self, key, value] { self->setBoolean(key, value); });
  }

  std::shared_ptr<Promise<void>> setBufferAsync(const std::string& key,
                                                const std::shared_ptr<ArrayBuffer>& value) override {
    // ArrayBuffers are only safely readable on the JS thread — copy now, not in the lambda.
    std::vector<uint8_t> copy(value->data(), value->data() + value->size());
    auto self = shared(this);
    return Promise<void>::async([self, key, copy = std::move(copy)] {
      self->setRaw(key, 3, copy.data(), copy.size());
    });
  }

  std::shared_ptr<Promise<void>> setJsonAsync(const std::string& key,
                                              const std::string& json) override {
    auto self = shared(this);
    return Promise<void>::async([self, key, json] { self->setJson(key, json); });
  }

  std::shared_ptr<Promise<std::optional<std::string>>>
  getStringAsync(const std::string& key) override {
    auto self = shared(this);
    return Promise<std::optional<std::string>>::async([self, key] { return self->getString(key); });
  }

  std::shared_ptr<Promise<std::optional<double>>>
  getNumberAsync(const std::string& key) override {
    auto self = shared(this);
    return Promise<std::optional<double>>::async([self, key] { return self->getNumber(key); });
  }

  std::shared_ptr<Promise<std::optional<bool>>>
  getBooleanAsync(const std::string& key) override {
    auto self = shared(this);
    return Promise<std::optional<bool>>::async([self, key] { return self->getBoolean(key); });
  }

  std::shared_ptr<Promise<std::optional<std::shared_ptr<ArrayBuffer>>>>
  getBufferAsync(const std::string& key) override {
    auto self = shared(this);
    return Promise<std::optional<std::shared_ptr<ArrayBuffer>>>::async(
        [self, key] { return self->getBuffer(key); });
  }

  std::shared_ptr<Promise<std::optional<std::string>>>
  getJsonAsync(const std::string& key) override {
    auto self = shared(this);
    return Promise<std::optional<std::string>>::async([self, key] { return self->getJson(key); });
  }

  std::shared_ptr<Promise<bool>> containsAsync(const std::string& key) override {
    auto self = shared(this);
    return Promise<bool>::async([self, key] { return self->contains(key); });
  }

  std::shared_ptr<Promise<bool>> removeAsync(const std::string& key) override {
    auto self = shared(this);
    return Promise<bool>::async([self, key] { return self->remove(key); });
  }

  std::shared_ptr<Promise<std::vector<std::string>>> getAllKeysAsync() override {
    auto self = shared(this);
    return Promise<std::vector<std::string>>::async([self] { return self->getAllKeys(); });
  }

  std::shared_ptr<Promise<void>> clearAllAsync() override {
    auto self = shared(this);
    return Promise<void>::async([self] { self->clearAll(); });
  }

  std::shared_ptr<Promise<void>> flushAsync() override {
    auto self = shared(this);
    return Promise<void>::async([self] { self->flush(); });
  }

  std::shared_ptr<Promise<void>>
  setManyStringAsync(const std::vector<std::string>& keys,
                     const std::vector<std::string>& values) override {
    auto self = shared(this);
    return Promise<void>::async(
        [self, keys, values] { self->setManyString(keys, values); });
  }

  std::shared_ptr<Promise<std::vector<std::variant<nitro::NullType, std::string>>>>
  getManyStringAsync(const std::vector<std::string>& keys) override {
    auto self = shared(this);
    return Promise<std::vector<std::variant<nitro::NullType, std::string>>>::async(
        [self, keys] { return self->getManyString(keys); });
  }

private:
  SccKvStore* _handle;

  struct ListenerBox {
    std::function<void(const std::optional<std::string>&)> fn;
  };
  std::unordered_map<uint64_t, std::unique_ptr<ListenerBox>> _listeners;

  static void listenerTrampoline(void* userData, const uint8_t* key, size_t keyLen) {
    auto* box = static_cast<ListenerBox*>(userData);
    if (key == nullptr) {
      box->fn(std::nullopt);
    } else {
      box->fn(std::string(reinterpret_cast<const char*>(key), keyLen));
    }
  }

  // HybridObject is a virtual base of the spec, so a static cast is not possible.
  static std::shared_ptr<HybridSccKvInstance> shared(HybridSccKvInstance* self) {
    return std::dynamic_pointer_cast<HybridSccKvInstance>(self->shared_from_this());
  }

  static const uint8_t* kptr(const std::string& s) {
    return reinterpret_cast<const uint8_t*>(s.data());
  }

  static void writeU32Le(uint8_t* out, uint32_t value) {
    out[0] = static_cast<uint8_t>(value);
    out[1] = static_cast<uint8_t>(value >> 8);
    out[2] = static_cast<uint8_t>(value >> 16);
    out[3] = static_cast<uint8_t>(value >> 24);
  }

  static uint32_t readU32Le(const uint8_t* data) {
    return static_cast<uint32_t>(data[0]) |
           (static_cast<uint32_t>(data[1]) << 8) |
           (static_cast<uint32_t>(data[2]) << 16) |
           (static_cast<uint32_t>(data[3]) << 24);
  }

  [[noreturn]] static void throwLastError(const char* op) {
    char* err = scc_kv_last_error();
    std::string msg = err != nullptr ? err : "unknown error";
    if (err != nullptr) scc_kv_free_cstring(err);
    throw std::runtime_error(std::string(op) + " failed: " + msg);
  }

  void setRaw(const std::string& key, uint8_t tag, const uint8_t* data, size_t len) {
    if (scc_kv_set(_handle, kptr(key), key.size(), tag, data, len) != 0) throwLastError("set");
  }

  void setRawTtl(const std::string& key, uint8_t tag, const uint8_t* data, size_t len,
                 double ttlMs) {
    constexpr double maxSafeInteger = 9007199254740991.0;
    if (!std::isfinite(ttlMs) || ttlMs <= 0 || ttlMs > maxSafeInteger ||
        std::floor(ttlMs) != ttlMs) {
      throw std::runtime_error("ttlMs must be a positive safe integer");
    }
    if (scc_kv_set_ttl(_handle, kptr(key), key.size(), tag, data, len,
                       static_cast<uint64_t>(ttlMs)) != 0) {
      throwLastError("setTtl");
    }
  }

  // Retaining a rare multi-megabyte read on every Nitro worker would inflate
  // process memory indefinitely, so only modest scratch buffers stay cached.
  std::optional<std::string> getStringLike(const std::string& key, uint8_t tag) {
    constexpr size_t initialCapacity = 4096;
    constexpr size_t maxRetainedCapacity = 256 * 1024;
    static thread_local std::vector<uint8_t> scratch(initialCapacity);
    std::vector<uint8_t> oversized;
    std::vector<uint8_t>* buffer = &scratch;
    while (true) {
      size_t needed = 0;
      int rc = scc_kv_get_raw(_handle, kptr(key), key.size(), tag, buffer->data(),
                              buffer->size(), &needed);
      if (rc < 0) throwLastError("get");
      if (rc == 0) return std::nullopt;
      if (needed <= buffer->size()) {
        return std::string(reinterpret_cast<const char*>(buffer->data()), needed);
      }
      if (needed <= maxRetainedCapacity) {
        scratch.resize(needed);
        buffer = &scratch;
      } else {
        oversized.resize(needed);
        buffer = &oversized;
      }
    }
  }
};

} // namespace margelo::nitro::scckv
