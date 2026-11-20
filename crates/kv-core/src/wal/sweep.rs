use compact_str::CompactString;

use super::writer::{Writer, take_logged};
use crate::record::Op;

impl Writer {
    pub(super) fn sweep_and_evict(&mut self) {
        let now = crate::now_ms();
        let doomed =
            self.ttl_scan
                .doomed(&self.map, &self.cfg.ttl_epoch, self.cfg.max_entries, now);
        for chunk in doomed.expired.chunks(crate::SWEEP_CHUNK) {
            if !self.remove_logged(chunk, |slot| slot.is_expired(now)) {
                return;
            }
        }
        for chunk in doomed.evicted.chunks(crate::SWEEP_CHUNK) {
            if !self.remove_logged(chunk, |_| true) {
                return;
            }
        }
    }

    fn remove_logged(
        &mut self,
        keys: &[CompactString],
        doomed: impl Fn(&crate::Slot) -> bool,
    ) -> bool {
        let mut removed = Vec::new();
        {
            let mut log = self.shared.log.lock().unwrap();
            if log.closed {
                return false;
            }
            for key in keys {
                if self.map.remove_if_sync(key, |slot| doomed(slot)).is_some() {
                    log.append(&Op::Delete { key });
                    removed.push(key);
                }
            }
            take_logged(&mut log, &mut self.pending);
        }
        for key in removed {
            self.cfg.listeners.notify(Some(key.as_str()));
        }
        true
    }
}
