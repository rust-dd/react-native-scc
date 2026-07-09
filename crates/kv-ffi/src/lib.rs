// The C ABI contract (pointer validity, UTF-8 keys) lives in the generated
// header and the readme; per-function safety boilerplate adds nothing.
#![allow(clippy::missing_safety_doc)]

mod batch;
mod buffers;
mod error;
mod handle;

use std::ffi::CStr;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::time::Duration;

use kv_core::{Durability, OpenOptions, Value};

use batch::PackedEntries;
use buffers::vec_to_raw;
use error::{clear_error, set_error};
pub use handle::SccKvStore;

unsafe fn cstr<'a>(p: *const c_char, what: &str) -> Result<&'a str, String> {
    if p.is_null() {
        return Err(format!("{what} is null"));
    }
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map_err(|_| format!("{what} is not valid UTF-8"))
}

/// Keys cross the boundary as (ptr, len) — no strlen, no UTF-8 scan. JSI
/// guarantees valid UTF-8; debug builds assert it.
unsafe fn key_str<'a>(p: *const u8, len: usize) -> Result<&'a str, String> {
    if p.is_null() {
        if len == 0 {
            return Ok("");
        }
        return Err("key is null".to_string());
    }
    let bytes = unsafe { std::slice::from_raw_parts(p, len) };
    debug_assert!(
        std::str::from_utf8(bytes).is_ok(),
        "key must be valid UTF-8"
    );
    Ok(unsafe { std::str::from_utf8_unchecked(bytes) })
}

fn guard<T>(sentinel: T, f: impl FnOnce() -> Result<T, String>) -> T {
    clear_error();
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(v)) => v,
        Ok(Err(msg)) => {
            set_error(msg);
            sentinel
        }
        Err(_) => {
            set_error("internal panic in kv-ffi");
            sentinel
        }
    }
}

unsafe fn store<'a>(h: *mut SccKvStore) -> Result<&'a SccKvStore, String> {
    unsafe { SccKvStore::borrow(h) }.ok_or_else(|| "store handle is null".to_string())
}

unsafe fn data_slice<'a>(data: *const u8, len: usize) -> Result<&'a [u8], String> {
    if len == 0 {
        Ok(&[])
    } else if data.is_null() {
        Err("data is null but len > 0".to_string())
    } else {
        Ok(unsafe { std::slice::from_raw_parts(data, len) })
    }
}

/// Applies a transaction batch atomically as one WAL record (all-or-nothing on
/// replay). `ptr`/`len` is the packed buffer `[u32 count]` + ops.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_apply_batch(
    handle: *mut SccKvStore,
    ptr: *const u8,
    len: usize,
) -> i32 {
    guard(-1, || {
        let s = unsafe { store(handle) }?;
        let data = unsafe { data_slice(ptr, len) }?;
        let ops = batch::decode_batch(data)?;
        s.store.apply_batch(&ops).map_err(|e| e.to_string())?;
        Ok(0)
    })
}

/// Opens (or returns the already-open) persistent store `id` under `dir`.
/// A non-empty `enc_key` passphrase enables encryption at rest (the 32-byte
/// cipher key is derived via SHA-256). Returns NULL on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_open(
    dir: *const c_char,
    id: *const c_char,
    strict: bool,
    recreate: bool,
    enc_key: *const u8,
    enc_key_len: usize,
    max_entries: usize,
    ttl_sweep_interval_ms: u64,
) -> *mut SccKvStore {
    guard(std::ptr::null_mut(), || {
        let dir = unsafe { cstr(dir, "dir") }?;
        let id = unsafe { cstr(id, "id") }?;
        let encryption_key = if enc_key_len > 0 {
            let bytes = unsafe { data_slice(enc_key, enc_key_len) }?;
            Some(kv_core::derive_encryption_key(bytes))
        } else {
            None
        };
        let opts = OpenOptions {
            durability: if strict {
                Durability::Strict
            } else {
                Durability::Relaxed
            },
            recreate,
            encryption_key,
            max_entries: (max_entries > 0).then_some(max_entries),
            ttl_sweep_interval: if ttl_sweep_interval_ms > 0 {
                Duration::from_millis(ttl_sweep_interval_ms)
            } else {
                OpenOptions::default().ttl_sweep_interval
            },
            ..OpenOptions::default()
        };
        let store = kv_core::open_or_get(Path::new(dir), id, opts).map_err(|e| e.to_string())?;
        Ok(SccKvStore {
            store,
            dir: Some(Path::new(dir).to_path_buf()),
            id: id.to_string(),
        }
        .into_raw())
    })
}

