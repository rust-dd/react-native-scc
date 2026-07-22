use super::*;

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
