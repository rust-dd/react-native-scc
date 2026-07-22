use std::hint::black_box;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use kv_core::{BatchOp, OpenOptions, Store, Value};

fn seed_keys(store: &Store, n: usize) -> Vec<String> {
    let keys: Vec<String> = (0..n).map(|i| format!("key_{i:05}")).collect();
    let payload = "x".repeat(64);
    for k in &keys {
        store.set(k, Value::Str(payload.clone().into())).unwrap();
    }
    keys
}

fn bench_get(c: &mut Criterion) {
    let store = Store::in_memory();
    let keys = seed_keys(&store, 1024);
    let mut i = 0usize;
    c.bench_function("get/str64/in_memory", |b| {
        b.iter(|| {
            i = (i + 1) & 1023;
            black_box(store.get(&keys[i]))
        })
    });

    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path(), "bench_get", OpenOptions::default()).unwrap();
    let keys = seed_keys(&store, 1024);
    let mut i = 0usize;
    c.bench_function("get/str64/wal", |b| {
        b.iter(|| {
            i = (i + 1) & 1023;
            black_box(store.get(&keys[i]))
        })
    });
}

fn bench_set(c: &mut Criterion) {
    let payload = "x".repeat(64);

    let store = Store::in_memory();
    let keys = seed_keys(&store, 1024);
    let mut i = 0usize;
    c.bench_function("set/overwrite_str64/in_memory", |b| {
        b.iter(|| {
            i = (i + 1) & 1023;
            store
                .set(&keys[i], Value::Str(payload.clone().into()))
                .unwrap()
        })
    });

    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path(), "bench_set", OpenOptions::default()).unwrap();
    let keys = seed_keys(&store, 1024);
    let mut i = 0usize;
    c.bench_function("set/overwrite_str64/wal", |b| {
        b.iter(|| {
            i = (i + 1) & 1023;
            store
                .set(&keys[i], Value::Str(payload.clone().into()))
                .unwrap()
        })
    });

    let mut n = 0u64;
    c.bench_function("set/insert_str64/wal", |b| {
        b.iter(|| {
            n += 1;
            store
                .set(&format!("fresh_{n}"), Value::Str(payload.clone().into()))
                .unwrap()
        })
    });
}

fn bench_mixed(c: &mut Criterion) {
    let payload = "x".repeat(64);
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path(), "bench_mix", OpenOptions::default()).unwrap();
    let keys = seed_keys(&store, 1024);
    let mut i = 0usize;
    c.bench_function("mixed/90r10w_str64/wal", |b| {
        b.iter(|| {
            i = (i + 1) & 1023;
            if i.is_multiple_of(10) {
                store
                    .set(&keys[i], Value::Str(payload.clone().into()))
                    .unwrap();
            } else {
                black_box(store.get(&keys[i]));
            }
        })
    });
}

fn transaction_ops() -> Vec<BatchOp> {
    (0..100)
        .map(|i| BatchOp::Set {
            key: format!("tx_{i:03}"),
            value: Value::Str("x".repeat(16).into()),
        })
        .collect()
}

fn bench_transaction_batch(c: &mut Criterion) {
    let borrowed = Store::in_memory();
    let owned = Store::in_memory();
    let mut group = c.benchmark_group("transaction_batch/100x_str16");
    group.throughput(Throughput::Elements(100));
    group.bench_function("borrowed_clone_values", |b| {
        b.iter_batched(
            transaction_ops,
            |ops| borrowed.apply_batch(black_box(&ops)).unwrap(),
            BatchSize::SmallInput,
        )
    });
    group.bench_function("owned_move_values", |b| {
        b.iter_batched(
            transaction_ops,
            |ops| owned.apply_batch_owned(black_box(ops)).unwrap(),
            BatchSize::SmallInput,
        )
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_get,
    bench_set,
    bench_mixed,
    bench_transaction_batch
);
criterion_main!(benches);
