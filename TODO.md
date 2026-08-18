# TODO

Known and deliberate, kept here so it is not rediscovered.

Sizing note, because it has skewed judgement before: this is a store for
persistent reactive state, not a settings file. Settings are its smallest case -
tens of keys, written by hand. A cache persisted between runs is just as much
the target, and that means thousands of keys, written in bursts, read in scans.
Costs dismissed as trivial at ten keys are not trivial at ten thousand, and the
entries below are sized for the larger case.

## `flush_prefix` commits a prefix, not a write

`durable()` on one field also lands every buffered write under the same
prefix. In redb and sqlite, `pending_prefix(&lock, prefix)` collects the whole
subtree and commits it in one transaction. The text backends go further:
`flush_prefix` ignores its argument and calls `save_now()`, rewriting the entire
document, so one durable write makes the whole store durable.

The granularity dates from when `Field` was the only primitive and a prefix held
exactly one value. With `ReactiveMap` and `Kv` a prefix now covers an arbitrary
number of unrelated keys.

Two consequences: a durable write costs more than it looks, and how much it
commits depends on the engine — a text backend is accidentally stronger than
redb. Both are documented on `Field::durable`; the wider behaviour must not be
relied on.

Not urgent. Correctness holds; predictability of cost does not.

## A write costs two reads

Inserting one key reads the old value twice. `map_insert` reads it typed, to
decide between `MapChange::Insert` and `MapChange::Update` and to carry
`old_value`:

```rust
let old_value = backend.get::<V>(&full_path)?;
```

Then the backend reads the same path again, as bytes, to fill `old` in the
`StoreEvent` - and on a buffer miss `committed_or_buffered` opens a redb read
transaction to do it.

That is where the microseconds go. Measured: inserting a new key ~5 us against
~2.1 us for overwriting an existing one. The gap is exactly the read
transaction, which the existing key avoids because it is still in `pending`.

So the cost is not overhead, it is the price of change events carrying the
previous value - the same design decision that makes `update` strict, showing up
again as write cost.

Three ways out, not exclusive:

- Have the backend hand back the bytes it already read, so the map layer decodes
  those instead of issuing its own typed read. Saves one transaction per insert
  of a new key.
- Skip the read entirely when nothing is subscribed to that path. Checking for
  subscribers is far cheaper than a transaction.
- Failing both, document it and add a batch write that pays the read once for a
  run of keys, so bulk loading stops being a per-key transaction.

## `scan_prefix` merges the write buffer in quadratic time

Every scan reads the committed table, then folds the pending writes over the
result. The fold is a linear search per pending key:

```rust
if let Some(pos) = results.iter().position(|(rk, _)| *rk == k) { ... }
```

With a small buffer this is nothing. With a large one it is O(n*m), and when the
whole map is still buffered - a burst of writes, or a long debounce - it degrades
to O(n^2). Measured on redb with 10 000 buffered entries: `len()` takes 364 ms,
against 3.95 ms at 1 000 and 3.93 us at 10. `is_empty`, `entries` and `keys` all
ride the same path, so an emptiness check that reads as free costs the same third
of a second. `scan_keys` has the same shape via `retain`.

The fix is a map lookup instead of a scan of `results`, which is a contained
change to the redb and sqlite backends.

Also worth noting from the same run: `entries().take(1)` costs as much as
consuming all 10 000, because the scan materialises every key and value before
the iterator is handed over. Decoding is lazy; the scan is not, and the scan is
the whole cost. The doc on `entries` should stop implying otherwise.

## `ReactiveCell` needs reworking: a weak view, not a source

The type is slated for rework, not a patch - the three faults below are one
cause, and fixing them separately would leave the model half-changed.

Decided: a cell is a view over data that lives somewhere else. If that data is
gone, every operation fails - it does not substitute a default, and it does not
put the value back.

Three things break today, and all three follow from the cell behaving like an
owner instead:

**It hands back a value the store never held.** `entry_cell` keeps its
`default` in the signal and never writes it, so `cell.get()` returns a number
while `map.get(&key)` returns `None` and `contains_key` says `false`. A field
does the opposite - it writes its default on creation - so one word means two
things depending on which primitive is under it.

**A write to a removed entry silently recreates it.** `cell.set(v)` after
`map.remove(key)` puts the key back. For a view of data that no longer exists,
the honest answer is an error.

**A stray cell pins the whole store open.** The writer closure captures a clone
of the `ReactiveMap`, which holds `store: Store` - an `Arc<dyn StoreBackend>` -
and `_keepalive` holds the subscription. So one forgotten cell in a UI keeps the
database file open and the debouncer thread alive long after the map and the
store handle were dropped. Holding `Weak` fixes that as a side effect.

