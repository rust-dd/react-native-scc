use std::time::Duration;

use kv_core::{Durability, OpenOptions, Store, Value, derive_encryption_key};

fn strict_opts() -> OpenOptions {
    OpenOptions {
        durability: Durability::Strict,
        group_window: Duration::from_millis(1),
        ..OpenOptions::default()
    }
}

#[test]
fn ttl_expires_reads_and_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let s = Store::open(dir.path(), "ttl", strict_opts()).unwrap();
        s.set_with_ttl("short", Value::Num(1.0), 150).unwrap();
        s.set_with_ttl("long", Value::Num(2.0), 60_000).unwrap();
        s.set("forever", Value::Num(3.0)).unwrap();

        assert_eq!(s.get("short"), Some(Value::Num(1.0)));
        assert!(s.contains("short"));
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(s.get("short"), None);
        assert!(!s.contains("short"));
        assert_eq!(s.get("long"), Some(Value::Num(2.0)));
        let keys = s.keys();
        assert!(!keys.contains(&"short".to_string()));
        assert!(keys.contains(&"long".to_string()));
        s.flush().unwrap();
        s.close().unwrap();
    }
    let s = Store::open(dir.path(), "ttl", strict_opts()).unwrap();
    assert_eq!(s.get("short"), None);
    assert_eq!(s.get("long"), Some(Value::Num(2.0)));
    assert_eq!(s.get("forever"), Some(Value::Num(3.0)));
    s.close().unwrap();
}

#[test]
fn sweeper_reclaims_expired_keys_and_notifies() {
    use std::sync::{Arc, Mutex};

    let dir = tempfile::tempdir().unwrap();
    let opts = OpenOptions {
        ttl_sweep_interval: Duration::from_millis(100),
        ..strict_opts()
    };
    let s = Store::open(dir.path(), "sweep", opts).unwrap();
    let events: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    s.subscribe(move |key| sink.lock().unwrap().push(key.map(str::to_string)));

    s.set_with_ttl("gone", Value::Bool(true), 50).unwrap();
    assert_eq!(s.len(), 1);
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(s.len(), 0);
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.as_deref() == Some("gone"))
            .count()
            >= 2,
        "expected set + sweep-delete notifications"
    );
    s.close().unwrap();
}

#[test]
fn len_excludes_expired_keys_before_sweeper() {
    let dir = tempfile::tempdir().unwrap();
    let opts = OpenOptions {
        ttl_sweep_interval: Duration::from_secs(3600),
        ..strict_opts()
    };
    let s = Store::open(dir.path(), "live-len", opts).unwrap();

    s.set_with_ttl("gone", Value::Bool(true), 30).unwrap();
    assert_eq!(s.len(), 1);
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(s.get("gone"), None);
    assert_eq!(s.len(), 0);
    assert!(s.is_empty());
    s.close().unwrap();
}

#[test]
fn eviction_enforces_max_entries() {
    let dir = tempfile::tempdir().unwrap();
    let opts = OpenOptions {
        max_entries: Some(50),
        ttl_sweep_interval: Duration::from_millis(100),
        ..strict_opts()
    };
    let s = Store::open(dir.path(), "evict", opts).unwrap();
    for i in 0..200 {
        s.set(&format!("k{i}"), Value::Num(i as f64)).unwrap();
    }
    std::thread::sleep(Duration::from_millis(900));
    let len = s.len();
    assert!(len <= 50, "eviction did not run: {len} entries left");
    assert!(len > 0);
    s.close().unwrap();
}

#[test]
fn encrypted_store_round_trips_and_rejects_wrong_key() {
    let dir = tempfile::tempdir().unwrap();
    let key = derive_encryption_key(b"correct horse battery staple");
    let opts = OpenOptions {
        encryption_key: Some(key),
        ..strict_opts()
    };
    {
        let s = Store::open(dir.path(), "vault", opts.clone()).unwrap();
        s.set("secret", Value::Str("classified".into())).unwrap();
        s.set_with_ttl("token", Value::Str("abc".into()), 60_000)
            .unwrap();
        s.flush().unwrap();
        s.close().unwrap();
    }

    let raw = std::fs::read(dir.path().join("vault.wal")).unwrap();
    let needle = b"classified";
    assert!(
        !raw.windows(needle.len()).any(|w| w == needle),
        "plaintext leaked into the WAL"
    );

    let s = Store::open(dir.path(), "vault", opts.clone()).unwrap();
    assert_eq!(s.get("secret"), Some(Value::Str("classified".into())));
    assert_eq!(s.get("token"), Some(Value::Str("abc".into())));
    s.close().unwrap();
    kv_core::close(Some(dir.path()), "vault").unwrap();

    let wrong = OpenOptions {
        encryption_key: Some(derive_encryption_key(b"wrong")),
        ..strict_opts()
    };
    assert!(Store::open(dir.path(), "vault", wrong).is_err());

    let keyless = strict_opts();
    assert!(Store::open(dir.path(), "vault", keyless).is_err());
}

#[test]
fn plaintext_store_rejects_key_and_legacy_files_load() {
    let dir = tempfile::tempdir().unwrap();
    {
        let s = Store::open(dir.path(), "plain", strict_opts()).unwrap();
        s.set("k", Value::Num(1.0)).unwrap();
        s.flush().unwrap();
        s.close().unwrap();
    }
    let with_key = OpenOptions {
        encryption_key: Some(derive_encryption_key(b"key")),
        ..strict_opts()
    };
    assert!(Store::open(dir.path(), "plain", with_key).is_err());

    // Headerless legacy WAL (pre-header format) still loads.
    let legacy_dir = tempfile::tempdir().unwrap();
    let wal = {
        let tmp = tempfile::tempdir().unwrap();
        let s = Store::open(tmp.path(), "src", strict_opts()).unwrap();
        s.set("legacy", Value::Str("ok".into())).unwrap();
        s.flush().unwrap();
        s.close().unwrap();
        let bytes = std::fs::read(tmp.path().join("src.wal")).unwrap();
        bytes[8..].to_vec()
    };
    std::fs::write(legacy_dir.path().join("old.wal"), &wal).unwrap();
    let s = Store::open(legacy_dir.path(), "old", strict_opts()).unwrap();
    assert_eq!(s.get("legacy"), Some(Value::Str("ok".into())));
    s.close().unwrap();
}

#[test]
fn encrypted_compaction_and_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let opts = OpenOptions {
        encryption_key: Some(derive_encryption_key(b"pw")),
        compact_min: 512,
        ..strict_opts()
    };
    {
        let s = Store::open(dir.path(), "db", opts.clone()).unwrap();
        for i in 0..300 {
            s.set("hot", Value::Num(i as f64)).unwrap();
            s.set(&format!("k{}", i % 10), Value::Str("payload".into()))
                .unwrap();
        }
        s.flush().unwrap();
        s.close().unwrap();
    }
    let snap_len = std::fs::metadata(dir.path().join("db.snap")).unwrap().len();
    assert!(snap_len > 0, "encrypted compaction never ran");

    let s = Store::open(dir.path(), "db", opts).unwrap();
    assert_eq!(s.get("hot"), Some(Value::Num(299.0)));
    assert_eq!(s.len(), 11);
    s.close().unwrap();
}
