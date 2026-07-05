use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::crypto::Cipher;
use crate::error::{Error, Result};
use crate::notify::Listeners;
use crate::record::{self, DecodeOutcome, Op};
use crate::snapshot;
use crate::value::Value;
use crate::wal::{Durability, WalHandle, WriterConfig};

#[derive(Clone, Debug)]
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

pub struct Store {
    map: Arc<crate::ValueMap>,
    listeners: Arc<Listeners>,
    closed: AtomicBool,
    wal: Option<WalHandle>,
}

impl Store {
    pub fn in_memory() -> Arc<Store> {
        Arc::new(Store {
            map: Arc::new(crate::new_value_map()),
            listeners: Arc::new(Listeners::new()),
            closed: AtomicBool::new(false),
            wal: None,
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
            },
            map.clone(),
            wal_len,
            snap_len,
        )?;
        Ok(Arc::new(Store {
            map,
            listeners,
            closed: AtomicBool::new(false),
            wal: Some(wal),
        }))
    }

    fn ensure_open(&self) -> Result<()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        Ok(())
    }

    pub fn set(&self, key: &str, value: Value) -> Result<()> {
        self.ensure_open()?;
        let rec = match &self.wal {
            Some(wal) => {
                wal.check()?;
                let hint = match &value {
                    Value::Str(s) | Value::Json(s) => s.len(),
                    Value::Bytes(b) => b.len(),
                    Value::Num(_) => 8,
                    Value::Bool(_) => 1,
                };
                let mut buf = wal.take_buffer(14 + key.len() + hint);
                record::encode(&Op::Set { key, value: &value }, &mut buf);
                Some(buf)
            }
            None => None,
        };
        apply_set(&self.map, key, value, 0);
        if let (Some(wal), Some(rec)) = (&self.wal, rec) {
            wal.append(rec)?;
        }
        self.listeners.notify(Some(key));
        Ok(())
    }

    /// Like `set`, but the key expires `ttl_ms` from now. Expired keys read
    /// as missing immediately; the background sweeper reclaims them.
    pub fn set_with_ttl(&self, key: &str, value: Value, ttl_ms: u64) -> Result<()> {
        self.ensure_open()?;
        let expires_at_ms = crate::now_ms().saturating_add(ttl_ms);
        let rec = match &self.wal {
            Some(wal) => {
                wal.check()?;
                let mut buf = wal.take_buffer(30 + key.len());
                record::encode(
                    &Op::SetTtl {
                        key,
                        value: &value,
                        expires_at_ms,
                    },
                    &mut buf,
                );
                Some(buf)
            }
            None => None,
        };
        apply_set(&self.map, key, value, expires_at_ms);
        if let (Some(wal), Some(rec)) = (&self.wal, rec) {
            wal.append(rec)?;
        }
        self.listeners.notify(Some(key));
        Ok(())
    }

    /// Batch write: all records land in one WAL append (a single channel
    /// send), listeners fire per key. Values are applied in iteration order.
    pub fn set_many<'a>(&self, entries: impl Iterator<Item = (&'a str, Value)>) -> Result<()> {
        self.ensure_open()?;
        let collect_keys = self.listeners.is_active();
        let mut notify_keys: Vec<compact_str::CompactString> = Vec::new();
        match &self.wal {
            Some(wal) => {
                wal.check()?;
                let mut buf = wal.take_buffer(256);
                for (key, value) in entries {
                    record::encode(&Op::Set { key, value: &value }, &mut buf);
                    apply_set(&self.map, key, value, 0);
                    if collect_keys {
                        notify_keys.push(key.into());
                    }
                }
                if !buf.is_empty() {
                    wal.append(buf)?;
                }
            }
            None => {
                for (key, value) in entries {
                    apply_set(&self.map, key, value, 0);
                    if collect_keys {
                        notify_keys.push(key.into());
                    }
                }
            }
        }
        for key in &notify_keys {
            self.listeners.notify(Some(key));
        }
        Ok(())
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

    pub fn delete(&self, key: &str) -> Result<bool> {
        self.ensure_open()?;
        if let Some(wal) = &self.wal {
            wal.check()?;
        }
        let existed = self.map.remove_sync(key).is_some();
        if existed {
            if let Some(wal) = &self.wal {
                let mut buf = wal.take_buffer(13 + key.len());
                record::encode(&Op::Delete { key }, &mut buf);
                wal.append(buf)?;
            }
            self.listeners.notify(Some(key));
        }
        Ok(existed)
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
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn clear(&self) -> Result<()> {
        self.ensure_open()?;
        if let Some(wal) = &self.wal {
            wal.check()?;
        }
        self.map.clear_sync();
        if let Some(wal) = &self.wal {
            let mut buf = wal.take_buffer(13);
            record::encode(&Op::Clear, &mut buf);
            wal.append(buf)?;
        }
        self.listeners.notify(None);
        Ok(())
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

    pub fn close(&self) -> Result<()> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        if let Some(wal) = &self.wal {
            wal.shutdown();
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

// Overwrite via update first: the common hot path skips allocating an owned
// key. Only a fresh insert pays for `to_string`.
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

fn replay_wal(path: &Path, map: &crate::ValueMap, cipher: Option<&Cipher>) -> Result<u64> {
    use crate::crypto::{self, FileFormat, FrameOutcome};

    let Some(mapped) = snapshot::map_file(path)? else {
        return Ok(0);
    };
    let data: &[u8] = &mapped;
    let (format, header_len) = crypto::parse_header(data);
    let encrypted = match format {
        FileFormat::Legacy => false,
        FileFormat::V1 { encrypted } => encrypted,
    };
    snapshot::check_key_matches(path, encrypted, cipher)?;
    let mut offset = header_len;
    if let Some(cipher) = cipher {
        let mut decrypted_any = false;
        while offset < data.len() {
            match cipher.decrypt_frame(&data[offset..]) {
                FrameOutcome::Frame {
                    plaintext,
                    consumed,
                } => {
                    decrypted_any = true;
                    let mut rec_off = 0usize;
                    let mut ok = true;
                    while rec_off < plaintext.len() {
                        match record::decode(&plaintext[rec_off..]) {
                            DecodeOutcome::Record { op, consumed } => {
                                record::apply(map, op);
                                rec_off += consumed;
                            }
                            DecodeOutcome::NeedMore | DecodeOutcome::Corrupt => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if !ok {
                        break;
                    }
                    offset += consumed;
                }
                FrameOutcome::NeedMore => break,
                FrameOutcome::Corrupt => {
                    // An unauthenticatable FIRST frame means the key is wrong
                    // (or the file is hostile) — refuse instead of truncating
                    // the log away. After good frames it is a torn tail.
                    if !decrypted_any {
                        return Err(Error::Crypto(format!(
                            "cannot decrypt {} — wrong encryption key?",
                            path.display()
                        )));
                    }
                    break;
                }
            }
        }
    } else {
        while offset < data.len() {
            match record::decode(&data[offset..]) {
                DecodeOutcome::Record { op, consumed } => {
                    record::apply(map, op);
                    offset += consumed;
                }
                DecodeOutcome::NeedMore | DecodeOutcome::Corrupt => break,
            }
        }
    }
    let total = data.len();
    drop(mapped);
    if offset < total {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|e| Error::Io {
                path: path.to_path_buf(),
                source: e,
            })?;
        file.set_len(offset as u64).map_err(|e| Error::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        file.sync_all().map_err(|e| Error::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
    }
    Ok(offset as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn set_get_typed_values() {
        let s = Store::in_memory();
        s.set("s", Value::Str("x".into())).unwrap();
        s.set("n", Value::Num(1.5)).unwrap();
        s.set("b", Value::Bool(true)).unwrap();
        assert_eq!(s.get("s"), Some(Value::Str("x".into())));
        assert_eq!(s.get("n"), Some(Value::Num(1.5)));
        assert_eq!(s.get("b"), Some(Value::Bool(true)));
        assert_eq!(s.get("missing"), None);
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn overwrite_changes_type() {
        let s = Store::in_memory();
        s.set("k", Value::Num(1.0)).unwrap();
        s.set("k", Value::Str("now a string".into())).unwrap();
        assert_eq!(s.get("k"), Some(Value::Str("now a string".into())));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn delete_contains_keys_clear() {
        let s = Store::in_memory();
        s.set("a", Value::Bool(false)).unwrap();
        s.set("b", Value::Num(2.0)).unwrap();
        assert!(s.contains("a"));
        assert!(s.delete("a").unwrap());
        assert!(!s.delete("a").unwrap());
        assert!(!s.contains("a"));
        let mut keys = s.keys();
        keys.sort();
        assert_eq!(keys, vec!["b".to_string()]);
        s.clear().unwrap();
        assert!(s.is_empty());
    }

    #[test]
    fn listeners_fire_correctly() {
        let s = Store::in_memory();
        let events: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let id = s.subscribe(move |key| {
            sink.lock().unwrap().push(key.map(str::to_string));
        });

        s.set("k1", Value::Num(1.0)).unwrap();
        s.delete("missing").unwrap();
        s.delete("k1").unwrap();
        s.clear().unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            vec![Some("k1".to_string()), Some("k1".to_string()), None]
        );

        assert!(s.unsubscribe(id));
        assert!(!s.unsubscribe(id));
        s.set("k2", Value::Num(2.0)).unwrap();
        assert_eq!(events.lock().unwrap().len(), 3);
    }

    #[test]
    fn closed_store_rejects_mutations_allows_reads() {
        let s = Store::in_memory();
        s.set("k", Value::Num(1.0)).unwrap();
        s.close().unwrap();
        assert!(matches!(s.set("k", Value::Num(2.0)), Err(Error::Closed)));
        assert!(matches!(s.clear(), Err(Error::Closed)));
        assert_eq!(s.get("k"), Some(Value::Num(1.0)));
    }

    fn fast_opts() -> OpenOptions {
        OpenOptions {
            durability: Durability::Strict,
            group_window: Duration::from_millis(1),
            ..OpenOptions::default()
        }
    }

    #[test]
    fn background_error_surfaces_on_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path(), "db", fast_opts()).unwrap();
        s.set("k", Value::Num(1.0)).unwrap();
        s.wal_for_test().unwrap().inject_error("io fail");
        assert!(matches!(
            s.set("k", Value::Num(2.0)),
            Err(Error::Background(_))
        ));
        assert_eq!(s.get("k"), Some(Value::Num(1.0)));
    }
}
