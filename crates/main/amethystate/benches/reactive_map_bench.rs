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

use amethystate::store::StoreBackend;
use amethystate::{ReactiveMap, Store, StoreBuilder};
use amethystate_core::test_utils::TempPath;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::time::Duration;

type Map = ReactiveMap<String, u64>;

/// A stored value with more than one field in it, which is what a declared
/// struct actually is.
///
/// A `u64` decodes in about eleven nanoseconds, so measuring parallelism
/// against one measures the handing out and nothing else. This is the smallest
/// shape that is not that.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default, PartialEq)]
struct Row {
    title: String,
    host: String,
    port: u16,
    done: bool,
    updated_at: i64,
}

const SIZES: [usize; 3] = [10, 1_000, 10_000];

/// The sizes for the two groups that answer "what does it cost to have this
/// much data", rather than "what does one operation cost".
///
/// It runs two decades further than [`SIZES`] because the answer is a design
/// input rather than a curiosity: this library is aimed at a million records
/// at the outside, and where opening stops fitting in a frame is what decides
/// whether a collection can be built on the rendering thread. Extrapolating
/// from ten thousand was an argument; these are measurements.
///
/// Only the scan and the open take it. The per-operation groups measure a
/// single insert or a single read, and those do not become more interesting
/// with a larger map behind them - they become slower to set up.
const OPEN_SIZES: [usize; 5] = [10, 1_000, 10_000, 100_000, 1_000_000];

/// A store and the guard that removes its files.
///
/// The guard is first in every tuple these helpers return, so it is declared
/// first at the call site and dropped last: the store closes, and only then is
/// the file swept. A million entries is 64 MiB of redb, and a bench run that
/// keeps them leaves that much behind per group.
fn store(tag: &str) -> (TempPath, Store) {
    let path = TempPath::new(tag);
    let store = StoreBuilder::new(path.path())
        .debounce(Duration::from_secs(100))
        .build()
        .unwrap();
    (path, store)
}

fn key(i: usize) -> String {
    format!("k{i:07}")
}

fn populated(tag: &str, n: usize) -> (TempPath, Store, Map) {
    let (path, store) = store(tag);
    let map = store.kv().map::<String, u64>("bench").unwrap();
    for i in 0..n {
        map.insert(key(i), &(i as u64)).unwrap();
    }
    (path, store, map)
}

/// The same, with everything committed and the buffer empty.
///
/// Which is the other half of the question and the one an application asks:
/// [`populated`] leaves every entry pending, so a scan over it folds the write
/// buffer and never reaches the engine. Starting up is the reverse - the file
/// holds the data and nothing is buffered - and the two are different code
/// with different costs. A figure from one of them does not answer for the
/// other.
fn committed(tag: &str, n: usize) -> (TempPath, Store, Map) {
    let (path, store, map) = populated(tag, n);
    StoreBackend::save_now(&store).unwrap();
    (path, store, map)
}

fn bench_writes(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_write_vs_size");

    for n in SIZES {
        let (_tmp, _store, map) = populated("map-insert", n);
        let mut next = n;
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::new("insert_new", n), &n, |b, _| {
            b.iter(|| {
                black_box(map.insert(key(next), &1).unwrap());
                next += 1;
            })
        });

        let (_tmp, _store, map) = populated("map-replace", n.max(1));
        group.bench_with_input(BenchmarkId::new("insert_existing", n), &n, |b, _| {
            b.iter(|| black_box(map.insert(key(0), &7).unwrap()))
        });

        let (_tmp, _store, map) = populated("map-update", n.max(1));
        group.bench_with_input(BenchmarkId::new("update", n), &n, |b, _| {
            b.iter(|| black_box(map.update(&key(0), &7).unwrap()))
        });

        let (_tmp, _store, map) = populated("map-modify", n.max(1));
        group.bench_with_input(BenchmarkId::new("modify", n), &n, |b, _| {
            b.iter(|| black_box(map.modify(&key(0), |v| *v += 1).unwrap()))
        });
    }

    group.finish();
}

fn bench_len(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_len_vs_size");

    for n in SIZES {
        let (_tmp, _store, map) = populated("map-len", n);
        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("len", n), &n, |b, _| {
            b.iter(|| black_box(map.len()))
        });
        group.bench_with_input(BenchmarkId::new("is_empty", n), &n, |b, _| {
            b.iter(|| black_box(map.is_empty()))
        });
    }

    group.finish();
}

