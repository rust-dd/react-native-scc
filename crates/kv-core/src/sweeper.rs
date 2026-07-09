use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::ValueMap;
use crate::notify::Listeners;

/// Owns the in-memory store's background sweep thread. Dropping it stops and
/// joins the thread, so the sweeper never outlives its store.
pub(crate) struct SweeperHandle {
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for SweeperHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            handle.thread().unpark();
            let _ = handle.join();
        }
    }
}

/// Spawns a thread that, every `sweep_interval`, reclaims expired keys and — when
/// `max_entries` is set — evicts down to fit, notifying listeners per removed key.
pub(crate) fn spawn(
    map: Arc<ValueMap>,
    listeners: Arc<Listeners>,
    max_entries: Option<usize>,
    sweep_interval: Duration,
) -> SweeperHandle {
    let shutdown = Arc::new(AtomicBool::new(false));
    let stop = shutdown.clone();
    let handle = std::thread::spawn(move || {
        loop {
            std::thread::park_timeout(sweep_interval);
            if stop.load(Ordering::Acquire) {
                break;
            }
            let now = crate::now_ms();
            for key in crate::compute_doomed(&map, now, max_entries) {
                if map.remove_sync(&key).is_some() {
                    listeners.notify(Some(&key));
                }
            }
        }
    });
    SweeperHandle {
        shutdown,
        handle: Some(handle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Store, Value};

    #[test]
    fn in_memory_sweeper_evicts_above_max() {
        let s = Store::in_memory_evicting(Some(2), Duration::from_millis(20));
        for i in 0..6 {
            s.set(&format!("k{i}"), Value::Num(i as f64)).unwrap();
        }
        std::thread::sleep(Duration::from_millis(250));
        assert!(s.len() <= 2, "expected <= 2 live, got {}", s.len());
    }
}