Consequences to work through:

- Reads and writes become fallible. That is heavier than returning `Option`,
  and it lands on cells over fields too, where the source practically never
  dies. Worth checking whether the error can be confined to the paths that can
  actually fail.
- Erasure forces the weakest common contract. `ReactiveCell` exists so fields,
  map entries and in-memory values share one type and one collection; once map
  entries are in that set, "the value is always there" stops being true of the
  set, which is why the current `T` return lies for every kind and not just for
  entries. A separate `EntryReactiveCell` would remove the lie and the erasure
  with it.
- An in-memory cell has no source to lose, so it never fails - the same API
  covers it, just never taking the error branch.
- The doctest on `entry_cell` currently asserts the recreation behaviour. It is
  meant to fail when this lands.

## A non-finite float is silently destroyed on the text backends

JSON has no `NaN` or infinity, so `serde_json` writes them as `null`, and
reading that back into an `f64` fails. Measured end to end, writing `f64::NAN`
into a field:

| | `field.get()` afterwards | typed read from the store |
| --- | --- | --- |
| redb | `NaN` | `Ok(Some(NaN))` |
| json | **`0`** | **`Err(Codec(..))`** |

On the text backends the write appears to succeed. The store event carries
`null`, the subscription decodes it, decoding fails, and `decode` falls back to
`T::default()` with a warning - so the in-memory value silently becomes `0`
while the file holds `null`. A later `Store::get` on the same path returns an
error instead, because that path propagates the codec failure rather than
substituting a default. The same bad bytes therefore behave two different ways
depending on which read reaches them.

Nothing about this is documented, and nothing rejects the write.

At minimum, say so: a field holding a float that can go non-finite is not
portable across backends. Better, refuse the write on a codec that cannot
represent the value, so it fails where it happens instead of turning into a
default three steps later.

Related: `decode` falling back to `Default` while `get` returns an error is a
split worth settling on its own. One of them is wrong.

## Metadata carries no format version - deliberately, for now

`PrefixMeta` and `SchemaSnapshot` both use `version` for the user's schema
version, from `#[amethystate(version = N)]`. Nothing records how `hash` was
computed, which drift rules produced it, or how steps were ordered.

So changing any of those algorithms is a one-way door: new code reads old bytes
and cannot tell they are old. Changing the hash makes every existing store
report total drift on the first run of the new version, because the stored
number was produced by a formula that no longer exists.

**Decided: not building this yet.** Compatibility is an obligation to somebody,
and right now there is nobody but the author. Change the format, eat the drift
once, move on.

Revisit when that stops being true - the first release someone else depends on,
or the first time "delete your store and start over" is not an acceptable
answer. At that point the shape is roughly:

- A store-level header, read before anything else, with a format number and the
  oldest format that can still read the store. The two differ: an additive
  change moves the first and not the second, so old readers keep working.
- A version tag per metadata record, since metadata is written per prefix at
  different times and a long-lived store ends up mixed. An externally tagged
  enum round-trips through both msgpack and JSON - checked - and costs one byte.
  Read it, lift it to the current shape in memory, write back the newest form,
  and old records upgrade as they are touched.
- The library version stored beside it for diagnostics only. It moves for
  reasons that have nothing to do with the format, so it cannot be the thing
  decisions compare.
- Byte fixtures of each old format in the repository, with a test that opens
  them. Otherwise "we still read format 1" is a claim that quietly stops being
  true at the first refactor.
- Snapshots describing the field tree down to primitive names rather than
  stopping at a digest per nested type, so a changed algorithm becomes a
  recomputation instead of a break.

## `Kv::check_type` compares printed type names, and only within one run

```rust
let wanted = std::any::type_name::<T>();
match resolve_field(path) {
    Some(meta) if meta.value_type_name != wanted => Err(WriteError::TypeMismatch { .. }),
    _ => Ok(()),
}
```

Two problems, and the second is the one that matters.

**`type_name` is not an identity.** The standard library documents it as
diagnostic output with no stability guarantee: the same type can print
differently depending on how it was named at the use site, and different types
can print the same. Today nothing breaks, because both strings come from the
same build and change together - the check works by coincidence, not by
construction.

**The check should survive a restart, and cannot.** A path claimed as one type
in an earlier run is not checked at all now; the guard only sees what this
process built. That is the wrong scope for a store whose whole point is that
data outlives the process.

`TypeId` is not the answer either, for the same reason `type_name` is not: it is
not reproducible across runs, so there is nothing to compare a stored value
against. Nor is it usable where a compile-time constant is needed - `TypeId::of`
is still not `const` on stable as of 1.90.

