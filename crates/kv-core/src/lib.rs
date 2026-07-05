mod crypto;
mod error;
mod notify;
mod record;
mod registry;
mod snapshot;
mod store;
mod value;
mod wal;

pub use compact_str::CompactString;
pub use crypto::derive_encryption_key;
pub use error::{Error, Result};
pub use registry::{close, in_memory, open_or_get};
pub use store::{OpenOptions, Store};
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
