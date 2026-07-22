use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::record::{self, Op};

use super::Writer;

impl Writer {
    pub(super) fn sweep_and_evict(&mut self) {
        let now = crate::now_ms();
        let (expired, evicted) = crate::compute_doomed(&self.map, now, self.cfg.max_entries);
        let mut removed = Vec::new();
        {
            let gate = Arc::clone(&self.cfg.mutation_gate);
            let _mutation = gate.lock().unwrap();
            if self.cfg.closed.load(Ordering::Acquire) {
                return;
            }
            for key in expired {
                // The entry may have been rewritten after the scan.
                if self
                    .map
                    .remove_if_sync(&key, |slot| slot.is_expired(now))
                    .is_some()
                {
                    self.enqueue_sweep_removal(&key);
                    removed.push(key);
                }
            }
            for key in evicted {
                if self.map.remove_sync(&key).is_some() {
                    self.enqueue_sweep_removal(&key);
                    removed.push(key);
                }
            }
        }
        for key in removed {
            self.cfg.listeners.notify(Some(&key));
        }
    }

    fn enqueue_sweep_removal(&self, key: &str) {
        let mut record = Vec::with_capacity(13 + key.len());
        record::encode(&Op::Delete { key }, &mut record);
        let _ = self.tx.send(super::Msg::Append(record));
    }
}
