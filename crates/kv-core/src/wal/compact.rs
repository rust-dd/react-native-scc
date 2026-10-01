use std::io::Write;
use std::sync::Arc;
use std::time::Instant;

use super::writer::{Writer, take_logged};
use crate::crypto;
use crate::snapshot;

impl Writer {
    pub(super) fn compact_threshold(&self) -> u64 {
        self.cfg.compact_min.max(2 * self.snap_len)
    }

    pub(super) fn compact(&mut self) {
        let gate = Arc::clone(&self.cfg.compact_gate);
        let records = {
            let _gate = gate.write().unwrap();
            // A kill after the snapshot rename replays this WAL over it, so queued batches go in first.
            let mut log = self.shared.log.lock().unwrap();
            take_logged(&mut log, &mut self.pending);
            drop(log);
            self.write_pending();
            snapshot::capture(&self.map)
        };
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
        let header = crypto::header_bytes(self.cfg.cipher.is_some());
        let truncated = self
            .file
            .set_len(0)
            .and_then(|()| self.file.write_all(&header))
            .and_then(|()| self.file.sync_all());
        match truncated {
            Ok(()) => {
                self.wal_len = header.len() as u64;
                self.dirty = false;
                self.last_sync = Instant::now();
            }
            Err(e) => self.fail(e.to_string()),
        }
    }

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
