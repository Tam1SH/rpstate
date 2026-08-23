//! Where the time goes in `ReactiveMap`.
//!
//! The questions an app author actually asks: does a write get slower as the
//! map fills up, what does `len()` cost when it has to scan, does `entries()`
//! pay for values it never yields, and what do subscribers and durability add
//! on top of a plain write.
//!
//! Every group builds its own store under its own path - redb holds an
//! exclusive lock for the life of the process - and uses a debounce far longer
//! than any run, so a "write" bench measures the write and not a flush.

#![allow(clippy::unit_arg)]

use amethystate::{ReactiveMap, Store, StoreBuilder, WritableMode};
use amethystate_core::test_utils::unique_path;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

type Map = ReactiveMap<String, u64, WritableMode>;

const SIZES: [usize; 3] = [10, 1_000, 10_000];

fn store(tag: &str) -> Store {
    StoreBuilder::new(unique_path(tag))
        .debounce(100_000)
        .build()
        .unwrap()
}

fn key(i: usize) -> String {
    format!("k{i:07}")
}

fn populated(tag: &str, n: usize) -> (Store, Map) {
    let store = store(tag);
    let map = store.kv().map::<String, u64>("bench").unwrap();
    for i in 0..n {
        map.insert(key(i), &(i as u64)).unwrap();
    }
    (store, map)
}

fn bench_writes(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_write_vs_size");

    for n in SIZES {
        let (_store, map) = populated("map-insert", n);
        let mut next = n;
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::new("insert_new", n), &n, |b, _| {
            b.iter(|| {
                black_box(map.insert(key(next), &1).unwrap());
                next += 1;
            })
        });

        let (_store, map) = populated("map-replace", n.max(1));
        group.bench_with_input(BenchmarkId::new("insert_existing", n), &n, |b, _| {
            b.iter(|| black_box(map.insert(key(0), &7).unwrap()))
        });

        let (_store, map) = populated("map-update", n.max(1));
        group.bench_with_input(BenchmarkId::new("update", n), &n, |b, _| {
            b.iter(|| black_box(map.update(key(0), &7).unwrap()))
        });

        let (_store, map) = populated("map-modify", n.max(1));
        group.bench_with_input(BenchmarkId::new("modify", n), &n, |b, _| {
            b.iter(|| black_box(map.modify(key(0), |v| *v += 1).unwrap()))
        });
    }

    group.finish();
}

fn bench_len(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_len_vs_size");

    for n in SIZES {
        let (_store, map) = populated("map-len", n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("len", n), &n, |b, _| {
            b.iter(|| black_box(map.len().unwrap()))
        });
        group.bench_with_input(BenchmarkId::new("is_empty", n), &n, |b, _| {
            b.iter(|| black_box(map.is_empty().unwrap()))
        });
    }

    group.finish();
}

fn bench_scans(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_scan_vs_size");

    for n in SIZES {
        let (_store, map) = populated("map-scan", n);
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(BenchmarkId::new("entries_all", n), &n, |b, _| {
            b.iter(|| black_box(map.entries().unwrap().count()))
        });
        group.bench_with_input(BenchmarkId::new("keys", n), &n, |b, _| {
            b.iter(|| black_box(map.keys().unwrap().len()))
        });
        group.bench_with_input(BenchmarkId::new("entries_take1", n), &n, |b, _| {
            b.iter(|| black_box(map.entries().unwrap().take(1).count()))
        });
    }

    group.finish();
}

/// The store's own scan, with the whole map still in the write buffer.
///
/// `ReactiveMap`'s `len`, `keys` and `entries` answer from the projection and
/// never reach this, so the cost of folding the buffer over the committed rows
/// is invisible from up there - and it is the cost anything addressing the
/// store by path pays.
fn bench_store_scan(c: &mut Criterion) {
    use amethystate::store::StoreBackend;

    let mut group = c.benchmark_group("store_scan_buffered");

    for n in SIZES {
        let (store, _map) = populated("store-scan", n);
        let prefix = amethystate_core::path::StorePath::segment("bench");
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(BenchmarkId::new("scan_keys", n), &n, |b, _| {
            b.iter(|| black_box(StoreBackend::scan_keys(&store, &prefix).unwrap().len()))
        });
        group.bench_with_input(BenchmarkId::new("scan_prefix", n), &n, |b, _| {
            b.iter(|| black_box(StoreBackend::scan_prefix(&store, &prefix).unwrap().len()))
        });
    }

    group.finish();
}

fn bench_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_read");

    for n in SIZES {
        let (_store, map) = populated("map-read", n);
        let hit = key(n / 2);
        let miss = "absent".to_string();

        group.bench_with_input(BenchmarkId::new("get_hit", n), &n, |b, _| {
            b.iter(|| black_box(map.get(&hit).unwrap()))
        });
        group.bench_with_input(BenchmarkId::new("get_miss", n), &n, |b, _| {
            b.iter(|| black_box(map.get(&miss).unwrap()))
        });
        group.bench_with_input(BenchmarkId::new("contains_key_hit", n), &n, |b, _| {
            b.iter(|| black_box(map.contains_key(&hit).unwrap()))
        });
    }

    group.finish();
}

fn bench_subscribers(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_write_vs_subscribers");

    for subs in [0usize, 1, 16, 64, 256, 1024, 4096] {
        let (_store, map) = populated("map-subs", 100);
        let handles: Vec<_> = (0..subs)
            .map(|_| {
                map.subscribe_any(|change| {
                    black_box(change);
                })
            })
            .collect();

        group.bench_with_input(BenchmarkId::new("subscribe_any", subs), &subs, |b, _| {
            b.iter(|| black_box(map.update(key(0), &7).unwrap()))
        });

        drop(handles);
    }

    let (_store, map) = populated("map-subkey", 100);
    let _sub = map.subscribe_key(key(0), |change| {
        black_box(change);
    });
    group.bench_function("subscribe_key_hit", |b| {
        b.iter(|| black_box(map.update(key(0), &7).unwrap()))
    });

    group.finish();
}

fn bench_durability(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_write_durability");
    group.sample_size(30);

    let (_store, map) = populated("map-buffered", 100);
    group.bench_function("buffered_insert", |b| {
        b.iter(|| black_box(map.insert(key(0), &7).unwrap()))
    });

    let (_store, map) = populated("map-durable", 100);
    group.bench_function("durable_insert_with_commit", |b| {
        b.iter(|| black_box(map.durable().insert(key(0), &7).unwrap()))
    });

    group.finish();
}

criterion_group!(
    reactive_map,
    bench_writes,
    bench_len,
    bench_scans,
    bench_store_scan,
    bench_reads,
    bench_subscribers,
    bench_durability
);
criterion_main!(reactive_map);