/// Returns the named in-memory store, creating it on first use. NULL on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_in_memory(
    id: *const c_char,
    max_entries: usize,
    ttl_sweep_interval_ms: u64,
) -> *mut SccKvStore {
    guard(std::ptr::null_mut(), || {
        let id = unsafe { cstr(id, "id") }?;
        let max = (max_entries > 0).then_some(max_entries);
        let sweep =
            (ttl_sweep_interval_ms > 0).then(|| Duration::from_millis(ttl_sweep_interval_ms));
        let store = kv_core::in_memory(id, max, sweep);
        Ok(SccKvStore {
            store,
            dir: None,
            id: id.to_string(),
        }
        .into_raw())
    })
}

/// Closes the underlying store and removes it from the registry. 0 ok, -1 error.
/// The handle itself must still be released with `scc_kv_release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_close(h: *mut SccKvStore) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        kv_core::close(s.dir.as_deref(), &s.id).map_err(|e| e.to_string())?;
        Ok(0)
    })
}

/// Frees the handle. The store stays registered (reopen returns it) unless
/// `scc_kv_close` was called first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_release(h: *mut SccKvStore) {
    if !h.is_null() {
        drop(unsafe { Box::from_raw(h) });
    }
}

/// Sets `key` to the value encoded as (tag, data, len). 0 ok, -1 error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_set(
    h: *mut SccKvStore,
    key: *const u8,
    key_len: usize,
    tag: u8,
    data: *const u8,
    len: usize,
) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        let key = unsafe { key_str(key, key_len) }?;
        let bytes = unsafe { data_slice(data, len) }?;
        let value =
            Value::decode(tag, bytes).ok_or_else(|| format!("invalid value for tag {tag}"))?;
        s.store.set(key, value).map_err(|e| e.to_string())?;
        Ok(0)
    })
}

/// Reads `key`. 1 found (out params set, buffer freed via `scc_kv_free`),
/// 0 missing, -1 error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_get(
    h: *mut SccKvStore,
    key: *const u8,
    key_len: usize,
    out_tag: *mut u8,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        let key = unsafe { key_str(key, key_len) }?;
        if out_tag.is_null() || out_data.is_null() || out_len.is_null() {
            return Err("output pointer is null".to_string());
        }
        match s.store.get(key) {
            Some(value) => {
                let mut buf = Vec::new();
                value.encode_into(&mut buf);
                let (ptr, len) = vec_to_raw(buf);
                unsafe {
                    *out_tag = value.tag();
                    *out_data = ptr;
                    *out_len = len;
                }
                Ok(1)
            }
            None => Ok(0),
        }
    })
}

/// 1 if `key` exists, 0 if not, -1 on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_contains(
    h: *mut SccKvStore,
    key: *const u8,
    key_len: usize,
) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        let key = unsafe { key_str(key, key_len) }?;
        Ok(s.store.contains(key) as i32)
    })
}

/// 1 removed, 0 missing, -1 error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_remove(h: *mut SccKvStore, key: *const u8, key_len: usize) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        let key = unsafe { key_str(key, key_len) }?;
        Ok(s.store.delete(key).map_err(|e| e.to_string())? as i32)
    })
}

