use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use crate::error::{Error, Result};
use crate::store::{OpenOptions, Store};

struct RegistryEntry {
    store: Arc<Store>,
    options: Option<OpenOptions>,
}

fn registry() -> &'static scc::HashMap<String, RegistryEntry, crate::FastState> {
    static REGISTRY: OnceLock<scc::HashMap<String, RegistryEntry, crate::FastState>> =
        OnceLock::new();
    REGISTRY.get_or_init(|| scc::HashMap::with_hasher(crate::FastState::default()))
}

fn registry_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
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
    let _guard = registry_lock().lock().unwrap();
    let _ = std::fs::create_dir_all(dir);
    let key = disk_key(dir, id);
    let stored_opts = normalized_options(&opts);
    if let Some(existing) = registry().read_sync(&key, |_, entry| {
        check_options(id, opts.recreate, &stored_opts, entry).map(|()| entry.store.clone())
    }) {
        return existing;
    }
    let store = Store::open(dir, id, opts)?;
    match registry().entry_sync(key) {
        scc::hash_map::Entry::Occupied(o) => {
            store.close()?;
            check_options(id, false, &stored_opts, o.get())?;
            Ok(o.get().store.clone())
        }
        scc::hash_map::Entry::Vacant(v) => {
            v.insert_entry(RegistryEntry {
                store: store.clone(),
                options: Some(stored_opts),
            });
            Ok(store)
        }
    }
}

fn normalized_options(opts: &OpenOptions) -> OpenOptions {
    let mut normalized = opts.clone();
    normalized.recreate = false;
    normalized
}

fn check_options(
    id: &str,
    recreate: bool,
    requested: &OpenOptions,
    existing: &RegistryEntry,
) -> Result<()> {
    if recreate {
        return Err(Error::Config(format!(
            "store '{id}' is already open; close it before reopening with recreate"
        )));
    }
    if existing.options.as_ref() == Some(requested) {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "store '{id}' is already open with different options"
        )))
    }
}

/// Returns the named in-memory store, creating it on first use.
pub fn in_memory(id: &str) -> Arc<Store> {
    let _guard = registry_lock().lock().unwrap();
    let key = format!("mem::{id}");
    if let Some(existing) = registry().read_sync(&key, |_, entry| entry.store.clone()) {
        return existing;
    }
    let store = Store::in_memory();
    match registry().entry_sync(key) {
        scc::hash_map::Entry::Occupied(o) => o.get().store.clone(),
        scc::hash_map::Entry::Vacant(v) => {
            v.insert_entry(RegistryEntry {
                store: store.clone(),
                options: None,
            });
            store
        }
    }
}

/// Closes and deregisters a store. `dir: None` targets in-memory stores.
pub fn close(dir: Option<&Path>, id: &str) -> Result<()> {
    let _guard = registry_lock().lock().unwrap();
    let key = match dir {
        Some(dir) => disk_key(dir, id),
        None => format!("mem::{id}"),
    };
    if let Some((_, entry)) = registry().remove_sync(&key) {
        entry.store.close()?;
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

    #[test]
    fn open_or_get_rejects_mismatched_options() {
        let dir = tempfile::tempdir().unwrap();
        let encrypted = OpenOptions {
            encryption_key: Some(crate::derive_encryption_key(b"secret")),
            ..OpenOptions::default()
        };
        let store = open_or_get(dir.path(), "secure", encrypted).unwrap();

        assert!(
            open_or_get(dir.path(), "secure", OpenOptions::default()).is_err(),
            "opening an encrypted live store without its key must fail"
        );
        assert!(
            open_or_get(
                dir.path(),
                "secure",
                OpenOptions {
                    recreate: true,
                    ..OpenOptions::default()
                },
            )
            .is_err(),
            "recreate must not be ignored while a store is live"
        );

        drop(store);
        close(Some(dir.path()), "secure").unwrap();
    }
}
