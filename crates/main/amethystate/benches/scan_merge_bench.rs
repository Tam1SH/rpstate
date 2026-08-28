//! What laying the write buffer over the engine's answer costs, on its own.
//!
//! A scan builds two sorted lists - what the engine holds and what is buffered
//! over it - and merges them. `reactive_map_bench` times the whole scan, where
//! the merge is a small part of a large number and its own cost cannot be read
//! off the total: at a hundred thousand entries a scan is tens of milliseconds
//! and the spread between samples is wider than the merge.
//!
//! So this measures the merge and nothing else, and measures both ways of
//! doing it against each other:
//!
//! - `allocating` - two iterators feeding a third list, asked of the allocator
//!   on every scan.
//! - `in_place` - the same walk, backwards, writing into the tail of the list
//!   the engine's side already owns, then shifting the answer down to the
//!   front.
//! - `shipped` - whichever of the two `merge_buffered` currently is, so the
//!   comparison says something about the code that runs rather than about two
//!   copies kept here.
//!
//! Both algorithms live here rather than one of them living in the library,
//! because the reason for choosing between them is a measurement and a
//! measurement needs both sides to still exist. `in_place` lost: removing the
//! allocation costs a `memmove` of the whole answer at the end, which is worse
//! than the allocation everywhere the buffer is not empty.
//!
//! The shapes are the states a store is actually in. Nothing buffered is the
//! moment after a flush; a handful buffered is a moment after somebody edited
//! something; half is what a burst of writes leaves; everything buffered is a
//! store whose debounce has not fired yet, which is where a bench harness
//! usually sits.

use amethystate::store::backend::utils::merge_buffered;
use amethystate_core::path::StorePath;
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

type Committed = Vec<(StorePath, Vec<u8>)>;
type Buffered = Vec<(StorePath, Option<Vec<u8>>)>;

/// Two iterators feeding a third list.
fn merge_allocating(committed: Committed, buffered: Buffered) -> Committed {
    let mut out = Vec::with_capacity(committed.len() + buffered.len());
    let mut left = committed.into_iter().peekable();
    let mut right = buffered.into_iter().peekable();

    loop {
        let take_left = match (left.peek(), right.peek()) {
            (Some((a, _)), Some((b, _))) => a <= b,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };

        if take_left {
            let (key, value) = left.next().expect("peeked");
            if right.peek().is_some_and(|(b, _)| *b == key) {
                continue;
            }
            out.push((key, value));
        } else {
            let (key, value) = right.next().expect("peeked");
            if let Some(value) = value {
                out.push((key, value));
            }
        }
    }

    out
}

/// The same walk backwards, writing into the tail of `committed`'s own buffer
/// and then shifting the answer down to the front.
///
/// Backwards because forwards would overwrite entries the walk has not read
/// yet. The shift at the end is what it costs, and is why it lost.
fn merge_in_place(mut committed: Committed, mut buffered: Buffered) -> Committed {
    fn empty_slot() -> (StorePath, Vec<u8>) {
        (StorePath::root(), Vec::new())
    }

    if buffered.is_empty() {
        return committed;
    }

    let mut c = committed.len();
    let mut b = buffered.len();
    let mut w = c + b;
    committed.resize(w, empty_slot());

    while c > 0 && b > 0 {
        match committed[c - 1].0.cmp(&buffered[b - 1].0) {
            std::cmp::Ordering::Greater => {
                c -= 1;
                w -= 1;
                committed[w] = std::mem::replace(&mut committed[c], empty_slot());
            }
            std::cmp::Ordering::Less => {
                b -= 1;
                if let (key, Some(value)) = buffered.pop().expect("b > 0") {
                    w -= 1;
                    committed[w] = (key, value);
                }
            }
            std::cmp::Ordering::Equal => {
                c -= 1;
                b -= 1;
                committed[c] = empty_slot();
                if let (key, Some(value)) = buffered.pop().expect("b > 0") {
                    w -= 1;
                    committed[w] = (key, value);
                }
            }
        }
    }

    while c > 0 {
        c -= 1;
        w -= 1;
        committed[w] = std::mem::replace(&mut committed[c], empty_slot());
    }

    while b > 0 {
        b -= 1;
        if let (key, Some(value)) = buffered.pop().expect("b > 0") {
            w -= 1;
            committed[w] = (key, value);
        }
    }

    committed.drain(..w);
    committed
}

