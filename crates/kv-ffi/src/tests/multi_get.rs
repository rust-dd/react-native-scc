use super::*;

fn pack_keys(keys: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for key in keys {
        out.extend_from_slice(&(key.len() as u32).to_le_bytes());
        out.extend_from_slice(key);
    }
    out
}

fn get_many_raw(
    h: *mut SccKvStore,
    packed: *const u8,
    packed_len: usize,
    count: usize,
) -> (i32, *mut u8, usize) {
    let mut data = std::ptr::dangling_mut::<u8>();
    let mut len = usize::MAX;
    let status = unsafe { scc_kv_get_many_str(h, packed, packed_len, count, &mut data, &mut len) };
    (status, data, len)
}

#[test]
fn packed_multi_get_preserves_order_duplicates_and_optional_strings() {
    let id = c("multi-get-semantics");
    let h = unsafe { scc_kv_in_memory(id.as_ptr(), 0, 0) };
    assert_eq!(set(h, "alpha", 0, b"one"), 0);
    assert_eq!(set(h, "", 0, b"root"), 0);
    assert_eq!(set(h, "empty", 0, b""), 0);
    assert_eq!(set(h, "árvíz", 0, "tükör".as_bytes()), 0);
    assert_eq!(set(h, "number", 1, &42.0f64.to_le_bytes()), 0);
    assert_eq!(set(h, "json", 4, br#"{"ok":true}"#), 0);
    let large = vec![b'z'; 8192];
    assert_eq!(set(h, "large", 0, &large), 0);

    let keys = [
        &b"alpha"[..],
        &b"missing"[..],
        &b"alpha"[..],
        &b"empty"[..],
        &b"number"[..],
        &b"json"[..],
        &b""[..],
        "árvíz".as_bytes(),
        &b"large"[..],
    ];
    let packed = pack_keys(&keys);
    let (status, data, len) = get_many_raw(h, packed.as_ptr(), packed.len(), keys.len());
    assert_eq!(status, 0, "{:?}", last_error());
    assert!(!data.is_null());
    let output = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();
    unsafe { scc_kv_free(data, len) };

    let mut expected = Vec::new();
    for value in [
        Some(&b"one"[..]),
        None,
        Some(&b"one"[..]),
        Some(&b""[..]),
        None,
        None,
        Some(&b"root"[..]),
        Some("tükör".as_bytes()),
        Some(large.as_slice()),
    ] {
        match value {
            Some(value) => {
                expected.extend_from_slice(&(value.len() as u32).to_le_bytes());
                expected.extend_from_slice(value);
            }
            None => expected.extend_from_slice(&u32::MAX.to_le_bytes()),
        }
    }
    assert_eq!(output, expected);

    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };
}

#[test]
fn packed_multi_get_accepts_empty_input() {
    let id = c("multi-get-empty");
    let h = unsafe { scc_kv_in_memory(id.as_ptr(), 0, 0) };
    let (status, data, len) = get_many_raw(h, std::ptr::null(), 0, 0);
    assert_eq!(status, 0, "{:?}", last_error());
    assert!(data.is_null());
    assert_eq!(len, 0);
    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };
}

#[test]
fn packed_multi_get_rejects_malformed_inputs_without_publishing_output() {
    let id = c("multi-get-malformed");
    let h = unsafe { scc_kv_in_memory(id.as_ptr(), 0, 0) };
    let one_key = pack_keys(&[b"key"]);
    let invalid_utf8 = pack_keys(&[&[0xff]]);
    let cases = [
        (&[1, 0, 0][..], 1, "truncated length field"),
        (&[2, 0, 0, 0, b'x'][..], 1, "truncated key"),
        (one_key.as_slice(), 0, "key count mismatch"),
        (one_key.as_slice(), 2, "key count mismatch"),
        (invalid_utf8.as_slice(), 1, "key is not valid UTF-8"),
    ];
    for (packed, count, expected_error) in cases {
        let (status, data, len) = get_many_raw(h, packed.as_ptr(), packed.len(), count);
        assert_eq!(status, -1);
        assert!(data.is_null());
        assert_eq!(len, 0);
        assert!(last_error().unwrap().contains(expected_error));
    }

    let (status, data, len) = get_many_raw(h, std::ptr::null(), 1, 1);
    assert_eq!(status, -1);
    assert!(data.is_null());
    assert_eq!(len, 0);
    assert!(last_error().unwrap().contains("data is null"));

    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };
}

#[test]
fn packed_multi_get_rejects_null_handle_and_output_pointers() {
    let id = c("multi-get-pointers");
    let h = unsafe { scc_kv_in_memory(id.as_ptr(), 0, 0) };
    let packed = pack_keys(&[b"key"]);
    let mut data = std::ptr::null_mut();
    let mut len = 0usize;

    assert_eq!(
        unsafe {
            scc_kv_get_many_str(
                h,
                packed.as_ptr(),
                packed.len(),
                1,
                std::ptr::null_mut(),
                &mut len,
            )
        },
        -1
    );
    assert!(last_error().unwrap().contains("output pointer is null"));
    assert_eq!(
        unsafe {
            scc_kv_get_many_str(
                h,
                packed.as_ptr(),
                packed.len(),
                1,
                &mut data,
                std::ptr::null_mut(),
            )
        },
        -1
    );
    assert!(last_error().unwrap().contains("output pointer is null"));

    let (status, data, len) = get_many_raw(std::ptr::null_mut(), packed.as_ptr(), packed.len(), 1);
    assert_eq!(status, -1);
    assert!(data.is_null());
    assert_eq!(len, 0);
    assert!(last_error().unwrap().contains("handle is null"));

    assert_eq!(unsafe { scc_kv_close(h) }, 0);
    unsafe { scc_kv_release(h) };
}
