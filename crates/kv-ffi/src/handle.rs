use std::sync::Arc;

use kv_core::Store;

pub struct SccKvStore {
    pub(crate) store: Arc<Store>,
}

impl SccKvStore {
    pub(crate) fn into_raw(self) -> *mut SccKvStore {
        Box::into_raw(Box::new(self))
    }

    pub(crate) unsafe fn borrow<'a>(ptr: *mut SccKvStore) -> Option<&'a SccKvStore> {
        unsafe { ptr.as_ref() }
    }
}
