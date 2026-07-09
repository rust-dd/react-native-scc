use std::ffi::CString;

use super::*;

use super::buffers::scc_kv_free;
use super::error::{scc_kv_free_cstring, scc_kv_last_error};

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

fn last_error() -> Option<String> {
    let p = scc_kv_last_error();
    if p.is_null() {
        return None;
    }
    let msg = unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_string();
    unsafe { scc_kv_free_cstring(p) };
    Some(msg)
}

fn set(h: *mut SccKvStore, key: &str, tag: u8, data: &[u8]) -> i32 {
    unsafe { scc_kv_set(h, key.as_ptr(), key.len(), tag, data.as_ptr(), data.len()) }
}

fn get_owned(h: *mut SccKvStore, key: &str) -> Option<(u8, Vec<u8>)> {
    let mut tag = 0u8;
    let mut data: *mut u8 = std::ptr::null_mut();
    let mut len = 0usize;
    match unsafe { scc_kv_get(h, key.as_ptr(), key.len(), &mut tag, &mut data, &mut len) } {
        1 => {
            let bytes = if len == 0 {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(data, len) }.to_vec()
            };
            unsafe { scc_kv_free(data, len) };
            Some((tag, bytes))
        }
        0 => None,
        other => panic!("get failed: {other}, err {:?}", last_error()),
    }
}

#[test]
fn round_trips_all_tags_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let dir_c = c(dir.path().to_str().unwrap());
    let id = c("ffi");
    let h = unsafe {
        scc_kv_open(
            dir_c.as_ptr(),
            id.as_ptr(),
            true,
            false,
            std::ptr::null(),
            0,
            0,
            0,
        )
    };
    assert!(!h.is_null(), "{:?}", last_error());

    assert_eq!(set(h, "s", 0, b"hi"), 0);
    let num = 42.5f64.to_le_bytes();
    assert_eq!(set(h, "n", 1, &num), 0);
    assert_eq!(set(h, "b", 2, &[1u8]), 0);
    assert_eq!(set(h, "y", 3, &[9u8, 8, 7]), 0);
    let json = br#"{"a":1}"#;
    assert_eq!(set(h, "j", 4, json), 0);

    assert_eq!(get_owned(h, "s"), Some((0, b"hi".to_vec())));
    assert_eq!(get_owned(h, "n"), Some((1, num.to_vec())));
    assert_eq!(get_owned(h, "b"), Some((2, vec![1])));
    assert_eq!(get_owned(h, "y"), Some((3, vec![9, 8, 7])));
    assert_eq!(get_owned(h, "j"), Some((4, json.to_vec())));
    assert_eq!(unsafe { scc_kv_len(h) }, 5);
    assert_eq!(unsafe { scc_kv_contains(h, "s".as_ptr(), 1) }, 1);
    assert_eq!(unsafe { scc_kv_flush(h) }, 0);
    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };

    let h2 = unsafe {
        scc_kv_open(
            dir_c.as_ptr(),
            id.as_ptr(),
            true,
            false,
            std::ptr::null(),
            0,
            0,
            0,
        )
    };
    assert!(!h2.is_null());
    assert_eq!(unsafe { scc_kv_len(h2) }, 5);
    assert_eq!(get_owned(h2, "n"), Some((1, num.to_vec())));
    assert_eq!(unsafe { scc_kv_remove(h2, "s".as_ptr(), 1) }, 1);
    assert_eq!(unsafe { scc_kv_remove(h2, "s".as_ptr(), 1) }, 0);
    assert_eq!(unsafe { scc_kv_clear(h2) }, 0);
    assert_eq!(unsafe { scc_kv_len(h2) }, 0);
    assert_eq!(unsafe { scc_kv_close(h2) }, 0);
    unsafe { scc_kv_release(h2) };
}