/// All keys packed as repeated `[u32 len LE][utf8 bytes]`. Empty store:
/// NULL with *out_len = 0. Error: NULL with *out_len = 1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_keys(h: *mut SccKvStore, out_len: *mut usize) -> *mut u8 {
    if out_len.is_null() {
        set_error("out_len is null");
        return std::ptr::null_mut();
    }
    unsafe { *out_len = 1 };
    guard(std::ptr::null_mut(), || {
        let s = unsafe { store(h) }?;
        let mut buf = Vec::new();
        for key in s.store.keys() {
            buf.extend_from_slice(&(key.len() as u32).to_le_bytes());
            buf.extend_from_slice(key.as_bytes());
        }
        let (ptr, len) = vec_to_raw(buf);
        unsafe { *out_len = len };
        Ok(ptr)
    })
}

/// 0 ok, -1 error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_clear(h: *mut SccKvStore) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        s.store.clear().map_err(|e| e.to_string())?;
        Ok(0)
    })
}

/// Durability barrier: blocks until the WAL is drained and fsynced. 0 ok, -1 error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_flush(h: *mut SccKvStore) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        s.store.flush().map_err(|e| e.to_string())?;
        Ok(0)
    })
}

/// Number of keys. 0 on a null handle (with error set).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_len(h: *mut SccKvStore) -> u64 {
    guard(0, || {
        let s = unsafe { store(h) }?;
        Ok(s.store.len() as u64)
    })
}

/// Fast-path string/bytes/json read into a caller-provided buffer, avoiding
/// heap allocation. Returns 1 found (with `*out_len` set; bytes are copied
/// only when `*out_len <= cap` — otherwise call again with a larger buffer),
/// 0 missing or type mismatch, -1 error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_get_raw(
    h: *mut SccKvStore,
    key: *const u8,
    key_len: usize,
    expected_tag: u8,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        let key = unsafe { key_str(key, key_len) }?;
        if out_len.is_null() || (buf.is_null() && cap > 0) {
            return Err("output pointer is null".to_string());
        }
        let found = s.store.with_value(key, |value| {
            if value.tag() != expected_tag {
                return 0;
            }
            let bytes: &[u8] = match value {
                Value::Str(v) | Value::Json(v) => v.as_bytes(),
                Value::Bytes(v) => v,
                Value::Num(_) | Value::Bool(_) => return 0,
            };
            unsafe { *out_len = bytes.len() };
            if bytes.len() <= cap {
                unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len()) };
            }
            1
        });
        Ok(found.unwrap_or(0))
    })
}

/// Zero-allocation number read. 1 found, 0 missing/type mismatch, -1 error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_get_f64(
    h: *mut SccKvStore,
    key: *const u8,
    key_len: usize,
    out: *mut f64,
) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        let key = unsafe { key_str(key, key_len) }?;
        if out.is_null() {
            return Err("out is null".to_string());
        }
        let found = s.store.with_value(key, |value| match value {
            Value::Num(n) => {
                unsafe { *out = *n };
                1
            }
            _ => 0,
        });
        Ok(found.unwrap_or(0))
    })
}

/// Zero-allocation boolean read. 1 found, 0 missing/type mismatch, -1 error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_get_bool(
    h: *mut SccKvStore,
    key: *const u8,
    key_len: usize,
    out: *mut bool,
) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        let key = unsafe { key_str(key, key_len) }?;
        if out.is_null() {
            return Err("out is null".to_string());
        }
        let found = s.store.with_value(key, |value| match value {
            Value::Bool(b) => {
                unsafe { *out = *b };
                1
            }
            _ => 0,
        });
        Ok(found.unwrap_or(0))
    })
}

/// Sets `key` with a TTL: the entry expires `ttl_ms` from now. 0 ok, -1 error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_set_ttl(
    h: *mut SccKvStore,
    key: *const u8,
    key_len: usize,
    tag: u8,
    data: *const u8,
    len: usize,
    ttl_ms: u64,
) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        let key = unsafe { key_str(key, key_len) }?;
        let bytes = unsafe { data_slice(data, len) }?;
        let value =
            Value::decode(tag, bytes).ok_or_else(|| format!("invalid value for tag {tag}"))?;
        s.store
            .set_with_ttl(key, value, ttl_ms)
            .map_err(|e| e.to_string())?;
        Ok(0)
    })
}

