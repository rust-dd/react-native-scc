use super::*;

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
            0,
            60_000,
        )
    };
    assert!(!h.is_null(), "{:?}", last_error());

    for i in 0..50 {
        let key = format!("keep{i}");
        assert_eq!(set(h, &key, 0, b"live"), 0);
    }
    for i in 0..5000 {
        let key = format!("tmp{i}");
        assert_eq!(
            unsafe { scc_kv_set_ttl(h, key.as_ptr(), key.len(), 0, b"x".as_ptr(), 1, 1) },
            0
        );
    }

    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };
    std::thread::sleep(std::time::Duration::from_millis(20));

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
