use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};

use crate::crypto::{self, Cipher};
use crate::error::{Error, Result};
use crate::notify::Listeners;
use crate::snapshot;

mod framing;
mod sweep;

use framing::write_encrypted_frames;

const MAX_RETAINED_BUFFER_CAPACITY: usize = 256 * 1024;

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
    pub compact_gate: Arc<RwLock<()>>,
    pub mutation_gate: Arc<Mutex<()>>,
    pub closed: Arc<AtomicBool>,
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
            tx: tx.clone(),
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

    #[cfg(test)]
    pub(crate) fn shutdown(&self) {
        self.signal_shutdown();
        self.join();
    }

    pub(crate) fn signal_shutdown(&self) {
        let _ = self.tx.send(Msg::Shutdown);
    }

    pub(crate) fn join(&self) {
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
    tx: Sender<Msg>,
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
                Ok(Msg::Append(mut rec)) => {
                    if self.first_pending.is_none() {
                        self.first_pending = Some(Instant::now());
                    }
                    if self.pending.is_empty() {
                        std::mem::swap(&mut self.pending, &mut rec);
                    } else {
                        self.pending.extend_from_slice(&rec);
                    }
                    self.recycle(rec);
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
        if let Some(d) = self.last_sweep.checked_add(self.cfg.sweep_interval) {
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

    fn write_batch(&mut self) {
        if self.pending.is_empty() {
            self.first_pending = None;
            return;
        }
        self.first_pending = None;
        let result = match &self.cfg.cipher {
            Some(cipher) => {
                write_encrypted_frames(&mut self.file, &self.cfg.wal_path, cipher, &self.pending)
            }
            None => self
                .file
                .write_all(&self.pending)
                .map(|()| self.pending.len() as u64)
                .map_err(|source| Error::Io {
                    path: self.cfg.wal_path.clone(),
                    source,
                }),
        };
        let written = match result {
            Ok(written) => written,
            Err(error) => {
                self.reset_pending();
                self.fail(error.to_string());
                return;
            }
        };
        self.reset_pending();
        self.wal_len += written;
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

    /// The write lock keeps a snapshot from observing half of an atomic batch.
    fn compact(&mut self) {
        let gate = Arc::clone(&self.cfg.compact_gate);
        let _gate = gate.write().unwrap();
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

    fn recycle(&self, mut buffer: Vec<u8>) {
        if should_retain_capacity(buffer.capacity()) {
            buffer.clear();
            let _ = self.pool_tx.try_send(buffer);
        }
    }

    fn reset_pending(&mut self) {
        self.pending.clear();
        if !should_retain_capacity(self.pending.capacity()) {
            self.pending = Vec::new();
        }
    }
}

fn should_retain_capacity(capacity: usize) -> bool {
    capacity <= MAX_RETAINED_BUFFER_CAPACITY
}

#[cfg(test)]
mod tests;
