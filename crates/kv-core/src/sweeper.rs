use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::ValueMap;
use crate::notify::Listeners;

/// Owns the in-memory store's background sweep thread. `stop()` (called from
/// `Store::close` and `Drop`) signals shutdown and joins, so the sweeper never
/// mutates or notifies past close.
pub(crate) struct SweeperHandle {
    shutdown: Arc<AtomicBool>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl SweeperHandle {
    pub(crate) fn stop(&self) {
        self.shutdown.store(true, Ordering::Release);
        let handle = self.handle.lock().unwrap().take();
        if let Some(handle) = handle {
            handle.thread().unpark();
            let _ = handle.join();
        }
    }
}

impl Drop for SweeperHandle {
    fn drop(&mut self) {
        self.stop();
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
            let (expired, evicted) = crate::compute_doomed(&map, now, max_entries);
            for key in expired {
                // Re-check under the entry lock: the key may have been
                // rewritten with a live value since the scan judged it expired.
                if map
                    .remove_if_sync(&key, |slot| slot.is_expired(now))
                    .is_some()
                {
                    listeners.notify(Some(&key));
                }
            }
            for key in evicted {
                if map.remove_sync(&key).is_some() {
                    listeners.notify(Some(&key));
                }
            }
        }
    });
    SweeperHandle {
        shutdown,
        handle: Mutex::new(Some(handle)),
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

    #[test]
    fn close_stops_the_sweeper() {
        let s = Store::in_memory_evicting(Some(2), Duration::from_millis(200));
        for i in 0..6 {
            s.set(&format!("k{i}"), Value::Num(i as f64)).unwrap();
        }
        s.close().unwrap();
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(s.len(), 6, "sweeper must not mutate a closed store");
    }
}
