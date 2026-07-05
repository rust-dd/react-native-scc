use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::FastState;

pub(crate) type Listener = Box<dyn Fn(Option<&str>) + Send + Sync>;

pub(crate) struct Listeners {
    next_id: AtomicU64,
    count: AtomicUsize,
    map: scc::HashMap<u64, Listener, FastState>,
}

impl Listeners {
    pub(crate) fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            count: AtomicUsize::new(0),
            map: scc::HashMap::with_hasher(FastState::default()),
        }
    }

    pub(crate) fn add(&self, f: Listener) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let _ = self.map.insert_sync(id, f);
        self.count.fetch_add(1, Ordering::Release);
        id
    }

    pub(crate) fn remove(&self, id: u64) -> bool {
        let removed = self.map.remove_sync(&id).is_some();
        if removed {
            self.count.fetch_sub(1, Ordering::Release);
        }
        removed
    }

    pub(crate) fn is_active(&self) -> bool {
        self.count.load(Ordering::Acquire) > 0
    }

    pub(crate) fn notify(&self, key: Option<&str>) {
        if self.count.load(Ordering::Acquire) == 0 {
            return;
        }
        self.map.iter_sync(|_, listener| {
            listener(key);
            true
        });
    }
}