What is left is the structural fingerprint, `AmeType::TYPE_HASH`: computed at
compile time from field names and primitive type names, so it is deterministic
across builds and survives renaming a module or the type itself. The cost is a
bound - `Kv::cell` currently takes any `Serialize + DeserializeOwned`, and would
need `T: AmeType`.

**Fixing the XOR fold is a prerequisite, not a separate task.** A fingerprint
that cannot see two fields swapping order or exchanging types is not a basis for
deciding whether a path holds the same type as before.

## Reordering struct fields silently corrupts data on redb

Two facts that are each defensible on their own, and together lose data.

**msgpack writes structs positionally.** `rmp_serde::to_vec` on
`struct A { first: u32, second: u64 }` gives `[146, 1, 2]` - `0x92` is "array of
two", and no field name appears. Reading those bytes into a struct whose fields
are declared in the other order yields `{ second: 1, first: 2 }`: no error, just
values landing in the wrong fields. The text backends key by name and are
unaffected - the same bytes read back correctly there.

**The schema hash is order-insensitive.** `hash.rs` folds each field as
`fnv1a(field_name) ^ <FieldTy as AmeType>::TYPE_HASH` with XOR, starting from
`0u32`. Reordering the fields cannot change the total, so drift detection sees
nothing.

So swapping two fields in a declaration is a breaking change to stored data on
the default backend, and nothing in the library notices. The user gets wrong
values, not a failed load.

XOR has a second blind spot from the same property: if two fields exchange
types - `a: u32, b: u64` becoming `a: u64, b: u32` - both terms are still
present, so the hash is unchanged there too, even though the layout differs.

Order-independence in the hash was presumably deliberate, since moving a field
in the source is not meant to be a schema change. That intent is only sound if
the wire format is keyed by name. Either the hash has to account for position,
or msgpack has to be told to write maps - `rmp_serde` does that with
`Serializer::with_struct_map`, at a cost in size.

Worth deciding before anything else here: the size saving is real, but so is
silently reading `first` as `second`.

### Fixing the fold

The right fold is already in the tree, one level up. `types::schema_hash` mixes
each field into a running state and multiplies by the FNV prime, so it is
order-sensitive:

```rust
let mut h: u32 = 0x811c9dc5;
while i < fields.len() {
    let fh = fnv1a(fields[i].name.as_bytes()) ^ (fields[i].type_hash as u32);
    h ^= fh;
    h = h.wrapping_mul(0x01000193);
    i += 1;
}
```

`gen_recursive_type_hash` in the macro is the one that XORs everything flat.
Two changes, both small:

- Inside a field, stop XORing the name against the type. Run both through the
  state instead, name first, so `("a", u32)` and `("a", u64)` differ and two
  fields cannot exchange types without moving the hash. `schema_hash` needs
  this too - it still does `fnv1a(name) ^ type_hash` per field.
- In the macro, replace the XOR chain with the same running fold. A `const`
  block with `let mut` and `const fn` calls is fine; `schema_hash` already
  proves it.

### The upgrade cost, accepted

Changing the algorithm changes every hash at once, so every existing store
reports total drift on the first run afterwards. With no outside users that is a
one-time annoyance on the author's own machine, not a problem to engineer
around - see the entry on format versioning for what this would take, and why it
is not being built yet.

## Writing the same value costs a full write

Nothing compares the incoming value to what is stored. An identical write
serialises, takes the buffer lock, inserts, wakes every subscriber and schedules
a flush - all to leave the data exactly as it was. A slider that rounds to the
same step, a form firing on blur without an edit, a cache revalidated on a
timer: each of those pays in full.

The comparison is nearly free where it belongs. `committed_or_buffered` already
fetches the old bytes to fill `old` in the `StoreEvent`, so a store-level dedupe
adds a memcmp to a read that has already happened and cancels everything after
it.

Compare the serialised bytes, not the values:

- No new bound. A `PartialEq` bound would be a breaking change for every value
  type, and it would compare what is in memory rather than what will be on disk.
- It is correct for floats. `NaN != NaN`, so a `PartialEq` dedupe silently fails
  exactly where it looks like it works; the msgpack encoding of `NaN` is stable
  and compares equal to itself.

One behaviour changes and must be documented rather than discovered: an
identical write stops producing an event. Meaning something by rewriting the
same value - "checked again, still valid" - is a real pattern for a cache, not
an abuse. It should get an explicit operation instead of riding on whether the
bytes happened to differ. `ReactiveCache` already separates the two: the value
dedupes, the stamp lives in the meta space and changes on its own.

This does not fix a GUI binding rendering twice. That is one write and one
notification coming back to its own author, which is what `Watch::external`
is for.

## The map already projects itself into memory, then ignores it

