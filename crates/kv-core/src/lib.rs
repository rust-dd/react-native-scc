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
pub use registry::{close, in_memory, open_or_get};
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

pub(crate) type ValueMap = scc::HashMap<String, Slot, FastState>;

pub(crate) fn new_value_map() -> ValueMap {
    ValueMap::with_hasher(FastState::default())
}

/// Selects keys to reclaim in one sweep, split by why they are doomed:
/// `expired` keys (capped at 4096 per pass) must be re-checked for expiry at
/// removal time — a concurrent rewrite makes them live again — while `evicted`
/// keys are arbitrary live keys removed to fit `max_entries`. The live count
/// is exact (a full scan when tracking), so the 4096 expired-cap never
/// inflates the eviction target.
pub(crate) fn compute_doomed(
    map: &ValueMap,
    now: u64,
    max_entries: Option<usize>,
) -> (Vec<String>, Vec<String>) {
    let track_live = max_entries.is_some();
    let mut expired: Vec<String> = Vec::new();
    let mut live: usize = 0;
    map.iter_sync(|k, slot| {
        if slot.is_expired(now) {
            if expired.len() < 4096 {
                expired.push(k.clone());
            }
        } else if track_live {
            live += 1;
        }
        track_live || expired.len() < 4096
    });
    let mut evicted: Vec<String> = Vec::new();
    if let Some(max) = max_entries
        && live > max
    {
        let mut need = live - max;
        map.iter_sync(|k, slot| {
            if !slot.is_expired(now) {
                evicted.push(k.clone());
                need -= 1;
            }
            need > 0
        });
    }
    (expired, evicted)
}
