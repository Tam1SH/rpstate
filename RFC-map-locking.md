# A map that can be walked and written at the same time

**Status: built.** `ReactiveMap` has one backing shape,
`ArcSwap<rpds::RedBlackTreeMapSync>`, and a read holds no lock. There is no
knob: the locking shape has no caller who would choose it.

## What this was

`view`, `entries` and `keys` each held a read guard on the projection for as
long as the returned thing lived - `view` a borrowed one, the other two an
owning one from `read_arc`. Every write took the write side of the same
`parking_lot::RwLock`, which is not reentrant, so this hung on the same thread
and stayed hung:

```rust
for (key, _) in widths.entries() {
    widths.remove(key)?;
}
```

It is the most ordinary loop anyone would write, and there was nothing at the
call site to suggest a lock was being held. The type says `Walk`, not `Guard`.
`entries()` is the idiomatic way to walk a map, so that was the shape callers
reached for first.

## The decision

A read becomes `load()` on an `ArcSwap` and holds nothing. A walk cannot block a
writer and cannot be blocked by one, in any order and on any thread. A write
replaces the pointer; the new version shares every node it did not touch, so it
stays O(log n) rather than copying the map.

Three things follow from the price being small in absolute terms rather than
small in ratio:

**A write costs 2.32 µs at 100k against 369 ns today.** The ratio is 6.3× and
the difference is two microseconds. On the largest map the envelope allows, on
the operation that is already the cheapest thing in the library.

**A walk costs 20-30% more at the sizes anything actually walks per frame**, and
the row that looks worst - `view()` at 6.1× - is 3× at 1k and 10k, because
`view()` clones nothing and so has no overhead for the difference to hide
behind. Three times 3.34 µs settles nothing.

**Memory goes up 1.9×**, 7.0 MB to 13.2 MB at 100k. That is the real recurring
cost and it is permanent, not peak.

### What it does not buy

Writing during a walk stops deadlocking and does not become correct. The walk
keeps yielding its own version, so a write made inside the loop is invisible to
the rest of that loop. For `for k in entries { remove(k) }` that is exactly the
wanted answer. For a loop that writes and then reads the same key back, the read
is stale and nothing says so.

Loud failure is being traded for quiet staleness, not for correctness.

### Why there is no knob

The backing type is not public - `view()` returns an opaque `Entries<'_, K, V>`
and `keys()`/`entries()` return `Walk<K, V, T>` - so both shapes could be
offered and selected per map at the declaration.

Nothing would select the locking one. It wins only where a map is written in
bulk and never walked while it is written, and a reactive map is written so that
something can observe it. Bulk writes with nobody reading are the store's job.
Shipping the knob would cost two implementations of every operation, two
semantics in the documentation, and a branch inside `Walk` and `Entries` on
every `next()`, in exchange for a branch no caller takes.

## What went with it

The same-thread guard - `ReadHold`, the `READ_HOLDS` thread-local, and the
`assert!(!walked_here(id))` on the write path - existed to turn the hang into a
panic, and has nothing left to watch.

`ReactiveMap::retain` stays on its own merit: it decides over one version and
emits one change per dropped entry rather than making the caller write the loop.
That reason is independent of locking.

Also not an answer, for the record: `&mut self` on writes would let the borrow
checker refuse the loop outright, and is unavailable - the map is `Arc`-shared,
`Clone`, and written from subscription callbacks, so every method takes `&self`
by necessity. A reentrant `RwLock` is not in `parking_lot`, and a reentrant
read-then-write over a `BTreeMap` would be aliasing UB rather than a deadlock.
`DashMap` carries its own version of the same trap, recorded in the projection
entry in `TODO.md`: hold a reference into the map, touch the same shard,
deadlock.

## The measurement

`benches/map_snapshot_bench.rs` compares four shapes over eight operations at
1k / 10k / 100k, and `benches/map_snapshot_memory.rs` counts live bytes under a
counting allocator. `im` and `rpds` are dev-dependencies for that and nothing
else.

