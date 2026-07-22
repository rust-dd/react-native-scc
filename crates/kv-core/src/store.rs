use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;

use crate::crypto::Cipher;
use crate::error::{Error, Result};
use crate::notify::Listeners;
use crate::snapshot;
use crate::value::Value;
use crate::wal::{Durability, WalHandle, WriterConfig};

mod mutation;
mod recovery;

use recovery::replay_wal;

const MUTATION_CLOSED: usize = 1usize << (usize::BITS - 1);
const MUTATION_COUNT_MASK: usize = MUTATION_CLOSED - 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenOptions {
    pub durability: Durability,
    pub recreate: bool,
    pub group_window: Duration,
    pub group_bytes: usize,
    pub fsync_interval: Duration,
    pub compact_min: u64,
    pub encryption_key: Option<[u8; 32]>,
    pub max_entries: Option<usize>,
    pub ttl_sweep_interval: Duration,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            durability: Durability::Relaxed,
            recreate: false,
            group_window: Duration::from_millis(8),
            group_bytes: 128 * 1024,
            fsync_interval: Duration::from_secs(1),
            compact_min: 4 * 1024 * 1024,
            encryption_key: None,
            max_entries: None,
            ttl_sweep_interval: Duration::from_secs(30),
        }
    }
}

/// A single operation in a crash-atomic WAL batch applied via [`Store::apply_batch`].
pub enum BatchOp {
    Set { key: String, value: Value },
    Delete { key: String },
}

pub struct Store {
    map: Arc<crate::ValueMap>,
    listeners: Arc<Listeners>,
    closed: Arc<AtomicBool>,
    wal: Option<WalHandle>,
    sweeper: Option<crate::sweeper::SweeperHandle>,
    /// Read side of the compaction gate: multi-op batches hold it across map
    /// application + WAL append so the writer's compact() (write side) can
    /// never snapshot a half-applied batch and truncate its record away.
    compact_gate: Arc<RwLock<()>>,
    mutation_gate: Option<Arc<Mutex<()>>>,
    ungated_mutations: AtomicUsize,
    close_gate: Mutex<()>,
}

struct MutationGuard<'a> {
    _gate: Option<MutexGuard<'a, ()>>,
    ungated_mutations: Option<&'a AtomicUsize>,
}

impl Drop for MutationGuard<'_> {
    fn drop(&mut self) {
        if let Some(ungated_mutations) = self.ungated_mutations {
            let previous = ungated_mutations.fetch_sub(1, Ordering::Release);
            debug_assert_ne!(previous & MUTATION_COUNT_MASK, 0);
        }
    }
}

impl Store {
    /// Creates a store whose mutations rely on the map's sharded locking.
    /// Closing waits for admitted mutations without serializing normal writes.
    pub fn in_memory() -> Arc<Store> {
        let closed = Arc::new(AtomicBool::new(false));
        Arc::new(Store {
            map: Arc::new(crate::new_value_map()),
            listeners: Arc::new(Listeners::new()),
            closed,
            wal: None,
            sweeper: None,
            compact_gate: Arc::new(RwLock::new(())),
            mutation_gate: None,
            ungated_mutations: AtomicUsize::new(0),
            close_gate: Mutex::new(()),
        })
    }

    /// In-memory store with a background sweeper thread that reclaims expired
    /// keys and, when `max_entries` is set, evicts down to fit.
    pub fn in_memory_evicting(max_entries: Option<usize>, sweep_interval: Duration) -> Arc<Store> {
        let map = Arc::new(crate::new_value_map());
        let listeners = Arc::new(Listeners::new());
        let closed = Arc::new(AtomicBool::new(false));
        let mutation_gate = Arc::new(Mutex::new(()));
        let sweeper = crate::sweeper::spawn(
            map.clone(),
            listeners.clone(),
            max_entries,
            sweep_interval,
            mutation_gate.clone(),
            closed.clone(),
        );
        Arc::new(Store {
            map,
            listeners,
            closed,
            wal: None,
            sweeper: Some(sweeper),
            compact_gate: Arc::new(RwLock::new(())),
            mutation_gate: Some(mutation_gate),
            ungated_mutations: AtomicUsize::new(0),
            close_gate: Mutex::new(()),
        })
    }

