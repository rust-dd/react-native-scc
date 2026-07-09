use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};

use crate::crypto::{self, Cipher};
use crate::error::{Error, Result};
use crate::notify::Listeners;
use crate::record::{self, Op};
use crate::snapshot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
    Relaxed,
    Strict,
}

pub(crate) struct WriterConfig {
    pub wal_path: PathBuf,
    pub snap_path: PathBuf,
    pub durability: Durability,
    pub group_window: Duration,
    pub group_bytes: usize,
    pub fsync_interval: Duration,
    pub compact_min: u64,
    pub cipher: Option<Arc<Cipher>>,
    pub listeners: Arc<Listeners>,
    pub sweep_interval: Duration,
    pub max_entries: Option<usize>,
}

enum Msg {
    Append(Vec<u8>),
    Flush(Sender<std::result::Result<(), String>>),
    Shutdown,
}

pub(crate) struct WalHandle {
    tx: Sender<Msg>,
    pool: Receiver<Vec<u8>>,
    join: Mutex<Option<JoinHandle<()>>>,
    error: Arc<Mutex<Option<String>>>,
}

impl WalHandle {
    pub(crate) fn spawn(
        cfg: WriterConfig,
        map: Arc<crate::ValueMap>,
        wal_len: u64,
        snap_len: u64,
    ) -> Result<WalHandle> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&cfg.wal_path)
            .map_err(|e| Error::Io {
                path: cfg.wal_path.clone(),
                source: e,
            })?;
        let mut wal_len = wal_len;
        if wal_len == 0 {
            let header = crypto::header_bytes(cfg.cipher.is_some());
            file.write_all(&header).map_err(|e| Error::Io {
                path: cfg.wal_path.clone(),
                source: e,
            })?;
            wal_len = header.len() as u64;
        }
        let (tx, rx) = unbounded();
        let (pool_tx, pool_rx) = bounded(64);
        let error = Arc::new(Mutex::new(None));
        let writer = Writer {
            cfg,
            map,
            file,
            error: error.clone(),
            pool_tx,
            wal_len,
            snap_len,
            pending: Vec::new(),
            first_pending: None,
            last_sync: Instant::now(),
            last_sweep: Instant::now(),
            dirty: false,
        };
        let join = std::thread::Builder::new()
            .name("kv-core-wal".into())
            .spawn(move || writer.run(rx))
            .map_err(|e| Error::Io {
                path: PathBuf::new(),
                source: e,
            })?;
        Ok(WalHandle {
            tx,
            pool: pool_rx,
            join: Mutex::new(Some(join)),
            error,
        })
    }

    /// Recycled record buffer from the writer, or a fresh one. Cuts a
    /// malloc/free pair from every mutation once the pool warms up.
    pub(crate) fn take_buffer(&self, capacity: usize) -> Vec<u8> {
        match self.pool.try_recv() {
            Ok(mut buf) => {
                buf.clear();
                buf.reserve(capacity);
                buf
            }
            Err(_) => Vec::with_capacity(capacity),
        }
    }

    pub(crate) fn check(&self) -> Result<()> {
        match self.error.lock().unwrap().clone() {
            Some(msg) => Err(Error::Background(msg)),
            None => Ok(()),
        }
    }

    pub(crate) fn append(&self, rec: Vec<u8>) -> Result<()> {
        self.check()?;
        self.tx.send(Msg::Append(rec)).map_err(|_| Error::Closed)
    }

    pub(crate) fn flush(&self) -> Result<()> {
        let (ack_tx, ack_rx) = bounded(1);
        self.tx
            .send(Msg::Flush(ack_tx))
            .map_err(|_| Error::Closed)?;
        match ack_rx.recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(msg)) => Err(Error::Background(msg)),
            Err(_) => Err(Error::Closed),
        }
    }

    pub(crate) fn shutdown(&self) {
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(join) = self.join.lock().unwrap().take() {
            let _ = join.join();
        }
    }

    #[cfg(test)]
    pub(crate) fn inject_error(&self, msg: &str) {
        *self.error.lock().unwrap() = Some(msg.to_string());
    }
}

struct Writer {
    cfg: WriterConfig,
    map: Arc<crate::ValueMap>,
    file: File,
    error: Arc<Mutex<Option<String>>>,
    pool_tx: Sender<Vec<u8>>,
    wal_len: u64,
    snap_len: u64,
    pending: Vec<u8>,
    first_pending: Option<Instant>,
    last_sync: Instant,
    last_sweep: Instant,
    dirty: bool,
}

