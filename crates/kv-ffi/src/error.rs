use std::cell::RefCell;
use std::ffi::CString;
use std::os::raw::c_char;

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

pub(crate) fn set_error(msg: impl Into<String>) {
    let msg = msg.into();
    let c = CString::new(msg).unwrap_or_else(|_| CString::new("invalid error message").unwrap());
    LAST_ERROR.with(|slot| *slot.borrow_mut() = Some(c));
}

pub(crate) fn clear_error() {
    LAST_ERROR.with(|slot| *slot.borrow_mut() = None);
}

/// Returns the last error on this thread as a heap CString, or NULL.
/// Caller frees with `scc_kv_free_cstring`.
#[unsafe(no_mangle)]
pub extern "C" fn scc_kv_last_error() -> *mut c_char {
    LAST_ERROR.with(|slot| match slot.borrow_mut().take() {
        Some(c) => c.into_raw(),
        None => std::ptr::null_mut(),
    })
}

/// Frees a string returned by `scc_kv_last_error`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_free_cstring(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}