fn key(i: usize) -> StorePath {
    StorePath::from_segments(["bench", &format!("k{i:07}")])
}

/// A value the size of a small stored struct. The merge moves these rather
/// than copying their bytes, so the length is about what the allocator was
/// asked for and not about what the merge does.
fn value(i: usize) -> Vec<u8> {
    vec![(i % 251) as u8; 48]
}

/// `n` committed entries with `pending` of them written again, spread across
/// the range so the buffer's turn comes throughout the walk rather than once.
///
/// `deletes` says how many of the buffered ops remove their key instead of
/// replacing it, which is the branch that makes the output shorter than either
/// input.
fn lists(n: usize, pending: usize, deletes: usize) -> (Committed, Buffered) {
    let committed: Committed = (0..n).map(|i| (key(i), value(i))).collect();

    let stride = (n / pending.max(1)).max(1);
    let buffered: Buffered = (0..n)
        .step_by(stride)
        .take(pending)
        .enumerate()
        .map(|(seen, i)| {
            let op = if seen < deletes {
                None
            } else {
                Some(value(i + 1))
            };
            (key(i), op)
        })
        .collect();

    (committed, buffered)
}

/// Everything in the buffer and nothing committed, which is a store that has
/// not flushed yet.
fn all_buffered(n: usize) -> (Committed, Buffered) {
    (
        Vec::new(),
        (0..n).map(|i| (key(i), Some(value(i)))).collect(),
    )
}

fn bench_merge(c: &mut Criterion) {
    let mut group = c.benchmark_group("scan_merge");

    for n in [1_000usize, 10_000, 100_000] {
        group.throughput(Throughput::Elements(n as u64));

        let shapes: [(&str, (Committed, Buffered)); 5] = [
            ("nothing buffered", (lists(n, 0, 0).0, Vec::new())),
            ("32 buffered", lists(n, 32, 0)),
            ("32 buffered, half of them deletes", lists(n, 32, 16)),
            ("half buffered", lists(n, n / 2, 0)),
            ("all buffered", all_buffered(n)),
        ];

        // Freeing the answer on its own, with nothing else in the way: the
        // list is built in setup and the routine only drops it. Whatever a
        // merge costs, a scan pays this too, and it is the same number
        // whichever algorithm ran.
        {
            let answer: Committed = (0..n).map(|i| (key(i), value(i))).collect();
            group.bench_with_input(BenchmarkId::new("free the answer", n), &n, |b, _| {
                b.iter_batched(|| answer.clone(), drop, BatchSize::LargeInput)
            });
        }

        for (shape, (committed, buffered)) in shapes {
            // `in_place` writes past the end of the engine's list, so the room
            // has to be there before it starts. A caller reserves it while
            // building the list, where it costs nothing; a `resize` inside the
            // merge would reallocate and copy, which is the allocation the
            // whole idea was to avoid. `reserved` says which arm gets it, so
            // each one is measured under the conditions it would really run
            // in.
            let arms: [(&str, fn(Committed, Buffered) -> Committed, bool); 3] = [
                ("allocating", merge_allocating, false),
                ("in_place", merge_in_place, true),
                ("shipped", merge_buffered, false),
            ];

            for (name, merge, reserved) in arms {
                let setup = || {
                    let mut committed = committed.clone();
                    if reserved {
                        committed.reserve(buffered.len());
                    }
                    (committed, buffered.clone())
                };

                // Returning the answer leaves its drop to `iter_batched`,
                // which frees the batch after it stops the clock. This is the
                // algorithm and nothing else.
                group.bench_with_input(
                    BenchmarkId::new(format!("{name}/{shape}"), n),
                    &n,
                    |b, _| b.iter_batched(setup, |(c, f)| merge(c, f), BatchSize::LargeInput),
                );

                // The same with the answer freed before the clock stops, which
                // is what a caller does. The difference between the two rows
                // is the cost of freeing it, measured rather than subtracted.
                group.bench_with_input(
                    BenchmarkId::new(format!("{name} and free it/{shape}"), n),
                    &n,
                    |b, _| {
                        b.iter_batched(
                            setup,
                            |(c, f)| black_box(merge(c, f).len()),
                            BatchSize::LargeInput,
                        )
                    },
                );
            }
        }
    }

    group.finish();
}

criterion_group!(scan_merge, bench_merge);
criterion_main!(scan_merge);
