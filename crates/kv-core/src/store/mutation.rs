use compact_str::CompactString;

use super::{BatchOp, Store};
use crate::error::Result;
use crate::record::{self, Op};
use crate::value::Value;

impl Store {
    pub fn set(&self, key: &str, value: Value) -> Result<()> {
        let op = Op::Set { key, value: &value };
        record::validate(&op)?;
        let rec = match &self.wal {
            Some(wal) => {
                wal.check()?;
                let mut buf = wal.take_buffer(14 + key.len() + value_len(&value));
                record::encode(&op, &mut buf);
                Some(buf)
            }
            None => None,
        };
        {
            let _mutation = self.begin_mutation()?;
            if let Some(wal) = &self.wal {
                wal.check()?;
            }
            apply_set(&self.map, key, value, 0);
            if let (Some(wal), Some(rec)) = (&self.wal, rec) {
                wal.append(rec)?;
            }
        }
        self.listeners.notify(Some(key));
        Ok(())
    }

    /// Like `set`, but the key expires `ttl_ms` from now. Expired keys read
    /// as missing immediately; the background sweeper reclaims them.
    pub fn set_with_ttl(&self, key: &str, value: Value, ttl_ms: u64) -> Result<()> {
        let expires_at_ms = crate::now_ms().saturating_add(ttl_ms);
        let op = Op::SetTtl {
            key,
            value: &value,
            expires_at_ms,
        };
        record::validate(&op)?;
        let rec = match &self.wal {
            Some(wal) => {
                wal.check()?;
                let mut buf = wal.take_buffer(22 + key.len() + value_len(&value));
                record::encode(&op, &mut buf);
                Some(buf)
            }
            None => None,
        };
        {
            let _mutation = self.begin_mutation()?;
            if let Some(wal) = &self.wal {
                wal.check()?;
            }
            apply_set(&self.map, key, value, expires_at_ms);
            if let (Some(wal), Some(rec)) = (&self.wal, rec) {
                wal.append(rec)?;
            }
        }
        self.listeners.notify(Some(key));
        Ok(())
    }

