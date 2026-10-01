use compact_str::CompactString;

use super::{BatchOp, Store};
use crate::error::Result;
use crate::record::{self, BatchSub, Op};
use crate::value::Value;
use crate::wal::Log;

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
        })?;
        self.listeners.notify(Some(key));
        Ok(())
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
            apply_set(&self.map, key, value, expires_at_ms);
        })?;
        self.listeners.notify(Some(key));
        Ok(())
    }

    /// Listeners fire per key; unlike `apply_batch`, recovery may keep only a prefix.
    pub fn set_many<'a>(&self, entries: impl Iterator<Item = (&'a str, Value)>) -> Result<()> {
        let entries = entries.collect::<Vec<_>>();
        for (key, value) in &entries {
            record::validate(&Op::Set { key, value })?;
        }
        let collect_keys = self.listeners.is_active();
        let notify_keys = self.mutate_atomically(|mut log| {
            let mut notify_keys = Vec::<CompactString>::new();
            for (key, value) in entries {
                if let Some(log) = log.as_deref_mut() {
                    log.append(&Op::Set { key, value: &value });
                }
                apply_set(&self.map, key, value, 0);
                if collect_keys {
                    notify_keys.push(key.into());
                }
            }
            notify_keys
        })?;
        self.notify_keys(&notify_keys);
        Ok(())
    }

    /// Applies `ops` as one crash-atomic unit while preserving the borrowed API.
    pub fn apply_batch(&self, ops: &[BatchOp]) -> Result<()> {
        if ops.is_empty() {
            return self.mutate(|_| ());
        }
        record::validate(&Op::Batch {
            ops: &batch_subs(ops),
        })?;
        let collect_keys = self.listeners.is_active();
        let notify_keys = self.mutate_atomically(|log| {
            if let Some(log) = log {
                log.append(&Op::Batch {
                    ops: &batch_subs(ops),
                });
            }
            self.apply_ops_borrowed(ops, collect_keys)
        })?;
        self.notify_keys(&notify_keys);
        Ok(())
    }

    /// Applies an owned crash-atomic batch without cloning values into the map.
    pub fn apply_batch_owned(&self, ops: Vec<BatchOp>) -> Result<()> {
        if ops.is_empty() {
            return self.mutate(|_| ());
        }
        record::validate(&Op::Batch {
            ops: &batch_subs(&ops),
        })?;
        let collect_keys = self.listeners.is_active();
        let notify_keys = self.mutate_atomically(|log| {
            if let Some(log) = log {
                log.append(&Op::Batch {
                    ops: &batch_subs(&ops),
                });
            }
            self.apply_ops_owned(ops, collect_keys)
        })?;
        self.notify_keys(&notify_keys);
        Ok(())
    }

    fn apply_ops_borrowed(&self, ops: &[BatchOp], collect: bool) -> Vec<CompactString> {
        let mut notify = Vec::new();
        for op in ops {
            let (key, changed) = match op {
                BatchOp::Set { key, value } => {
                    apply_set(&self.map, key, value.clone(), 0);
                    (key, true)
                }
                BatchOp::Delete { key } => (key, self.map.remove_sync(key).is_some()),
            };
            if collect && changed {
                notify.push(key.as_str().into());
            }
        }
        notify
    }

    fn apply_ops_owned(&self, ops: Vec<BatchOp>, collect: bool) -> Vec<CompactString> {
        let mut notify = Vec::new();
        for op in ops {
            match op {
                BatchOp::Set { key, value } => {
                    if collect {
                        notify.push(key.as_str().into());
                    }
                    apply_set_owned(&self.map, key, value, 0);
                }
                BatchOp::Delete { key } => {
                    let changed = self.map.remove_sync(&key).is_some();
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
        let existed = self.mutate(|log| {
            let existed = self.map.remove_sync(key).is_some();
            if existed && let Some(log) = log {
                log.append(&Op::Delete { key });
            }
            existed
        })?;
        if existed {
            self.listeners.notify(Some(key));
        }
        Ok(existed)
    }

    pub fn clear(&self) -> Result<()> {
        self.mutate(|log| {
            self.map.clear_sync();
            if let Some(log) = log {
                log.append(&Op::Clear);
            }
        })?;
        self.listeners.notify(None);
        Ok(())
    }
}

fn batch_subs(ops: &[BatchOp]) -> Vec<BatchSub<'_>> {
    ops.iter()
        .map(|op| match op {
            BatchOp::Set { key, value } => BatchSub::Set { key, value },
            BatchOp::Delete { key } => BatchSub::Delete { key },
        })
        .collect()
}

fn apply_set(map: &crate::ValueMap, key: &str, value: Value, expires_at_ms: u64) {
    let mut slot = Some(crate::Slot {
        value,
        expires_at_ms,
    });
    let updated = map
        .update_sync(key, |_, existing| {
            *existing = slot.take().expect("slot consumed twice")
        })
        .is_some();
    if !updated {
        match map.entry_sync(key.to_string()) {
            scc::hash_map::Entry::Occupied(mut o) => {
                *o.get_mut() = slot.take().expect("slot consumed twice")
            }
            scc::hash_map::Entry::Vacant(v) => {
                v.insert_entry(slot.take().expect("slot consumed twice"));
            }
        }
    }
}

fn apply_set_owned(map: &crate::ValueMap, key: String, value: Value, expires_at_ms: u64) {
    let mut slot = Some(crate::Slot {
        value,
        expires_at_ms,
    });
    let updated = map
        .update_sync(&key, |_, existing| {
            *existing = slot.take().expect("slot consumed twice")
        })
        .is_some();
    if !updated {
        match map.entry_sync(key) {
            scc::hash_map::Entry::Occupied(mut o) => {
                *o.get_mut() = slot.take().expect("slot consumed twice")
            }
            scc::hash_map::Entry::Vacant(v) => {
                v.insert_entry(slot.take().expect("slot consumed twice"));
            }
        }
    }
}