/// Fast-path string set: no tag round trip, no UTF-8 scan. 0 ok, -1 error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_set_str(
    h: *mut SccKvStore,
    key: *const u8,
    key_len: usize,
    data: *const u8,
    len: usize,
) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        let key = unsafe { key_str(key, key_len) }?;
        let bytes = unsafe { data_slice(data, len) }?;
        debug_assert!(
            std::str::from_utf8(bytes).is_ok(),
            "scc_kv_set_str requires valid UTF-8"
        );
        let value = unsafe { kv_core::CompactString::from_utf8_unchecked(bytes) };
        s.store
            .set(key, Value::Str(value))
            .map_err(|e| e.to_string())?;
        Ok(0)
    })
}

/// Fast-path number set. 0 ok, -1 error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_set_f64(
    h: *mut SccKvStore,
    key: *const u8,
    key_len: usize,
    value: f64,
) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        let key = unsafe { key_str(key, key_len) }?;
        s.store
            .set(key, Value::Num(value))
            .map_err(|e| e.to_string())?;
        Ok(0)
    })
}

/// Fast-path boolean set. 0 ok, -1 error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_set_bool(
    h: *mut SccKvStore,
    key: *const u8,
    key_len: usize,
    value: bool,
) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        let key = unsafe { key_str(key, key_len) }?;
        s.store
            .set(key, Value::Bool(value))
            .map_err(|e| e.to_string())?;
        Ok(0)
    })
}

/// Batch string set. `data` holds `count` packed entries, each
/// `[u32 key_len LE][key][u32 val_len LE][val]`. All records land in a single
/// WAL append; a malformed buffer applies nothing. 0 ok, -1 error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_set_many_str(
    h: *mut SccKvStore,
    data: *const u8,
    len: usize,
    count: usize,
) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        let data = unsafe { data_slice(data, len) }?;
        let entries = PackedEntries::validate(data, count)?;
        s.store.set_many(entries).map_err(|e| e.to_string())?;
        Ok(0)
    })
}

/// Listener callback: `key == NULL` means "everything changed" (clear);
/// otherwise `key`/`key_len` is the UTF-8 key, not NUL-terminated.
pub type SccKvListener =
    unsafe extern "C" fn(user_data: *mut std::ffi::c_void, key: *const u8, key_len: usize);

struct ListenerCtx {
    cb: SccKvListener,
    user_data: *mut std::ffi::c_void,
}

// The caller (C++) guarantees the callback and user_data are usable from any
// thread; mutations may notify from the JS thread or the async pool.
unsafe impl Send for ListenerCtx {}
unsafe impl Sync for ListenerCtx {}

/// Subscribes to change events. Returns a listener id (> 0), or 0 on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_subscribe(
    h: *mut SccKvStore,
    cb: SccKvListener,
    user_data: *mut std::ffi::c_void,
) -> u64 {
    guard(0, || {
        let s = unsafe { store(h) }?;
        let ctx = ListenerCtx { cb, user_data };
        Ok(s.store.subscribe(move |key| {
            // Reference the whole struct so the closure captures ListenerCtx
            // itself (Send + Sync), not its raw-pointer fields one by one.
            let ctx = &ctx;
            match key {
                Some(k) => unsafe { (ctx.cb)(ctx.user_data, k.as_ptr(), k.len()) },
                None => unsafe { (ctx.cb)(ctx.user_data, std::ptr::null(), 0) },
            }
        }))
    })
}

/// 1 removed, 0 unknown id, -1 error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_unsubscribe(h: *mut SccKvStore, id: u64) -> i32 {
    guard(-1, || {
        let s = unsafe { store(h) }?;
        Ok(s.store.unsubscribe(id) as i32)
    })
}

#[cfg(test)]
mod tests;
