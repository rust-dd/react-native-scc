use crate::record::Op;

use super::writer::{Writer, take_logged};

impl Writer {
    pub(super) fn sweep_and_evict(&mut self) {
        let now = crate::now_ms();
        let (expired, evicted) = crate::compute_doomed(&self.map, now, self.cfg.max_entries);
        if expired.is_empty() && evicted.is_empty() {
            return;
        }
        let mut removed = Vec::new();
        {
            let mut log = self.shared.log.lock().unwrap();
            if log.closed {
                return;
            }
            for key in expired {
                // The entry may have been rewritten after the scan.
                if self
                    .map
                    .remove_if_sync(&key, |slot| slot.is_expired(now))
                    .is_some()
                {
                    log.append(&Op::Delete { key: &key });
                    removed.push(key);
                }
            }
            for key in evicted {
                if self.map.remove_sync(&key).is_some() {
                    log.append(&Op::Delete { key: &key });
                    removed.push(key);
                }
            }
            take_logged(&mut log, &mut self.pending);
        }
        for key in removed {
            self.cfg.listeners.notify(Some(key.as_str()));
        }
    }
}
