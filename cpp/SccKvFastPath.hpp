#pragma once

#include "scc_kv_ffi.h"

#include <jsi/jsi.h>

#include <cstdint>
#include <memory>
#include <string>
#include <utility>
#include <vector>

namespace margelo::nitro::scckv::fastpath {

namespace jsi = facebook::jsi;

// Mirrors Hermes's utf8(): lone surrogates become U+FFFD, so the output is always valid UTF-8.
inline void appendUtf16AsUtf8(std::string& out, const char16_t* data, size_t length) {
  size_t ascii = 0;
  while (ascii < length && data[ascii] < 0x80) ascii++;
  size_t start = out.size();
  if (ascii == length) {
    out.resize(start + length);
    char* narrow = out.data() + start;
    for (size_t index = 0; index < length; index++) narrow[index] = static_cast<char>(data[index]);
    return;
  }
  out.resize(start + ascii + (length - ascii) * 3);
  char* cursor = out.data() + start;
  for (size_t index = 0; index < ascii; index++) *cursor++ = static_cast<char>(data[index]);
  for (size_t index = ascii; index < length; index++) {
    uint32_t unit = data[index];
    if (unit < 0x80) {
      *cursor++ = static_cast<char>(unit);
      continue;
    }
    if (unit < 0x800) {
      *cursor++ = static_cast<char>(0xC0 | (unit >> 6));
      *cursor++ = static_cast<char>(0x80 | (unit & 0x3F));
      continue;
    }
    if (unit >= 0xD800 && unit <= 0xDBFF && index + 1 < length && data[index + 1] >= 0xDC00 &&
        data[index + 1] <= 0xDFFF) {
      uint32_t point = 0x10000 + ((unit - 0xD800) << 10) + (data[index + 1] - 0xDC00);
      index++;
      *cursor++ = static_cast<char>(0xF0 | (point >> 18));
      *cursor++ = static_cast<char>(0x80 | ((point >> 12) & 0x3F));
      *cursor++ = static_cast<char>(0x80 | ((point >> 6) & 0x3F));
      *cursor++ = static_cast<char>(0x80 | (point & 0x3F));
      continue;
    }
    if (unit >= 0xD800 && unit <= 0xDFFF) unit = 0xFFFD;
    *cursor++ = static_cast<char>(0xE0 | (unit >> 12));
    *cursor++ = static_cast<char>(0x80 | ((unit >> 6) & 0x3F));
    *cursor++ = static_cast<char>(0x80 | (unit & 0x3F));
  }
  out.resize(static_cast<size_t>(cursor - out.data()));
}

inline void appendUtf8(jsi::Runtime& rt, const jsi::Value& value, std::string& out) {
  if (!value.isString()) throw jsi::JSError(rt, "expected a string");
  auto collect = [&out](bool ascii, const void* data, size_t length) {
    if (ascii) {
      out.append(static_cast<const char*>(data), length);
    } else {
      appendUtf16AsUtf8(out, static_cast<const char16_t*>(data), length);
    }
  };
  value.getString(rt).getStringData(rt, collect);
}

inline void readUtf8(jsi::Runtime& rt, const jsi::Value& value, std::string& out) {
  out.clear();
  appendUtf8(rt, value, out);
}

inline void appendField(jsi::Runtime& rt, const jsi::Value& value, std::string& packed) {
  size_t lengthAt = packed.size();
  packed.append(4, '\0');
  appendUtf8(rt, value, packed);
  size_t length = packed.size() - lengthAt - 4;
  if (length > UINT32_MAX) throw jsi::JSError(rt, "string exceeds the batch limit");
  for (size_t shift = 0; shift < 4; shift++) {
    packed[lengthAt + shift] = static_cast<char>((length >> (shift * 8)) & 0xFF);
  }
}

inline const uint8_t* bytes(const std::string& text) {
  return reinterpret_cast<const uint8_t*>(text.data());
}

[[noreturn]] inline void throwLastError(jsi::Runtime& rt, const char* op) {
  char* err = scc_kv_last_error();
  std::string message = err != nullptr ? err : "unknown error";
  if (err != nullptr) scc_kv_free_cstring(err);
  throw jsi::JSError(rt, std::string(op) + " failed: " + message);
}

inline void expectArguments(jsi::Runtime& rt, size_t count, size_t expected) {
  if (count < expected) throw jsi::JSError(rt, "missing arguments");
}

constexpr size_t maxRetainedCapacity = 256 * 1024;

inline void trimRetained(std::string& buffer) {
  if (buffer.capacity() > maxRetainedCapacity) std::string().swap(buffer);
}

inline std::string& keyBuffer() {
  static thread_local std::string buffer;
  return buffer;
}

inline std::string& valueBuffer() {
  static thread_local std::string buffer;
  return buffer;
}

inline jsi::Value getStringLike(jsi::Runtime& rt, SccKvStore* handle, const std::string& key,
                                uint8_t tag) {
  static thread_local std::vector<uint8_t> scratch(4096);
  std::vector<uint8_t> oversized;
  std::vector<uint8_t>* buffer = &scratch;
  while (true) {
    size_t needed = 0;
    int rc = scc_kv_get_raw(handle, bytes(key), key.size(), tag, buffer->data(), buffer->size(),
                            &needed);
    if (rc < 0) throwLastError(rt, "get");
    if (rc == 0) return jsi::Value::undefined();
    if (needed <= buffer->size()) {
      return jsi::String::createFromUtf8(rt, buffer->data(), needed);
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

// `owner` keeps the handle alive while JS holds any of these functions.
inline jsi::Object create(jsi::Runtime& rt, SccKvStore* handle, std::shared_ptr<void> owner) {
  jsi::Object calls(rt);
  auto define = [&](const char* name, unsigned arity, jsi::HostFunctionType function) {
    calls.setProperty(rt, name,
                      jsi::Function::createFromHostFunction(rt, jsi::PropNameID::forAscii(rt, name),
                                                            arity, std::move(function)));
  };
  auto key = [](jsi::Runtime& rt, const jsi::Value* args, size_t count, size_t arity) -> std::string& {
    expectArguments(rt, count, arity);
    std::string& buffer = keyBuffer();
    readUtf8(rt, args[0], buffer);
    return buffer;
  };

  define("getString", 1, [handle, owner, key](jsi::Runtime& rt, const jsi::Value&, const jsi::Value* args, size_t count) {
    return getStringLike(rt, handle, key(rt, args, count, 1), 0);
  });
  define("getJson", 1, [handle, owner, key](jsi::Runtime& rt, const jsi::Value&, const jsi::Value* args, size_t count) {
    return getStringLike(rt, handle, key(rt, args, count, 1), 4);
  });
  define("getNumber", 1, [handle, owner, key](jsi::Runtime& rt, const jsi::Value&, const jsi::Value* args, size_t count) {
    const std::string& name = key(rt, args, count, 1);
    double value = 0;
    int rc = scc_kv_get_f64(handle, bytes(name), name.size(), &value);
    if (rc < 0) throwLastError(rt, "get");
    return rc == 0 ? jsi::Value::undefined() : jsi::Value(value);
  });
  define("getBoolean", 1, [handle, owner, key](jsi::Runtime& rt, const jsi::Value&, const jsi::Value* args, size_t count) {
    const std::string& name = key(rt, args, count, 1);
    bool value = false;
    int rc = scc_kv_get_bool(handle, bytes(name), name.size(), &value);
    if (rc < 0) throwLastError(rt, "get");
    return rc == 0 ? jsi::Value::undefined() : jsi::Value(value);
  });
  define("contains", 1, [handle, owner, key](jsi::Runtime& rt, const jsi::Value&, const jsi::Value* args, size_t count) {
    const std::string& name = key(rt, args, count, 1);
    int rc = scc_kv_contains(handle, bytes(name), name.size());
    if (rc < 0) throwLastError(rt, "contains");
    return jsi::Value(rc == 1);
  });
  define("remove", 1, [handle, owner, key](jsi::Runtime& rt, const jsi::Value&, const jsi::Value* args, size_t count) {
    const std::string& name = key(rt, args, count, 1);
    int rc = scc_kv_remove(handle, bytes(name), name.size());
    if (rc < 0) throwLastError(rt, "remove");
    return jsi::Value(rc == 1);
  });
  define("setString", 2, [handle, owner, key](jsi::Runtime& rt, const jsi::Value&, const jsi::Value* args, size_t count) {
    const std::string& name = key(rt, args, count, 2);
    std::string& value = valueBuffer();
    readUtf8(rt, args[1], value);
    int rc = scc_kv_set_str(handle, bytes(name), name.size(), bytes(value), value.size());
    trimRetained(value);
    if (rc != 0) throwLastError(rt, "set");
    return jsi::Value::undefined();
  });
  define("getManyString", 1, [handle, owner](jsi::Runtime& rt, const jsi::Value&, const jsi::Value* args, size_t count) {
    expectArguments(rt, count, 1);
    jsi::Array keys = args[0].asObject(rt).asArray(rt);
    size_t length = keys.size(rt);
    jsi::Array values(rt, length);
    std::string& key = keyBuffer();
    for (size_t index = 0; index < length; index++) {
      readUtf8(rt, keys.getValueAtIndex(rt, index), key);
      values.setValueAtIndex(rt, index, getStringLike(rt, handle, key, 0));
    }
    return values;
  });
  define("setManyString", 2, [handle, owner](jsi::Runtime& rt, const jsi::Value&, const jsi::Value* args, size_t count) {
    expectArguments(rt, count, 2);
    jsi::Array keys = args[0].asObject(rt).asArray(rt);
    jsi::Array values = args[1].asObject(rt).asArray(rt);
    size_t length = keys.size(rt);
    if (values.size(rt) != length) throw jsi::JSError(rt, "keys and values length mismatch");
    if (length == 0) return jsi::Value::undefined();
    std::string& packed = valueBuffer();
    packed.clear();
    for (size_t index = 0; index < length; index++) {
      appendField(rt, keys.getValueAtIndex(rt, index), packed);
      appendField(rt, values.getValueAtIndex(rt, index), packed);
    }
    int rc = scc_kv_set_many_str(handle, bytes(packed), packed.size(), length);
    trimRetained(packed);
    if (rc != 0) throwLastError(rt, "setMany");
    return jsi::Value::undefined();
  });
  define("setNumber", 2, [handle, owner, key](jsi::Runtime& rt, const jsi::Value&, const jsi::Value* args, size_t count) {
    const std::string& name = key(rt, args, count, 2);
    if (!args[1].isNumber()) throw jsi::JSError(rt, "expected a number");
    if (scc_kv_set_f64(handle, bytes(name), name.size(), args[1].getNumber()) != 0) {
      throwLastError(rt, "set");
    }
    return jsi::Value::undefined();
  });
  define("setBoolean", 2, [handle, owner, key](jsi::Runtime& rt, const jsi::Value&, const jsi::Value* args, size_t count) {
    const std::string& name = key(rt, args, count, 2);
    if (!args[1].isBool()) throw jsi::JSError(rt, "expected a boolean");
    if (scc_kv_set_bool(handle, bytes(name), name.size(), args[1].getBool()) != 0) {
      throwLastError(rt, "set");
    }
    return jsi::Value::undefined();
  });
  return calls;
}

} // namespace margelo::nitro::scckv::fastpath