impl Writer {
    fn run(mut self, rx: Receiver<Msg>) {
        loop {
            match rx.recv_timeout(self.next_timeout()) {
                Ok(Msg::Append(rec)) => {
                    if self.first_pending.is_none() {
                        self.first_pending = Some(Instant::now());
                    }
                    self.pending.extend_from_slice(&rec);
                    let _ = self.pool_tx.try_send(rec);
                    if self.pending.len() >= self.cfg.group_bytes {
                        self.write_batch();
                    }
                }
                Ok(Msg::Flush(ack)) => {
                    self.write_batch();
                    self.sync();
                    let result = match self.error.lock().unwrap().clone() {
                        Some(msg) => Err(msg),
                        None => Ok(()),
                    };
                    let _ = ack.send(result);
                }
                Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                    self.write_batch();
                    self.sync();
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.tick();
        }
    }

    fn next_timeout(&self) -> Duration {
        let now = Instant::now();
        let mut deadline: Option<Instant> = None;
        if let Some(first) = self.first_pending {
            let d = first + self.cfg.group_window;
            deadline = Some(deadline.map_or(d, |x: Instant| x.min(d)));
        }
        if self.dirty && self.cfg.durability == Durability::Relaxed {
            let d = self.last_sync + self.cfg.fsync_interval;
            deadline = Some(deadline.map_or(d, |x: Instant| x.min(d)));
        }
        match deadline {
            Some(d) => d
                .saturating_duration_since(now)
                .max(Duration::from_millis(1)),
            None => Duration::from_millis(500),
        }
    }

    fn tick(&mut self) {
        if let Some(first) = self.first_pending
            && first.elapsed() >= self.cfg.group_window
        {
            self.write_batch();
        }
        if self.dirty
            && self.cfg.durability == Durability::Relaxed
            && self.last_sync.elapsed() >= self.cfg.fsync_interval
        {
            self.sync();
        }
        if self.wal_len >= self.compact_threshold() {
            self.compact();
        }
        if (self.cfg.max_entries.is_some() || self.last_sweep.elapsed() >= self.cfg.sweep_interval)
            && self.last_sweep.elapsed() >= self.cfg.sweep_interval
        {
            self.sweep_and_evict();
            self.last_sweep = Instant::now();
        }
    }

    /// Reclaims expired keys and, when `max_entries` is set, evicts arbitrary
    /// live keys until the store fits. Deletions are WAL-logged and notified
    /// like any other mutation.
    fn sweep_and_evict(&mut self) {
        let now = crate::now_ms();
        let mut doomed: Vec<String> = Vec::new();
        self.map.iter_sync(|k, slot| {
            if slot.is_expired(now) {
                doomed.push(k.clone());
            }
            doomed.len() < 4096
        });
        if let Some(max) = self.cfg.max_entries {
            let live = self.map.len().saturating_sub(doomed.len());
            if live > max {
                let mut need = live - max;
                self.map.iter_sync(|k, slot| {
                    if !slot.is_expired(now) {
                        doomed.push(k.clone());
                        need -= 1;
                    }
                    need > 0
                });
            }
        }
        for key in doomed {
            if self.map.remove_sync(&key).is_some() {
                if self.first_pending.is_none() {
                    self.first_pending = Some(Instant::now());
                }
                record::encode(&Op::Delete { key: &key }, &mut self.pending);
                self.cfg.listeners.notify(Some(&key));
            }
        }
    }

    fn write_batch(&mut self) {
        if self.pending.is_empty() {
            self.first_pending = None;
            return;
        }
        let batch = std::mem::take(&mut self.pending);
        self.first_pending = None;
        let on_disk: Vec<u8>;
        let bytes: &[u8] = match &self.cfg.cipher {
            Some(cipher) => {
                let mut framed = Vec::with_capacity(batch.len() + 32);
                if let Err(e) = cipher.encrypt_frame(&batch, &mut framed) {
                    self.fail(e.to_string());
                    return;
                }
                on_disk = framed;
                &on_disk
            }
            None => &batch,
        };
        if let Err(e) = self.file.write_all(bytes) {
            self.fail(e.to_string());
            return;
        }
        self.wal_len += bytes.len() as u64;
        self.dirty = true;
        if self.cfg.durability == Durability::Strict {
            self.sync();
        }
    }

    fn sync(&mut self) {
        if !self.dirty {
            return;
        }
        if let Err(e) = self.file.sync_data() {
            self.fail(e.to_string());
            return;
        }
        self.dirty = false;
        self.last_sync = Instant::now();
    }

    fn compact_threshold(&self) -> u64 {
        self.cfg.compact_min.max(2 * self.snap_len)
    }

    fn compact(&mut self) {
        match snapshot::write_atomic(&self.cfg.snap_path, &self.map, self.cfg.cipher.as_deref()) {
            Ok(n) => {
                self.snap_len = n;
                if let Err(e) = self.file.set_len(0) {
                    self.fail(e.to_string());
                    return;
                }
                let header = crypto::header_bytes(self.cfg.cipher.is_some());
                if let Err(e) = self.file.write_all(&header) {
                    self.fail(e.to_string());
                    return;
                }
                if let Err(e) = self.file.sync_all() {
                    self.fail(e.to_string());
                    return;
                }
                self.wal_len = header.len() as u64;
                self.dirty = false;
                self.last_sync = Instant::now();
            }
            Err(e) => self.fail(e.to_string()),
        }
    }

    fn fail(&mut self, msg: String) {
        let mut slot = self.error.lock().unwrap();
        if slot.is_none() {
            *slot = Some(msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{self, DecodeOutcome, Op, OwnedOp};
    use crate::value::Value;

    fn test_cfg(dir: &std::path::Path) -> WriterConfig {
        WriterConfig {
            wal_path: dir.join("t.wal"),
            snap_path: dir.join("t.snap"),
            durability: Durability::Relaxed,
            group_window: Duration::from_millis(5),
            group_bytes: 128 * 1024,
            fsync_interval: Duration::from_millis(50),
            compact_min: u64::MAX,
            cipher: None,
            listeners: Arc::new(Listeners::new()),
            sweep_interval: Duration::from_secs(3600),
            max_entries: None,
        }
    }

    fn encode_set(key: &str, value: &Value) -> Vec<u8> {
        let mut buf = Vec::new();
        record::encode(&Op::Set { key, value }, &mut buf);
        buf
    }

    fn decode_all(data: &[u8]) -> Vec<OwnedOp> {
        let mut ops = Vec::new();
        let mut off = crypto::HEADER_LEN;
        while off < data.len() {
            match record::decode(&data[off..]) {
                DecodeOutcome::Record { op, consumed } => {
                    ops.push(op);
                    off += consumed;
                }
                other => panic!("bad record at {off}: {other:?}"),
            }
        }
        ops
    }

    #[test]
    fn flush_makes_records_durable() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = test_cfg(dir.path());
        let wal_path = cfg.wal_path.clone();
        let handle = WalHandle::spawn(cfg, Arc::new(crate::new_value_map()), 0, 0).unwrap();
        handle.append(encode_set("a", &Value::Num(1.0))).unwrap();
        handle
            .append(encode_set("b", &Value::Str("x".into())))
            .unwrap();
        handle.flush().unwrap();
        let ops = decode_all(&std::fs::read(&wal_path).unwrap());
        assert_eq!(ops.len(), 2);
        assert_eq!(
            ops[0],
            OwnedOp::Set {
                key: "a".into(),
                value: Value::Num(1.0)
            }
        );
        handle.shutdown();
    }

    #[test]
    fn group_window_writes_without_flush() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = test_cfg(dir.path());
        let wal_path = cfg.wal_path.clone();
        let handle = WalHandle::spawn(cfg, Arc::new(crate::new_value_map()), 0, 0).unwrap();
        handle.append(encode_set("k", &Value::Bool(true))).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let ops = decode_all(&std::fs::read(&wal_path).unwrap());
        assert_eq!(ops.len(), 1);
        handle.shutdown();
    }

    #[test]
    fn shutdown_drains_pending() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = test_cfg(dir.path());
        let wal_path = cfg.wal_path.clone();
        let handle = WalHandle::spawn(cfg, Arc::new(crate::new_value_map()), 0, 0).unwrap();
        for i in 0..10 {
            handle
                .append(encode_set(&format!("k{i}"), &Value::Num(i as f64)))
                .unwrap();
        }
        handle.shutdown();
        assert_eq!(decode_all(&std::fs::read(&wal_path).unwrap()).len(), 10);
    }

    #[test]
    fn sticky_error_rejects_appends_and_flush() {
        let dir = tempfile::tempdir().unwrap();
        let handle =
            WalHandle::spawn(test_cfg(dir.path()), Arc::new(crate::new_value_map()), 0, 0).unwrap();
        handle.inject_error("disk full");
        assert!(matches!(
            handle.append(encode_set("k", &Value::Num(1.0))),
            Err(Error::Background(msg)) if msg == "disk full"
        ));
        assert!(matches!(handle.flush(), Err(Error::Background(_))));
        handle.shutdown();
    }

    #[test]
    fn compaction_truncates_wal_and_writes_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = test_cfg(dir.path());
        cfg.compact_min = 256;
        let wal_path = cfg.wal_path.clone();
        let snap_path = cfg.snap_path.clone();
        let map = Arc::new(crate::new_value_map());
        let _ = map.insert_sync("final".to_string(), crate::slot(Value::Str("state".into())));
        let handle = WalHandle::spawn(cfg, map.clone(), 0, 0).unwrap();
        for i in 0..50 {
            handle
                .append(encode_set("hot", &Value::Num(i as f64)))
                .unwrap();
            handle.flush().unwrap();
        }
        handle.shutdown();
        assert!(std::fs::metadata(&wal_path).unwrap().len() < 256 + crypto::HEADER_LEN as u64);
        let loaded = crate::new_value_map();
        snapshot::load(&snap_path, &loaded, None).unwrap();
        assert_eq!(
            loaded.read_sync("final", |_, s| s.value.clone()),
            Some(Value::Str("state".into()))
        );
    }
}
