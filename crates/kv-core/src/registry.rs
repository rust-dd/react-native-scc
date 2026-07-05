use std::path::Path;
use std::sync::{Arc, OnceLock};

use crate::error::Result;
use crate::store::{OpenOptions, Store};

fn registry() -> &'static scc::HashMap<String, Arc<Store>, crate::FastState> {
    static REGISTRY: OnceLock<scc::HashMap<String, Arc<Store>, crate::FastState>> = OnceLock::new();
    REGISTRY.get_or_init(|| scc::HashMap::with_hasher(crate::FastState::default()))
}

// Canonicalized so two spellings of the same directory (symlink, `./`,
// trailing slash) can never open two stores — and two WAL writers — over the
// same files.
fn disk_key(dir: &Path, id: &str) -> String {
    let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    format!("disk::{}::{id}", canonical.display())
}

/// Opens (or returns the already-open) persistent store for `dir`/`id`.
pub fn open_or_get(dir: &Path, id: &str, opts: OpenOptions) -> Result<Arc<Store>> {
    let _ = std::fs::create_dir_all(dir);
    let key = disk_key(dir, id);
    if let Some(existing) = registry().read_sync(&key, |_, s| s.clone()) {
        return Ok(existing);
    }
    let store = Store::open(dir, id, opts)?;
    match registry().entry_sync(key) {
        scc::hash_map::Entry::Occupied(o) => {
            store.close()?;
            Ok(o.get().clone())
        }
        scc::hash_map::Entry::Vacant(v) => {
            v.insert_entry(store.clone());
            Ok(store)
        }
    }
}

/// Returns the named in-memory store, creating it on first use.
pub fn in_memory(id: &str) -> Arc<Store> {
    let key = format!("mem::{id}");
    if let Some(existing) = registry().read_sync(&key, |_, s| s.clone()) {
        return existing;
    }
    let store = Store::in_memory();
    match registry().entry_sync(key) {
        scc::hash_map::Entry::Occupied(o) => o.get().clone(),
        scc::hash_map::Entry::Vacant(v) => {
            v.insert_entry(store.clone());
            store
        }
    }
}

/// Closes and deregisters a store. `dir: None` targets in-memory stores.
pub fn close(dir: Option<&Path>, id: &str) -> Result<()> {
    let key = match dir {
        Some(dir) => disk_key(dir, id),
        None => format!("mem::{id}"),
    };
    if let Some((_, store)) = registry().remove_sync(&key) {
        store.close()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    #[test]
    fn same_id_returns_same_store() {
        let dir = tempfile::tempdir().unwrap();
        let a = open_or_get(dir.path(), "shared", OpenOptions::default()).unwrap();
        let b = open_or_get(dir.path(), "shared", OpenOptions::default()).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        a.set("k", Value::Num(1.0)).unwrap();
        assert_eq!(b.get("k"), Some(Value::Num(1.0)));
        close(Some(dir.path()), "shared").unwrap();
    }

    #[test]
    fn close_allows_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let a = open_or_get(dir.path(), "cycle", OpenOptions::default()).unwrap();
        a.set("k", Value::Str("v".into())).unwrap();
        a.flush().unwrap();
        drop(a);
        close(Some(dir.path()), "cycle").unwrap();
        let b = open_or_get(dir.path(), "cycle", OpenOptions::default()).unwrap();
        assert_eq!(b.get("k"), Some(Value::Str("v".into())));
        close(Some(dir.path()), "cycle").unwrap();
    }

    #[test]
    fn path_spellings_resolve_to_same_store() {
        let dir = tempfile::tempdir().unwrap();
        let spelled = dir
            .path()
            .join(".")
            .join("..")
            .join(dir.path().file_name().unwrap());
        let a = open_or_get(dir.path(), "canon", OpenOptions::default()).unwrap();
        let b = open_or_get(&spelled, "canon", OpenOptions::default()).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        close(Some(dir.path()), "canon").unwrap();
    }

    #[test]
    fn in_memory_registry_dedupes() {
        let a = in_memory("state");
        let b = in_memory("state");
        assert!(Arc::ptr_eq(&a, &b));
        close(None, "state").unwrap();
    }
}
