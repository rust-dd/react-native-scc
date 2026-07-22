use super::*;
use crate::record::{self, DecodeOutcome, Op, OwnedOp};
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
        cipher: None,
        listeners: Arc::new(Listeners::new()),
        sweep_interval: Duration::from_secs(3600),
        max_entries: None,
        compact_gate: Arc::new(RwLock::new(())),
        mutation_gate: Arc::new(Mutex::new(())),
        closed: Arc::new(AtomicBool::new(false)),
    }
}

fn encode_set(key: &str, value: &Value) -> Vec<u8> {
    let mut buf = Vec::new();
    record::encode(&Op::Set { key, value }, &mut buf);
    buf
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
    handle.append(encode_set("a", &Value::Num(1.0))).unwrap();
    handle
        .append(encode_set("b", &Value::Str("x".into())))
        .unwrap();
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
    handle.append(encode_set("k", &Value::Bool(true))).unwrap();
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
        handle
            .append(encode_set(&format!("k{i}"), &Value::Num(i as f64)))
            .unwrap();
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
        handle.append(encode_set("k", &Value::Num(1.0))),
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
        handle
            .append(encode_set("hot", &Value::Num(i as f64)))
            .unwrap();
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
fn buffer_retention_is_bounded() {
    assert!(should_retain_capacity(MAX_RETAINED_BUFFER_CAPACITY));
    assert!(!should_retain_capacity(MAX_RETAINED_BUFFER_CAPACITY + 1));
}
