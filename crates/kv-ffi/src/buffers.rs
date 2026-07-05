/// Leaks a Vec as (ptr, len) for C. Freed by `scc_kv_free`.
pub(crate) fn vec_to_raw(v: Vec<u8>) -> (*mut u8, usize) {
    let boxed = v.into_boxed_slice();
    let len = boxed.len();
    if len == 0 {
        return (std::ptr::null_mut(), 0);
    }
    (Box::into_raw(boxed) as *mut u8, len)
}

/// Frees a buffer returned by `scc_kv_get` / `scc_kv_keys`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
    }
}
