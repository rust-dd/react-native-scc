use std::sync::atomic::Ordering;
use std::time::Instant;

use super::*;
use crate::record::{DecodeOutcome, OwnedOp};
use crate::snapshot;
use crate::value::Value;

fn test_cfg(dir: &std::path::Path) -> WriterConfig {
    WriterConfig {
        wal_path: dir.join("t.wal"),
        snap_path: dir.join("t.snap"),
        durability: Durability::Relaxed,
        group_window: Duration::from_millis(5),
        group_bytes: 128 * 1024,
        fsync_interval: Duration::from_millis(50),
        compact_min: u64::MAX,
        linger: Duration::from_millis(20),
        cipher: None,
        listeners: Arc::new(Listeners::new()),
        sweep_interval: Duration::from_secs(3600),
        max_entries: None,
        compact_gate: Arc::new(RwLock::new(())),
    }
}

fn append_set(handle: &WalHandle, key: &str, value: Value) -> Result<()> {
    handle.mutate(|log| log.append(&Op::Set { key, value: &value }))
}

fn decode_all(data: &[u8]) -> Vec<OwnedOp> {
    let mut ops = Vec::new();
    let mut off = crypto::HEADER_LEN;
    while off < data.len() {
        match record::decode(&data[off..]) {
            DecodeOutcome::Record { op, consumed } => {
                ops.push(op);
                off += consumed;
            }
            other => panic!("bad record at {off}: {other:?}"),
        }
    }
    ops
}

#[test]
fn flush_makes_records_durable() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = test_cfg(dir.path());
    let wal_path = cfg.wal_path.clone();
    let handle = WalHandle::spawn(cfg, Arc::new(crate::new_value_map()), 0, 0).unwrap();
    append_set(&handle, "a", Value::Num(1.0)).unwrap();
    append_set(&handle, "b", Value::Str("x".into())).unwrap();
    handle.flush().unwrap();
    let ops = decode_all(&std::fs::read(&wal_path).unwrap());
    assert_eq!(ops.len(), 2);
    assert_eq!(
        ops[0],
        OwnedOp::Set {
            key: "a".into(),
            value: Value::Num(1.0)
        }
    );
    handle.shutdown();
}

#[test]
fn group_window_writes_without_flush() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = test_cfg(dir.path());
    let wal_path = cfg.wal_path.clone();
    let handle = WalHandle::spawn(cfg, Arc::new(crate::new_value_map()), 0, 0).unwrap();
    append_set(&handle, "k", Value::Bool(true)).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let ops = decode_all(&std::fs::read(&wal_path).unwrap());
    assert_eq!(ops.len(), 1);
    handle.shutdown();
}

#[test]
fn shutdown_drains_pending() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = test_cfg(dir.path());
    let wal_path = cfg.wal_path.clone();
    let handle = WalHandle::spawn(cfg, Arc::new(crate::new_value_map()), 0, 0).unwrap();
    for i in 0..10 {
        append_set(&handle, &format!("k{i}"), Value::Num(i as f64)).unwrap();
    }
    handle.shutdown();
    assert_eq!(decode_all(&std::fs::read(&wal_path).unwrap()).len(), 10);
}

#[test]
fn sticky_error_rejects_appends_and_flush() {
    let dir = tempfile::tempdir().unwrap();
    let handle =
        WalHandle::spawn(test_cfg(dir.path()), Arc::new(crate::new_value_map()), 0, 0).unwrap();
    handle.inject_error("disk full");
    assert!(matches!(
        append_set(&handle, "k", Value::Num(1.0)),
        Err(Error::Background(msg)) if msg == "disk full"
    ));
    assert!(matches!(handle.flush(), Err(Error::Background(_))));
    handle.shutdown();
}