#[test]
fn keys_packing_decodes() {
    let id = c("keys-test");
    let h = unsafe { scc_kv_in_memory(id.as_ptr(), 0, 0) };
    for k in ["alpha", "b", "gamma_gamma"] {
        assert_eq!(set(h, k, 0, b"x"), 0);
    }
    let mut len = 0usize;
    let ptr = unsafe { scc_kv_keys(h, &mut len) };
    assert!(!ptr.is_null());
    let data = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    unsafe { scc_kv_free(ptr, len) };

    let mut keys = Vec::new();
    let mut off = 0usize;
    while off < data.len() {
        let n = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        keys.push(String::from_utf8(data[off..off + n].to_vec()).unwrap());
        off += n;
    }
    keys.sort();
    assert_eq!(keys, ["alpha", "b", "gamma_gamma"]);
    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };
}

#[test]
fn errors_are_reported() {
    assert!(
        unsafe {
            scc_kv_open(
                std::ptr::null(),
                std::ptr::null(),
                false,
                false,
                std::ptr::null(),
                0,
                0,
                0,
            )
        }
        .is_null()
    );
    assert!(last_error().unwrap().contains("dir is null"));

    assert_eq!(
        unsafe {
            scc_kv_set(
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                0,
                std::ptr::null(),
                0,
            )
        },
        -1
    );
    assert!(last_error().unwrap().contains("handle is null"));

    let id = c("err-test");
    let h = unsafe { scc_kv_in_memory(id.as_ptr(), 0, 0) };
    assert_eq!(set(h, "k", 1, b"xx"), -1);
    assert!(last_error().unwrap().contains("invalid value for tag 1"));
    assert_eq!(set(h, "k", 9, &[]), -1);
    assert_eq!(
        unsafe { scc_kv_set(h, std::ptr::null(), 3, 0, std::ptr::null(), 0) },
        -1
    );
    assert!(last_error().unwrap().contains("key is null"));
    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };
}

#[test]
fn empty_string_value_round_trips() {
    let id = c("empty-test");
    let h = unsafe { scc_kv_in_memory(id.as_ptr(), 0, 0) };
    assert_eq!(set(h, "empty", 0, &[]), 0);
    assert_eq!(get_owned(h, "empty"), Some((0, Vec::new())));
    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };
}

#[test]
fn fast_paths_round_trip() {
    let id = c("fast-test");
    let h = unsafe { scc_kv_in_memory(id.as_ptr(), 0, 0) };

    assert_eq!(
        unsafe { scc_kv_set_str(h, "s".as_ptr(), 1, b"hello".as_ptr(), 5) },
        0
    );
    assert_eq!(unsafe { scc_kv_set_f64(h, "n".as_ptr(), 1, 42.5) }, 0);
    assert_eq!(unsafe { scc_kv_set_bool(h, "b".as_ptr(), 1, true) }, 0);

    let mut buf = [0u8; 64];
    let mut len = 0usize;
    assert_eq!(
        unsafe { scc_kv_get_raw(h, "s".as_ptr(), 1, 0, buf.as_mut_ptr(), buf.len(), &mut len) },
        1
    );
    assert_eq!(&buf[..len], b"hello");

    let mut num = 0f64;
    assert_eq!(unsafe { scc_kv_get_f64(h, "n".as_ptr(), 1, &mut num) }, 1);
    assert_eq!(num, 42.5);
    let mut flag = false;
    assert_eq!(unsafe { scc_kv_get_bool(h, "b".as_ptr(), 1, &mut flag) }, 1);
    assert!(flag);

    assert_eq!(
        unsafe { scc_kv_get_f64(h, "missing".as_ptr(), 7, &mut num) },
        0
    );
    assert_eq!(unsafe { scc_kv_get_f64(h, "s".as_ptr(), 1, &mut num) }, 0);
    assert_eq!(
        unsafe { scc_kv_get_raw(h, "n".as_ptr(), 1, 0, buf.as_mut_ptr(), buf.len(), &mut len) },
        0
    );

    let mut tiny = [0u8; 2];
    assert_eq!(
        unsafe {
            scc_kv_get_raw(
                h,
                "s".as_ptr(),
                1,
                0,
                tiny.as_mut_ptr(),
                tiny.len(),
                &mut len,
            )
        },
        1
    );
    assert_eq!(len, 5);
    assert_eq!(tiny, [0u8; 2]);

    assert_eq!(get_owned(h, "s"), Some((0, b"hello".to_vec())));

    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };
}

