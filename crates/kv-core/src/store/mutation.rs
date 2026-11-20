use compact_str::CompactString;

use super::{BatchOp, Store};
use crate::error::Result;
use crate::record::{self, BatchSub, Op};
use crate::value::Value;
use crate::wal::Log;

// Listeners are notified inside each mutation, under the same lock as the write, so events
// from different threads arrive in write order.
impl Store {
    fn mutate<R>(&self, f: impl FnOnce(Option<&mut Log>) -> R) -> Result<R> {
        match &self.wal {
            Some(wal) => wal.mutate(|log| f(Some(log))),
            None => {
                let _mutation = self.begin_mutation()?;
                Ok(f(None))
            }
        }
    }

    fn mutate_atomically<R>(&self, f: impl FnOnce(Option<&mut Log>) -> R) -> Result<R> {
        let _compaction = self.wal.as_ref().map(|_| self.compact_gate.read().unwrap());
        self.mutate(f)
    }

    pub fn set(&self, key: &str, value: Value) -> Result<()> {
        record::validate(&Op::Set { key, value: &value })?;
        self.mutate(|log| {
            if let Some(log) = log {
                log.append(&Op::Set { key, value: &value });
            }
            apply_set(&self.map, key, value, 0);
            self.listeners.notify(Some(key));
        })
    }

    pub fn set_str(&self, key: &str, value: &str) -> Result<()> {
        record::validate(&Op::SetStr { key, value })?;
        self.mutate(|log| {
            if let Some(log) = log {
                log.append(&Op::SetStr { key, value });
            }
            apply_set_str(&self.map, key, value);
            self.listeners.notify(Some(key));
        })
    }

    /// Like `set`, but the key expires `ttl_ms` from now. Expired keys read
    /// as missing immediately; the background sweeper reclaims them.
    pub fn set_with_ttl(&self, key: &str, value: Value, ttl_ms: u64) -> Result<()> {
        let expires_at_ms = crate::now_ms().saturating_add(ttl_ms);
        record::validate(&Op::SetTtl {
            key,
            value: &value,
            expires_at_ms,
        })?;
        self.mutate(|log| {
            if let Some(log) = log {
                log.append(&Op::SetTtl {
                    key,
                    value: &value,
                    expires_at_ms,
                });
            }
            self.ensure_ttl_sweeper();
            apply_set(&self.map, key, value, expires_at_ms);
            self.ttl_epoch
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            self.listeners.notify(Some(key));
        })
    }

