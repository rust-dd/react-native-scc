use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use kv_core::{Durability, OpenOptions, Store, Value};

fn strict_opts() -> OpenOptions {
    OpenOptions {
        durability: Durability::Strict,
        group_window: Duration::from_millis(1),
        compact_min: u64::MAX,
        ..OpenOptions::default()
    }
}

enum TestOp {
    Set(&'static str, Value),
    Delete(&'static str),
    Clear,
}

fn ops() -> Vec<TestOp> {
    vec![
        TestOp::Set("a", Value::Num(1.0)),
        TestOp::Set("b", Value::Str("x".into())),
        TestOp::Set("a", Value::Bool(true)),
        TestOp::Delete("b"),
        TestOp::Set("c", Value::Bytes(vec![1, 2, 3])),
        TestOp::Clear,
        TestOp::Set("d", Value::Str("end".into())),
    ]
}

fn expected_states() -> Vec<BTreeMap<String, Value>> {
    let mut states = vec![BTreeMap::new()];
    let mut cur: BTreeMap<String, Value> = BTreeMap::new();
    for op in ops() {
        match op {
            TestOp::Set(k, v) => {
                cur.insert(k.to_string(), v);
            }
            TestOp::Delete(k) => {
                cur.remove(k);
            }
            TestOp::Clear => cur.clear(),
        }
        states.push(cur.clone());
    }
    states
}

fn store_state(store: &Store) -> BTreeMap<String, Value> {
    store
        .keys()
        .into_iter()
        .map(|k| {
            let v = store.get(&k).unwrap();
            (k, v)
        })
        .collect()
}

fn write_wal(dir: &Path) -> Vec<u64> {
    let store = Store::open(dir, "crash", strict_opts()).unwrap();
    let mut boundaries = vec![0u64];
    for op in ops() {
        match op {
            TestOp::Set(k, v) => store.set(k, v).unwrap(),
            TestOp::Delete(k) => {
                store.delete(k).unwrap();
            }
            TestOp::Clear => store.clear().unwrap(),
        }
        store.flush().unwrap();
        boundaries.push(std::fs::metadata(dir.join("crash.wal")).unwrap().len());
    }
    store.close().unwrap();
    boundaries
}

#[test]
fn every_wal_truncation_recovers_a_committed_prefix() {
    let src = tempfile::tempdir().unwrap();
    let boundaries = write_wal(src.path());
    let wal = std::fs::read(src.path().join("crash.wal")).unwrap();
    assert_eq!(*boundaries.last().unwrap() as usize, wal.len());
    let states = expected_states();

    for cut in 0..=wal.len() {
        let work = tempfile::tempdir().unwrap();
        std::fs::write(work.path().join("crash.wal"), &wal[..cut]).unwrap();

        let complete_records = boundaries.iter().filter(|b| **b <= cut as u64).count() - 1;
        let store = Store::open(work.path(), "crash", strict_opts()).unwrap();
        assert_eq!(
            store_state(&store),
            states[complete_records],
            "wrong state after truncation at byte {cut}"
        );
        store.close().unwrap();
    }
}

#[test]
fn snapshot_plus_truncated_wal_recovers() {
    let dir = tempfile::tempdir().unwrap();
    {
        let opts = OpenOptions {
            compact_min: 64,
            ..strict_opts()
        };
        let store = Store::open(dir.path(), "db", opts).unwrap();
        for i in 0..100 {
            store.set(&format!("k{i}"), Value::Num(i as f64)).unwrap();
        }
        store.flush().unwrap();
        store.close().unwrap();
    }
    let wal_path = dir.path().join("db.wal");
    let wal = std::fs::read(&wal_path).unwrap();
    if !wal.is_empty() {
        std::fs::write(&wal_path, &wal[..wal.len() / 2]).unwrap();
    }
    let store = Store::open(dir.path(), "db", strict_opts()).unwrap();
    for (key, value) in store
        .keys()
        .iter()
        .map(|k| (k.clone(), store.get(k).unwrap()))
    {
        let i: f64 = key[1..].parse().unwrap();
        assert_eq!(value, Value::Num(i), "corrupted value for {key}");
    }
    store.close().unwrap();
}