fn pack_entries(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (k, v) in entries {
        out.extend_from_slice(&(k.len() as u32).to_le_bytes());
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(&(v.len() as u32).to_le_bytes());
        out.extend_from_slice(v.as_bytes());
    }
    out
}

#[test]
fn batch_set_many_applies_all_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let dir_c = c(dir.path().to_str().unwrap());
    let id = c("batch");
    let h = unsafe {
        scc_kv_open(
            dir_c.as_ptr(),
            id.as_ptr(),
            true,
            false,
            std::ptr::null(),
            0,
            0,
            0,
        )
    };
    assert!(!h.is_null());

    let long = "z".repeat(100);
    let packed = pack_entries(&[("a", "1"), ("b", ""), ("c", &long)]);
    assert_eq!(
        unsafe { scc_kv_set_many_str(h, packed.as_ptr(), packed.len(), 3) },
        0
    );
    assert_eq!(unsafe { scc_kv_len(h) }, 3);
    assert_eq!(get_owned(h, "a"), Some((0, b"1".to_vec())));
    assert_eq!(get_owned(h, "b"), Some((0, Vec::new())));
    assert_eq!(get_owned(h, "c"), Some((0, long.as_bytes().to_vec())));

    let truncated = &packed[..packed.len() - 1];
    assert_eq!(
        unsafe { scc_kv_set_many_str(h, truncated.as_ptr(), truncated.len(), 3) },
        -1
    );
    assert!(last_error().unwrap().contains("truncated"));

    assert_eq!(
        unsafe { scc_kv_set_many_str(h, packed.as_ptr(), packed.len(), 2) },
        -1
    );
    assert!(last_error().unwrap().contains("count mismatch"));

    assert_eq!(unsafe { scc_kv_flush(h) }, 0);
    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };

    let h2 = unsafe {
        scc_kv_open(
            dir_c.as_ptr(),
            id.as_ptr(),
            true,
            false,
            std::ptr::null(),
            0,
            0,
            0,
        )
    };
    assert_eq!(unsafe { scc_kv_len(h2) }, 3);
    assert_eq!(get_owned(h2, "c"), Some((0, long.as_bytes().to_vec())));
    assert_eq!(unsafe { scc_kv_close(h2) }, 0);
    unsafe { scc_kv_release(h2) };
}

#[test]
fn ttl_and_encryption_via_ffi() {
    let dir = tempfile::tempdir().unwrap();
    let dir_c = c(dir.path().to_str().unwrap());
    let id = c("vault");
    let pw = b"passphrase";
    let h = unsafe {
        scc_kv_open(
            dir_c.as_ptr(),
            id.as_ptr(),
            true,
            false,
            pw.as_ptr(),
            pw.len(),
            0,
            0,
        )
    };
    assert!(!h.is_null(), "{:?}", last_error());

    assert_eq!(
        unsafe { scc_kv_set_ttl(h, "tmp".as_ptr(), 3, 0, b"v".as_ptr(), 1, 100) },
        0
    );
    assert_eq!(
        unsafe { scc_kv_set_str(h, "keep".as_ptr(), 4, b"x".as_ptr(), 1) },
        0
    );
    assert_eq!(get_owned(h, "tmp"), Some((0, b"v".to_vec())));
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(get_owned(h, "tmp"), None);
    assert_eq!(unsafe { scc_kv_flush(h) }, 0);
    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };

    let raw = std::fs::read(dir.path().join("vault.wal")).unwrap();
    assert!(!raw.windows(4).any(|w| w == b"keep"));

    let h2 = unsafe {
        scc_kv_open(
            dir_c.as_ptr(),
            id.as_ptr(),
            true,
            false,
            pw.as_ptr(),
            pw.len(),
            0,
            0,
        )
    };
    assert!(!h2.is_null());
    assert_eq!(get_owned(h2, "keep"), Some((0, b"x".to_vec())));
    assert_eq!(unsafe { scc_kv_close(h2) }, 0);
    unsafe { scc_kv_release(h2) };

    let wrong = b"other";
    let h3 = unsafe {
        scc_kv_open(
            dir_c.as_ptr(),
            id.as_ptr(),
            true,
            false,
            wrong.as_ptr(),
            wrong.len(),
            0,
            0,
        )
    };
    assert!(h3.is_null());
    assert!(last_error().unwrap().contains("wrong encryption key"));
}

