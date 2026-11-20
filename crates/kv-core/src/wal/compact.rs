use std::io::Write;
use std::sync::Arc;
use std::time::Instant;

use super::framing::write_encrypted_frames;
use super::writer::{Writer, take_logged};
use crate::error::{Error, Result};
use crate::record;
use crate::snapshot;

impl Writer {
    pub(super) fn compact_threshold(&self) -> u64 {
        self.cfg.compact_min.max(2 * self.snap_len)
    }

    pub(super) fn compact(&mut self) {
        let gate = Arc::clone(&self.cfg.compact_gate);
        let (records, mut tail) = {
            let _gate = gate.write().unwrap();
            // A kill after the snapshot rename replays this WAL over it, so queued batches go in first.
            let mut log = self.shared.log.lock().unwrap();
            take_logged(&mut log, &mut self.pending);
            drop(log);
            self.append_pending();
            let records = snapshot::capture(&self.map);
            self.after_capture();
            // Writes admitted during capture may be in the snapshot out of order, so both WALs replay them.
            let mut tail = Vec::new();
            take_logged(&mut self.shared.log.lock().unwrap(), &mut tail);
            (records, tail)
        };
        record::seal(&mut tail);
        self.pending.extend_from_slice(&tail);
        self.append_pending();
        // Losing an unsynced WAL tail to power loss after the rename would replay a stale log.
        self.sync();
        if self
            .shared
            .failed
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        match snapshot::persist(&self.cfg.snap_path, records, self.cfg.cipher.as_deref()) {
            Ok(written) => self.snap_len = written,
            Err(e) => {
                self.fail(e.to_string());
                return;
            }
        }
        if self.crash_after_snapshot() {
            return;
        }
        match self.replace_wal(&tail) {
            Ok(len) => {
                self.wal_len = len;
                self.dirty = false;
                self.last_sync = Instant::now();
                self.compacted();
            }
            Err(e) => self.fail(e.to_string()),
        }
    }

    fn replace_wal(&mut self, tail: &[u8]) -> Result<u64> {
        let path = self.cfg.wal_path.clone();
        let tmp = path.with_extension("wal.tmp");
        let io = |p: &std::path::Path| {
            let p = p.to_path_buf();
            move |source| Error::Io { path: p, source }
        };
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&tmp)
            .map_err(io(&tmp))?;
        file.set_len(0).map_err(io(&tmp))?;
        let prefix = super::fresh_prefix(self.cfg.cipher.as_deref())?;
        file.write_all(&prefix).map_err(io(&tmp))?;
        let body = match &self.cfg.cipher {
            Some(cipher) => write_encrypted_frames(&mut file, &tmp, cipher, tail)?,
            None => {
                file.write_all(tail).map_err(io(&tmp))?;
                tail.len() as u64
            }
        };
        file.sync_all().map_err(io(&tmp))?;
        std::fs::rename(&tmp, &path).map_err(io(&path))?;
        snapshot::sync_parent(&path);
        self.file = file;
        Ok(prefix.len() as u64 + body)
    }

    #[cfg(test)]
    fn after_capture(&self) {
        let hook = self.shared.hooks.after_capture.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
    }

    #[cfg(not(test))]
    fn after_capture(&self) {}

    #[cfg(test)]
    fn compacted(&self) {
        use std::sync::atomic::Ordering;
        self.shared.hooks.compactions.fetch_add(1, Ordering::AcqRel);
    }

    #[cfg(not(test))]
    fn compacted(&self) {}

    #[cfg(test)]
    fn crash_after_snapshot(&self) -> bool {
        use std::sync::atomic::Ordering;
        let hooks = &self.shared.hooks;
        let crash = hooks.crash_after_snapshot.load(Ordering::Acquire);
        if crash {
            hooks.crashed.store(true, Ordering::Release);
        }
        crash
    }

    #[cfg(not(test))]
    fn crash_after_snapshot(&self) -> bool {
        false
    }
}
