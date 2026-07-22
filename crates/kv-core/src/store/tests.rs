use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;

#[test]
fn set_get_typed_values() {
    let s = Store::in_memory();
    s.set("s", Value::Str("x".into())).unwrap();
    s.set("n", Value::Num(1.5)).unwrap();
    s.set("b", Value::Bool(true)).unwrap();
    assert_eq!(s.get("s"), Some(Value::Str("x".into())));
    assert_eq!(s.get("n"), Some(Value::Num(1.5)));
    assert_eq!(s.get("b"), Some(Value::Bool(true)));
    assert_eq!(s.get("missing"), None);
    assert_eq!(s.len(), 3);
}

#[test]
fn overwrite_changes_type() {
    let s = Store::in_memory();
    s.set("k", Value::Num(1.0)).unwrap();
    s.set("k", Value::Str("now a string".into())).unwrap();
    assert_eq!(s.get("k"), Some(Value::Str("now a string".into())));
    assert_eq!(s.len(), 1);
}

#[test]
fn delete_contains_keys_clear() {
    let s = Store::in_memory();
    s.set("a", Value::Bool(false)).unwrap();
    s.set("b", Value::Num(2.0)).unwrap();
    assert!(s.contains("a"));
    assert!(s.delete("a").unwrap());
    assert!(!s.delete("a").unwrap());
    assert!(!s.contains("a"));
    let mut keys = s.keys();
    keys.sort();
    assert_eq!(keys, vec!["b".to_string()]);
    s.clear().unwrap();
    assert!(s.is_empty());
}

#[test]
fn listeners_fire_correctly() {
    let s = Store::in_memory();
    let events = Arc::<Mutex<Vec<Option<String>>>>::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let id = s.subscribe(move |key| {
        sink.lock().unwrap().push(key.map(str::to_string));
    });

    s.set("k1", Value::Num(1.0)).unwrap();
    s.delete("missing").unwrap();
    s.delete("k1").unwrap();
    s.clear().unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        vec![Some("k1".to_string()), Some("k1".to_string()), None]
    );

    assert!(s.unsubscribe(id));
    assert!(!s.unsubscribe(id));
    s.set("k2", Value::Num(2.0)).unwrap();
    assert_eq!(events.lock().unwrap().len(), 3);
}

#[test]
fn closed_store_rejects_mutations_allows_reads() {
    let s = Store::in_memory();
    s.set("k", Value::Num(1.0)).unwrap();
    s.close().unwrap();
    assert!(matches!(s.set("k", Value::Num(2.0)), Err(Error::Closed)));
    assert!(matches!(s.clear(), Err(Error::Closed)));
    assert_eq!(s.get("k"), Some(Value::Num(1.0)));
}

#[test]
fn ungated_close_waits_for_an_admitted_mutation() {
    let store = Store::in_memory();
    assert!(store.mutation_gate.is_none());
    let mutation = store.begin_mutation().unwrap();
    let closer_store = store.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let closer = std::thread::spawn(move || {
        closer_store.close().unwrap();
        done_tx.send(()).unwrap();
    });

    while !store.closed.load(Ordering::Acquire) {
        std::thread::yield_now();
    }
    assert!(matches!(
        done_rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    drop(mutation);
    done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    closer.join().unwrap();
    assert_eq!(
        store.ungated_mutations.load(Ordering::Acquire),
        MUTATION_CLOSED
    );
    assert!(matches!(
        store.set("late", Value::Bool(true)),
        Err(Error::Closed)
    ));
}

#[test]
fn plain_in_memory_mutations_admit_concurrently() {
    let store = Store::in_memory();
    let first = store.begin_mutation().unwrap();
    let second_store = store.clone();
    let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let second = std::thread::spawn(move || {
        let _mutation = second_store.begin_mutation().unwrap();
        acquired_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });

    acquired_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(first);
    release_tx.send(()).unwrap();
    second.join().unwrap();
    store.close().unwrap();
}

fn fast_opts() -> OpenOptions {
    OpenOptions {
        durability: Durability::Strict,
        group_window: Duration::from_millis(1),
        ..OpenOptions::default()
    }
}

#[test]
fn background_error_surfaces_on_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path(), "db", fast_opts()).unwrap();
    s.set("k", Value::Num(1.0)).unwrap();
    s.wal_for_test().unwrap().inject_error("io fail");
    assert!(matches!(
        s.set("k", Value::Num(2.0)),
        Err(Error::Background(_))
    ));
    assert_eq!(s.get("k"), Some(Value::Num(1.0)));
}

#[test]
fn oversized_record_is_rejected_before_apply() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path(), "db", fast_opts()).unwrap();
    let oversized = vec![0u8; crate::record::MAX_PAYLOAD as usize];

    assert!(s.set("huge", Value::Bytes(oversized)).is_err());
    assert_eq!(s.get("huge"), None);
}

#[test]
fn apply_batch_applies_all_ops() {
    let s = Store::in_memory();
    s.set("keep", Value::Str("x".into())).unwrap();
    s.set("drop", Value::Str("y".into())).unwrap();
    s.apply_batch(&[
        BatchOp::Set {
            key: "counter".into(),
            value: Value::Num(2.0),
        },
        BatchOp::Set {
            key: "meta".into(),
            value: Value::Json(r#"{"n":2}"#.into()),
        },
        BatchOp::Delete { key: "drop".into() },
    ])
    .unwrap();
    assert_eq!(s.get("counter"), Some(Value::Num(2.0)));
    assert_eq!(s.get("meta"), Some(Value::Json(r#"{"n":2}"#.into())));
    assert_eq!(s.get("drop"), None);
    assert_eq!(s.get("keep"), Some(Value::Str("x".into())));
}

#[test]
fn torn_batch_record_applies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path(), "tx", fast_opts()).unwrap();
    s.set("base", Value::Num(1.0)).unwrap();
    s.apply_batch(&[
        BatchOp::Set {
            key: "a".into(),
            value: Value::Num(9.0),
        },
        BatchOp::Set {
            key: "b".into(),
            value: Value::Num(9.0),
        },
    ])
    .unwrap();
    s.close().unwrap();
    drop(s);

    let wal = dir.path().join("tx.wal");
    let len = std::fs::metadata(&wal).unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&wal)
        .unwrap()
        .set_len(len - 1)
        .unwrap();

    let reopened = Store::open(dir.path(), "tx", fast_opts()).unwrap();
    assert_eq!(reopened.get("base"), Some(Value::Num(1.0)));
    assert_eq!(
        reopened.get("a"),
        None,
        "torn batch must not partially apply"
    );
    assert_eq!(reopened.get("b"), None);
}