    /// Batch write: all records land in one WAL append (a single channel
    /// send), listeners fire per key. Values are applied in iteration order.
    pub fn set_many<'a>(&self, entries: impl Iterator<Item = (&'a str, Value)>) -> Result<()> {
        let entries = entries.collect::<Vec<_>>();
        for (key, value) in &entries {
            record::validate(&Op::Set { key, value })?;
        }
        let mut rec = match &self.wal {
            Some(wal) => {
                wal.check()?;
                let mut buf = wal.take_buffer(256);
                for (key, value) in &entries {
                    record::encode(&Op::Set { key, value }, &mut buf);
                }
                Some(buf)
            }
            None => None,
        };
        let collect_keys = self.listeners.is_active();
        let mut notify_keys = Vec::<CompactString>::new();
        {
            let _mutation = self.begin_mutation()?;
            if let Some(wal) = &self.wal {
                wal.check()?;
            }
            let _compaction = self.wal.as_ref().map(|_| self.compact_gate.read().unwrap());
            for (key, value) in entries {
                apply_set(&self.map, key, value, 0);
                if collect_keys {
                    notify_keys.push(key.into());
                }
            }
            if let (Some(wal), Some(buf)) = (&self.wal, rec.take())
                && !buf.is_empty()
            {
                wal.append(buf)?;
            }
        }
        for key in &notify_keys {
            self.listeners.notify(Some(key));
        }
        Ok(())
    }

    /// Applies `ops` as one crash-atomic unit while preserving the borrowed API.
    pub fn apply_batch(&self, ops: &[BatchOp]) -> Result<()> {
        if ops.is_empty() {
            let _mutation = self.begin_mutation()?;
            return Ok(());
        }
        let rec = self.encode_batch(ops)?;
        let collect_keys = self.listeners.is_active();
        let mut notify_keys = Vec::<CompactString>::new();
        {
            let _mutation = self.begin_mutation()?;
            if let Some(wal) = &self.wal {
                wal.check()?;
            }
            let _compaction = self.wal.as_ref().map(|_| self.compact_gate.read().unwrap());
            self.apply_ops_borrowed(ops, collect_keys, &mut notify_keys);
            if let (Some(wal), Some(rec)) = (&self.wal, rec) {
                wal.append(rec)?;
            }
        }
        self.notify_keys(&notify_keys);
        Ok(())
    }

    /// Applies an owned crash-atomic batch without cloning values into the map.
    pub fn apply_batch_owned(&self, ops: Vec<BatchOp>) -> Result<()> {
        if ops.is_empty() {
            let _mutation = self.begin_mutation()?;
            return Ok(());
        }
        let rec = self.encode_batch(&ops)?;
        let collect_keys = self.listeners.is_active();
        let mut notify_keys = Vec::<CompactString>::new();
        {
            let _mutation = self.begin_mutation()?;
            if let Some(wal) = &self.wal {
                wal.check()?;
            }
            let _compaction = self.wal.as_ref().map(|_| self.compact_gate.read().unwrap());
            self.apply_ops_owned(ops, collect_keys, &mut notify_keys);
            if let (Some(wal), Some(rec)) = (&self.wal, rec) {
                wal.append(rec)?;
            }
        }
        self.notify_keys(&notify_keys);
        Ok(())
    }

    fn encode_batch(&self, ops: &[BatchOp]) -> Result<Option<Vec<u8>>> {
        let subs = ops
            .iter()
            .map(|op| match op {
                BatchOp::Set { key, value } => record::BatchSub::Set { key, value },
                BatchOp::Delete { key } => record::BatchSub::Delete { key },
            })
            .collect::<Vec<_>>();
        let op = Op::Batch { ops: &subs };
        record::validate(&op)?;
        match &self.wal {
            Some(wal) => {
                wal.check()?;
                let mut buf = wal.take_buffer(256);
                record::encode(&op, &mut buf);
                Ok(Some(buf))
            }
            None => Ok(None),
        }
    }

    fn apply_ops_borrowed(&self, ops: &[BatchOp], collect: bool, notify: &mut Vec<CompactString>) {
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
    }

    fn apply_ops_owned(&self, ops: Vec<BatchOp>, collect: bool, notify: &mut Vec<CompactString>) {
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
    }

    fn notify_keys(&self, keys: &[CompactString]) {
        for key in keys {
            self.listeners.notify(Some(key));
        }
    }

    pub fn delete(&self, key: &str) -> Result<bool> {
        let op = Op::Delete { key };
        record::validate(&op)?;
        let rec = match &self.wal {
            Some(wal) => {
                wal.check()?;
                let mut buf = wal.take_buffer(13 + key.len());
                record::encode(&op, &mut buf);
                Some(buf)
            }
            None => None,
        };
        let existed = {
            let _mutation = self.begin_mutation()?;
            if let Some(wal) = &self.wal {
                wal.check()?;
            }
            let existed = self.map.remove_sync(key).is_some();
            if existed && let (Some(wal), Some(rec)) = (&self.wal, rec) {
                wal.append(rec)?;
            }
            existed
        };
        if existed {
            self.listeners.notify(Some(key));
        }
        Ok(existed)
    }

    pub fn clear(&self) -> Result<()> {
        let rec = match &self.wal {
            Some(wal) => {
                wal.check()?;
                let mut buf = wal.take_buffer(13);
                record::encode(&Op::Clear, &mut buf);
                Some(buf)
            }
            None => None,
        };
        {
            let _mutation = self.begin_mutation()?;
            if let Some(wal) = &self.wal {
                wal.check()?;
            }
            self.map.clear_sync();
            if let (Some(wal), Some(rec)) = (&self.wal, rec) {
                wal.append(rec)?;
            }
        }
        self.listeners.notify(None);
        Ok(())
    }
}

fn value_len(value: &Value) -> usize {
    match value {
        Value::Str(s) | Value::Json(s) => s.len(),
        Value::Num(_) => 8,
        Value::Bool(_) => 1,
        Value::Bytes(b) => b.len(),
    }
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