**Read the method before the numbers, because it changed them.** Shapes are
measured one per process: several in one process let heap fragmentation from the
write benches skew the walks by up to 2×. Each shape is then run twice, in
opposite order, and only agreeing runs are used. Three campaigns were run and
every one had exactly one contaminated shape - and each time the bad run was the
one that *disagreed with the other two*, not the one with the wide confidence
interval. Criterion's interval describes variance inside a run and says nothing
about a machine that got busy halfway through. A single run of this benchmark is
not evidence, however quiet the machine looks.

The `entries()` figure moved by 37% between the first campaign and the verified
one, which was the difference between a 2.7× penalty and a 1.6× one.

| at 100k entries | `RwLock<BTreeMap>` | `ArcSwap<rpds>` | |
| --- | --- | --- | --- |
| write an existing key | 369 ns | 2.32 µs | 6.3× |
| take a snapshot | 20.3 ns | 23.7 ns | free either way |
| one `get` | 309 ns | 339 ns | noise |
| walk without cloning | 494 µs | 2.99 ms | 6.1× |
| `entries()` | 7.19 ms | 11.57 ms | 1.6× |
| `keys()` | 6.98 ms | 11.19 ms | 1.6× |
| resident memory | 7.0 MB | 13.2 MB | 1.9× |

100k is the edge of the envelope. At the sizes a view walks per frame:

| | 1k | 10k | 100k |
| --- | --- | --- | --- |
| write an existing key | 5.6× | 6.8× | 6.3× |
| `entries()` | 1.2× | 1.3× | 1.6× |
| `view()` | 2.9× | 3.0× | 6.1× |

The worst row - `view()` at 100k - is a cache cliff rather than noise: two
independent processes gave 2.989 ms and 2.984 ms, walking 13.2 MB of
pointer-linked red-black nodes against 1.32 MB at 10k.

One number is worth reading twice. While a snapshot taken before a write is
still alive, `BTreeMap` holds **the whole map** - 7.0 MB at 100k, because the
snapshot is a copy - and `rpds` holds **1.0 KB**. That is what makes holding a
read across a frame a reasonable thing to do at all.

### Why not a copying snapshot

`ArcSwap<BTreeMap>` keeps reads free the same way, and `Signal` already works
like that, so the pattern is in the codebase. The write copies the whole map:
**11.7 ms** at 100k, two dropped frames per slider movement. Fine for a map of
ten, disqualified as the general answer.

### `rpds` over `im`

`im::OrdMap` is a B-tree whose nodes hold up to 64 pairs, and path-copying
clones *every pair in every node on the path* - hundreds of allocations per
write once `V` allocates, which it does here. At 100k: 12.8 µs and 17.8 KB held
per write against `rpds`'s 2.32 µs and 1.0 KB. Five and a half times the time,
seventeen times the memory, and the gap widens with whatever `(K, V)` costs to
clone. `im` wins only on `entries()`, 8.36 ms against 11.57, which is 1.4× of
the half of the trade worth less.

**The dependency is a cost of its own.** `rpds` is at 1.2.1; `im`'s last release
is 15.1.0 and its maintenance status was not checked. That check belongs before
the dependency moves out of `[dev-dependencies]`.

## The invariant the projection depends on

`map_apply_remote_change`, off the store subscription, is the only writer to the
cache. That one path covers writes made here and edits made to the file from
outside alike, which is what keeps the projection and the store agreeing.

A second writer and the two diverge, and `remove`'s gate turns silent: absent in
the cache answers `Ok(None)` and deletes nothing, on a key that is really there.

Scope, since it decides the memory question: millions of rows and blobs are not
this library's business - reach for the database directly there. Within the size
it does target, holding the map resident is the right trade.

## What this does not settle

- **Contention is unmeasured.** Everything is single-threaded, and 20.3 ns for
  `RwLock` is an uncontended acquire. This does not change the decision -
  deadlock is correctness, not throughput - but it means no number here supports
  a claim about readers and writers meeting.
- `K = String`, `V = u64`: one heap allocation per entry. With a `Copy` key the
  copying shape gets cheaper and the `im`/`rpds` write gap narrows; both figures
  are proportional to what cloning `(K, V)` costs.
- `rcu` is one CAS per write. A real implementation will still want a mutex on
  the writer side, and that is not in the benchmark.