#[test]
fn open_accepts_eviction_options() {
    let dir = tempfile::tempdir().unwrap();
    let dir_c = c(dir.path().to_str().unwrap());
    let id = c("evict-ffi");
    let h = unsafe {
        scc_kv_open(
            dir_c.as_ptr(),
            id.as_ptr(),
            true,
            false,
            std::ptr::null(),
            0,
            2,
            20,
        )
    };
    assert!(!h.is_null(), "{:?}", last_error());

    for i in 0..6 {
        let key = format!("k{i}");
        assert_eq!(set(h, &key, 0, b"value"), 0);
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(unsafe { scc_kv_len(h) } <= 2);

    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };
}

#[test]
fn eviction_keeps_live_keys_when_many_expire_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let dir_c = c(dir.path().to_str().unwrap());
    let id = c("evict-live");
    let h = unsafe {
        scc_kv_open(
            dir_c.as_ptr(),
            id.as_ptr(),
            false,
            false,
            std::ptr::null(),
            0,
            100,
            20,
        )
    };
    assert!(!h.is_null(), "{:?}", last_error());

    // 50 permanent keys, comfortably under maxEntries=100 — must never be evicted.
    for i in 0..50 {
        let key = format!("keep{i}");
        assert_eq!(set(h, &key, 0, b"live"), 0);
    }
    // More than the 4096 sweep-scan cap of short-TTL keys, so a single sweep sees
    // more expired entries than it collects. Guards against computing the eviction
    // target from `map.len() - min(expired, 4096)`, which over-counts live entries
    // and evicts the permanent keys.
    for i in 0..5000 {
        let key = format!("tmp{i}");
        assert_eq!(
            unsafe { scc_kv_set_ttl(h, key.as_ptr(), key.len(), 0, b"x".as_ptr(), 1, 1) },
            0
        );
    }

    std::thread::sleep(std::time::Duration::from_millis(400));

    for i in 0..50 {
        let key = format!("keep{i}");
        assert!(
            get_owned(h, &key).is_some(),
            "live key {key} was evicted while under the cap"
        );
    }
    assert!(unsafe { scc_kv_len(h) } <= 100);

    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };
}

fn pack_batch(ops: &[(u8, &str, u8, &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&(ops.len() as u32).to_le_bytes());
    for (op, key, tag, val) in ops {
        buf.push(*op);
        buf.extend_from_slice(&(key.len() as u32).to_le_bytes());
        buf.extend_from_slice(key.as_bytes());
        if *op == 1 {
            buf.push(*tag);
            buf.extend_from_slice(&(val.len() as u32).to_le_bytes());
            buf.extend_from_slice(val);
        }
    }
    buf
}

#[test]
fn apply_batch_via_ffi() {
    let dir = tempfile::tempdir().unwrap();
    let dir_c = c(dir.path().to_str().unwrap());
    let id = c("tx-ffi");
    let h = unsafe {
        scc_kv_open(
            dir_c.as_ptr(),
            id.as_ptr(),
            true,
            false,
            std::ptr::null(),
            0,
            0,
            0,
        )
    };
    assert!(!h.is_null(), "{:?}", last_error());
    assert_eq!(set(h, "drop", 0, b"x"), 0);
    let packed = pack_batch(&[(1, "keep", 0, b"v"), (0, "drop", 0, b"")]);
    assert_eq!(
        unsafe { scc_kv_apply_batch(h, packed.as_ptr(), packed.len()) },
        0,
        "{:?}",
        last_error()
    );
    assert_eq!(get_owned(h, "keep"), Some((0u8, b"v".to_vec())));
    assert_eq!(get_owned(h, "drop"), None);
    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };
}