    /// Recovered state is snapshot ∘ WAL-replay; records duplicated across the
    /// two are harmless because ops are idempotent and replayed in enqueue order.
    pub fn open(dir: &Path, id: &str, opts: OpenOptions) -> Result<Arc<Store>> {
        std::fs::create_dir_all(dir).map_err(|e| Error::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let snap_path = dir.join(format!("{id}.snap"));
        let wal_path = dir.join(format!("{id}.wal"));
        if opts.recreate {
            for p in [&snap_path, &wal_path] {
                match std::fs::remove_file(p) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        return Err(Error::Io {
                            path: p.clone(),
                            source: e,
                        });
                    }
                }
            }
        }
        let cipher = opts.encryption_key.map(|k| Arc::new(Cipher::new(&k)));
        let listeners = Arc::new(Listeners::new());
        let map = Arc::new(crate::new_value_map());
        let compact_gate = Arc::new(RwLock::new(()));
        let mutation_gate = Arc::new(Mutex::new(()));
        let closed = Arc::new(AtomicBool::new(false));
        let snap_len = snapshot::load(&snap_path, &map, cipher.as_deref())?;
        let wal_len = replay_wal(&wal_path, &map, cipher.as_deref())?;
        let wal = WalHandle::spawn(
            WriterConfig {
                wal_path,
                snap_path,
                durability: opts.durability,
                group_window: opts.group_window,
                group_bytes: opts.group_bytes,
                fsync_interval: opts.fsync_interval,
                compact_min: opts.compact_min,
                cipher,
                listeners: listeners.clone(),
                sweep_interval: opts.ttl_sweep_interval,
                max_entries: opts.max_entries,
                compact_gate: compact_gate.clone(),
                mutation_gate: mutation_gate.clone(),
                closed: closed.clone(),
            },
            map.clone(),
            wal_len,
            snap_len,
        )?;
        Ok(Arc::new(Store {
            map,
            listeners,
            closed,
            wal: Some(wal),
            sweeper: None,
            compact_gate,
            mutation_gate: Some(mutation_gate),
            ungated_mutations: AtomicUsize::new(0),
            close_gate: Mutex::new(()),
        }))
    }

    fn ensure_open(&self) -> Result<()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        Ok(())
    }

    fn begin_mutation(&self) -> Result<MutationGuard<'_>> {
        if let Some(gate) = &self.mutation_gate {
            let guard = gate.lock().unwrap();
            self.ensure_open()?;
            return Ok(MutationGuard {
                _gate: Some(guard),
                ungated_mutations: None,
            });
        }

        let previous = self.ungated_mutations.fetch_add(1, Ordering::AcqRel);
        if previous & MUTATION_CLOSED != 0 {
            self.ungated_mutations.fetch_sub(1, Ordering::Release);
            return Err(Error::Closed);
        }
        assert_ne!(previous & MUTATION_COUNT_MASK, MUTATION_COUNT_MASK);
        Ok(MutationGuard {
            _gate: None,
            ungated_mutations: Some(&self.ungated_mutations),
        })
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        self.with_value(key, Value::clone)
    }

    /// Zero-clone read: runs `f` against the stored value under the bucket
    /// guard. Keep `f` short — it blocks writers to the same bucket.
    /// Expired keys read as missing.
    pub fn with_value<R>(&self, key: &str, f: impl FnOnce(&Value) -> R) -> Option<R> {
        self.map
            .read_sync(key, |_, slot| {
                if slot.expires_at_ms != 0 && crate::now_ms() >= slot.expires_at_ms {
                    None
                } else {
                    Some(f(&slot.value))
                }
            })
            .flatten()
    }

    pub fn contains(&self, key: &str) -> bool {
        self.map
            .read_sync(key, |_, slot| {
                slot.expires_at_ms == 0 || crate::now_ms() < slot.expires_at_ms
            })
            .unwrap_or(false)
    }

    pub fn keys(&self) -> Vec<String> {
        let now = crate::now_ms();
        let mut out = Vec::with_capacity(self.map.len());
        self.map.iter_sync(|k, slot| {
            if !slot.is_expired(now) {
                out.push(k.clone());
            }
            true
        });
        out
    }

    pub fn len(&self) -> usize {
        let now = crate::now_ms();
        let mut len = 0;
        self.map.iter_sync(|_, slot| {
            if !slot.is_expired(now) {
                len += 1;
            }
            true
        });
        len
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn subscribe(&self, f: impl Fn(Option<&str>) + Send + Sync + 'static) -> u64 {
        self.listeners.add(Box::new(f))
    }

    pub fn unsubscribe(&self, id: u64) -> bool {
        self.listeners.remove(id)
    }

    pub fn flush(&self) -> Result<()> {
        self.ensure_open()?;
        match &self.wal {
            Some(wal) => wal.flush(),
            None => Ok(()),
        }
    }

    /// Stops new map and WAL mutations and waits for admitted ones to finish.
    /// Listener callbacks already dispatched by a mutation may finish later.
    pub fn close(&self) -> Result<()> {
        let _close = self.close_gate.lock().unwrap();
        if let Some(gate) = &self.mutation_gate {
            {
                let _mutation = gate.lock().unwrap();
                if self.closed.swap(true, Ordering::AcqRel) {
                    return Ok(());
                }
                if let Some(sweeper) = &self.sweeper {
                    sweeper.signal_stop();
                }
                if let Some(wal) = &self.wal {
                    wal.signal_shutdown();
                }
            }
        } else {
            if self.closed.load(Ordering::Acquire) {
                return Ok(());
            }
            let previous = self
                .ungated_mutations
                .fetch_or(MUTATION_CLOSED, Ordering::AcqRel);
            debug_assert_eq!(previous & MUTATION_CLOSED, 0);
            self.closed.store(true, Ordering::Release);
            while self.ungated_mutations.load(Ordering::Acquire) & MUTATION_COUNT_MASK != 0 {
                std::thread::yield_now();
            }
            if let Some(sweeper) = &self.sweeper {
                sweeper.signal_stop();
            }
            if let Some(wal) = &self.wal {
                wal.signal_shutdown();
            }
        }
        if let Some(sweeper) = &self.sweeper {
            sweeper.join();
        }
        if let Some(wal) = &self.wal {
            wal.join();
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn wal_for_test(&self) -> Option<&WalHandle> {
        self.wal.as_ref()
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests;