fn bench_scans(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_scan_vs_size");

    for n in SIZES {
        let (_tmp, _store, map) = populated("map-scan", n);
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(BenchmarkId::new("entries_all", n), &n, |b, _| {
            b.iter(|| black_box(map.entries().count()))
        });
        group.bench_with_input(BenchmarkId::new("keys", n), &n, |b, _| {
            b.iter(|| black_box(map.keys().len()))
        });
        group.bench_with_input(BenchmarkId::new("entries_take1", n), &n, |b, _| {
            b.iter(|| black_box(map.entries().take(1).count()))
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
    group.sample_size(10);

    for n in OPEN_SIZES {
        let (_tmp, store, _map) = populated("store-scan", n);
        let prefix = amethystate_core::path::StorePath::segment("bench");
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(BenchmarkId::new("scan_keys", n), &n, |b, _| {
            b.iter(|| black_box(StoreBackend::scan_keys(&store, &prefix).unwrap().len()))
        });
        group.bench_with_input(BenchmarkId::new("scan_prefix", n), &n, |b, _| {
            b.iter(|| black_box(StoreBackend::scan_prefix(&store, &prefix).unwrap().len()))
        });

        let (_tmp2, store, _map) = committed("store-scan-committed", n);
        group.bench_with_input(BenchmarkId::new("scan_keys_committed", n), &n, |b, _| {
            b.iter(|| black_box(StoreBackend::scan_keys(&store, &prefix).unwrap().len()))
        });
        group.bench_with_input(BenchmarkId::new("scan_prefix_committed", n), &n, |b, _| {
            b.iter(|| black_box(StoreBackend::scan_prefix(&store, &prefix).unwrap().len()))
        });
    }

    group.finish();
}

/// Opening a map over entries that are already there, which is the scan plus
/// everything done with its answer.
///
/// The end of the chain, and the only place that says whether a cost was
/// removed or moved: `load_map` used to parse every key the scan handed it, so
/// a scan that hands back paths can only be judged from here.
fn bench_map_open(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_open_over_existing");
    group.sample_size(10);

    for n in OPEN_SIZES {
        let (_tmp, store, _map) = populated("map-open", n);
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(BenchmarkId::new("open", n), &n, |b, _| {
            b.iter(|| black_box(store.kv().map::<String, u64>("bench").unwrap()))
        });

        let (_tmp2, store, _map) = committed("map-open-committed", n);
        group.bench_with_input(BenchmarkId::new("open_committed", n), &n, |b, _| {
            b.iter(|| black_box(store.kv().map::<String, u64>("bench").unwrap()))
        });
    }

    group.finish();
}

fn bench_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_read");

    for n in SIZES {
        let (_tmp, _store, map) = populated("map-read", n);
        let hit = key(n / 2);
        let miss = "absent".to_string();

        group.bench_with_input(BenchmarkId::new("get_hit", n), &n, |b, _| {
            b.iter(|| black_box(map.get(&hit).unwrap()))
        });
        group.bench_with_input(BenchmarkId::new("get_miss", n), &n, |b, _| {
            b.iter(|| black_box(map.get(&miss).unwrap()))
        });
        group.bench_with_input(BenchmarkId::new("contains_key_hit", n), &n, |b, _| {
            b.iter(|| black_box(map.contains_key(&hit)))
        });
    }

    group.finish();
}

fn bench_subscribers(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_write_vs_subscribers");

    for subs in [0usize, 1, 16, 64, 256, 1024, 4096] {
        let (_tmp, _store, map) = populated("map-subs", 100);
        let handles: Vec<_> = (0..subs)
            .map(|_| {
                map.subscribe_any(|change| {
                    black_box(change);
                })
            })
            .collect();

        group.bench_with_input(BenchmarkId::new("subscribe_any", subs), &subs, |b, _| {
            b.iter(|| black_box(map.update(&key(0), &7).unwrap()))
        });

        drop(handles);
    }

    let (_tmp, _store, map) = populated("map-subkey", 100);
    let _sub = map.subscribe_key(key(0), |change| {
        black_box(change);
    });
    group.bench_function("subscribe_key_hit", |b| {
        b.iter(|| black_box(map.update(&key(0), &7).unwrap()))
    });

    group.finish();
}

fn bench_durability(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_write_durability");
    group.sample_size(30);

    let (_tmp, _store, map) = populated("map-buffered", 100);
    group.bench_function("buffered_insert", |b| {
        b.iter(|| black_box(map.insert(key(0), &7).unwrap()))
    });

    let (_tmp, _store, map) = populated("map-durable", 100);
    group.bench_function("durable_insert_with_commit", |b| {
        b.iter(|| black_box(map.durable().insert(key(0), &7).unwrap()))
    });

    group.finish();
}

/// What inside an open could be done on more than one core, and what it would
/// buy.
///
/// Opening is a scan and then a decode, and the scan is most of it: at a
/// million entries `scan_keys` is 3.7 s of a 6.3 s open before a single value
/// is looked at. So "decode in parallel" aims at the smaller half, and the
/// question worth measuring is what the larger half is made of. Two of the
/// three pieces are ordinary CPU work over independent items and would divide
/// across cores; the third is a walk down a B-tree and would not.
///
/// These measure the pieces on their own, away from the store, because the
/// point is the ceiling rather than the integration: parsing every key and
/// decoding every value are what an open would have to do however it is
/// arranged, and subtracting them from the scan leaves the walk.
fn bench_open_parallelism(c: &mut Criterion) {
    use amethystate_core::path::StorePath;
    use rayon::prelude::*;

    let mut group = c.benchmark_group("open_parallelism");
    group.sample_size(10);

    // Close enough together to find where splitting the work starts paying
    // for itself: below some size the handing out and collecting costs more
    // than the work does, and a decade between samples cannot say where.
    for n in [
        100usize, 300, 1_000, 3_000, 10_000, 30_000, 100_000, 1_000_000,
    ] {
        let joined: Vec<String> = (0..n).map(|i| format!("bench.{}", key(i))).collect();
        let encoded: Vec<Vec<u8>> = (0..n)
            .map(|i| rmp_serde::to_vec(&(i as u64)).unwrap())
            .collect();
        let rows: Vec<Vec<u8>> = (0..n)
            .map(|i| {
                rmp_serde::to_vec(&Row {
                    title: format!("row number {i}"),
                    host: "127.0.0.1".to_string(),
                    port: (i % 65535) as u16,
                    done: i.is_multiple_of(3),
                    updated_at: i as i64,
                })
                .unwrap()
            })
            .collect();

        group.throughput(Throughput::Elements(n as u64));

        // Folded rather than counted: a `map` whose results are thrown away is
        // a computation the compiler may drop, and a benchmark that measures
        // nothing reports something.
        //
        // Folded on `as_str`, not on `len`: a path splits its levels lazily,
        // `len` is one of the things that asks for the split, and a scan never
        // does - so measuring it would price work the scan does not pay.
        group.bench_with_input(BenchmarkId::new("parse_keys_seq", n), &n, |b, _| {
            b.iter(|| {
                black_box(
                    joined
                        .iter()
                        .map(|j| StorePath::parse_joined(j).unwrap().as_str().len())
                        .sum::<usize>(),
                )
            })
        });
        group.bench_with_input(BenchmarkId::new("parse_keys_par", n), &n, |b, _| {
            b.iter(|| {
                black_box(
                    joined
                        .par_iter()
                        .map(|j| StorePath::parse_joined(j).unwrap().as_str().len())
                        .sum::<usize>(),
                )
            })
        });

        group.bench_with_input(BenchmarkId::new("decode_rows_seq", n), &n, |b, _| {
            b.iter(|| {
                black_box(
                    rows.iter()
                        .map(|v| rmp_serde::from_slice::<Row>(v).unwrap())
                        .fold(0i64, |acc, r| acc ^ r.updated_at),
                )
            })
        });
        group.bench_with_input(BenchmarkId::new("decode_rows_par", n), &n, |b, _| {
            b.iter(|| {
                black_box(
                    rows.par_iter()
                        .map(|v| rmp_serde::from_slice::<Row>(v).unwrap())
                        .map(|r| r.updated_at)
                        .reduce(|| 0i64, |a, b| a ^ b),
                )
            })
        });

        group.bench_with_input(BenchmarkId::new("decode_values_seq", n), &n, |b, _| {
            b.iter(|| {
                black_box(
                    encoded
                        .iter()
                        .map(|v| rmp_serde::from_slice::<u64>(v).unwrap())
                        .fold(0u64, |acc, v| acc ^ v),
                )
            })
        });
        group.bench_with_input(BenchmarkId::new("decode_values_par", n), &n, |b, _| {
            b.iter(|| {
                black_box(
                    encoded
                        .par_iter()
                        .map(|v| rmp_serde::from_slice::<u64>(v).unwrap())
                        .reduce(|| 0u64, |a, b| a ^ b),
                )
            })
        });
    }

    group.finish();
}

criterion_group!(
    reactive_map,
    bench_writes,
    bench_len,
    bench_scans,
    bench_store_scan,
    bench_map_open,
    bench_reads,
    bench_subscribers,
    bench_durability,
    bench_open_parallelism
);
criterion_main!(reactive_map);