#[test]
fn in_memory_accepts_eviction_options() {
    let id = c("evict-mem");
    let h = unsafe { scc_kv_in_memory(id.as_ptr(), 2, 20) };
    assert!(!h.is_null(), "{:?}", last_error());

    for i in 0..6 {
        let key = format!("k{i}");
        assert_eq!(set(h, &key, 0, b"value"), 0);
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(unsafe { scc_kv_len(h) } <= 2);

    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };
}

unsafe extern "C" fn on_change(user_data: *mut std::ffi::c_void, key: *const u8, key_len: usize) {
    let events = unsafe { &*(user_data as *const std::sync::Mutex<Vec<Option<String>>>) };
    let entry = if key.is_null() {
        None
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(key, key_len) };
        Some(String::from_utf8(bytes.to_vec()).unwrap())
    };
    events.lock().unwrap().push(entry);
}

#[test]
fn listener_receives_set_delete_clear_and_batch() {
    use std::sync::{Arc, Mutex};

    let events: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));

    let id_c = c("listener-test");
    let h = unsafe { scc_kv_in_memory(id_c.as_ptr(), 0, 0) };
    let user_data = Arc::as_ptr(&events) as *mut std::ffi::c_void;
    let sub = unsafe { scc_kv_subscribe(h, on_change, user_data) };
    assert!(sub > 0);

    assert_eq!(set(h, "k", 0, b"v"), 0);
    assert_eq!(unsafe { scc_kv_remove(h, "k".as_ptr(), 1) }, 1);
    assert_eq!(unsafe { scc_kv_clear(h) }, 0);
    let packed = pack_entries(&[("p", "1"), ("q", "2")]);
    assert_eq!(
        unsafe { scc_kv_set_many_str(h, packed.as_ptr(), packed.len(), 2) },
        0
    );
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            Some("k".to_string()),
            Some("k".to_string()),
            None,
            Some("p".to_string()),
            Some("q".to_string())
        ]
    );

    assert_eq!(unsafe { scc_kv_unsubscribe(h, sub) }, 1);
    assert_eq!(unsafe { scc_kv_unsubscribe(h, sub) }, 0);
    assert_eq!(set(h, "k", 0, b"v"), 0);
    assert_eq!(events.lock().unwrap().len(), 5);

    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };
}

#[test]
fn listener_sees_writes_from_other_handles() {
    use std::sync::{Arc, Mutex};

    let events: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));

    let dir = tempfile::tempdir().unwrap();
    let dir_c = c(dir.path().to_str().unwrap());
    let id_c = c("shared");
    let h1 = unsafe {
        scc_kv_open(
            dir_c.as_ptr(),
            id_c.as_ptr(),
            false,
            false,
            std::ptr::null(),
            0,
            0,
            0,
        )
    };
    let h2 = unsafe {
        scc_kv_open(
            dir_c.as_ptr(),
            id_c.as_ptr(),
            false,
            false,
            std::ptr::null(),
            0,
            0,
            0,
        )
    };
    let sub =
        unsafe { scc_kv_subscribe(h1, on_change, Arc::as_ptr(&events) as *mut std::ffi::c_void) };
    assert!(sub > 0);

    assert_eq!(set(h2, "cross", 0, b"x"), 0);
    assert_eq!(*events.lock().unwrap(), vec![Some("cross".to_string())]);

    assert_eq!(unsafe { scc_kv_unsubscribe(h1, sub) }, 1);
    assert_eq!(unsafe { scc_kv_close(h1) }, 0);
    unsafe { scc_kv_release(h1) };
    unsafe { scc_kv_release(h2) };
}
