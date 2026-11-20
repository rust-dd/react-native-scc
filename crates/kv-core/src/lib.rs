mod crypto;
mod error;
mod notify;
mod record;
mod registry;
mod snapshot;
mod store;
mod sweeper;
mod value;
mod wal;

pub use compact_str::CompactString;
pub use crypto::derive_encryption_key;
pub use error::{Error, Result};
pub use registry::{close_store, in_memory, open_or_get};
pub use store::{BatchOp, OpenOptions, Store};
pub use value::Value;
pub use wal::Durability;

pub(crate) type FastState = foldhash::fast::RandomState;

/// Map slot: value plus expiry. `expires_at_ms == 0` means "never expires",
/// so keys without TTL pay only a branch, never a clock read.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Slot {
    pub value: Value,
    pub expires_at_ms: u64,
}

#[cfg(test)]
pub(crate) fn slot(value: Value) -> Slot {
    Slot {
        value,
        expires_at_ms: 0,
    }
}

impl Slot {
    pub(crate) fn is_expired(&self, now_ms: u64) -> bool {
        self.expires_at_ms != 0 && now_ms >= self.expires_at_ms
    }
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) type ValueMap = scc::HashMap<CompactString, Slot, FastState>;

pub(crate) fn new_value_map() -> ValueMap {
    ValueMap::with_hasher(FastState::default())
}

/// Removals per lock hold, so a large sweep never stalls writers for long.
pub(crate) const SWEEP_CHUNK: usize = 1024;

#[derive(Default)]
pub(crate) struct Doomed {
    /// Re-check expiry at removal time: a concurrent rewrite makes the key live again.
    pub expired: Vec<CompactString>,
    pub evicted: Vec<CompactString>,
    pub saw_ttl: bool,
}

/// Expired keys are capped at 4096 per pass, but the live count stays exact so the cap never
/// inflates eviction. `ttl_free` (no slot can expire) skips the scan.
pub(crate) fn compute_doomed(
    map: &ValueMap,
    now: u64,
    max_entries: Option<usize>,
    ttl_free: bool,
) -> Doomed {
    let mut doomed = Doomed::default();
    let live = if ttl_free {
        if max_entries.is_none() {
            return doomed;
        }
        map.len()
    } else {
        let track_live = max_entries.is_some();
        let mut live: usize = 0;
        map.iter_sync(|k, slot| {
            doomed.saw_ttl |= slot.expires_at_ms != 0;
            if slot.is_expired(now) {
                if doomed.expired.len() < 4096 {
                    doomed.expired.push(k.clone());
                }
            } else if track_live {
                live += 1;
            }
            track_live || doomed.expired.len() < 4096
        });
        live
    };
    if let Some(max) = max_entries
        && live > max
    {
        let mut need = live - max;
        map.iter_sync(|k, slot| {
            if !slot.is_expired(now) {
                doomed.evicted.push(k.clone());
                need -= 1;
            }
            need > 0
        });
    }
    doomed
}

/// Tracks when a full scan last found no expiring slot. TTL writers bump the
/// epoch after inserting, so a scan that read the old epoch is never trusted.
#[derive(Default)]
pub(crate) struct TtlScan {
    ttl_free_at: Option<u64>,
}

impl TtlScan {
    pub(crate) fn doomed(
        &mut self,
        map: &ValueMap,
        epoch: &std::sync::atomic::AtomicU64,
        max_entries: Option<usize>,
        now: u64,
    ) -> Doomed {
        let at = epoch.load(std::sync::atomic::Ordering::Acquire);
        let ttl_free = self.ttl_free_at == Some(at);
        let doomed = compute_doomed(map, now, max_entries, ttl_free);
        if !ttl_free {
            self.ttl_free_at = (!doomed.saw_ttl).then_some(at);
        }
        doomed
    }
}
