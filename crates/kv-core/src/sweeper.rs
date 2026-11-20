use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::ValueMap;
use crate::error::{Error, Result};
use crate::notify::Listeners;

/// `stop()` (from `Store::close` and `Drop`) signals and joins, so the sweeper never mutates or
/// notifies past close.
pub(crate) struct SweeperHandle {
    shutdown: Arc<AtomicBool>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl SweeperHandle {
    pub(crate) fn signal_stop(&self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(handle) = self.handle.lock().unwrap().as_ref() {
            handle.thread().unpark();
        }
    }

    pub(crate) fn join(&self) {
        let handle = self.handle.lock().unwrap().take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }

    pub(crate) fn stop(&self) {
        self.signal_stop();
        self.join();
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
    mutation_gate: Arc<Mutex<()>>,
    closed: Arc<AtomicBool>,
    ttl_epoch: Arc<AtomicU64>,
) -> Result<SweeperHandle> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let stop = shutdown.clone();
    let spawned = std::thread::Builder::new()
        .name("kv-core-sweeper".into())
        .spawn(move || {
            let mut scan = crate::TtlScan::default();
            loop {
                std::thread::park_timeout(sweep_interval);
                if stop.load(Ordering::Acquire) {
                    break;
                }
                let now = crate::now_ms();
                let doomed = scan.doomed(&map, &ttl_epoch, max_entries, now);
                let chunks = doomed
                    .expired
                    .chunks(crate::SWEEP_CHUNK)
                    .map(|chunk| (chunk, true))
                    .chain(
                        doomed
                            .evicted
                            .chunks(crate::SWEEP_CHUNK)
                            .map(|chunk| (chunk, false)),
                    );
                for (chunk, expiring) in chunks {
                    {
                        let _mutation = mutation_gate.lock().unwrap();
                        if stop.load(Ordering::Acquire) || closed.load(Ordering::Acquire) {
                            return;
                        }
                        for key in chunk {
                            if map
                                .remove_if_sync(key, |slot| !expiring || slot.is_expired(now))
                                .is_some()
                            {
                                listeners.notify(Some(key.as_str()));
                            }
                        }
                    }
                    std::thread::sleep(crate::SWEEP_PAUSE);
                }
            }
        });
    let handle = spawned.map_err(|source| Error::Io {
        path: PathBuf::new(),
        source,
    })?;
    Ok(SweeperHandle {
        shutdown,
        handle: Mutex::new(Some(handle)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Store, Value};

    #[test]
    fn in_memory_sweeper_evicts_above_max() {
        let s = Store::in_memory_evicting(Some(2), Duration::from_millis(20)).unwrap();
        for i in 0..6 {
            s.set(&format!("k{i}"), Value::Num(i as f64)).unwrap();
        }
        std::thread::sleep(Duration::from_millis(250));
        assert!(s.len() <= 2, "expected <= 2 live, got {}", s.len());
    }

    #[test]
    fn ttl_scan_skips_until_the_epoch_moves() {
        let map = crate::new_value_map();
        let _ = map.insert_sync("plain".into(), crate::slot(Value::Num(1.0)));
        let epoch = AtomicU64::new(0);
        let mut scan = crate::TtlScan::default();
        assert!(scan.doomed(&map, &epoch, None, 10).expired.is_empty());

        let expiring = crate::Slot {
            value: Value::Num(1.0),
            expires_at_ms: 5,
        };
        let _ = map.insert_sync("ttl".into(), expiring);
        assert!(scan.doomed(&map, &epoch, None, 10).expired.is_empty());
        epoch.fetch_add(1, Ordering::Release);
        assert_eq!(scan.doomed(&map, &epoch, None, 10).expired.len(), 1);
    }

    #[test]
    fn close_stops_the_sweeper() {
        let s = Store::in_memory_evicting(Some(2), Duration::from_millis(200)).unwrap();
        for i in 0..6 {
            s.set(&format!("k{i}"), Value::Num(i as f64)).unwrap();
        }
        s.close().unwrap();
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(s.len(), 6, "sweeper must not mutate a closed store");
    }
}
