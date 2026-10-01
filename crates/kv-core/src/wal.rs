use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{JoinHandle, Thread};
use std::time::Duration;

use crossbeam_channel::{Sender, bounded};

use crate::crypto::{self, Cipher};
use crate::error::{Error, Result};
use crate::notify::Listeners;
use crate::record::{self, Op};

mod compact;
mod framing;
mod sweep;
mod writer;

use writer::Writer;

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
    pub linger: Duration,
    pub cipher: Option<Arc<Cipher>>,
    pub listeners: Arc<Listeners>,
    pub sweep_interval: Duration,
    pub max_entries: Option<usize>,
    pub compact_gate: Arc<RwLock<()>>,
}

type FlushAck = Sender<std::result::Result<(), String>>;

/// Its mutex is also the mutation gate, so log order always matches map order.
#[derive(Default)]
pub(crate) struct Log {
    bytes: Vec<u8>,
    flushes: Vec<FlushAck>,
    closed: bool,
}

impl Log {
    pub(crate) fn append(&mut self, op: &Op) {
        record::encode_unsealed(op, &mut self.bytes);
    }
}

struct Shared {
    log: Mutex<Log>,
    /// True while the writer will poll the log anyway, so appends need not wake it.
    armed: AtomicBool,
    failed: AtomicBool,
    error: Mutex<Option<String>>,
    #[cfg(test)]
    hooks: TestHooks,
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestHooks {
    pub hold_writes: AtomicBool,
    pub compact_now: AtomicBool,
    pub crash_after_snapshot: AtomicBool,
    pub crashed: AtomicBool,
}

pub(crate) struct WalHandle {
    shared: Arc<Shared>,
    writer: Thread,
    join: Mutex<Option<JoinHandle<()>>>,
    group_bytes: usize,
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
        let shared = Arc::new(Shared {
            log: Mutex::new(Log::default()),
            armed: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            error: Mutex::new(None),
            #[cfg(test)]
            hooks: TestHooks::default(),
        });
        let group_bytes = cfg.group_bytes;
        let writer = Writer::new(cfg, shared.clone(), map, file, wal_len, snap_len);
        let join = std::thread::Builder::new()
            .name("kv-core-wal".into())
            .spawn(move || writer.run())
            .map_err(|e| Error::Io {
                path: PathBuf::new(),
                source: e,
            })?;
        Ok(WalHandle {
            shared,
            writer: join.thread().clone(),
            join: Mutex::new(Some(join)),
            group_bytes,
        })
    }

    pub(crate) fn check(&self) -> Result<()> {
        if !self.shared.failed.load(Ordering::Acquire) {
            return Ok(());
        }
        match self.shared.error.lock().unwrap().clone() {
            Some(msg) => Err(Error::Background(msg)),
            None => Ok(()),
        }
    }

    pub(crate) fn mutate<R>(&self, f: impl FnOnce(&mut Log) -> R) -> Result<R> {
        let (result, before, after) = {
            let mut log = self.shared.log.lock().unwrap();
            if log.closed {
                return Err(Error::Closed);
            }
            self.check()?;
            let before = log.bytes.len();
            let result = f(&mut log);
            (result, before, log.bytes.len())
        };
        if after > before {
            self.notify_writer(before, after);
        }
        Ok(result)
    }

    fn notify_writer(&self, before: usize, after: usize) {
        let disarmed = !self.shared.armed.load(Ordering::Acquire)
            && !self.shared.armed.swap(true, Ordering::AcqRel);
        let crossed = before < self.group_bytes && after >= self.group_bytes;
        if disarmed || crossed {
            self.writer.unpark();
        }
    }

    pub(crate) fn flush(&self) -> Result<()> {
        let (ack_tx, ack_rx) = bounded(1);
        {
            let mut log = self.shared.log.lock().unwrap();
            if log.closed {
                return Err(Error::Closed);
            }
            log.flushes.push(ack_tx);
        }
        self.writer.unpark();
        match ack_rx.recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(msg)) => Err(Error::Background(msg)),
            Err(_) => Err(Error::Closed),
        }
    }

    pub(crate) fn close(&self) -> bool {
        let first = {
            let mut log = self.shared.log.lock().unwrap();
            !std::mem::replace(&mut log.closed, true)
        };
        self.writer.unpark();
        first
    }

    pub(crate) fn join(&self) {
        if let Some(join) = self.join.lock().unwrap().take() {
            let _ = join.join();
        }
    }

    #[cfg(test)]
    pub(crate) fn shutdown(&self) {
        self.close();
        self.join();
    }

    #[cfg(test)]
    pub(crate) fn inject_error(&self, msg: &str) {
        *self.shared.error.lock().unwrap() = Some(msg.to_string());
        self.shared.failed.store(true, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn hooks(&self) -> &TestHooks {
        &self.shared.hooks
    }

    #[cfg(test)]
    pub(crate) fn wake(&self) {
        self.writer.unpark();
    }
}

#[cfg(test)]
mod tests;