    /// Listeners fire per key; unlike `apply_batch`, recovery may keep only a prefix.
    pub fn set_many<'a>(&self, entries: impl Iterator<Item = (&'a str, Value)>) -> Result<()> {
        let entries = entries.collect::<Vec<_>>();
        for (key, value) in &entries {
            record::validate(&Op::Set { key, value })?;
        }
        self.mutate_atomically(|mut log| {
            let collect_keys = self.listeners.is_active();
            let mut notify_keys = Vec::<&str>::new();
            for (key, value) in entries {
                if let Some(log) = log.as_deref_mut() {
                    log.append(&Op::Set { key, value: &value });
                }
                apply_set(&self.map, key, value, 0);
                if collect_keys {
                    notify_keys.push(key);
                }
            }
            for key in notify_keys {
                self.listeners.notify(Some(key));
            }
        })
    }

    /// Applies `ops` as one crash-atomic unit; clones into the owned path that production uses.
    pub fn apply_batch(&self, ops: &[BatchOp]) -> Result<()> {
        self.apply_batch_owned(ops.to_vec())
    }

    /// Applies an owned crash-atomic batch without cloning values into the map.
    pub fn apply_batch_owned(&self, ops: Vec<BatchOp>) -> Result<()> {
        if ops.is_empty() {
            return self.mutate(|_| ());
        }
        record::validate(&Op::Batch {
            ops: &batch_subs(&ops),
        })?;
        self.mutate_atomically(|log| {
            if let Some(log) = log {
                log.append(&Op::Batch {
                    ops: &batch_subs(&ops),
                });
            }
            let changed = self.apply_ops_owned(ops, self.listeners.is_active());
            self.notify_keys(&changed);
        })
    }

    fn apply_ops_owned(&self, ops: Vec<BatchOp>, collect: bool) -> Vec<CompactString> {
        let mut notify = Vec::new();
        for op in ops {
            match op {
                BatchOp::Set { key, value } => {
                    if collect {
                        notify.push(key.as_str().into());
                    }
                    apply_set(&self.map, key, value, 0);
                }
                BatchOp::Delete { key } => {
                    let changed = remove_live(&self.map, &key);
                    if collect && changed {
                        notify.push(key.into());
                    }
                }
            }
        }
        notify
    }

    fn notify_keys(&self, keys: &[CompactString]) {
        for key in keys {
            self.listeners.notify(Some(key));
        }
    }

    pub fn delete(&self, key: &str) -> Result<bool> {
        record::validate(&Op::Delete { key })?;
        self.mutate(|log| {
            let existed = remove_live(&self.map, key);
            if existed {
                if let Some(log) = log {
                    log.append(&Op::Delete { key });
                }
                self.listeners.notify(Some(key));
            }
            existed
        })
    }

    /// Returns how many live keys it removed, counted in the same pass.
    pub fn clear(&self) -> Result<usize> {
        let now = crate::now_ms();
        self.mutate(|log| {
            let mut live = 0usize;
            self.map.retain_sync(|_, slot| {
                live += usize::from(!slot.is_expired(now));
                false
            });
            if let Some(log) = log {
                log.append(&Op::Clear);
            }
            self.listeners.notify(None);
            live
        })
    }

    /// Deletes the keys matching `prefix` at scan time as one crash-atomic batch.
    pub fn delete_prefix(&self, prefix: &str) -> Result<usize> {
        let ops = self
            .keys_with_prefix(prefix)
            .into_iter()
            .map(|key| BatchOp::Delete { key })
            .collect::<Vec<_>>();
        if ops.is_empty() {
            return Ok(0);
        }
        record::validate(&Op::Batch {
            ops: &batch_subs(&ops),
        })?;
        self.mutate_atomically(|log| {
            if let Some(log) = log {
                log.append(&Op::Batch {
                    ops: &batch_subs(&ops),
                });
            }
            let removed = ops
                .iter()
                .filter_map(|op| match op {
                    BatchOp::Delete { key } if remove_live(&self.map, key) => {
                        Some(CompactString::from(key.as_str()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            self.notify_keys(&removed);
            removed.len()
        })
    }
}

/// Removes `key` either way, but an expired entry was already gone to readers.
fn remove_live(map: &crate::ValueMap, key: &str) -> bool {
    map.remove_sync(key)
        .is_some_and(|(_, slot)| !slot.is_expired(crate::now_ms()))
}

fn batch_subs(ops: &[BatchOp]) -> Vec<BatchSub<'_>> {
    ops.iter()
        .map(|op| match op {
            BatchOp::Set { key, value } => BatchSub::Set { key, value },
            BatchOp::Delete { key } => BatchSub::Delete { key },
        })
        .collect()
}

fn apply_set_str(map: &crate::ValueMap, key: &str, value: &str) {
    let updated = map
        .update_sync(key, |_, slot| {
            slot.expires_at_ms = 0;
            match &mut slot.value {
                // Reusing a far larger buffer would pin memory the new value never needs.
                Value::Str(existing) if existing.capacity() <= value.len().max(64) * 2 => {
                    existing.clear();
                    existing.push_str(value);
                }
                other => *other = Value::Str(CompactString::from(value)),
            }
        })
        .is_some();
    if !updated {
        apply_set(map, key, Value::Str(CompactString::from(value)), 0);
    }
}

fn apply_set<K: AsRef<str> + Into<CompactString>>(
    map: &crate::ValueMap,
    key: K,
    value: Value,
    expires_at_ms: u64,
) {
    let mut slot = Some(crate::Slot {
        value,
        expires_at_ms,
    });
    let updated = map
        .update_sync(key.as_ref(), |_, existing| {
            *existing = slot.take().expect("slot consumed twice")
        })
        .is_some();
    if !updated {
        match map.entry_sync(key.into()) {
            scc::hash_map::Entry::Occupied(mut o) => {
                *o.get_mut() = slot.take().expect("slot consumed twice")
            }
            scc::hash_map::Entry::Vacant(v) => {
                v.insert_entry(slot.take().expect("slot consumed twice"));
            }
        }
    }
}
