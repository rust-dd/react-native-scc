use std::sync::{Arc, Mutex};

use super::*;

unsafe extern "C" fn on_change(user_data: *mut std::ffi::c_void, key: *const u8, key_len: usize) {
    let events = unsafe { &*(user_data as *const Mutex<Vec<Option<String>>>) };
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
    let events = Arc::<Mutex<Vec<Option<String>>>>::new(Mutex::new(Vec::new()));

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
    let events = Arc::<Mutex<Vec<Option<String>>>>::new(Mutex::new(Vec::new()));

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
