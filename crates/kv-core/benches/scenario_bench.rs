use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use kv_core::{OpenOptions, Store, Value};

const IDLE_GAP: Duration = Duration::from_millis(2);

fn keys(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("item.{index:06}")).collect()
}

fn seed(store: &Store, keys: &[String], payload: &str) {
    for key in keys {
        store.set(key, Value::Str(payload.into())).unwrap();
    }
}

fn random_order(count: usize, len: usize) -> Vec<usize> {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % count as u64) as usize
        })
        .collect()
}

fn bench_after_idle(c: &mut Criterion) {
    let mut group = c.benchmark_group("after_idle_2ms");
    group
        .sample_size(10)
        .measurement_time(Duration::from_secs(4));
    let dir = tempfile::tempdir().unwrap();
    let stores = [
        ("in_memory", Store::in_memory()),
        (
            "wal",
            Store::open(dir.path(), "idle", OpenOptions::default()).unwrap(),
        ),
    ];
    let payload = "x".repeat(16);
    let keys = keys(100);
    for (name, store) in &stores {
        seed(store, &keys, &payload);
        group.bench_function(BenchmarkId::new("set_str16", name), |b| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for index in 0..iters as usize {
                    std::thread::sleep(IDLE_GAP);
                    let value = Value::Str(payload.as_str().into());
                    let started = Instant::now();
                    store.set(&keys[index % keys.len()], value).unwrap();
                    total += started.elapsed();
                }
                total
            })
        });
        group.bench_function(BenchmarkId::new("get_str16", name), |b| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for index in 0..iters as usize {
                    std::thread::sleep(IDLE_GAP);
                    let started = Instant::now();
                    black_box(store.with_value(&keys[index % keys.len()], |_| ()));
                    total += started.elapsed();
                }
                total
            })
        });
    }
    group.finish();
}

fn bench_random_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_get_str16");
    let payload = "x".repeat(16);
    for count in [1_000usize, 10_000, 100_000] {
        let store = Store::in_memory();
        let keys = keys(count);
        seed(&store, &keys, &payload);
        let order = random_order(count, 1 << 16);
        let mut position = 0usize;
        group.bench_function(BenchmarkId::from_parameter(count), |b| {
            b.iter(|| {
                position = (position + 1) & 0xffff;
                black_box(store.with_value(&keys[order[position]], |_| ()))
            })
        });
    }
    group.finish();
}

fn copy_files(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
    }
}

fn bench_reopen(c: &mut Criterion) {
    let mut group = c.benchmark_group("reopen_10k_str64");
    group.sample_size(10);
    let payload = "x".repeat(64);
    let keys = keys(10_000);
    for (name, compacted) in [("wal_replay", false), ("snapshot", true)] {
        let template = tempfile::tempdir().unwrap();
        let never = OpenOptions {
            compact_min: u64::MAX,
            ..OpenOptions::default()
        };
        let store = Store::open(template.path(), "reopen", never).unwrap();
        seed(&store, &keys, &payload);
        store.close().unwrap();
        if compacted {
            let eager = OpenOptions {
                compact_min: 1,
                ..OpenOptions::default()
            };
            let store = Store::open(template.path(), "reopen", eager).unwrap();
            // The writer compacts in the tick after serving the flush; close() joins it.
            store.flush().unwrap();
            store.close().unwrap();
        }
        group.bench_function(name, |b| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let dir = tempfile::tempdir().unwrap();
                    copy_files(template.path(), dir.path());
                    let started = Instant::now();
                    let store = Store::open(dir.path(), "reopen", OpenOptions::default()).unwrap();
                    total += started.elapsed();
                    assert_eq!(store.len(), keys.len());
                    store.close().unwrap();
                }
                total
            })
        });
    }
    group.finish();
}

fn bench_durable(c: &mut Criterion) {
    let mut group = c.benchmark_group("durable");
    group.sample_size(10);
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path(), "durable", OpenOptions::default()).unwrap();
    let payload = "x".repeat(256);
    let keys = keys(1000);
    group.bench_function("1000x_set_str256_then_flush", |b| {
        b.iter(|| {
            for key in &keys {
                store.set(key, Value::Str(payload.as_str().into())).unwrap();
            }
            store.flush().unwrap();
        })
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_after_idle,
    bench_random_get,
    bench_reopen,
    bench_durable
);
criterion_main!(benches);