`reactive_map_with_path_only` scans the prefix at construction and decodes every
entry into `ReactiveMapCore::cache`. The full projection is built and paid for -
one scan, one deserialisation per key - before the map is handed over.

Almost nothing reads it:

| operation | source of truth |
| --- | --- |
| `get` (sync) | backend |
| `len`, `is_empty` | backend, rescanned per call |
| `entries`, `keys` | backend |
| `remove` | **cache**, as a gate: absent there means `Ok(None)` and no delete |
| `get` (async) | **cache** |

So `len()` spends 386 ms scanning 10 000 keys while the answer is a
`HashMap::len()` away in the same struct, and the sync and async halves of the
API disagree about where the truth lives.

The gate in `remove` is the sharp end. It only works because the constructor
seeded the cache; anything that leaves the cache and the store out of step makes
`remove` a silent no-op on a key that is really there.

This is a bug rather than a design cost: the memory is already spent, the scan
is already paid, and the reads take the slow path anyway. Reads should come from
the projection, with the backend consulted only where the projection cannot
answer.

Two things to settle while doing it:

- **The lock is wrong.** `cache: Arc<Mutex<HashMap<K, V>>>` serialises readers
  against each other for no reason. Once reads actually go through it, that is
  the hot path - and the load is many readers against an occasional writer, so
  `DashMap` fits it better than one `RwLock` over the whole map: readers on
  different shards never meet. Its own trap is worth knowing before it bites -
  holding a reference into the map while touching the same shard again
  deadlocks, so `entries` has to collect rather than hand out an iterator that
  keeps shard guards alive.
- **Staying in sync.** The cache is currently filled by `map_apply_remote_change`
  off the store subscription, which covers writes and external file edits. That
  path has to remain the only writer, or the two diverge again.

Scope, since it decides the memory question: millions of rows and blobs are not
this library's business - reach for the database directly there. Within the size
it does target, holding the map resident is the right trade.

## `ReactiveCache`

A cache primitive shaped like `ReactiveMap`, for derived data that is expensive
to recompute and cheap to throw away. Designed, not built.

- `get_or_insert_with(key, stamp, f)` is the only way to read. The stamp is an
  opaque `u64` supplied by the caller — a file hash, an mtime, `now / ttl`, or
  any mix. A stamp that does not match is a miss, so time-based expiry, input-
  based invalidation and schema-based invalidation are all the same mechanism.
- No epoch and no lazy invalidation: `remove` and `clear` delete. Two ways to
  invalidate, one of which quietly leaks disk, is the kind of pair that gets
  chosen wrong.
- Excluded from migrations by living under its own prefix, and dropped rather
  than migrated when the schema hash changes — derived data recomputes.
- Everything the cache itself needs lives in the meta space, keyed per entry
  rather than per prefix, so values on disk stay plain and readable and a write
  does not rewrite a shared blob.
- Write the value first and the stamp second: a crash between them costs a miss,
  never a false hit.

## Documentation

The public API is documented with runnable, asserted examples: `Field`,
`ReactiveMap`, `ReactiveCell`, `Kv`, `Store`, `StoreBuilder`, `Watch`,
`LocalScope`, and the migration builder and context. The macros keep `ignore`
examples - `#[amethystate]` cannot expand inside this crate's own doctests,
because the macro resolves the crate to `crate` and a doctest compiles as a
separate crate where that means something else. Examples reach the same types
through `store::field_with_path` and `Kv`, which need no macro.

**Several of these document today's behaviour, and today's behaviour is on this
list.** They are written to fail rather than quietly go stale, but they will
need rewriting as the entries above land:

| doc | what it records | changes with |
| --- | --- | --- |
| `entry_cell` doctest | a write to a removed key recreates it | the `ReactiveCell` rework |
| `ReactiveCell` methods | `get` returns `T`, never absence | the same |
| `ReactiveMap::len`, `entries` | the cost is a scan, and `take(1)` saves nothing | reads moving to the projection |
| `Field::durable` | what each engine family commits, and that a text backend is accidentally stronger | `flush_prefix` becoming per-write |
| `Store::decode` | corrupt bytes yield `Default` with a warning | settling the split against `get`, which errors |
| `Field::set`, `ReactiveMap::insert` | every write reaches the store | value dedupe |
| `Kv::cell` | a path's type is remembered for this run only, and a second type is refused | `check_type` becoming persistent, which also puts an `AmeType` bound on the method |
| `Kv::set`, `Kv::get` | any type at any path, unchecked | the same |

Sorting is documented on `keys` and pointed at from `entries`: the order is the
store's, over the key's string form, so numeric keys come back `10, 100, 9`.
That one is not expected to change.

When the list is empty, turn on `#![deny(missing_docs)]` for the documented
modules so the next undocumented public item cannot land quietly.
