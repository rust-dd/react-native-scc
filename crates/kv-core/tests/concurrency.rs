use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Duration;

use kv_core::{OpenOptions, Store, Value};

#[test]
fn concurrent_writers_readers_and_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let opts = OpenOptions {
        compact_min: 2048,
        group_window: Duration::from_millis(1),
        ..OpenOptions::default()
    };
    let store = Store::open(dir.path(), "stress", opts.clone()).unwrap();

    const WRITERS: usize = 4;
    const OPS: usize = 500;
    const KEYS_PER_WRITER: usize = 50;

    let stop = Arc::new(AtomicBool::new(false));
    let readers: Vec<_> = (0..2)
        .map(|_| {
            let store = store.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut reads = 0usize;
                while !stop.load(Ordering::Relaxed) {
                    for t in 0..WRITERS {
                        if let Some(Value::Num(n)) = store.get(&format!("t{t}_k0")) {
                            assert!(n >= 0.0 && n < OPS as f64);
                            reads += 1;
                        }
                    }
                }
                reads
            })
        })
        .collect();

    let writers: Vec<_> = (0..WRITERS)
        .map(|t| {
            let store = store.clone();
            std::thread::spawn(move || {
                for j in 0..OPS {
                    let key = format!("t{t}_k{}", j % KEYS_PER_WRITER);
                    store.set(&key, Value::Num(j as f64)).unwrap();
                }
            })
        })
        .collect();

    for w in writers {
        w.join().unwrap();
    }
    stop.store(true, Ordering::Relaxed);
    for r in readers {
        assert!(r.join().unwrap() > 0, "reader never observed a value");
    }

    store.flush().unwrap();
    let expected_last = |k: usize| (OPS - KEYS_PER_WRITER + k) as f64;
    for t in 0..WRITERS {
        for k in 0..KEYS_PER_WRITER {
            assert_eq!(
                store.get(&format!("t{t}_k{k}")),
                Some(Value::Num(expected_last(k))),
                "wrong live value t{t}_k{k}"
            );
        }
    }
    assert_eq!(store.len(), WRITERS * KEYS_PER_WRITER);
    store.close().unwrap();

    let reopened = Store::open(dir.path(), "stress", opts).unwrap();
    assert_eq!(reopened.len(), WRITERS * KEYS_PER_WRITER);
    for t in 0..WRITERS {
        for k in 0..KEYS_PER_WRITER {
            assert_eq!(
                reopened.get(&format!("t{t}_k{k}")),
                Some(Value::Num(expected_last(k))),
                "wrong persisted value t{t}_k{k}"
            );
        }
    }
    reopened.close().unwrap();
}

#[test]
fn concurrent_same_key_writes_match_recovered_state() {
    const WRITERS: usize = 8;
    const ROUNDS: usize = 256;

    let dir = tempfile::tempdir().unwrap();
    let opts = OpenOptions {
        compact_min: u64::MAX,
        group_window: Duration::from_millis(1),
        ..OpenOptions::default()
    };
    let store = Store::open(dir.path(), "same_key", opts.clone()).unwrap();
    let barrier = Arc::new(Barrier::new(WRITERS + 1));
    let writers = (0..WRITERS)
        .map(|writer| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                for round in 0..ROUNDS {
                    barrier.wait();
                    store
                        .set(
                            &format!("round_{round}"),
                            Value::Num((round * WRITERS + writer) as f64),
                        )
                        .unwrap();
                    barrier.wait();
                }
            })
        })
        .collect::<Vec<_>>();

    let mut expected = Vec::with_capacity(ROUNDS);
    for round in 0..ROUNDS {
        barrier.wait();
        barrier.wait();
        expected.push(store.get(&format!("round_{round}")).unwrap());
    }
    for writer in writers {
        writer.join().unwrap();
    }
    store.flush().unwrap();
    store.close().unwrap();

    let reopened = Store::open(dir.path(), "same_key", opts).unwrap();
    for (round, value) in expected.into_iter().enumerate() {
        assert_eq!(reopened.get(&format!("round_{round}")), Some(value));
    }
    reopened.close().unwrap();
}
