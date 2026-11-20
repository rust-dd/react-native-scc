use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::store::{OpenOptions, Store};

#[derive(PartialEq)]
enum StoredOptions {
    Disk(OpenOptions),
    Mem {
        max_entries: Option<usize>,
        sweep_interval: Option<Duration>,
    },
}

struct RegistryEntry {
    store: Arc<Store>,
    options: StoredOptions,
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

fn lock_registry() -> MutexGuard<'static, ()> {
    registry_lock()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

// Canonical, so two spellings of one directory never open two WAL writers over the same files.
fn disk_key(dir: &Path, id: &str) -> String {
    let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    format!("disk::{}::{id}", canonical.display())
}

/// Opens (or returns the already-open) persistent store for `dir`/`id`.
pub fn open_or_get(dir: &Path, id: &str, opts: OpenOptions) -> Result<Arc<Store>> {
    let _guard = lock_registry();
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
                options: StoredOptions::Disk(stored_opts),
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
    match &existing.options {
        StoredOptions::Disk(opts) if opts == requested => Ok(()),
        _ => Err(Error::Config(format!(
            "store '{id}' is already open with different options"
        ))),
    }
}

/// Returns the named in-memory store, creating it on first use. Reopening an
/// id with different eviction options is an error, mirroring the disk path.
pub fn in_memory(
    id: &str,
    max_entries: Option<usize>,
    sweep_interval: Option<Duration>,
) -> Result<Arc<Store>> {
    let _guard = lock_registry();
    let key = format!("mem::{id}");
    let requested = StoredOptions::Mem {
        max_entries,
        sweep_interval,
    };
    if let Some(existing) = registry().read_sync(&key, |_, entry| {
        if entry.options == requested {
            Ok(entry.store.clone())
        } else {
            Err(Error::Config(format!(
                "store '{id}' is already open with different options"
            )))
        }
    }) {
        return existing;
    }
    let store = if max_entries.is_some() || sweep_interval.is_some() {
        Store::in_memory_evicting(
            max_entries,
            sweep_interval.unwrap_or(Duration::from_secs(30)),
        )?
    } else {
        Store::in_memory()
    };
    match registry().entry_sync(key) {
        scc::hash_map::Entry::Occupied(o) => Ok(o.get().store.clone()),
        scc::hash_map::Entry::Vacant(v) => {
            v.insert_entry(RegistryEntry {
                store: store.clone(),
                options: requested,
            });
            Ok(store)
        }
    }
}

/// Deregisters by identity, so a stale handle never closes a store reopened under its id.
pub fn close_store(store: &Arc<Store>) -> Result<()> {
    let _guard = lock_registry();
    registry().retain_sync(|_, entry| !Arc::ptr_eq(&entry.store, store));
    store.close()
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
        close_store(&a).unwrap();
    }

    #[test]
    fn close_allows_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let a = open_or_get(dir.path(), "cycle", OpenOptions::default()).unwrap();
        a.set("k", Value::Str("v".into())).unwrap();
        a.flush().unwrap();
        close_store(&a).unwrap();
        drop(a);
        let b = open_or_get(dir.path(), "cycle", OpenOptions::default()).unwrap();
        assert_eq!(b.get("k"), Some(Value::Str("v".into())));
        close_store(&b).unwrap();
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
        close_store(&a).unwrap();
    }

    #[test]
    fn stale_close_leaves_reopened_store_open() {
        let dir = tempfile::tempdir().unwrap();
        let a = open_or_get(dir.path(), "stale", OpenOptions::default()).unwrap();
        let b = open_or_get(dir.path(), "stale", OpenOptions::default()).unwrap();
        close_store(&a).unwrap();
        let c = open_or_get(dir.path(), "stale", OpenOptions::default()).unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
        close_store(&b).unwrap();
        c.set("k", Value::Num(1.0)).unwrap();
        assert!(Arc::ptr_eq(
            &c,
            &open_or_get(dir.path(), "stale", OpenOptions::default()).unwrap()
        ));
        close_store(&c).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn close_deregisters_after_symlinked_dir_is_wiped() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        let link = root.path().join("link");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let a = open_or_get(&link, "wiped", OpenOptions::default()).unwrap();
        a.set("k", Value::Num(1.0)).unwrap();
        a.flush().unwrap();
        std::fs::remove_dir_all(&real).unwrap();
        close_store(&a).unwrap();
        std::fs::create_dir(&real).unwrap();
        let b = open_or_get(&link, "wiped", OpenOptions::default()).unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(b.get("k"), None);
        close_store(&b).unwrap();
    }

    #[test]
    fn a_panic_under_the_registry_lock_does_not_wedge_it() {
        let _ = std::panic::catch_unwind(|| {
            let _guard = registry_lock().lock();
            panic!("simulated panic while opening");
        });
        let store = in_memory("after-panic", None, None).unwrap();
        close_store(&store).unwrap();
    }

    #[test]
    fn in_memory_registry_dedupes() {
        let a = in_memory("state", None, None).unwrap();
        let b = in_memory("state", None, None).unwrap();
        assert!(in_memory("state", Some(5), None).is_err());
        assert!(Arc::ptr_eq(&a, &b));
        close_store(&a).unwrap();
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

        close_store(&store).unwrap();
    }
}
