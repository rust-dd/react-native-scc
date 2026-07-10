use std::time::Duration;

use kv_core::{BatchOp, Durability, OpenOptions, Store, Value};

fn fast_opts() -> OpenOptions {
    OpenOptions {
        durability: Durability::Strict,
        group_window: Duration::from_millis(1),
        ..OpenOptions::default()
    }
}

#[test]
fn survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let s = Store::open(dir.path(), "db", fast_opts()).unwrap();
        s.set("str", Value::Str("v".into())).unwrap();
        s.set("num", Value::Num(9.5)).unwrap();
        s.set("gone", Value::Bool(true)).unwrap();
        s.delete("gone").unwrap();
        s.flush().unwrap();
        s.close().unwrap();
    }
    let s = Store::open(dir.path(), "db", fast_opts()).unwrap();
    assert_eq!(s.get("str"), Some(Value::Str("v".into())));
    assert_eq!(s.get("num"), Some(Value::Num(9.5)));
    assert_eq!(s.get("gone"), None);
    assert_eq!(s.len(), 2);
}

#[test]
fn clear_persists() {
    let dir = tempfile::tempdir().unwrap();
    {
        let s = Store::open(dir.path(), "db", fast_opts()).unwrap();
        s.set("a", Value::Num(1.0)).unwrap();
        s.clear().unwrap();
        s.set("b", Value::Num(2.0)).unwrap();
        s.flush().unwrap();
    }
    let s = Store::open(dir.path(), "db", fast_opts()).unwrap();
    assert_eq!(s.get("a"), None);
    assert_eq!(s.get("b"), Some(Value::Num(2.0)));
}

#[test]
fn torn_tail_is_truncated() {
    let dir = tempfile::tempdir().unwrap();
    {
        let s = Store::open(dir.path(), "db", fast_opts()).unwrap();
        s.set("committed", Value::Num(1.0)).unwrap();
        s.flush().unwrap();
    }
    let wal_path = dir.path().join("db.wal");
    let mut data = std::fs::read(&wal_path).unwrap();
    let full_len = data.len();
    data.extend_from_slice(&[0xAB; 7]);
    std::fs::write(&wal_path, &data).unwrap();

    let s = Store::open(dir.path(), "db", fast_opts()).unwrap();
    assert_eq!(s.get("committed"), Some(Value::Num(1.0)));
    s.close().unwrap();
    assert_eq!(
        std::fs::metadata(&wal_path).unwrap().len() as usize,
        full_len
    );
}

#[test]
fn recreate_wipes_files() {
    let dir = tempfile::tempdir().unwrap();
    {
        let s = Store::open(dir.path(), "db", fast_opts()).unwrap();
        s.set("old", Value::Num(1.0)).unwrap();
        s.flush().unwrap();
    }
    let opts = OpenOptions {
        recreate: true,
        ..fast_opts()
    };
    let s = Store::open(dir.path(), "db", opts).unwrap();
    assert!(s.is_empty());
}

#[test]
fn compaction_keeps_data_and_shrinks_wal() {
    let dir = tempfile::tempdir().unwrap();
    let opts = OpenOptions {
        compact_min: 1024,
        ..fast_opts()
    };
    {
        let s = Store::open(dir.path(), "db", opts.clone()).unwrap();
        for i in 0..500 {
            s.set("hot", Value::Num(i as f64)).unwrap();
            s.set(&format!("k{}", i % 10), Value::Str("payload".into()))
                .unwrap();
        }
        s.flush().unwrap();
        s.close().unwrap();
    }
    let snap_len = std::fs::metadata(dir.path().join("db.snap")).unwrap().len();
    assert!(snap_len > 0, "compaction never ran");
    let wal_len = std::fs::metadata(dir.path().join("db.wal")).unwrap().len();
    assert!(wal_len < 4096, "wal was not truncated: {wal_len}");

    let s = Store::open(dir.path(), "db", opts).unwrap();
    assert_eq!(s.get("hot"), Some(Value::Num(499.0)));
    assert_eq!(s.len(), 11);
}

#[test]
fn writes_during_compaction_survive() {
    let dir = tempfile::tempdir().unwrap();
    let opts = OpenOptions {
        compact_min: 512,
        ..fast_opts()
    };
    {
        let s = Store::open(dir.path(), "db", opts.clone()).unwrap();
        for round in 0..20 {
            for i in 0..50 {
                s.set(
                    &format!("r{round}_i{i}"),
                    Value::Num((round * 50 + i) as f64),
                )
                .unwrap();
            }
        }
        s.flush().unwrap();
        s.close().unwrap();
    }
    let s = Store::open(dir.path(), "db", opts).unwrap();
    assert_eq!(s.len(), 1000);
    assert_eq!(s.get("r19_i49"), Some(Value::Num(999.0)));
    assert_eq!(s.get("r0_i0"), Some(Value::Num(0.0)));
}

#[test]
fn batches_race_compaction_and_reopen_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let opts = OpenOptions {
        compact_min: 512,
        ..fast_opts()
    };
    let payload = "x".repeat(512);
    {
        let s = Store::open(dir.path(), "db", opts.clone()).unwrap();
        // Padded batches keep the writer compacting while the caller holds the
        // compaction gate's read side — a deadlocked gate hangs this test, and
        // the reopen below must always see a and b moved together.
        for i in 0..300 {
            s.apply_batch(&[
                BatchOp::Set {
                    key: "a".into(),
                    value: Value::Num(i as f64),
                },
                BatchOp::Set {
                    key: format!("pad{i}"),
                    value: Value::Str(payload.as_str().into()),
                },
                BatchOp::Set {
                    key: "b".into(),
                    value: Value::Num(i as f64),
                },
            ])
            .unwrap();
        }
        s.flush().unwrap();
        s.close().unwrap();
    }
    let s = Store::open(dir.path(), "db", opts).unwrap();
    assert_eq!(s.get("a"), s.get("b"));
    assert_eq!(s.get("a"), Some(Value::Num(299.0)));
}