#[test]
fn compaction_truncates_wal_and_writes_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = test_cfg(dir.path());
    cfg.compact_min = 256;
    let wal_path = cfg.wal_path.clone();
    let snap_path = cfg.snap_path.clone();
    let map = Arc::new(crate::new_value_map());
    let _ = map.insert_sync("final".to_string(), crate::slot(Value::Str("state".into())));
    let handle = WalHandle::spawn(cfg, map.clone(), 0, 0).unwrap();
    for i in 0..50 {
        append_set(&handle, "hot", Value::Num(i as f64)).unwrap();
        handle.flush().unwrap();
    }
    handle.shutdown();
    assert!(std::fs::metadata(&wal_path).unwrap().len() < 256 + crypto::HEADER_LEN as u64);
    let loaded = crate::new_value_map();
    snapshot::load(&snap_path, &loaded, None).unwrap();
    assert_eq!(
        loaded.read_sync("final", |_, s| s.value.clone()),
        Some(Value::Str("state".into()))
    );
}

#[test]
fn encrypted_pending_records_split_at_the_frame_limit_and_recover() {
    let dir = tempfile::tempdir().unwrap();
    let encryption_key = crypto::derive_encryption_key(b"large-wal-frame");
    let opts = crate::OpenOptions {
        group_window: Duration::from_secs(3600),
        group_bytes: usize::MAX,
        compact_min: u64::MAX,
        encryption_key: Some(encryption_key),
        ..crate::OpenOptions::default()
    };
    let store = crate::Store::open(dir.path(), "large", opts.clone()).unwrap();
    store.set("small", Value::Bool(true)).unwrap();
    let record_overhead = 1 + 4 + "large".len() + 1;
    let large_len = crate::record::MAX_PAYLOAD as usize - record_overhead;
    store
        .set("large", Value::Bytes(vec![0x5a; large_len]))
        .unwrap();
    store.flush().unwrap();

    {
        let data = std::fs::read(dir.path().join("large.wal")).unwrap();
        let mut offset = crypto::HEADER_LEN;
        let mut frames = 0usize;
        while offset < data.len() {
            let ciphertext_len =
                u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
            assert!(ciphertext_len <= crypto::MAX_FRAME_PLAINTEXT + 16);
            offset += 4 + 12 + ciphertext_len;
            assert!(offset <= data.len());
            frames += 1;
        }
        assert_eq!(offset, data.len());
        assert_eq!(frames, 2);
    }

    store.close().unwrap();
    drop(store);
    let reopened = crate::Store::open(dir.path(), "large", opts).unwrap();
    assert_eq!(reopened.get("small"), Some(Value::Bool(true)));
    assert_eq!(
        reopened.with_value("large", |value| match value {
            Value::Bytes(bytes) => (bytes.len(), bytes.first().copied(), bytes.last().copied()),
            _ => unreachable!(),
        }),
        Some((large_len, Some(0x5a), Some(0x5a)))
    );
    reopened.close().unwrap();
    drop(reopened);

    let wal_path = dir.path().join("large.wal");
    let wal_len = std::fs::metadata(&wal_path).unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&wal_path)
        .unwrap()
        .set_len(wal_len - 1)
        .unwrap();
    let prefix = crate::Store::open(
        dir.path(),
        "large",
        crate::OpenOptions {
            encryption_key: Some(encryption_key),
            ..crate::OpenOptions::default()
        },
    )
    .unwrap();
    assert_eq!(prefix.get("small"), Some(Value::Bool(true)));
    assert_eq!(prefix.get("large"), None);
    prefix.close().unwrap();
}

#[test]
fn closed_log_rejects_mutations_and_flush() {
    let dir = tempfile::tempdir().unwrap();
    let handle =
        WalHandle::spawn(test_cfg(dir.path()), Arc::new(crate::new_value_map()), 0, 0).unwrap();
    append_set(&handle, "k", Value::Num(1.0)).unwrap();
    assert!(handle.close());
    assert!(!handle.close());
    assert!(matches!(
        append_set(&handle, "late", Value::Num(2.0)),
        Err(Error::Closed)
    ));
    assert!(matches!(handle.flush(), Err(Error::Closed)));
    handle.join();
    assert_eq!(
        decode_all(&std::fs::read(dir.path().join("t.wal")).unwrap()).len(),
        1
    );
}

