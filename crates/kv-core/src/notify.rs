use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

pub(crate) type Listener = Box<dyn Fn(Option<&str>) + Send + Sync>;

type Entries = Arc<Vec<(u64, Arc<Listener>)>>;

/// Copy-on-write, so notify calls listeners without holding a lock a listener could need.
pub(crate) struct Listeners {
    next_id: AtomicU64,
    count: AtomicUsize,
    entries: RwLock<Entries>,
}

impl Listeners {
    pub(crate) fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            count: AtomicUsize::new(0),
            entries: RwLock::new(Arc::new(Vec::new())),
        }
    }

    pub(crate) fn add(&self, f: Listener) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut entries = self.entries.write().unwrap();
        let mut next = entries.as_ref().clone();
        next.push((id, Arc::new(f)));
        self.count.store(next.len(), Ordering::Release);
        *entries = Arc::new(next);
        id
    }

    pub(crate) fn remove(&self, id: u64) -> bool {
        let mut entries = self.entries.write().unwrap();
        let mut next = entries.as_ref().clone();
        let before = next.len();
        next.retain(|(entry, _)| *entry != id);
        let removed = next.len() != before;
        if removed {
            self.count.store(next.len(), Ordering::Release);
            *entries = Arc::new(next);
        }
        removed
    }

    pub(crate) fn is_active(&self) -> bool {
        self.count.load(Ordering::Acquire) > 0
    }

    pub(crate) fn notify(&self, key: Option<&str>) {
        if !self.is_active() {
            return;
        }
        let entries = Arc::clone(&self.entries.read().unwrap());
        for (_, listener) in entries.iter() {
            listener(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn a_listener_can_unsubscribe_and_subscribe_while_notified() {
        let listeners = Arc::new(Listeners::new());
        let ids = Arc::new(Mutex::new(Vec::new()));
        let (inner, seen) = (listeners.clone(), ids.clone());
        let id = listeners.add(Box::new(move |_| {
            let mut seen = seen.lock().unwrap();
            if let Some(id) = seen.pop() {
                inner.remove(id);
                seen.push(inner.add(Box::new(|_| {})));
            }
        }));
        ids.lock().unwrap().push(id);
        listeners.notify(Some("k"));
        assert!(listeners.is_active());
    }
}
