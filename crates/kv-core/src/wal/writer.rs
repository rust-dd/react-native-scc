use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::framing::write_encrypted_frames;
use super::{Durability, FlushAck, Log, Shared, WriterConfig};
use crate::error::Error;
use crate::record;

const MAX_RETAINED_BUFFER_CAPACITY: usize = 256 * 1024;

pub(super) struct Writer {
    pub(super) cfg: WriterConfig,
    pub(super) shared: Arc<Shared>,
    pub(super) map: Arc<crate::ValueMap>,
    pub(super) file: File,
    pub(super) wal_len: u64,
    pub(super) snap_len: u64,
    pub(super) pending: Vec<u8>,
    last_write: Option<Instant>,
    last_input: Instant,
    pub(super) last_sync: Instant,
    last_sweep: Instant,
    pub(super) dirty: bool,
}

impl Writer {
    pub(super) fn new(
        cfg: WriterConfig,
        shared: Arc<Shared>,
        map: Arc<crate::ValueMap>,
        file: File,
        wal_len: u64,
        snap_len: u64,
    ) -> Writer {
        let now = Instant::now();
        Writer {
            cfg,
            shared,
            map,
            file,
            wal_len,
            snap_len,
            pending: Vec::new(),
            last_write: None,
            last_input: now,
            last_sync: now,
            last_sweep: now,
            dirty: false,
        }
    }

    pub(super) fn run(mut self) {
        loop {
            let (closed, flushes) = self.collect();
            let urgent =
                closed || !flushes.is_empty() || self.pending.len() >= self.cfg.group_bytes;
            if (urgent || self.window_elapsed()) && !self.writes_held(urgent) {
                self.write_pending();
            }
            if closed || !flushes.is_empty() {
                self.sync();
                let result = match self.shared.error.lock().unwrap().clone() {
                    Some(msg) => Err(msg),
                    None => Ok(()),
                };
                for ack in flushes {
                    let _ = ack.send(result.clone());
                }
            }
            if closed {
                return;
            }
            self.maintain();
            if self.crashed() {
                return;
            }
            self.park();
        }
    }

    fn collect(&mut self) -> (bool, Vec<FlushAck>) {
        let mut log = self.shared.log.lock().unwrap();
        if take_logged(&mut log, &mut self.pending) {
            self.last_input = Instant::now();
        }
        (log.closed, std::mem::take(&mut log.flushes))
    }

    fn window_elapsed(&self) -> bool {
        !self.pending.is_empty()
            && self
                .last_write
                .is_none_or(|at| at.elapsed() >= self.cfg.group_window)
    }

    pub(super) fn write_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        record::seal(&mut self.pending);
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
        self.pending.clear();
        if self.pending.capacity() > MAX_RETAINED_BUFFER_CAPACITY {
            self.pending = Vec::new();
        }
        self.last_write = Some(Instant::now());
        match result {
            Ok(written) => {
                self.wal_len += written;
                self.dirty = true;
                if self.cfg.durability == Durability::Strict {
                    self.sync();
                }
            }
            Err(error) => self.fail(error.to_string()),
        }
    }

    pub(super) fn sync(&mut self) {
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

    fn maintain(&mut self) {
        if self.dirty
            && self.cfg.durability == Durability::Relaxed
            && self.last_sync.elapsed() >= self.cfg.fsync_interval
        {
            self.sync();
        }
        if self.wal_len >= self.compact_threshold() || self.compaction_requested() {
            self.compact();
        }
        if self.last_sweep.elapsed() >= self.cfg.sweep_interval {
            self.sweep_and_evict();
            self.last_sweep = Instant::now();
        }
    }

    fn park(&mut self) {
        if self.pending.is_empty()
            && self.shared.armed.load(Ordering::Acquire)
            && self.last_input.elapsed() >= self.cfg.linger
        {
            // Disarm before the last look, so a later append sees it through the mutex and wakes us.
            self.shared.armed.store(false, Ordering::Release);
            let log = self.shared.log.lock().unwrap();
            if !log.bytes.is_empty() || !log.flushes.is_empty() || log.closed {
                return;
            }
        }
        std::thread::park_timeout(self.next_timeout());
    }

    fn next_timeout(&self) -> Duration {
        let now = Instant::now();
        let mut deadline = self.last_sweep.checked_add(self.cfg.sweep_interval);
        if !self.pending.is_empty() {
            let due = self
                .last_write
                .and_then(|at| at.checked_add(self.cfg.group_window))
                .unwrap_or(now);
            deadline = earliest(deadline, Some(due));
        }
        if self.shared.armed.load(Ordering::Acquire) {
            deadline = earliest(deadline, now.checked_add(self.cfg.group_window));
        }
        if self.dirty && self.cfg.durability == Durability::Relaxed {
            deadline = earliest(
                deadline,
                self.last_sync.checked_add(self.cfg.fsync_interval),
            );
        }
        match deadline {
            Some(at) => at
                .saturating_duration_since(now)
                .max(Duration::from_millis(1)),
            None => Duration::from_secs(3600),
        }
    }

    pub(super) fn fail(&mut self, msg: String) {
        let mut slot = self.shared.error.lock().unwrap();
        if slot.is_none() {
            *slot = Some(msg);
        }
        self.shared.failed.store(true, Ordering::Release);
    }

    #[cfg(test)]
    fn writes_held(&self, urgent: bool) -> bool {
        !urgent && self.shared.hooks.hold_writes.load(Ordering::Acquire)
    }

    #[cfg(not(test))]
    fn writes_held(&self, _urgent: bool) -> bool {
        false
    }

    #[cfg(test)]
    fn compaction_requested(&self) -> bool {
        self.shared.hooks.compact_now.swap(false, Ordering::AcqRel)
    }

    #[cfg(not(test))]
    fn compaction_requested(&self) -> bool {
        false
    }

    #[cfg(test)]
    fn crashed(&self) -> bool {
        self.shared.hooks.crashed.load(Ordering::Acquire)
    }

    #[cfg(not(test))]
    fn crashed(&self) -> bool {
        false
    }
}

pub(super) fn take_logged(log: &mut Log, pending: &mut Vec<u8>) -> bool {
    if log.bytes.is_empty() {
        return false;
    }
    if pending.is_empty() {
        std::mem::swap(pending, &mut log.bytes);
    } else {
        pending.extend_from_slice(&log.bytes);
        log.bytes.clear();
    }
    true
}

fn earliest(current: Option<Instant>, candidate: Option<Instant>) -> Option<Instant> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => Some(current.min(candidate)),
        (current, candidate) => current.or(candidate),
    }
}