#[test]
fn records_reach_the_file_after_idle_without_flush() {
    let dir = tempfile::tempdir().unwrap();
    let opts = crate::OpenOptions {
        group_window: Duration::from_millis(2),
        writer_linger: Duration::from_millis(5),
        compact_min: u64::MAX,
        ..crate::OpenOptions::default()
    };
    let store = crate::Store::open(dir.path(), "idle", opts).unwrap();
    for round in 0..20 {
        store
            .set(&format!("k{round}"), Value::Num(round as f64))
            .unwrap();
        std::thread::sleep(Duration::from_millis(40));
        let ops = decode_all(&std::fs::read(dir.path().join("idle.wal")).unwrap());
        assert_eq!(ops.len(), round + 1, "record {round} stranded in the log");
    }
    store.close().unwrap();
}

#[test]
fn concurrent_sporadic_writers_never_strand_records() {
    const THREADS: u64 = 4;
    const WRITES: u64 = 300;
    const KEYS: u64 = 20;
    let dir = tempfile::tempdir().unwrap();
    let opts = crate::OpenOptions {
        group_window: Duration::from_millis(1),
        writer_linger: Duration::ZERO,
        compact_min: u64::MAX,
        ..crate::OpenOptions::default()
    };
    let store = crate::Store::open(dir.path(), "race", opts).unwrap();
    let writers = (0..THREADS)
        .map(|thread| {
            let store = store.clone();
            std::thread::spawn(move || {
                let mut state = (thread + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
                for write in 0..WRITES {
                    let key = format!("t{thread}_{}", write % KEYS);
                    store.set(&key, Value::Num(write as f64)).unwrap();
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    if state % 4 == 0 {
                        std::thread::sleep(Duration::from_micros(state % 700));
                    }
                }
            })
        })
        .collect::<Vec<_>>();
    for writer in writers {
        writer.join().unwrap();
    }
    std::thread::sleep(Duration::from_millis(100));
    let copy = tempfile::tempdir().unwrap();
    std::fs::copy(dir.path().join("race.wal"), copy.path().join("race.wal")).unwrap();
    store.close().unwrap();
    let recovered = crate::Store::open(copy.path(), "race", crate::OpenOptions::default()).unwrap();
    for thread in 0..THREADS {
        for key in 0..KEYS {
            let last = WRITES - KEYS + key;
            assert_eq!(
                recovered.get(&format!("t{thread}_{key}")),
                Some(Value::Num(last as f64)),
                "t{thread}_{key} stranded"
            );
        }
    }
    recovered.close().unwrap();
}

#[test]
fn compaction_logs_queued_batches_before_the_snapshot_replaces_them() {
    let dir = tempfile::tempdir().unwrap();
    let opts = crate::OpenOptions {
        compact_min: u64::MAX,
        ..crate::OpenOptions::default()
    };
    let store = crate::Store::open(dir.path(), "tx", opts).unwrap();
    let set = |key: &str, value: f64| crate::BatchOp::Set {
        key: key.into(),
        value: Value::Num(value),
    };
    store.apply_batch(&[set("a", 0.0)]).unwrap();
    store.flush().unwrap();
    let wal = store.wal_for_test().unwrap();
    wal.hooks().hold_writes.store(true, Ordering::Release);
    store.apply_batch(&[set("a", 1.0), set("b", 1.0)]).unwrap();
    wal.hooks()
        .crash_after_snapshot
        .store(true, Ordering::Release);
    wal.hooks().compact_now.store(true, Ordering::Release);
    wal.wake();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !wal.hooks().crashed.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline, "compaction never ran");
        std::thread::sleep(Duration::from_millis(1));
    }
    let crashed = tempfile::tempdir().unwrap();
    for name in ["tx.snap", "tx.wal"] {
        std::fs::copy(dir.path().join(name), crashed.path().join(name)).unwrap();
    }
    store.close().unwrap();
    let recovered =
        crate::Store::open(crashed.path(), "tx", crate::OpenOptions::default()).unwrap();
    assert_eq!(
        (recovered.get("a"), recovered.get("b")),
        (Some(Value::Num(1.0)), Some(Value::Num(1.0))),
        "a transaction must recover whole or not at all"
    );
    recovered.close().unwrap();
}
