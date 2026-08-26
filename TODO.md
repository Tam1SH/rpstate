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

**Done.** `results` is a `BTreeMap` and `keys` a `BTreeSet`, so folding the
buffer is an insert or a remove per pending key, and the final `sort` goes with
the ordering the container already keeps. The old code searched a `Vec`
linearly for each of them while iterating a `HashMap` it never looked anything
up in - the lookup went the wrong way round.

Measured on redb with everything still buffered, at 10 000 entries:
`scan_keys` 344.6 ms to 5.09 ms, `scan_prefix` 394.1 ms to 8.13 ms. At 1 000,
`scan_keys` 4.14 ms to 0.32 ms. Ten times the size cost 83 times the time
before and 16 times after.

The 364 ms above was this path, reached through `len()`. It is not reachable
that way any more: `ReactiveMap`'s `len`, `keys` and `entries` answer from the
projection, so `reactive_map_bench`'s `map_len_vs_size` is flat at 680 ns from
10 to 10 000 and measures the projection rather than a scan. The group that
does reach the store is `store_scan_buffered`, added with the fix; the module
doc's line about what `len()` costs "when it has to scan" is left over from
when it did.

Also worth noting from the same run: `entries().take(1)` costs as much as
consuming all 10 000, because the scan materialises every key and value before
the iterator is handed over. Decoding is lazy; the scan is not, and the scan is
the whole cost. The doc on `entries` should stop implying otherwise.

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

**Re-measured, and the account above is wrong twice.**
`tests/non_finite_float.rs` writes `f64::NAN` through a field on every engine.

It is not the text engines' problem, and it is not one engine's either. TOML
and RON have `nan` and `inf` in their grammars and carry the value intact, as
msgpack does. JSON cannot spell it - and **two** engines store JSON, because
sqlite encodes its values with `sonic_rs`, which answers the same way
`serde_json` does. So three of five are fine, and the one that is not named for
a text format is among the two that are not.

Which makes it a property of the format rather than of the file. Reading the
engine list as "the text ones" was what hid it: the failure follows the codec,
and the codec is not visible in the engine's name.

And nothing substitutes a default. `StoreExt::decode` stopped doing that this
release, and the field's subscription never did: handed bytes it cannot decode,
it logs and **leaves the signal alone**. So the handle keeps reporting the value
it held before - a field last set to `5.0` goes on saying `5.0` about a store
that now holds `null`. The zero in the original table was the value that
happened to be there already, and reading it as a default sent this entry after
the wrong mechanism.

Which makes it worse than recorded, not better: a substituted default is at
least a value nobody wrote, while a stale one is indistinguishable from a
successful write.

`set` still returns `Ok`, and a typed read of the same path still fails, so the
disagreement between the two reads stands. The test pins all of it and goes
green today; its failure message becomes the finding on the day any answer
moves.

**Deprioritised.** The upstream half has been open since January 2017
(`serde-rs/json#202`) and is a property of the format rather than the crate:
JSON has no non-finite floats, and `serde_json` follows `JSON.stringify` in
writing `null`. Nothing here is waiting on that. The part worth doing when this
comes back up is the stale value, which is not about floats at all - any decode
failure leaves a handle confidently reporting the past.

At minimum, say so: a field holding a float that can go non-finite is not
portable across backends. Better, refuse the write on a codec that cannot
represent the value, so it fails where it happens instead of turning into a
default three steps later.

Related: `decode` falling back to `Default` while `get` returns an error is a
split worth settling on its own. One of them is wrong.

## Toml answers "holds nothing" and "is not there" with the same document

Measured in `tests/absent_or_null.rs`, writing `None` into an `Option<String>`
field and then reading the file:

| engine | the document | `get::<Option<String>>` |
| --- | --- | --- |
| json | `"note": null` | `Some(None)` |
| ron | `"note": None` | `Some(None)` |
| toml | no such key | `None` |

Toml has no null, so `toml_edit::ser` writes an empty document for a `None`
field, `serialize_node` gets no `val` back and returns `Item::None`, and a table
that is handed `Item::None` reports no such key. The write path read the node
back with an unconditional `unwrap` and so **panicked** on `field.set(None)`,
which is how this was found; it now reports the removal the document performed.

What is left is not a bug in the code. An absent key is how every toml config
expresses an optional setting, and it is the only thing the format offers, so
`set(None)` and `delete` produce the same file. A field whose declared default
is `None` round-trips exactly; one whose default is `Some(..)` reads its default
back, because on the next open there is nothing to say the key was emptied on
purpose.

This is the case for a schema saying which properties may be null: on toml the
document alone cannot answer it, and the declaration is the only place the
answer exists. See *The schema belongs in the store, as JSON Schema*.

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

**Done: there is no type check.** `check_type` is gone rather than repaired -
"Decided: the library guarantees paths, and says nothing about types" below is
what it was traded for. What a path holds is the writer's business; what refuses
a write is ownership of the path, and that is spelled out under "Done: ownership
is by declared path".

## Reordering struct fields silently corrupts data on redb - it does not

**The heading was wrong, and the code says so.** `generate/data.rs` sorts the
fields by name before writing the `_Data` struct, and `_Data` is what gets
serialised, so the layout is alphabetical however the declaration was written.
Moving a field in the source moves nothing on disk. The sort arrived with the
drift machinery itself, `5445026` on 2026-05-13, so this entry was written
without it in view. `tests/schema_hash_order.rs` pins the bytes rather than the
hash, because the sort is what makes it true and the test should fail if the
sort goes.

What is real is the other half below: two fields trading types. Every name and
every type survives that, only the pairing moves, and a fold that XORs the
fields cannot see it - so `u64` bytes are read as `u32` with nothing said.
`tests/type_identity.rs` already records that one, along with five more the
same fold is blind to.

**Not being fixed, deliberately.** The running fold was written and reverted:
it closes the trade and four of the collisions in that catalogue, and the cost
is that every entry in the catalogue has to be rewritten - a document about a
gate that is expected to go. The hashes stand or fall with the fork below on
whether the file is a store or a picture of a type, and there is no sense
sharpening a gate that question may remove. What follows is kept as the record
of what the gate can and cannot see, not as a plan.

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

## Two type hashes, both weak, and the weaker one feeds the gate

There are two computations and confusing them sends a fix to the wrong file.

`schema_hash` in `migration/types.rs` folds each field with
`h ^= fnv1a(name) ^ type_hash; h = h.wrapping_mul(..)`. The multiply makes it
order-sensitive, so swapping two fields' types *is* caught here.

`gen_recursive_type_hash` in `amethystate-macros/src/hash.rs` is a bare
`0 ^ fnv1a(name) ^ H(ty) ^ ..` with no seed and no mixing. It emits `TYPE_HASH`
for every derived type and every generated `_Data` struct - and it reaches the
migration gate through `FieldDescriptor::type_hash`, which is an input to
`schema_hash`. So the pure XOR is not a side channel; it is laundered into the
decision that runs migrations.

Reproduced in `tests/type_identity.rs`: 22 `const _: () = assert!(..)`, so the
build fails the moment any of them stops holding. Nothing runs at test time
because nothing needs to.

**The generic impls cancel with themselves.** Three lines in `types.rs` settle
it: `Vec<T>` is `fnv1a("Vec") ^ T`, `Option<T>` likewise, `HashMap<K, V>` is
`fnv1a("HashMap") ^ K ^ V`. Therefore

| | equals |
| --- | --- |
| `Option<Option<u32>>` | `u32` |
| `Vec<Vec<u32>>` | `u32` |
| `Vec<Option<u32>>` | `Option<Vec<u32>>` |
| `HashMap<u32, u64>` | `HashMap<u64, u32>` |
| `HashMap<T, T>` for every `T` | `fnv1a("HashMap")` |

The map row reaches the gate: a `ReactiveMap<u32, u64>` field changed to
`ReactiveMap<u64, u32>` leaves `SCHEMA_HASH` identical. Keys are stored as path
text and parsed with `FromStr`, values decoded by the codec, so every entry is
then read with the two decoders exchanged. No step runs and no drift is
reported.

**Zero is both a value and the sentinel for "unknown".** `component_needs_work`
and `migrate_prefix` both guard on `target_hash != 0`, and a schema hashing to
exactly zero is constructible. Such a prefix leaves schema checking for the life
of the application: no drift is ever detected whatever its fields become.
Separately, five unrelated shapes all hash to zero today - an empty struct, a
unit struct, a tuple struct, an enum, and a union, which the derive accepts
rather than refusing.

**A name and a type cancel inside one field.** `fnv1a(name) ^ type_hash` with
nothing between them, so a brute force finds pairs: `{volume_level: f64}` and
`{span_max_len: bool}` - two structs with no field in common - share a
`SCHEMA_HASH`. Likewise adding two fields can be free.

**A nested struct's swap defeats the multiply.** A nested field contributes
`0xDEADBEEF ^ Inner_Data::TYPE_HASH`, and `TYPE_HASH` does not move when the
inner struct's field types are swapped. A nested struct has no prefix of its
own, so the outer hash is the only gate its data has.

**Two different numbers are both called the schema hash and both are written to
the same stored field.** `SchemaEntry::schema_hash` is `_Data::TYPE_HASH`;
`MigrationStepEntry::schema_hash` is `AmeStateFields::SCHEMA_HASH`; they are
never equal. A migrating run writes the second into `SchemaSnapshot`, and
`ensure_snapshots` immediately overwrites it with the first. Whatever ends up
stored is not the number the gate compares against, so the field cannot be
trusted or reused.

**Also missing, found in passing.** No `AmeType` for `char`, `()`, `Box<T>`,
`Arc<T>`, `BTreeMap`, `HashSet`, arrays or tuples - `Box` in particular is how a
recursive type is written and is simply unusable. Generics are unsupported: the
derive emits `impl AmeType for #name` without `split_for_impl()`. A type
recursive through `Vec` fails const evaluation with E0391 and needs a way to
opt a field out of expansion.

Checked and clean: `cfg!(feature = "tauri")` reads the right crate's features,
since the facade forwards the feature to the macro crate.

**Direction.** Widening the hash does not fix any of this - every collision
above is structural, not a birthday collision. Two shapes are worth considering:
fold properly (seed, then per field absorb ordinal, name and type, with mixing
between) and reserve zero; or stop hashing at the gate and compare the stored
schema against the declared one, which `SchemaSnapshot` and `calculate_drift`
already have most of the machinery for. The second inverts the residual failure
from a missed migration to a spurious diff, which is the right direction when a
missed migration silently misreads saved settings and a spurious diff costs one
nag. It is also the option that wants the format version above.

### Decided: the library guarantees paths, and says nothing about types

Confirmed by running rather than by deriving - `Vec<Vec<u32>>` and `u32` print
the same number, so do `Vec<Vec<Vec<u32>>>` and `Vec<u32>`,
`HashMap<String, u32>` and `HashMap<u32, String>`, and two structs whose field
names are swapped between the same two types.

Widening or reseeding is not enough, and neither is replacing the number with a
description. The route was walked and abandoned, which is worth recording so it
is not walked again:

- a *shape* composed bottom-up through the trait fixes the cancellation, and has
  to be a function rather than a constant, because only a function can carry the
  visited set that cuts a recursive type - const evaluation has no memory
  between calls, which is why `#[derive(AmeType)]` on `Tree { children:
  Vec<Tree> }` does not compile today even though the recursion terminates;
- the orphan rule then bites for any type from another crate, answerable with
  autoref fallback to an opaque case, feature-gated impls, or both;
- and *none of it works anyway*, because the stored form of a value is decided
  by arbitrary code - `deserialize_with`, `untagged`, `flatten` - which a dozen
  GUI projects use as a matter of course. A description of the Rust type is not
  a description of what is on disk.

**Drift in the value is inexpressible, not merely unimplemented.** A change that
breaks decoding is caught by the read, which happens at the same moment the gate
would have run - `field_with_path` decodes every declared field on
construction - and reports the path, the type asked for and the codec's own
sentence. A change that preserves the type and alters the meaning, seconds to
milliseconds, is invisible to any type description whatever. That leaves the
gate a narrow band where the read already answers, and the current hash does not
even cover that: it cancels.

So the contract narrows to what is exactly knowable, and the type layer goes:

| goes | stays |
| --- | --- |
| `AmeType`, its derive, its bound on the eight `Kv` sites | `StorePath` and everything checking it |
| `TYPE_HASH`, `SCHEMA_HASH`, `FieldDescriptor::type_hash`, `PrefixMeta::hash` | `MigrationContext` - already path-keyed, unchanged |
| `calculate_drift`, `NaggingRecord`, the `target_hash != 0` gates | step ordering, `Gap`, `Downgrade`, per-prefix isolation, the log |
| the orphan hole, recursion in const, `Shape`, serde tracing, a feature matrix | `AmeStateFields::FIELDS` - as the owned *path* set, not a shape |

`AmeType::TYPE_NAME` is replaced by `std::any::type_name::<T>()`, which fixes a
live bug on the way out: `TYPE_NAME` is `stringify!`, the field registry records
`any::type_name`, and `check_type` compares the two - so asking for one path
twice with the same type fails for anything but a primitive. Verified:
``path `b` is already `alloc::string::String`, asked for `String` ``, and `Vec`
loses its parameter entirely.

### Decided: four layers, and none of them is a Rust type

The dead end was trying to derive the stored shape from the declared type. The
stored shape is on disk, in the format's own fundamental types, and every engine
already knows it - a text node is a `serde_json::Value` or a `toml_edit::Item`,
sqlite has a column type, redb has msgpack's tag. The codec even says it out
loud today: `invalid type: integer 800, expected a string`.

So the record is built from what is there, not from what the code says, and it
falls into four layers with four different sources:

| layer | known by | says |
| --- | --- | --- |
| path | the library, exactly | where a value lives |
| role | the schema, exactly | `field` or `map` |
| shape | the disk, exactly | integer, text, object, array |
| meaning | the author | `version` |

None needs a description of a Rust type, so `AmeType`, `Shape`-as-a-trait, serde
tracing, the orphan rule, third-party impls and recursion in const evaluation
all fall away together. A description read off data is finite by construction -
there is no cycle to break - and `deserialize_with` or `untagged` cannot lie to
it, because whatever they produced is what got written.

### Done: ownership is by declared path

`Kv::guard` refused anything under a declared prefix, so a struct with
`prefix = "app"` took the whole subtree and `app.myplugin.enabled` was refused
though nobody had declared it. Settings are extended from outside constantly - a
plugin, a theme, a person editing the file - and none of that collides with a
declared field.

`FieldDescriptor` now carries `role` (`Field`, `Map`, `Node`) and, for a node,
its children, so the set of declared paths is known without opening the store.
A cycle in that reference is impossible: a construction cycle is already refused
at compile time by `CONSTRUCTION_TERMINATES`.

Two directions collide and both are refused. A path may lie inside a declared
one - a field owns whatever is under it, since that is the inside of its value,
and a map owns its entries. Or a declared path may lie under the one being
written, where a value or a map would take the level those paths live on. A node
owns neither; it collides only through its children. The three refusals now say
which:

```
`typed.port` is declared by a schema
`typed` holds `typed.port`, which a schema declares
`typed.port.x` is inside `typed.port`, which a schema declares
```

**Unreviewed: `Collision` has not been read through by its owner.** The rule
above is written down; the code implementing it is not yet understood well
enough to be relied on, and that is a reason to go over it rather than a note to
file away. Two things are already known about it.

The two directions are spelled `declared.starts_with(path)` and
`path.starts_with(&declared)` - the same call with the arguments swapped,
meaning opposite things, and separated by the `match` on the role, so the
mirror is not visible where it is read.

The `Holds` check runs before the role is examined, so it can answer with a
node. A schema declaring `app.panel.left` and a write to `app` gives
`Holds(app.panel)`, naming a path the schema never declared - by this model a
node is not a record. Answering with the declared leaf under it is both more
precise and simpler: a node holds nothing itself, so it needs no check of its
own and can always descend.

`Kv::clear` follows from it: everything under the handle that no schema
declared goes, the declared paths stay, and a level that merely *holds* a
declared path is descended into rather than skipped - otherwise an extension
writing beside a nested struct's field would be immortal. It returns what went
and what stayed, so a settings screen can say what it reset. How deep those
paths are follows the engine's scan, which is the divergence recorded below;
what went is the same on all five.

**The set it reads is the linked one, and that is wrong.** `schema_collision`
walks `inventory::iter::<SchemaEntry>`, which is collected at link time and
therefore holds exactly the structs this binary happens to link. Two programs
over one store answer differently: a CLI that links a subset writes straight
over paths the application declares, and a path the application leaves alone
can look owned somewhere else. The same file, two answers, decided by a build
configuration rather than by the store.

What owns a path is a fact about the store, so it has to be read from the store
- the schema snapshot in the metadata, which is already written per prefix. It
records `fields` today and would need `role` and the nested children beside
them, which is the same record the four-layer design above calls for, so the two
converge rather than competing. The linked inventory stays what the *code*
declares, and the two being compared is drift, which is the other half.

Still to do on the same footing: `reset_to_defaults`, and migration cleanup
deleting the declared path rather than the subtree.

`reset_to_defaults` needs a backend method that does not exist. The trait has
`is_initialized` and `mark_initialized` and no way to unmark, so dropping the
declared paths without clearing their markers gives "restore defaults" that
deletes the settings and never re-seeds them - the marker is exactly what tells
a namespace it has been here before. That is a method on `StoreBackend` and five
implementations, which is why it is not folded into this change.

**Two roles recorded, a third derived.** A nested node exists - that is what
`#[amestate(nested)]` declares - but it stores nothing, so it gets no record of
its own: a path is a node exactly when some record's path lies under it.
`app.panel` is never written down, `app.panel.left` is, and node-ness falls out.
Recording it too would be a second source of truth that can disagree with the
first.

Deriving it buys something worth having. `store.get(["app", "panel"])` reads an
ancestor of declared paths, where the engines answer three different ways today
- `Ok(None)` on the flat ones, a decode failure on json and ron, the first
child's value on toml. A declared node holds no value and cannot, so `Ok(None)`
is the right answer everywhere, and `an_ancestor_is_not_a_value` can be
tightened from "never hand back what is underneath" to that. Only for declared
paths; elsewhere an ancestor stays indistinguishable from a struct-valued field.

That also settles what children in a document mean, which is the ambiguity that
broke three separate changes in one day:

| the record says | children under the path are |
| --- | --- |
| `field` | the interior of one value |
| `map` | entries |
| nothing - the path is undeclared | unknowable, and honestly so |

**Shape as a JSON Schema subset**, one notation for every engine, serialised
into whatever the metadata file already is - a toml meta writes the same
structure as tables, and serde does that part. `{"type": "integer"}` for a
number field, `{"type": "object", "additionalProperties": {..}}` for a map.

Two things to write down when it is built. JSON Schema is a validation
language, not a shape record: it has `anyOf`, `not`, `pattern`, and "are these
two schemas equal" is not well defined for one. Only a canonical subset is
emitted and comparison is structural - that has to be stated, or a reader is
entitled to expect semantics that are not there. And the record describes the
*store*, not the code: an `f64` holding `1` is written down as whatever the
format stored, which is exactly what makes two runs comparable.

**What each layer catches, and none of them is redundant:**

- a field renamed or removed - by the path set. Nothing else can: an absent
  path is indistinguishable from a fresh install, so the read cannot tell, and
  today the user's setting silently reverts to its default with the old value
  orphaned;
- a value whose kind changed - by the shape, before decoding, and reportable as
  "the file has a number here, the code wants text" rather than a codec's
  sentence;
- a value that will not decode at all - by the read, which happens anyway;
- the same kind meaning something else, seconds to milliseconds - only by
  `version`, and that is the honest limit.

## The last write of a store's life can fail without a trace

Every backend family flushes its buffer from `Drop` - [`redb`](crates/main/amethystate/src/store/backend/redb/mod.rs),
[`sqlite`](crates/main/amethystate/src/store/backend/sqlite/mod.rs), [`text`](crates/main/amethystate/src/store/backend/text/store.rs) -
and all three discard the result: `let _ = self.save_now();`.

That flush is the one a short-lived process depends on, and it is the one whose
failure nobody can observe. A locked file, a full disk, a permission error at
exit - the process ends reporting success and the data is not there. `Drop`
cannot return an error, so the value is real, but it can log, and today it does
not even do that.

Two levels worth having:

- log the failure at `error`, so the loss leaves a trace;
- an explicit `close()` that returns the result, for callers that would rather
  find out while they can still do something about it.

Found while chasing a suspected loss that turned out to be the separator bug
above. The flush had in fact succeeded - which was only knowable by adding a
probe to `Drop`.

**Done for the first, by one helper the three share.**
`utils::report_closing_flush` logs at `error` with the file, and each `Drop`
hands it the result it used to throw away. `error` rather than `warn` because
the store is past the point of retrying or telling anyone: the background
ladder keeps `warn` for a flush that is being retried and `error` for one that
gave up, and this is the second kind with nobody left to inform.
`a_closing_flush_that_fails_leaves_a_trace` breaks the disk under a redb store,
drops it, and reads the log back.

The second is already there under another name: `save_now` is on `StoreBackend`
and returns the result, so a caller who wants to know calls it before dropping.
What is uneven is the named form - `close` exists on redb and sqlite and not on
text, and sqlite's takes `&mut self` where redb's takes `&self`.

## Migration cleanup addresses a field by its Rust name, not by where it is stored

`#[amestate(key = "...")]` moves a field somewhere else on disk, and the
cleanup that runs after a migration does not follow. `FieldDescriptor.name`
carries the Rust identifier - `fname_str` in `generate/data.rs` - while the path
is built from `e.key.unwrap_or(fname)` a few lines below. With an override the
two are different strings, and the bookkeeping uses the first.

Reproduced in `tests/keyed_field_rename.rs`, the two failing cases `#[ignore]`d
so the suite stays green. A third case is the control: the same removal without
an override cleans up correctly, so the override is what breaks it.

**Reading the old value works.** The migration function is handed the old struct
through `AmeData`, which respects the override, so a rename carries the value
across exactly as written. What fails is only the removal afterwards.

**The old location is never emptied.** `delete(old_f.name)` in
`migration/context.rs` removes `keyed.left_panel_visible` while the value sits
at `keyed.panels.left.visible`. Deleting an absent key is deliberately not an
error, so nothing is reported.

So a renamed field leaves a copy of itself behind at the old path, and a field
dropped from the schema keeps its value forever - which is the worse of the two,
since dropping a field is how a migration is supposed to get rid of something
that should no longer be stored.

**`schema_hash` has the same blind spot.** It folds `name`, so changing only the
`key` moves the data on disk and leaves the hash identical: no migration runs
and no drift is reported. Not covered by the tests above.

The descriptor should carry the stored name alongside the Rust identifier, and
each user should take the one it means. A stored name is a path, and saying so
in the type is what keeps the two from being confused - so this lands as step 1
of the plan under "The API does not distinguish a path from a name".

## The API does not distinguish a path from a name

A dot inside a string means "next level" in some places and is meant to be an
ordinary character in others, and nothing in the types says which is which.
Where the two meet, the composed string has already lost the boundary.

| takes a string that means | | stated anywhere |
| --- | --- | --- |
| `prefix = "..."` | a path | no |
| `key = "..."` | a path - `tests/migration_complex.rs` relies on it | no |
| `Kv` paths | a path | no |
| a `ReactiveMap` key | a name | no, and it is split anyway |

The first three work as intended; they are an unwritten convention, and under
it there is no way to write a name that simply contains a dot. The fourth is a
bug, because the intent there is the opposite.

`#[rename(old => new)]` is safe by construction - it parses `Ident`s, which
cannot contain a dot.

**What the bug costs.** Reproduced in `tests/map_dotted_keys.rs`, both cases
`#[ignore]`d so the suite stays green. Flat backends store the key whole and are
unaffected, so a single-backend run never sees it:

| | `get` by exact key | `keys` / `entries` / `len` | key that prefixes another |
| --- | --- | --- | --- |
| redb, sqlite | correct | correct | correct |
| json, toml, ron | correct | counts nodes at the level | value destroyed |

Three keys `a.exe`, `a.dll`, `b.exe` give `len() == 2`: `a` and `b` are the
nodes. Now that reads come from the map's projection this is invisible while the
process runs - the projection is keyed by `K`, not by the document tree - and
appears on the next start, when the projection is rebuilt from the prefix. The
reproduction reopens the store for exactly that reason.

Worse, writing `a` and then `a.b` turns the leaf into a branch and the value
under `a` is gone - reading it fails to decode. Both writes returned `Ok`.

**Two schemas can claim the same place on disk.** `key = "panels.left.visible"`
under `prefix = "coll"` and a plain field under `prefix = "coll.panels"` compose
to the same path, and nothing checks for it. Reproduced in
`tests/prefix_overlap.rs`, both cases `#[ignore]`d:

- matching types share the slot silently - a write through one struct lands on
  the other's field, last writer wins;
- disagreeing types surface as `invalid type: boolean, expected u32` while the
  second struct is being constructed, which is a decode failure standing in for
  a name collision.

**A prefix can land on another struct's field.** `prefix = "root"` with a field
`b`, and `prefix = "root.b"`, put a leaf and a branch on the same node. This one
is invisible from the public API: `Field::get()` answers from the signal in
memory, so both structs report their own values for as long as the process
lives. Only the store disagrees:

| | `store.get("root.b")` | `store.get("root.b.x")` |
| --- | --- | --- |
| redb | `Some(10)` | `Some(20)` |
| json | `Err(invalid type: map, expected u32)` | - |
| toml | `Some(20)` - the branch's value | - |

The toml row is the worst of the three: no error, the type matches, the number
is wrong. And the damage only becomes visible on the next start, when the
signals have to come off the disk.

Forbidding a dot inside `key` is a check the macro can make on its own, ahead of
any of the above, and it makes `key` mean a name rather than a path. It closes
one of the three ways to nest - `prefix` and nested field names remain - but it
is the surprising one, and it is compile-time.

### Decided: paths carry segments

Compatibility with existing files is not a constraint - the implementation has
enough bugs that the data written by it is not worth preserving. That removes
the format migration from the work and lets each step land on its own.

Escaping does not disappear, it moves: a flat backend still has to compose one
byte string, and the separator inside it has to be escaped. The difference is
that this becomes a private detail of one engine's key encoding rather than a
rule the API asks callers to observe. Tree backends escape nothing - they walk a
node per segment.

Since the layout breaks anyway, this is the one cheap moment to put a format
version in the metadata. Without it an older file reads as a corrupt one rather
than an old one, and adding it later is a second break.

The steps, each standing on its own:

1. the descriptor carries the stored name next to the Rust identifier - fixes
   the migration cleanup above, touches no layout;
2. a path type carrying segments, with the join done at the boundary with the
   engine; the macro knows its segments at compile time, so the static case is
   `&'static [&'static str]` and allocates nothing;
3. a map key becomes exactly one segment;
4. the macro separates a name from a path - `key` is a name and a dot in it is
   an error, nesting gets its own attribute;
5. `scan_prefix` matches a segment boundary rather than a string prefix;
6. registration refuses two schemas that claim the same path - segments do not
   prevent a collision, they only stop one from happening by accident;
7. a format version in the metadata.

### Done: the static path checks itself

Step 2 above left a hole. `StorePath` refuses an empty level and keeps its
joined form in step with its segments, and property tests pin both - but only
for paths built at runtime. `from_static`, which is where every path from
`#[amethystate(prefix = ...)]` comes from, took both halves on trust, and the
macro built them with `dotted.split('.').filter(|s| !s.is_empty())` and
`Vec::join`. So an empty level was silently dropped rather than refused, and a
level containing the escape character produced a joined form the runtime would
never have written - the two halves of one path disagreeing, which is exactly
what injectivity rests on.

Both are now checked at compile time, in two places on purpose. The macro
validates the prefix and points `compile_error!` at the attribute, which is
where the readable message belongs. `from_static` re-checks in a `const fn`, so
the halves disagreeing is a const-eval failure - which catches a wrong macro,
and a hand-written `impl StateScope` too. Const panics carry no formatting, so
that one names the invariant rather than the level; that is why both exist.

Left open by it:

- **Field keys are not validated.** `#[amestate(key = "a..b")]` used to collapse
  to `a.b` in silence; it now panics at first use, because `path_literal` puts
  `from_static` in a non-const position for field keys. Louder, but the wrong
  shape - the macro should refuse it at the attribute like a prefix, which needs
  `StoreFieldEntry::key` to carry a span instead of being a plain `String`.
- **`"."` is still an untyped root sentinel**, special-cased by string
  comparison in `path_parts`, in `wasm.rs` four times and in `data.rs`'s
  `PARENT_PREFIX`. `Option<Vec<String>>` with `Some(vec![])` for the root would
  delete the comparison rather than guard it.
- **`PARENT_PREFIX` and the wasm codegen use the raw prefix**, unescaped, so a
  prefix holding the escape character makes them disagree with
  `StateScope::KEY`. Nothing tests that.

And a divergence to decide rather than fix: `.` separates levels in the
attribute (`prefix = "a.b"` is two levels, and `key` reads the same way) but is
part of a single name in `Kv::namespace("a.b")` - which is deliberate on the
`Kv` side, pinned by a doctest, and unmentioned on either.

## The error model: `error-stack`, and what it has to buy

### What is wrong now

Ten `thiserror` enums, nested by `#[error(transparent)]`. `StorageError` wraps
five engines plus the codec, the migration engine and `StorePathError`;
`WriteError` wraps `StorageError` in turn. Transparent nesting keeps the
innermost message and throws away every layer that knew something useful, so
what reaches a caller is the engine's sentence and nothing else:

    Error: no such table: data

Which path, which store, which operation, which prefix a migration was on - all
of it was known at some frame on the way out and none of it is in the value. The
enums cannot fix this by adding fields: the context differs per call site, not
per variant, and a variant per call site is not a design.

Three consequences already written down elsewhere in this file:

- a key that will not parse is skipped in silence, because there is nowhere to
  put "which key, in which file" (the section below);
- the conformance suite cannot assert failures, only successes, because the
  errors are not distinguishable enough to assert on (the section near the end);
- a failed migration reports the engine's error, not which prefix or which step
  it was on.

### What `error-stack` gives

A `Report<C>` is one context type plus a stack of frames, each carrying
attachments. The context is the *kind* of failure; the attachments are the
*particulars*, added by whoever knew them:

    store.get_raw(path)
        .change_context(StorageError::Read)
        .attach_printable_lazy(|| format!("path: {path}"))?;

The type stays one type. The message becomes the whole chain, printed as a tree,
with each attachment beside the frame that added it. That is precisely the shape
this library needs, because the useful context is always positional.

`stackerror` was the other candidate and is the wrong one here: it makes errors
opaque plus a code, which suits a boundary crate, not one whose callers branch
on what happened.

### The shape it becomes

- Keep the enums as *contexts*, but shrink them. A context should name what
  failed, not restate the cause: `StorageError::{Read, Write, Flush, Open,
  Migrate, Decode, Encode}` rather than one variant per engine. The engine's own
  error becomes an attached frame.
- Public signatures become `Result<T, Report<StorageError>>` and
  `Result<T, Report<WriteError>>`. `StorageResult<T>` and `WriteResult<T>` stay
  as the aliases; most call sites do not change shape.
- Engine crates keep their own error types and stop being variants of
  `StorageError`; they attach.
- `?` keeps working through `change_context`, but every `?` that crosses a layer
  boundary has to name what it was doing. That is the actual work, and the
  actual value.

### Order

1. Add the dependency and convert `amethystate-core` first - it has the
   smallest surface (`WriteError<E>`, `StorePathError`) and everything else
   depends on it.
2. `StorageError`: collapse the engine variants into contexts, attach the engine
   errors. Everything compiles at each step because the alias absorbs it.
3. Attach at the boundaries that know something: path on every store operation,
   file on every text-engine operation, prefix and step on every migration.
4. Then, and only then: replace the silent `else { continue }` skips with a
   skip that carries a report, and write the backend conformance suite's failure
   half.

### What it costs

Every `From` impl that exists only to nest one enum in another goes away;
`reactive/error.rs`'s hand-written `From<core::WriteError<E>>` goes with it. The
enums get smaller. The cost is at the `?` sites: a bare `?` across a layer is no
longer enough, and there are on the order of a few hundred. That is the point -
each one is a place where context is being dropped today.

## A key that will not parse disappears from a scan without a word

The text backends rebuild a key from the document tree and read it back with
`StorePath::parse_joined`. Where that fails - a hand-edited file, a key written
before the encoding existed - the code does

    let Ok(child_path) = StorePath::parse_joined(&full_key) else {
        continue;
    };

so the entry is absent from `scan_prefix`, `scan_keys`, and therefore from
`len`, `keys` and the map's projection, with nothing in the log and no error to
the caller. The same shape is in `document.rs`, where a child whose name cannot
be pushed onto the prefix is skipped.

Skipping is the right behaviour; being silent about it is not. This wants the
error carrying enough context to say which key and which file, which is what the
`error-stack` move above is for - so it should be fixed as part of that rather
than by bolting a `warn!` on now and leaving the shape behind.

**Done, both halves.** The two scan walkers now carry the failure up with the
key attached and a line saying the document holds a key this library could not
have written; `generic_scan` logs the child it passed over at `warn`, naming
the prefix and the name, which is what "Decided: a key with no name" below
settles it as.

## Migration cleanup deletes one key, so a composite field survives being dropped

The cleanup emitted by `migrate.rs` and the same loop in
`MigrationContext::nested` call `ctx.delete(field.name)` - a single key. A
`ReactiveMap` field lives at `prefix.field.<key>` and a `nested` field at
`prefix.field.<leaf>`; the branch itself holds nothing, so the delete removes
nothing and every entry stays on disk.

Dropping a `ReactiveMap<String, u32>` field that held `alpha = 7` leaves
`dropmap.cache.alpha` readable afterwards. Same for a dropped `nested` field.

Fails on **redb and sqlite**; the text backends delete a document node and take
the subtree with it, so the two families disagree about what a migration leaves
behind. Reproduced in `tests/migration_cleanup_composite.rs`, with a control
dropping a plain scalar that is cleaned up correctly.

That this is unhandled rather than deliberate is visible in
`tests/migration_reactive_map.rs`, where the migration hand-deletes
`routes.{key}` in a loop to work around it.

Renaming such a field is the same cause with a worse result: the new location is
written while the old subtree stays, leaving two live copies. Distinct from the
`key`-override finding above - this one needs no override at all.

## `Kv::guard` does not cover `as_root` structs

`guard` rejects a path under a declared `prefix`, and an `as_root` struct's
fields sit at bare paths, so nothing matches and `Kv` writes over them with any
type.

`store.kv().set("width", &"oops".to_string())` against an `as_root` struct
owning `width: u32` returns `Ok`, and after a reopen the struct fails to
construct - which is the failure `guard`'s own doc says it exists to prevent.
All five backends. Reproduced in `tests/kv_guard_root.rs`, with a control
showing the identical write against a prefixed struct is refused.

**Done by ownership moving to the declared path.** A root struct's prefix is
the root rather than nothing, so its fields are declared paths like any others
and the walk reaches them without a special case. `` `width` is declared by a
schema `` is what the write gets now, and both tests in `kv_guard_root.rs` run.

## `Kv` refuses the same type it just recorded

`check_type` compares `T::TYPE_NAME` while `register_field` stores
`std::any::type_name::<T>()`. For `u32` the two strings agree; for `String` they
are `"String"` and `"alloc::string::String"`, and for a derived type the bare
identifier against the fully qualified path. So asking twice for the same path
and the same type fails:

    let a = kv.cell("theme", "dark".to_string())?;   // records the long form
    let b = kv.cell("theme", "dark".to_string())?;   // Err(TypeMismatch)

Introduced by moving `check_type` off `std::any::type_name`, which is unstable
across compilers, onto the name a type declares. The move is right; what is
missing is the other half - the registry has to record the same string, or the
comparison has to be over `TYPE_HASH` rather than either name. The entry above
about the two hashes covers what that costs.

**Done: the check is gone.** Neither half was worth having - see the note under
"`Kv::check_type` compares printed type names". `register_field` still records
`std::any::type_name`, now as `value_type_name` and for display only.

## `build()` runs no generated migrations, and nothing at the call site says so

`build_with_report` calls `collect_codegen` before opening; `build` does not.
Both open a store, both succeed, and only one runs the steps `#[migrate]`
emitted. A program that opens with `build` compiles, starts, and silently skips
every generated migration - and then the version gate sees a prefix behind the
code and reports drift for a reason that has nothing to do with the data.

`init_global` goes through `build_with_report`, so the same application migrates
or does not depending on which of the two setups it copied.

Either `build` should collect too, or it should refuse to open when a generated
step exists for a prefix it is about to touch. Silently doing half the work is
the one option that should go.

## The global store never flushes, because statics are never dropped

Every backend commits its buffer from `Drop`, and that is what makes "closing
cleanly loses nothing" true. `GLOBAL_STORE` is a `OnceLock<Store>`, and Rust
does not drop statics at exit, so for an application built on `init_global` the
`Drop` never runs and every write younger than the debounce interval is lost on
a clean return from `main`.

This also qualifies the debouncer fix: the pending write reaches disk on drop,
where a drop happens.

**Half done: the step exists, and has to be called.** `shutdown()` writes what
the global store holds and returns the result, and `Store::close` does the same
for an ordinary one. That is the whole of what a library can do here - nothing
in the process tells it that `main` is returning. Statics are not dropped,
there is no stable `atexit` in std, and `ctor`-style destructors run at a time
nobody controls and cannot run Rust destructors safely.

The debouncer thread cannot stand in for the call, which is worth writing down
because it looks like it could. It leaves on `RecvTimeoutError::Disconnected` -
the sender being dropped, which is the store being dropped, which is exactly
the event that never happens for a static. The signal is derived from the thing
whose absence is the problem.

## `#[migrate]` can only be found through the linker

`inventory` is link-time collection, and it is the only way a `#[migrate]` step
reaches a store. An application that would rather hand its steps over
explicitly has nowhere to hand them to.

The shape wanted is an attribute - `#[migrate(link, ...)]` - where the linker
path is the one that is asked for, and the plain form yields something the
caller passes in themselves.

**A slice of `fn` will not carry it.** A step is
`MigrationStepEntry { prefix, target_version, description, dependencies,
struct_name, schema_hash, fields, run }`, and everything but `run` is derived
from the type; a bare `fn(&mut MigrationContext) -> StorageResult<()>` cannot
be walked back to the type it came from. The macro also submits the entry
anonymously, inside `inventory::submit!`, so there is nothing to name. The
non-linker form has to emit a named `const` of the entry, and the builder needs
a method taking `&[MigrationStepEntry]` - with `collect_codegen` rewritten as
that method fed from `inventory::iter`, so both paths join in one place.

**Which way round the default goes decides how loud the mistake is.** With
`link` opt-in, an existing `#[migrate]` keeps compiling and stops running, and
today that is silent in exactly the case that matters:

- `MigrationError::Gap` does exist, fails the component and leaves the version
  in the meta where it was - `engine.rs` has a test for it;
- but the target version reaches the plan through `collect_codegen`. Without
  it the plan never learns that version 2 is expected, so there is nothing to
  compare and no gap to report;
- and `build` drops the report without logging, where `build_with_report` calls
  `log_to_tracing`.

So before the default moves, the target version has to come from the type
rather than from the collector, and `build` has to say something when a
component failed. With those two, an unregistered step is a failure with a
name; without them it is a store that quietly keeps reading old data with a new
struct.

**Done the second way: linker by default, `#[migrate(explicit)]` to opt out.**
Nothing that exists changes behaviour, which is worth more here than the
tidier default - the mistake the other order allows is a store that keeps
reading old data under a new struct, with no error and no log line, and the
two repairs it needs are on this list rather than done.

The macro emits the entry as a `const` named for the function - uppercased, so
`fn settings_v1_to_v2` leaves `SETTINGS_V1_TO_V2` - and skips the
`inventory::submit!`. `MigrationBuilder::add_steps` takes them, and
`collect_codegen` is that method fed from `inventory::iter`, so the grouping
lives once. `tests/migration_explicit.rs` is one fixture opened twice: the step
is invisible to the sweep, and runs when it is handed over.

## Opening a large map, and where the time actually went

Measured because the reactive-table design needed a number and the one it had
was an extrapolation. At a million entries on redb, committed, warm:

| | before | after |
|---|---|---|
| `scan_keys` | 1.31 s | 0.99 s |
| `scan_prefix` | 1.42 s | 1.11 s |
| open | 2.45 s | 1.73 s |

**Two guesses were wrong before a profiler settled it.** Reserving capacity in
the two hash maps an open builds - `load_map`'s `HashMap` and the projection's
`DashMap`, both of which grew from nothing - changed nothing measurable. The
capacity is still reserved, because sizing a map whose size is known is right,
but it bought nothing and is not a performance change. Then the sampling
profile said a third of the run was in `RtlFreeHeap` and friends and named no
single hot function of ours, which reads as allocation-bound, spread thin.

**`dhat` is what answered it**, by counting blocks and attributing them, where
a sampling profile only says how much time the allocator got. At 20 000
entries, `StorePath::parse_joined` was the largest single site at 18% of all
allocations, and `split_checked` - splitting a key into a level per `Arc<str>`
- another 14%. Both for levels that the map load reads once and the flat
engines never read at all.

**So a `StorePath` splits its levels only when asked.** `parse_joined`
validates the string without allocating - it has to stay eager, because a key
that will not parse must be refused where it is read - and defers the split.
`PartialEq` and `Hash` moved to the joined form, where `Ord` already was, which
is sound because the escaping is injective and which stops equality from waking
the levels. `StorePath::name_under` reads the level below a prefix straight off
the joined string, borrowing unless the name carries an escape, and `load_map`
uses it instead of `starts_with` plus `segment_at`. After that `split_checked`
does not appear in the allocation profile at all.

That also makes `Borrow<str>` sound for the first time - all three of `Eq`,
`Ord` and `Hash` now answer from one form - so a map keyed by paths could be
probed with a key a flat engine already holds. Not implemented, but the door is
open and `a_path_hashes_like_its_key` is what keeps it that way.

**Then the fold, which was the larger half.** A scan built a
`BTreeMap<StorePath, Vec<u8>>` from the engine and searched it with the write
buffer. Both sides already arrive sorted, so the tree bought nothing and cost a
walk per committed key. Merging two sorted lists took `scan_prefix` at a
million from 1.11 s to 0.60 s and an open from 1.73 s to 1.20 s - half again
on top of the path work, and the largest single win of the lot. sqlite needed
`ORDER BY key` added: it always depended on that order and the tree was hiding
it.

**And what a benchmark says depends on what it benchmarks.** Three conclusions
here were drawn from measuring the wrong thing, and each survived until
something forced a second look:

- `.map(..).count()` throws its results away, and a compiler may throw the work
  away with them. Folding instead.
- Folding on `StorePath::len` forces the very split the laziness avoids, so it
  priced work a scan does not do: parsing a million keys reads 448 ms that way
  and 159 ms on `as_str`.
- Decoding was measured on `u64` and called not worth dividing. A `u64` decodes
  in eleven nanoseconds; a value with five fields in it takes two hundred, and
  a million of them are 204 ms rather than 11. It divides ×4.9, same as the
  keys. Nothing about the first measurement was wrong except what it was of.

The crossover for both is between three hundred and a thousand entries, which
is why `parallel_reads` does not divide below a thousand. Watch the shape of a
run rather than its numbers when the machine is busy: a run that reported 10^4
taking as long as 3·10^4 was contaminated, and the arithmetic said so before
any intuition did.

**The cache the deferral came with is gone too.** An `Arc<OnceLock<..>>` held
the split once it happened, and that `Arc` was an allocation on every parsed
key - 200 000 blocks of 2.7 million in a twenty-thousand-entry run, for a cache
the scan path never read. `Segments::Deferred` carries nothing now, and a level
comes back as a `Cow`: one holding an escaped separator is not a run of the
joined string and has to be assembled, one without is borrowed from it. Total
allocations fell to 2.44 million.

Holding the cell inline was never an option - the macro builds paths as
`static`, and a `OnceLock` anywhere in the type makes every `StorePath`
non-`Freeze`, which the borrow checker refuses there. That is what the `Arc`
was for.

`TextDocument` still takes `&[&str]`, so the eleven call sites in the document
engines borrow from the `Cow`s in a second vector. Left as it is deliberately:
those engines walk short paths on small stores, which is what they are for.

**And then the scan stopped building anything at all.** Pricing the value copy
on its own said four percent and not worth a trait change, which was the wrong
unit: a caller that decodes each entry where it sees it needs neither the copy
*nor* the path built for the key, and the key's `Arc<str>` was twice the copy.
Counted together it is the copy, the string, and the walk that parses it.

`StoreBackend::visit_prefix` hands each entry to a closure as `(&str, &[u8])`,
defaulted through `scan_prefix` so a backend outside this crate stays correct
without knowing it exists. redb overrides it and streams: the buffer is
collected and sorted first, because it is what is pending rather than what is
stored and is small beside it, and the engine's side is ranged over with a
cursor and merged into the visitor as it goes. `name_under_key` reads the level
below a prefix straight out of a stored key, validating it on the way, so a
malformed key is still refused where it is read.

Loading a map on one thread takes that path. Dividing the decode wants the
entries in hand, so `parallel_reads` keeps the owning scan - the two ways to
open cost differently and both are worth having.

Allocations in a twenty-thousand-entry run went 2.44 million to 2.32, and the
value copy left the profile entirely. Opening a million five-field rows: 2.0 s
to 1.87 s on one thread, and 1.55 s to 1.27 s across cores.

**What is left cannot be taken.** A path built in code holds its own string,
and a value costs what its type costs to decode - two thirds of the difference
between a `u64` map and a five-field one is `deserialize` building two
`String`s per row, which no scan work touches. What the profile still shows
above those is the *write* path: `path::join`, `try_push`, `get_erased` and
`committed_or_buffered` are around a fifth of the blocks in a run that fills a
store, and nothing here has looked at them.

## `amethystate-tauri` does not compile

Nine errors, all one thing: `impl AmeBackendAsync for TauriBackend` declares
`type Error = String`, and the trait now wants an `error_stack::Report`, whose
context must implement `StdError` - which `String` does not. The trait moved to
`error-stack` and the adapter stayed where it was.

It stopped being cosmetic. `cargo build --workspace` fails on it, and so does
anything that builds the workspace by default - `cargo flamegraph` did, which
is how it turned up: a profiling run died on an adapter it had no interest in.
Every workspace-wide command now needs `-p amethystate` to step around it.

**Done, and the type it needed was already there.** `error::Error` in the same
crate is a `thiserror` enum with `Command` and `Serde` variants, implements
`StdError`, and is `Send + Sync` - it just was not what the impl declared.
`type Error = Error` and the nine signatures return `Report<Self::Error>`.

Two things the compiler found on the way. `scan_prefix` returned
`Vec<(String, _)>` where the trait wants `Vec<(StorePath, _)>`, so it never
carried the path change from this release; it parses the key now and reports
which one it was when a key will not parse. And the plugin's own errors come
back as strings over the bridge - `invoke_result` deserialises them - so a
`String` is what crosses, and `commanded` is the single place it becomes an
error a report can carry, with the command and the path attached.

## The macro picks the branch, the compiler judges the pick

Which code the macro writes for a field is decided from the syntax: a nested
struct by `#[amestate(nested)]`, a map by `get_map_types` comparing the last
path segment against `"ReactiveMap"`, everything else a scalar. It has to be
decided there. A macro chooses tokens before there is a type to ask, so no
amount of trait machinery can move the *branch* into the type system.

**What the type answers is the description.** `shape::Probe<T>` reports `ROLE`
and `OPTIONAL` for every type there is: an inherent impl for each shape this
crate knows, and a trait const for the rest, which an inherent associated const
shadows. `FieldDescriptor` takes both from it, so the record of the shape is
read off the type and not off the branch - and it sees what a spelling cannot.
An alias resolves (`type Sessions = ReactiveMap<..>` answers `Map`,
`type Port = u16` answers not-optional), a renamed import resolves, and
`Option<Foreign>` answers optional while `Foreign` implements nothing at all.
That last one is why the probe is a probe and not a trait: nothing is required
of a leaf, so a leaf may come from a crate where no derive can be added.

**And the branch is checked against the answer.** Beside each non-nested field
the macro emits `assert!(Probe::<T>::ROLE.same(what the branch assumed))`, so a
wrong branch fails by name. `tests/fails/a_map_by_name_only.rs` - a foreign type
called `ReactiveMap` - fails with one sentence where it used to compile into the
wrong code. `tests/fails/map_through_an_alias.rs` still fails with a wall of
`Serialize` errors, because const evaluation runs after type checking, but ours
is among them and names the field and the fix.

**What is left is the emission.** Making an aliased map *work* rather than
diagnose needs `K` and `V` without reading them off the written type, which
means associated types on a trait the macro can name - and reaching that trait
needs a uniform `<#ty as AmeField>::Stored` in every branch. Leaves would need a
blanket impl:

```rust
impl<T: Serialize + DeserializeOwned + Default + Clone> AmeField for T { .. }
impl<K, V> AmeField for ReactiveMap<K, V> { .. }
```

and the compiler refuses the pair as overlapping - not because they overlap
now, since `ReactiveMap` implements neither trait, but because it may not assume
they never will. Specialization is not stable; autoref specialization is the way
through on stable, at the price of generated code that reads badly and
diagnostics that read worse when it fails to apply.

That is worth doing when the schema says what a description must contain, not
before: what such a trait hands back is whatever ends up in the file. `role` and
`optional` already come from the type, which was the part that blocked the
schema.

## The debouncer has two states and needs four

Alive and `is_poisoned`, and the second means a panic. There is no way to say
"stop taking work, write what is left, and be done", which is what closing
wants:

- after `shutdown()` the thread is still running and can schedule another
  flush, so the store is closed in the sense that matters and open in the sense
  that shows;
- a retry streak on the way out keeps retrying into a process that is about to
  end, where one report and a stop would do;
- "stopped because it was asked to" and "stopped because it died" are the same
  observable, and only one of them is a bug.

Not a fix for the static above - that needs the call either way - but it is
what makes the call mean something definite.

**Where the trigger comes from, since the phases do not invent it.** pingora
models the same thing as an enum of service phases, and the transition into
graceful shutdown is driven by a `SIGTERM` handler the server installs: the
library holds the phases, the outside world delivers the event. A desktop
application has the same event under other names - a window closing, winit's
`LoopExiting`, Tauri's exit event - and this crate already has an integration
sitting on each of them. So `shutdown()` need not stay on the user's memory:
the integration that already knows the application is quitting can call it.

`atexit` is the other candidate and is worth less than it looks. It exists on
both platforms through the C runtime, takes an `extern "C" fn()` with no
context - which suits a static fine - and runs when `main` returns or `exit` is
called. It does not run on `abort`, `panic = "abort"`, `_exit`, a kill, or a
power cut, so it covers only the case an application can already handle with
one line, and none of the cases where data is actually lost. Other threads keep
running while its handlers do.

## `.pipe()` keeps its sources alive with two of them and drops them with one

`IntoPipeline for R: Reactive<T>` (`core/primitives/pipeline.rs:250`) subscribes
to the source and drops it, keeping only `keepalive()` - which is `None` for
`Field` and `ReactiveMap`. The tuple impl (`:290`) captures a clone of every
source, so those live. One method name, opposite ownership. Confirmed by
running it:

| built as | after the source is dropped |
| --- | --- |
| `port.pipe().map(..)` | frozen at the old value |
| `(host, port).pipe().map(..)` | still live |
| `port.into_cell().pipe().map(..)` | frozen |
| `port.subscribe(cb)`, handle held | dead, no callback |

The third row is the worst: `into_cell` and `Kv::cell` exist precisely to be the
handle that owns, and `.pipe()` throws that away - `ReactiveCell::keepalive`
does not include `_owner`. The README's own pattern is affected: a component
that pipes one field and lets go of the state struct shows the right first value
and never updates again, with nothing to warn it.

**Done with the first half, which made the second unnecessary.** `pipe` pushes
`Arc::new(self)` into `keepalive`, so a pipeline holds the source and not only
whatever the source was holding. The prescription here also called for
`ReactiveCell::keepalive` to carry `_owner` - it does not need to. Once the
pipeline keeps the cell itself, the cell's own `_owner` lives with it, and the
test written for that case passes with the cell untouched. Written down because
the second change looked necessary until the first one landed.

The tuple form never had the bug, and now the two agree for the same reason
rather than by accident: it kept its sources through the closure that re-reads
them all.

`tests/pipe_keeps_its_source.rs` writes through the store rather than through a
field handle, because a `Field` clone shares one inner - holding one to write
with would keep the subscription alive by itself and the test would pass either
way. Checked by reverting the fix: the one-source case goes back to reporting
the value it started with.

That a bare `SignalSubscription` does not keep its source alive is still a real
choice and still undocumented.

## `SignalSubscription` is `Clone`, and dropping any clone cancels the original

`core/primitives/signal.rs:36` - the derive copies the id, `Drop` calls
`cleanup(id)`, and cleanup retains by id. So a clone is a second trigger rather
than a co-owner. Confirmed by running it: clone the handle, drop the clone, and
the original stops firing while still held. `ReactiveScope` is `Clone` too, and
that is what the macro's `subscribe_all` hands back - so a component handle with
a derived `Clone` stops updating after being cloned once.

The type is a cancellation token; it should not be `Clone` at all, or it should
be an `Arc<Inner>` whose `Inner: Drop` unsubscribes.

**Done: it is not `Clone`, and neither is `ReactiveScope`.** The `Arc<Inner>`
half of that choice keeps an API nothing asks for - no `SignalSubscription` or
`ReactiveScope` in the tree is cloned, and the clones in `Field` and
`ReactiveMap` are of `Arc<StoreSubscription>` and
`Arc<Mutex<SubscriptionHandle>>`, which are already shared. A second copy of
the right to end a subscription is a second way to end it, so there is none.
Pinned by `tests/fails/subscription_not_clone.rs`, since a type that cannot be
cloned cannot be tested for it at run time.

Removing it named its one victim, which is the one the entry above predicted:
`amethystate-gpui` derives `Clone` on `RpView`, which holds a `ReactiveScope` -
so cloning a view and dropping the clone stopped the original updating. That
adapter no longer builds, and is left for the adapter pass; the fix there is an
`Arc<ReactiveScope>` at the holder that wants sharing, said once and locally,
rather than on the token.

## The error model's seams with the outside world

Three, none about the contexts themselves - those are right.

`Report<C>` does not implement `std::error::Error`, so `?` from a
`StorageResult` into an `anyhow::Result` does not compile. `Box<dyn Error>`
works; `anyhow` is what an application's `main`, its Tauri commands and its task
bodies are actually written in, and every call site there becomes
`.map_err(|e| anyhow!("{e:?}"))`, which throws away the tree the whole
conversion was for. `Report::into_error()` is the sanctioned exit and nothing
points at it.

`error_stack` is in every public signature and is not re-exported from the
facade, though `serde`, `uuid`, `inventory` and `serde_json` all are. A caller
who wants `.attach()` must add the dependency themselves and keep the version in
lock-step or the traits do not apply.

There is no `From<Report<StorageError>>` for `Report<WriteError>`, so the store
layer and the reactive layer do not compose with a bare `?`. `WriteError` is
local, so the impl is allowed.

## `ReactiveMap`'s reads return `Result`, and none of them can fail

`reactive/map.rs:129,145,157,196,228,233`. Since reads moved to the projection,
`get`, `contains_key`, `entries`, `keys`, `len` and `is_empty` are `Ok(..)` with
nothing fallible above them. This is the line a GUI types most often, in a
render function with nothing to return an error to. `get` also takes `&K`, so
with a `String` key it is `widths.get(&"cpu".to_string())` - the doctests do
exactly that seven times.

**Done, both halves.** The six reads return their values, and
`get`/`contains_key`/`remove` take `&Q where K: Borrow<Q>`.

`remove` needed no change in `amethystate-core`: `map_remove` wants an owned
`K` because `MapChange::Remove` carries one, and the projection already holds
it - so the wrapper looks the key up and hands the owned one down. A removal of
an absent key still costs nothing and still returns `Ok(None)`.

Roughly sixty call sites across the tests, the benches and the doctests, each
of which was an `.unwrap()` or a `.to_string()` that now reads as what it
means.

**And it decides something for the table.** A read is infallible here only
because the map is resident - the projection holds everything, so a lookup
cannot touch disk and cannot fail. A collection that is not resident has a
`get` that reads, and a read that fails. So residency is not a detail of the
table's design either: it is the thing that settles whether a read returns a
value or a `Result`, and `ReactiveMap` has already answered it one way. Any
windowed form is a second type rather than a parameter on this one, or the
`Result` comes back to the line a GUI writes most often. See the reactive-table
RFC, which is being written against this.

## `AmeType` locks every foreign type out, and the user cannot let it back in

`Kv::get`/`set`/`cell`/`map` and every persistent leaf field require
`T: AmeType`. Impls exist for the numeric primitives, `bool`, `String`, `Vec`,
`Option`, `HashMap`. `IpAddr`, `Duration`, `PathBuf`, `SystemTime`, `BTreeMap`,
`HashSet`, arrays and tuples are therefore unstorable - and the user cannot fix
it, because both trait and type are foreign and the orphan rule forbids the
impl. This is not a coverage gap that more impls close; it is a hole with no
user-side patch, and it needs an escape hatch before the bound spreads further.
`Kv::get` takes the bound and never uses it.

A type the user writes is not locked out - `#[derive(AmeType)]` covers it. The
hole is a type from another crate, where neither the trait nor the type is
theirs. The way out is written up under the schema hash below: make the trait
optional, with the shape falling back to the type's written name when no impl
exists.

## Four sync map ops with no callers

**The trait itself is not the dead weight, and counting implementors was the
wrong question.** `AmeBackendSync` exists so that `map_ops` and `field_ops` are
written once against a backend and instantiated twice - for the sync world and
for the async one, whose `AmeBackendAsync` twin `TauriBackend` implements. One
implementor on the sync side is what a symmetric pair looks like from one side,
not an abstraction nobody uses. Removing it would mean either duplicating the
ops or fastening them to `Store`, and the async half would have nothing to
share with.

What is left of this entry is the four functions below, which really do have
nobody to call them.

`map_get`, `map_contains_key`, `map_entries` and `map_len`
(`core/primitives/map_ops.rs`) have **zero callers** anywhere now that the map
reads from its projection - confirmed by grep - while remaining `pub` and
re-exported, and still doing the buffered scan the `flush_prefix` entry measures
at 364 ms. The async twins are live; the sync four are not.

## Smaller, and cheap

- ~~`ReadOnlyReactiveMap` and `WritableReactiveMap` alias `Field`, not
  `ReactiveMap`, and take one type parameter where a map needs two.~~ **Done.**
  They alias `ReactiveMap<K, V, _>` now. A copy of the field aliases whose
  right-hand side was never changed, public, and used by nothing - the workspace
  had no callers, which is how it survived. The field aliases are live, so the
  pair earns its place once it means what it says.
- `reactive_map_with_path<TScope, ..>` binds `TScope: StateScope` and never uses
  it; callers turbofish four parameters for nothing.
- `Kv::keys` returns absolute paths, where `ReactiveMap::keys` returns
  `Vec<K>`. It should return the names below the namespace. (It returns
  `Vec<StorePath>` rather than `Vec<String>` now, which is the type being
  honest, not the answer being right.)
- ~~`StorePath::from_static` is public and unchecked, with a doc saying
  `joined` must match `segments` and nothing enforcing it. It exists for the
  macro; `#[doc(hidden)]` it.~~ **Stale, and so is the advice.** `check_static`
  enforces both at const-eval time, so a `const` whose halves disagree does not
  compile. And hiding it would now be wrong: its doc contemplates a
  hand-written `StateScope`, and an author writing one needs to be able to find
  the constructor.
- A leaf field with no `default` panics the proc macro
  (`generate/init.rs:115`), pointing at the attribute rather than the field, so
  a struct with ten fields does not say which one. The map and nested branches
  four lines above fall back to `Default::default()`.
- `get_map_types` decides a field is a map by matching the last path segment
  against the literal string `"ReactiveMap"`, so a type alias or a renaming
  import generates a scalar field instead. It does not reach disk: the `_Data`
  struct derives `Serialize` and `Deserialize` and `ReactiveMap` implements
  neither, so it stops at a compile error - an obscure one, about a missing
  `Serialize` in generated code, naming neither the field nor the reason. Make
  the misclassification say so itself. See the entry below on asking the
  compiler instead of guessing.
- Every prefixed struct gets a generated `new()` that calls `global_store()`,
  so the most obviously named constructor is the one that panics when there is
  no global store. There is no `try_init_global`.
- `ReactiveCell::update`/`modify` return `SourceGone` for an absent map key,
  whose message sends the reader looking for a lifetime bug they do not have.
  `KeyNotFound` is in the same enum.
- The README's headline example does not compile: `amethystate::Result` does not
  exist.

## Errors that reach nobody

From an audit of every bare `?` and every silent skip in `core/` and
`amethystate/src`. Ordered by what it costs.

**A failed migration is invisible through `StoreBuilder::build`.** The engine
turns a failure into data - `ComponentOutcome::Failed { error }` inside an
`Ok(report)` - and `build` (`store/builder.rs:262`) discards the report.
`build_with_report` calls `log_to_tracing`; `build` does not, and
`MigrationReport` is not `#[must_use]`. Confirmed by running it: a store at v1
with a v2 step that returns `Err` opens successfully, silently, holding
pre-migration data, and the application then runs new code against old data.
That is the thing migrations exist to prevent. `confy::get_store` and every
doctest take this path.

**Every engine discards its last flush on drop.** `let _ = self.close()` in
`redb/mod.rs:147`, `sqlite/mod.rs:516`, and `let _ = self.save_now()` in
`text/store.rs:178`. `close` is the only thing that commits the write buffer at
shutdown. redb's `close` even attaches "flushing the buffer before close", and
the attachment goes on the floor. `Drop` cannot return, but it can log.

**`confy::load_or_else` deletes the config file on any store-open error**
(`confy/mod.rs:410`). `get_store` fails on a poisoned mutex, on `create_dir_all`,
and on `build()` - which covers the database being locked by another process and
permission denied. None of those mean the config is bad; all of them delete it.
The report is discarded, so the error the user finally sees describes the
freshly recreated store rather than the original failure.

**`CommitSignal` reduces a report to one bool** (`store/durable.rs:35`). Every
producer has a `Report` in hand and throws it away; `outcome` then builds a bare
`CommitFailed` from nothing. A user awaiting a durable write on a full disk gets
the same one line as one whose database was deleted. Two smaller faults in the
same struct: `last_failed` is one flag rather than per-generation, so a waiter
across two overlapping flushes reads the wrong result; and `Commit::gone` gives
the same `CommitFailed`, so "the store was dropped" and "the write did not land"
are indistinguishable.

**The migration engine does not attach what the error model documents it
will.** `store/error.rs` says the frames around a step - which prefix, which
version, which store - are put there by the engine. At `engine.rs:371`
(`step.run(&mut ctx)?`) it holds all three and attaches none. Same for every
bare `?` on the bookkeeping calls in `migrate_prefix`, where `ensure_snapshots`
in the same file attaches carefully. On sqlite, whose `run_migrations` also does
not name the store where redb's does, a failed migration yields a report with no
locating information at all.

**Every interceptor rejection reports the same thing, including the one that is
not a rejection.** `run_interceptors` distinguishes three outcomes; all five
call sites collapse them to `Intercepted`. The damaging one is depth
exhaustion - nothing rejected anything, the guard refused to run because the
write is ten levels deep in interceptor-triggered recursion, which is a bug in
the caller's own code reported as a validation refusal.

**The file watcher can go deaf without saying so.** `text/store.rs:320` -
`let Ok(event) = res else { return };`. `notify` delivers its own failures
through that channel: a dropped watch, a lost handle, queue overflow. After one,
the store may stop seeing external edits entirely, and the only symptom is that
they stop arriving.

**`restore_from_backup` discards its errors while `open` claims the restore
happened.** `text/store.rs:95-104` is four `let _ =` over `fs::copy` and
`fs::remove_file`; `open` then attaches "the files were restored from their
backups". If the copy failed, that attachment is a claim the discarded error
would have refuted, and a reader who believes it will not check the file.

**`entry_cell` turns a read failure into "the key is empty"**
(`reactive/entry_cell.rs:61`), which is the vocabulary the cell reserves for a
removed key. The real defect is the signature: `entry_cell` returns
`ReactiveCell<V>` with nowhere to put an error.

**Poisoned-lock fallbacks that silently disable a subsystem.**
`map_core.rs:289,298,310` fail open in `notify` while the same file uses
`.lock().unwrap()` in seven other places - so a poisoned mutex makes
`subscribe_any` panic while `notify` quietly delivers to nobody, permanently.
`observability/mod.rs:77,87` does the same to the registry that `Kv::check_type`
consults, turning off the guard against one path being claimed as two types.

**`Kv::keys` breaks the `Kv` error type** (`store/kv.rs:204`): it returns
`StorageResult` where every other method returns `WriteResult`, so a caller
using `get` and `keys` in one function needs two error types.

## The background flush can fail silently, and a waiter on it can hang

Found while converting the engines to `error-stack`, in redb and sqlite alike.

The debouncer callback is `FnMut()` with nowhere to return to, so it discards
every error: redb's closure is an `Option`-returning block full of `.ok()?`
(`backend/redb/mod.rs:235-254`), sqlite's uses `Err(_)` and `.is_err()`
(`backend/sqlite/mod.rs:587-644`). A full disk, a missing table and the test's
`SIMULATE_WRITE_FAILURE` all collapse into one bare `false`, and nothing is
logged even though `tracing` is already in scope in both files. This is the
background write path, so a user's data fails to land with no trace anywhere.

Worse in sqlite: if `conn.transaction()` or any of the three `prepare` calls
fails, the closure returns **without** calling `commits_save.finished(..)`. A
`Commit` riding on that flush is never woken. That is a hang, not a lost error.

redb's synchronous `flush_prefix` has the matching hole: `commits.finished(true)`
is only on the success path (`backend/redb/mod.rs:134`), so every `?` above it
returns without telling the waiters anything.

`StorageError::CommitFailed` is the context these want, and `CommitSignal`
already carries a failure flag - what is missing is calling it on the way out.

**Done, on all five engines.** The background debouncer retries a failed
flush at a fixed interval instead of swallowing the first failure, and keeps
retrying until it lands or the store is dropped. `retry_budget` does not bound
that - a full disk is usually somebody about to delete something, and a store
that stopped trying could not heal when they did. It bounds the *silence*: a
streak outliving it escalates once, waking any `Commit` waiter with a failure
and asking `on_persist_failure` what writers should be told from there.

That answer is [`AfterGivingUp`]: `Fail` (the default with no callback) marks
`PersistHealth`, so every later write returns `StorageError::CommitFailed`
naming the reason until a flush lands and clears it; `Ignore` says nothing and
keeps buffering; `Poison` is the old behaviour, now opt-in. Poisoning the
writer for a disk that is briefly full is the reaction least worth having by
default - the application is running, its reads are fine, and the thing it
most needs is to be told, not killed. All three configurable per store
(`StoreBuilder::retry_interval`, `::retry_budget`, `::on_persist_failure`).

The `changes.is_empty()` early return that skipped notifying entirely - so
`flush_async()` on an idle store hung forever, no failure required - is gone
too, folded into the same mechanism as a trivial success. `apply_pending`
factors the table-writing loop out of both the sync and background paths,
which is also what gives the background one a real error to log instead of
`.ok()?`.

[`AfterGivingUp`]: crates/main/amethystate/src/store/config.rs

**What "retry" cannot mean on redb, found while building this.** A real I/O
error - not the test's `SIMULATE_WRITE_FAILURE`, which returns before ever
reaching `Database` - sets an `AtomicBool` in redb's own `CachedFile`
(`cached_file.rs`, `io_failed`) that nothing in the crate ever clears. Every
`begin_write` *and* `begin_read` after that checks it first and returns
`StorageError::PreviousIo` without touching disk - confirmed against redb
4.1.0's own source, including its own test at `db.rs:1395-1410` doing exactly
this. So a retry loop that just calls `begin_write` again is not retrying the
failing operation; on the one failure mode this was built for, it is spinning
at `retry_interval` until the budget runs out, on a `Database` handle that
already decided it is dead - and taking every *read* down with it, not only
writes. The doc's own wording says how to recover: close and reopen the
`Database`. Doing that live would mean every holder of `db: Arc<Database>` in
`RedbStoreInner` - not only the flush path - going through something
swappable (`ArcSwap` is already a workspace dependency) that notices
`PreviousIo`/`DatabaseClosed` and reopens rather than a bare `Arc`.

**Done: redb trades the handle in.** `Fail` and `Ignore` both promise that a
flush landing later heals the store; on redb that was a promise the engine
could not keep, since the retry could never land. It now reopens instead.

`db` is an `ArcSwapOption<Database>` rather than an `Arc<Database>`, and the
`None` is the point: redb holds the file lock for as long as a `Database` is
alive, so reopening is not "make the new one and swap it in" - the old has to
be dropped before `Database::create` can take the lock back. The caller holds
`write_lock` across the gap.

Which also settles what a durable write does, and it needed no separate code:
`flush_prefix` takes `write_lock` first and the reopen holds the same lock, so
a commit runs before or after the swap and never during. A durable write waits,
which is what it promises anyway; a read or a scan takes no such lock, sees the
`None` and is told, rather than blocking a UI thread on a file operation. Keep
the two on one lock and that stays true for free.

Both flush paths reopen on `PreviousIo` - the background one so the retry loop
lands on the next attempt, the synchronous one so a durable write recovers
instead of reporting something the caller can do nothing about.

The one thing that had to be true is that nobody else holds a `Database`, or
the lock never comes back. One did: the background flush held its own clone,
which would have kept the file locked for the life of the thread. It holds the
swap now. `the_database_can_be_traded_for_a_fresh_one_under_a_live_store`
exists to fail the moment a second handle reappears anywhere.

A real `PreviousIo` end to end is covered too, and it was worth the trouble.
`a_disk_that_fails_for_real_is_recovered_by_trading_the_handle` opens the store
on a `StorageBackend` that fails its writes - redb's own seam, reached through
`create_with_backend`, so the latch that follows is redb's rather than a
simulation of it - takes the disk away, gives it back, and asserts the
buffered write lands. It failed on its first run, because `is_previous_io`
answered `false` to a genuine `PreviousIo`: the predicate matched on this
crate's `RedbStoreError`, and the errors that actually carry the latch are
redb's own. `begin_write` fails with a `TransactionError` and `commit` with a
`CommitError`, and `.doing()` is a `change_context` that leaves them in the
report unwrapped. So the reopen would never have fired on the one failure it
was built for, and every test that passed until then had reached the latch by
constructing it rather than by breaking a disk.

The whole of it lives in `backend\redb\recovery.rs` - the swappable handle, the
predicate, the trade, and the tests that break a disk to reach it.

The failing disk is armed by path rather than by a flag, and that is not
tidiness. A global switch is consulted by `create_database`, so while one test
held it on, any store opening in parallel got a broken disk - which is exactly
what `test_drop_behavior_is_deterministic` did, being one of the tests here
without `#[serial]`. It arrives as a failure in a test that has nothing to do
with any of this, whose own code never mentions a disk. Naming the one path
that may break means a test that did not ask for one cannot be handed it, and
the guard puts the disk away even when an assertion panics.

**sqlite and the text engines do not share redb's problem.** Neither rusqlite
nor SQLite itself has anything resembling `io_failed`: a failed write rolls
back its own transaction and leaves the `Connection` usable for the next one,
which is the whole premise `busy_timeout`-style retrying on SQLite already
relies on. The text engines write a whole file with `persist_atomic` and have
no live handle to poison at all. So the same mechanism - retry, budget,
poison, notify - is wired into all five engines now, and only on redb is the
retry itself unable to do what its name says; sqlite and the text engines get
a real second chance, not just a wait.

**Done, the rest of it.** `apply_pending` (redb, sqlite) factors the
table-writing loop out of both the sync and background call sites within each
engine - not across engines, which the architecture pass below this entry
found not worth it. `utils::init_key` replaces four hand-written
`format!("__init::{namespace}")`s with one. `RetryPolicy`
(`StoreConfig::retry_policy`, `StoreBuilder::retry_interval`/`::retry_budget`)
and `on_persist_failure` are configurable per store, defaulting to a 5 second
interval and a 60 second total budget - sized against this project's own
stated write profile (thousands of buffered keys in a burst, not a handful of
settings) rather than guessed, and closer to what a survey of comparable
systems found than the redb `busy_timeout` convention would suggest on its
own: nothing surveyed actually retries silently for a bounded time and then
deliberately crashes - the real spectrum runs from failing fast with no retry
at all (redb's own stance, and Core Data's explicit advice against retrying a
failed save) to a bounded *count* of attempts degrading to read-only rather
than crashing (RocksDB, VS Code) to crashing on the very first failure with no
retry (PostgreSQL's fsync `PANIC`, adopted because retrying itself was unsafe -
Linux clears the dirty-page error flag after reporting it once, so a retry can
silently succeed over data that never actually landed). What landed is closest to the middle
of that spectrum - keep trying, degrade rather than die, and let the
application escalate if it wants to - with the crash kept available and
nobody's default. Three tests in `redb/mod.rs` pin it: writes fail rather than
the process, a disk that comes back heals the store with nothing restarted,
and `Poison` still takes the writer down for an application that asks.

## The sqlite migration adapter still scans by `GLOB`

`backend/sqlite/migration.rs:128` builds its prefix scan as
`WHERE key GLOB ?` with `format!("{}*", prefix)`. `utils::key_range` exists
precisely so this is not done: a name may hold GLOB metacharacters - `panel[0]`
is a name - and nothing escapes them. `ui*` also matches `uix.width`, with no
separator boundary. The main engine's path was fixed; this one was missed.

## `confy`'s error conversion is written against an error model that is gone

`confy/mod.rs:134-177` destructures `RpError::TextStore(..)`, `RpError::Codec(..)`,
`RpError::Path(..)`. `StorageError` is now a payload-free enum naming the
operation, so that whole match is stale. It only compiles under `confy-compat`,
which is why nothing has noticed.

## A durable write commits a different amount on each engine

`Durable::set` calls `flush_prefix`; `Durable::set_async` calls `flush_async`,
which every backend implements as a full `save_now` of the whole buffer. The
same pair on `ReactiveCell`, `ReactiveMap` and `Kv` behaves the same way. So the
two forms of one operation have different granularity, and the documentation of
the sync form is spent explaining a granularity the async form does not have.

The engines then disagree about the sync form too.
`tests/durability_crash.rs` runs one statement against all five: write one field
plainly, one durably, abort the process, reopen. The durable write is there
every time. The plain one is gone on redb and sqlite - and present on json, toml
and ron, because `flush_prefix` there ignores its prefix and calls `save_now`
(`backend/text/store.rs:642`), so committing anything commits everything.

Nothing about that is unsafe, and it is why the test asserts it rather than
papering over it. But `Durable` is documented as a promise about one write, and
on three of five engines it is a promise about the store. A caller batching
writes for cost cannot tell which they have.

Distinct from the `flush_prefix` entry at the top, which is about how much rides
along with a prefix flush.

## `LocalScope::clear` does the opposite of what it says

The doc reads "drops the queued values without delivering them, leaving the
subscriptions in place". The body clears `subs` and `pumps` - the subscriptions
unsubscribe on drop, and the queued values live in the pumps' captured buffers,
so it drops exactly what it promises to keep and keeps nothing it promises to
drop.

Which side is the bug is a decision: there is currently no way to discard a
backlog without unsubscribing, and the name suggests there should be.
`LocalScope::len` and `is_empty` document a queue length too and return the
subscription count.

## The book documents a library that is no longer there

Found by reading it end to end against the sources. Not a list of typos - these
are things a reader following the book cannot make work:

- `set_or_create` appears in five pages and exists nowhere; it is `insert`
  since the rename. One section is built entirely on it.
- `StoreBuilder::collect_migrations` and `amethystate::Result` do not exist.
- The migration pages destructure a report out of `build()`, which returns a
  store.
- `Concepts/reactive-cell.md` documents the owning cell throughout: it teaches
  building cells and dropping the struct they came from, which now yields a map
  of dead cells, and never mentions `into_cell`, `into_entry_cell` or
  `Kv::cell`. `entry_cell` is shown with a `default` argument it no longer
  takes, and `get()` is used as `T` rather than `Option<T>`, so several
  snippets would not compile.
- `Concepts/kv.md` predates namespaces: `keys` is shown with an argument, and
  every dotted example now addresses one name rather than the levels it means.
  It also teaches the type check (`// Err(TypeMismatch)`), which is gone along
  with the variant; what refuses a `Kv` write is ownership of the path.
- The dioxus and leptos pages name the provider component `amethystateProvider`;
  it is `AmeStateProvider`. The dioxus page uses both.

Rustdoc has its own: the macro's own documentation gives constructors that do
not exist (`new(&Arc<Store>)` where the real one takes no arguments and the
store is already `Arc`-backed), and says `default` is required on leaf fields
where the code falls back to `Default::default()`. `Kv::set` and `Kv::remove`
open by promising the durability their `Durable` counterparts provide.

`Concepts/observability.md` promises `location` is the caller's `file:line`,
which it now is: `#[track_caller]` runs the whole way through the `Watch`
builder - `register`, `register_with_source`, `stream`, the `watch_raw`
declaration and its four implementations - so a subscription made the way the
subscriptions chapter teaches records the call site rather than a line in this
library.

## What tampering with a text document does, found by doing it

`tests/tamper_*.rs` write a store, edit the file the way a person or another
tool would, and reopen. Every failing test asserts the behaviour that would be
right, so its failure message is the finding. Worst first; six of these lose
data with no error at all.

The suite is ordinary tests now: what still fails carries an `#[ignore]` naming
the finding, and everything else is green. Every file but
`tamper_engine_contrast.rs` is gated on a text feature, and that one is the
control - on redb and sqlite it passes, which is the point of it.

**A gate is not a choice of engine.** `#![cfg(any(feature = "json", ...))]`
decides whether a file is compiled; which engine it runs against is
`default_backend()`, and that prefers redb. With `default = ["redb"]`, any
build that has redb on runs these tests against redb, so the seeded document is
a file the store never opens - which is the `--all-features` cell of CI. Under
`--features json` the suite was 0 of 6 on `tamper_names`, and `watcher_race`
was green while testing nothing, the file watcher being a text-engine part that
redb does not have. Every store in the eight affected files now names its
backend, through `common::text_backend()` where the format follows the build
and `Backend::Toml` in the toml-only file. A test about documents that does not
say which engine it wants is asserting about redb.

**A level named `.` is the whole document.** `normalise_parts` maps `["."]` to
the root (`document.rs:45`), and `StorePath::segment(".")` is a legal one-level
path, so `kv.set(".", &value)` replaces the entire document and `get_raw` on
`.` returns the whole store. `delete(["."])` removes nothing and emits a
`Delete` anyway. json, toml, ron; redb and sqlite have no root alias and are
unaffected. `tamper_dot_sentinel.rs`, 7 failures on each format.

**An empty TOML file is a valid empty document.** `TomlDocument::parse`
(`toml_doc.rs:84`) has no root check, where json and ron reject a non-object
root. An editor's truncate-then-write window therefore reads as "every key
deleted": subscribers are told, and the next save writes the emptiness back.
The watcher's debounce cannot help, because the truncated file parses.
`tamper_live.rs`, `tamper_toml_inline.rs`.

**Writing under a TOML inline table or array-of-tables empties it.**
`ensure_map` tests `is_table()`, false for `Item::Value(InlineTable)` and
`ArrayOfTables`, and replaces the node (`toml_doc.rs:24`). `cfg = { width,
height }` plus one `set(["cfg","scale"])` loses both. `tamper_toml_inline.rs`.

**A declared section holding a scalar or a list is wiped at startup.** Same
`ensure_map` in all three formats; `field_with_path` writes its default when the
read is `None`, and the walk to the parent replaces whatever stood there.
`tamper_shapes.rs`.

**TOML reads a section back as one of its children.** `with_bytes_de` renders a
non-value node as `val = ...` and cuts at the first `=`, which for a table is
the one inside it (`toml_doc.rs:150`). `[cfg.width]\npx = 800` reads as
`Some(800)`. json and ron error here, which is the right answer.

**Deleting inside a TOML inline table reports success and removes nothing.**
`remove_child` uses `as_table_mut()` (`toml_doc.rs:33`); `store.rs:426` emits
the `Delete` regardless, so a bound `Field` resets to its default while the
store still holds the old value, and a restart brings it back.

**The metadata is a second file nothing binds to the data.** Versions,
snapshots and `__init` markers live in `path.with_extension("meta")`
(`store.rs:191`). Losing it replays migrations over migrated data - 21 doubles
to 42, then to 84 - restores defaults the user deleted, and a forged marker
suppresses the real ones. redb and sqlite keep this in the same transaction as
the data, so it cannot come apart. `tamper_meta.rs`.

Decided: bind the two files rather than merge them. Folding the metadata into
the data document would make every save rewrite bookkeeping that can be large
next to the data it describes. Instead the metadata carries a checksum of the
data, which has to be *maintained* and not only checked - a checksum that goes
stale on the first ordinary write reports a divergence on every startup - so it
is written in the same save, data first and metadata second, and a crash between
the two reads as a divergence rather than as quietly wrong state.

What a divergence then means: the metadata is untrusted, so nothing is replayed
and nothing is re-seeded from it. Versions cannot be recovered, so a migration
does not run and says why. The `__init` markers can be recovered, through the
empty node written up above.

**An unrelated pending write rolls back a concurrent external edit.**
`sync_external_changes` refuses to pull while `writes != persisted`
(`store.rs:826`) and a persist writes the whole document from memory, so one
buffered write anywhere discards every hand edit, including to untouched keys.
`tamper_live.rs`.

**A broken external edit is dropped without a word and then overwritten.**
`D::parse` fails, `sync_external_changes` returns early (`store.rs:815`),
nothing reaches the caller, and the next save replaces the half-written file.

**The data and metadata shared one backup path.** Fixed: the copy keeps the
whole name and adds `.bak`, so `store.db.bak` and `store.meta.bak` are two
files. `with_extension("bak")` gave `store.bak` for both, so the second copy
landed on the first and the data had no backup left - and it named a file the
store never created, a `store.bak` a person put there themselves, which it
overwrote and then deleted. Both tests in `tamper_broken_file.rs` run now.

**A key with no name is invisible to every scan.** Decided: it stays that way,
and now says so. A document may hold `{"": 1}` and a level with no name is not a
path, so the scan passes over it and logs at `warn` - listing it would hand back
a key that does not read back as a path, and refusing would let one name nobody
meant to write stop the store from listing anything else. The value keeps its
place in the file and survives a save; nothing addressed by a path reaches it.
Written up on `scan_keys` and `Kv::keys` through `store/scan_contract.md`.

Making it addressable was weighed and dropped. Only one case is genuinely
ambiguous - `["cfg", ""]` already joins to `"cfg."` and is merely refused, while
`[""]` collides with the root - so a marker pair such as `\0` would settle it.
That means changing `join`, `parse_joined` and `joins_to` together, in the one
function that has no right to be wrong, to address a key that nothing in the
library can write and nobody writes on purpose.

**Duplicate keys diverge, and every engine is already right.** `{"a":1,"a":2}`
opens on json and ron with the last value winning and the first gone at the next
save; toml refuses to open, naming the line. Neither is a defect of ours: the
TOML spec forbids a key defined twice, and RFC 8259 leaves it undefined, so
last-wins is what every other json tool does with the same file. Unifying them
would make each engine behave unlike every tool a person edits that format with,
which is the more surprising answer. The parsers resolve it before the document
reaches us, so there is nothing to report even if we wanted to.

Held up under the same tampering, worth knowing: wrong scalar types at a
declared field fail loudly on all three; undeclared keys survive a rewrite; a
truncated or scalar-rooted file is refused and left byte-for-byte intact; a
scan over a prefix lists the value at the prefix itself identically on all five
engines.

## What the conformance suite says the engines disagree about

`tests/backend_conformance.rs` states twenty-nine properties about what a store
is and runs each against every engine compiled in. redb and sqlite pass all
twenty-nine. What the three text formats fail is the finding: they share one
implementation and diverge from the flat engines in exactly one place, the
document walk.

json and ron fail two: `a_scan_lists_exactly_what_is_under_the_prefix` and
`writing_then_deleting_leaves_the_store_as_it_was`. toml fails those and
`an_ancestor_is_not_a_value`, through the `with_bytes_de` cut at the first `=`.

Four that used to fail no longer do. `a_level_named_dot_is_an_ordinary_level`
and `a_leaf_and_a_branch_coexist_at_one_name` are written up above.
`a_write_leaves_every_other_path_alone` went with the second path parser.
`deleting_what_is_not_there_changes_nothing` was toml alone, and its cause was
`Navigable::get_child_mut` reaching a child through `Item`'s `Index`, which
inserts the key it is asked for - so the walk to an absent path built the
levels on the way. It now goes through `as_table_like_mut`, which is what
`remove_child` beside it already did.

Read those counts against the next paragraph: which inputs a property sees is
not the same twice.

**The failing set moves between runs, so it is not a gate.** `config()` sets
`cases: 24` with `failure_persistence: None` and no seed, so every run draws
different names. Two runs of the same tree gave json 2 and toml 4 one time and
json 3 and toml 3 another - a genuine regression is indistinguishable from a
different draw. Either pin the seed for the properties whose divergence is
recorded, or `cfg_attr`-ignore them per engine so what is green is decided
rather than drawn. The generated-input value is worth keeping somewhere; it is
worth keeping away from the set that says whether the tree is broken.

### Decided: a document engine refuses where it cannot represent

A tree cannot hold a value at a node and values under it at once, so property 12
asks the text engines for a document that does not exist. Three ways out were
weighed: make the flat engines enforce the same restriction (a range scan on
every write, and it forbids what those engines can do perfectly well); give the
document a reserved key for "the value of this node" (kills hand-editability,
which is the reason the text engines exist, and collides with a real key of that
name); or let the engines differ and replace destruction with refusal.

The third. Property 12 becomes a disjunction - the two coexist, *or* the second
write is refused and the first survives - which all five engines can satisfy and
which still forbids the thing that actually hurts, silent destruction.

That generalises: the suite states one contract for engines built on genuinely
different substrates, and the parts of it a document cannot honour are better
recorded per engine than demanded of everyone. What stays universal is the
narrow surface the schema itself uses, because the generated code calls
`field_with_path` without knowing the engine and is unsound if that surface
differs.

Two things the change has to get right. The destruction is one line, written
three times - `ensure_map` replaces the node when it is not a map
(`json_doc.rs:25`, `toml_doc.rs:25`, `ron_doc.rs:33`), and `insert_child` calls
it, so both write orders destroy through it. And the refusal must not travel up
through `field_with_path`'s seeding write, which nobody asked for: a field whose
parent is occupied keeps its default in memory and leaves the file alone,
rather than failing the whole struct.

The collision is reachable from the schema, not only from `Kv` - `prefix =
"root"` with a field `b` alongside `prefix = "root.b"`, as written up above - so
the refusal has to name both declarations, not just the two paths.

### What the refusal can and cannot see

Half of it is undetectable, which the first attempt at the change proved by
breaking every migration test on the text engines. A serialized struct is a map
with children; so is a level with values under it. In a document the two are the
same bytes. The store's own bookkeeping writes a struct at `schema.<prefix>` and
also writes under it, so a rule of "refuse a write at a level that has children"
refuses the library's own meta writes.

So the two directions are not symmetric:

- Writing *under* a level that holds a plain value is unambiguous - a scalar is
  never a branch - and is refused.
- Writing *at* a level that has children is refused only when the incoming value
  is not itself a map. A map written over a map is taken as the update it almost
  always is.

What is left uncovered: a struct written over a level that had unrelated values
under it. The flat engines keep both, a document engine cannot, and nothing in
the bytes says which was meant. That is the residual divergence, and property 12
is written to allow it rather than to pretend otherwise.

It kills a third idea too, and this one is worth writing down because it looks
harmless. Pruning a branch that a delete just emptied would fix
`writing_then_deleting_leaves_the_store_as_it_was` - the byte-identity property -
but a node that has just lost its last child is `{}`, and a field whose value is
an empty map is stored as `{}` as well. Deleting inside a stored map would then
delete the field. The property it buys is cosmetic and the failure it risks is
not, so the empty node stays and the property stays recorded. `delete_prefix`
removes the subtree node whole, and `load_map` skips a scanned key equal to the
map's own path, which is where the leftover actually used to hurt.

### The empty node is load-bearing, not litter

There is a second and stronger reason not to prune it, found while working out
what a lost metadata file can be recovered from. "This namespace was seeded" is
one bit that no amount of reading the data reproduces - except that it does,
through exactly this leftover:

```
{ "items": {} }        the map existed and was emptied  -> do not seed
{ "unrelated": 1 }     the map never existed            -> seed
```

Without it the two are the same observable state, and `tamper_meta`'s
`losing_the_metadata_file_does_not_resurrect_removed_defaults` and
`a_forged_marker_does_not_suppress_the_defaults` demand opposite answers for it.
So the byte-identity property is not a deferred fix, it is a permanent
divergence: a document engine cannot both round-trip byte for byte and remember
that a namespace was once written.

The flat engines have no such node - there is no key at `items` - and need none:
their metadata lives in the same transaction as the data and cannot be lost on
its own. The recovery route exists exactly where it is needed.

The same ambiguity kills the matching idea for `delete`, and there it cannot be
worked around. `delete` refusing to remove a node with children looks right -
`delete(["a"])` where only `a.b` exists should take nothing, which is what a
flat engine holding no key at `a` answers - but a field whose value is a map or
a struct is stored as exactly that node, so the rule refuses to delete it.
`set` can tell the two apart by looking at the value being written; `delete` is
handed nothing but a path. So it removes whatever is there, and property 5
belongs in the same recorded-divergence bucket as property 12 rather than being
demanded of everyone.

**A scan on a text engine goes one level deep.** `scan_prefix_impl` and
`scan_keys_impl` set `target_depth = parts.len() + 1` (`text/store.rs:825`,
`:1004`), so a scan lists direct children only; anything deeper comes back as
the intermediate branch, with a serialized subtree for its value. redb ranges
the whole subtree and sqlite ranges `key_range`, so both list every key at any
depth. A value three levels down is invisible to a scan of its grandparent.
`ReactiveMap` survives this only because a map's entries are always exactly one
level below it. `Store::scan_keys` means two different things depending on the
engine.

**`delete` at a path that holds no value takes everything under it.**
`generic_delete` removes the node, so `delete(["a"])` where only `a.b` exists
deletes `a.b`. On the flat engines there is no key at `a` and nothing happens.
On a document engine `delete` and `delete_prefix` are the same call.

**On toml, deleting an absent path creates the levels on the way to it.**
`Navigable::get_child_mut` for toml is `Item::get_mut`, which is
`Index::index_mut`, which does `entry(key).or_insert(Item::None)`.
`generic_delete` walks the heads with it, so the walk vivifies, and the phantom
branches are then listed by the next scan. json and ron do not - a difference
*within* the shared text implementation.

**Reading a path that holds no value but has values under it gives three
answers.** redb and sqlite say `Ok(None)`. json and ron give a decode error, the
branch object not being a `u32`. toml gives the child's value, through the
`with_bytes_de` cut at the first `=`. None of the text answers is `None`.

**The error model does not agree.** redb and sqlite report undecodable bytes
with `current_context() == StorageError::Codec`. All three text engines wrap it
once more at `text/store.rs:652`, so the outermost context is `Read` and `Codec`
is a frame below. A caller matching on `current_context()` cannot tell "the
bytes are the wrong type" from "the file would not read". This is exactly what
the error model was meant to make assertable.

**Events: covered now, and it found what it was written to find.** Properties
22-24 state what one operation emits: a write is one `Set` carrying the value
that landed and the one it replaced; a delete is one `Delete` carrying the value
that went, and a delete that removed nothing says nothing; `delete_prefix` is
one `DeletePrefix` at the prefix rather than a `Delete` per key.

The middle one failed on **all five** engines, not only the text ones - each
emitted a `Delete` with `old: None, new: None` for a path that held nothing, so
a subscriber acted on a change that did not happen. Each engine now returns
before the event, and before scheduling a flush for a document it did not
change.

Still uncovered: concurrency between two handles, the async surface,
`is_initialized` across a failed flush, and value shapes past `u32`/`String` -
nested structs, enums and sequences are where the three text formats differ most
from each other and from msgpack.

## A cleared map leaves a node behind, and only on the text engines

`clear()` deletes the prefix. On redb and sqlite the keys go and nothing is
left. On the text engines the container stays: after clearing `probe.items` the
json document holds `{"probe": {"items": {}}}`, and the next scan of the prefix
reports the prefix itself as a stored key with an empty object for its value.

Two consequences, one of them already load-bearing. `load_map` reads a scan
strictly, so an empty node at the map's own path was a hard failure on reopen -
`clear_survives_a_store_rebuild` went red on all three text engines. It now
skips a scanned key equal to the map's path, on the grounds that a map's entries
are the level below it and nothing is stored at the path itself; that is right
whatever the engine leaves behind, and it does not soften the strictness about
keys that really are under the path. `map_len` still counts the node, though
`ReactiveMap::len` reads its own projection and so does not.

The root cause is `delete_prefix` not pruning a branch it emptied, which is also
why `writing_then_deleting_leaves_the_store_as_it_was` fails on the text
engines. Fixing the prune fixes both, and would let the skip go.

## An interceptor says why it refused, and the field drops it

`FieldCore::run_interceptors` returns `Err(String)` naming what happened -
`"Maximum intercept depth reached"` is a bug in the caller's code, a refusal by
a filter is not - and both call sites throw it away with `map_err(|_| ...)`
(`field_ops.rs:22`, `reactive/field.rs:452`). The report that reaches the caller
says only "an interceptor rejected the change", so a validating interceptor
turning a value down and interceptors recursing past the depth guard are the
same message.

The map side is fixed: `map_apply_change` attaches the sentence and names what
the change reached, so a refused `insert` and a refused `clear` no longer render
identically. The field wants the same, and the ephemeral branch in
`field.rs:452` wants a `Report` rather than a bare `FieldError`, which is
separately why that one carries no path at all.

**Done.** Both field call sites carry the reason through
`FieldError::intercepted`, and the ephemeral branch builds a `Report` naming the
field and saying that nothing was going to be stored either way. Both are pinned
by snapshot in `tests/error_reports.rs`, so a refusal that collapses back to one
sentence fails a test.

## The schema belongs in the store, as JSON Schema

Its own track, and the answer several entries here have been waiting for.

**Everything migrations, in one place.** The entries are spread through this
file because they were found at different times; what they have in common is
that the code is the only thing that knows a shape. Ordered by what has to be
decided first.

| entry | where it stands |
| --- | --- |
| this one - the schema in the store | decides the rest |
| The fork under all of it: is the file a store, or a picture of a type | the same question, asked wider |
| Metadata carries no format version - deliberately, for now | where a format version would live, once there is a schema |
| Two type hashes, both weak, and the weaker one feeds the gate | stops being the gate; kept as the record |
| Reordering struct fields silently corrupts data on redb | the fold was written and reverted for this reason |
| `build()` runs no generated migrations, and nothing at the call site says so | becomes a comparison the store makes, not a question of who collected what |
| Migration cleanup addresses a field by its Rust name, not by where it is stored | six ignored tests waiting on it |
| Migration cleanup deletes one key, so a composite field survives being dropped | same repair as the row above |
| `#[migrate]` can only be found through the linker | **done** - `#[migrate(explicit)]` and `add_steps` |
| The sqlite migration adapter still scans by `GLOB` | independent, and small |
| A failed migration is invisible through `StoreBuilder::build` | inside *Errors that reach nobody*; independent of all of this |

The last two are the only ones worth touching before the track is decided.
Everything above them is either waiting on it or is the record of why something
was stopped.

The code is the only thing that knows a shape today. A snapshot records what
was seen last time, `inventory` gathers what is declared this time, and a pair
of weak hashes stands in for comparing them - which is why a hash collision is
a data-loss bug rather than a diagnostic one, and why the entry above on
reordering fields had to reason about fold algorithms at all. A schema the
store carries in a form that is not this crate's own would replace the hashes
with a comparison, and the comparison would be able to say *what* differs
rather than *that* something does.

JSON Schema rather than a private format because the point is that the file
answers for itself. A store that carries one can be read by something that is
not this library, migrated by a tool that is not this binary, and diffed by a
person - none of which is possible while the shape lives only in a Rust type
and a `u32`.

JSON Schema is the **model**, not the storage: each engine encodes the document
the way it encodes everything else, msgpack under redb included. Writing it as
literal text inside a binary store so that `strings` finds it buys nothing - a
binary engine needs a viewer either way, and sqlite's existing viewers already
render its json, so nothing about them changes.

**Decided, and it is two halves.** Plain JSON Schema for what a value looks
like - nothing of this crate's invention in it, so anything that reads JSON
Schema reads ours. And this store's own semantics as a role per declared path,
from a closed set: `field`, `map`, `nested`, `table`. What the schema cannot
say is which paths are levels and which hold values, and that is exactly what a
store needs to know; the roles say it and nothing else has to.

Three of the four are already in the code - `Role::Field`, `Role::Map`,
`Role::Node`, which is `nested` under another name - and carried per field in
`FieldDescriptor`. `table` arrives with the primitive; see the reactive-table
RFC. So the vocabulary is not being invented here, it is being written down and
made persistent.

### How the shape is learned: ask the compiler, not the spelling

A field contributes four scalars to the document - its name, its role, whether
it is optional, and the name of its type. Three of those are answers about a
type, and the compiler is the one that has them.

`AmeType` is required of nested `_Data` structs, which the macro generates, and
of nothing else. A leaf may be any type at all, including a foreign one from a
crate where no derive can be added, and what the macro writes about it is its
name. Describing it further at compile time would mean requiring a derive on
user types, which costs more than the description is worth - but at run time
serde will describe it for nothing, which is *A leaf is opaque at compile time,
and serde can open it at run time* below.

The questions are asked through a probe, which is inherent impls for the shapes
this crate knows and a trait fallback for every other type:

```rust
pub struct Probe<T: ?Sized>(PhantomData<T>);

pub trait AnyShape { const OPTIONAL: bool = false; const ROLE: Role = Role::Field; }
impl<T: ?Sized> AnyShape for Probe<T> {}

impl<T> Probe<Option<T>>            { pub const OPTIONAL: bool = true; }
impl<K, V> Probe<ReactiveMap<K, V>> { pub const ROLE: Role = Role::Map; }
```

An inherent associated const shadows the trait's, so the compiler picks. The
macro emits `<Probe<#ty>>::ROLE` and never looks at how the type was written.
Measured on stable 1.95, including in `const` context.

Two properties follow, and both are why this rather than matching on the
spelled name:

- **`Option<Foreign>` answers `true` while `Foreign` implements nothing.** The
  modifier is visible without the inner type being bound by anything.
- **Aliases and renamed imports resolve.** `type Maybe = Option<Foreign>` is
  optional, `use Option as Perhaps` is optional, `type Port = u16` is not. A
  name match would answer all three wrong and report drift for a rename.

`OPTIONAL` decides whether `null` joins the property's type and whether the
property is in `required`. It says nothing about what is underneath, which is
why the probe needs no way to hand back an inner type - it answers predicates
only, so it needs no associated types and depends on no unstable feature.

**Built, in `shape.rs`, and it reaches the file.** `Role` and `optional` come to
`FieldDescriptor` from the probe, the branch the macro picked from the spelling
is asserted against what the type answers, and `SchemaSnapshot` carries the
whole thing down - `StoredShape` per field, recursively through a `Node`'s
children, which the snapshot did not record at all before. So the shape is in
the store rather than only in the binary that opened it, which is the half this
track exists for. See *The macro picks the branch, the compiler judges the pick*
for what is left on the macro end.

A snapshot written before this holds no `shape`, and it reads back as `None`
rather than as a default. Absent has to mean unknown: a comparison that read the
default as a claim would report every store written before today as having
changed shape, when all that changed is what gets written down.

**What a field records is its name, its shape, and its spelling.** No
`type_hash`: the only thing that read it was the per-field type comparison, and
the alternative - comparing `type_name` - is the mistake the probe exists to
avoid, since a spelling moves when a rename or an alias does and the type has
not. So `type_name` stays as what a person or the inspector reads, and nothing
compares it.

**Which leaves the comparison saying less, on purpose.** `SchemaDiff` is
`added` and `removed`, by name. A field whose type changed under one name nags -
the whole-struct hashes still disagree - and the diff has nothing to say about
it, which `a_type_that_changed_under_one_name_nags_without_a_diff` pins. What
replaces it is a comparison of two schema documents, and that is this track.

### A leaf is opaque at compile time, and serde can open it at run time

What the macro writes down about a leaf is the name of its type, because that is
all a macro can know without requiring a derive on user types. So these three
write the same record - `"Mode"`, role `Field`, not optional:

| before | after |
| --- | --- |
| `enum Mode { A, B }` | `enum Mode { A, B, C }` |
| `enum Mode { A, B }` | `enum Mode { X, Y }` |
| `struct P { x: u8 }` | `struct P { x: u8, y: u8 }` |

**But serde already knows, and it will say.** A derived `Deserialize` tells the
`Deserializer` what it expects before any data is read, and the arguments carry
the answer:

```rust
fn deserialize_struct(self, name, fields: &'static [&'static str], v)
fn deserialize_enum(self, name, variants: &'static [&'static str], v)
```

A `Deserializer` written to record which method was called and stop reports
`Struct { name: "Point", fields: ["x", "y"] }` and
`Enum { name: "Mode", variants: ["Idle", "Busy", "Off"] }` - the whole variant
list, from a type that implements nothing of this crate's. No instance is
needed: this is `T::deserialize`, not `Serialize`, which would need a value and
would then show one variant out of three. Measured, not assumed.

The only bound is `Deserialize`, which every leaf already satisfies - a leaf
that could not be deserialized could not be stored.

Driving that to the end is what
[`serde-reflection`](https://crates.io/crates/serde-reflection) does, and what
it yields is the whole recursive shape:

```
Leaf:  Struct([mode: TypeName("Mode"), where_: TypeName("Point"),
               tags: Seq(Str), note: Option(Str)])
Mode:  Enum({0: Idle → Unit, 1: Busy → Struct([since: U64]),
             2: Off → NewType(Bool)})
Point: Struct([x: U8, y: U8])
```

Including the variant *indices*, which is what a non-self-describing codec
writes - msgpack under redb - so an old registry is enough to reread old bytes
rather than only to notice they changed.

**Where tracing gives up**, from the crate's own error type and measured:

| cause | error | recourse |
| --- | --- | --- |
| `flatten`, `tag`, `tag` + `content`, `untagged` | `NotSupported` | none - the type is not ours |
| a hand-written `Deserialize` that validates | `Custom` | trace a value instead |
| an enum met only inside a container | `MissingVariants` | trace that enum by name, which we cannot reach |
| an empty `Option`, `Vec` or map in a traced value | `UnknownFormatInContainer` | a fuller value |
| `Serialize` and `Deserialize` disagreeing | `Incompatible` | none |

The first row is the uncomfortable one: those attributes exist for
self-describing formats, which is what json, toml and ron are, so a config
struct using `#[serde(flatten)]` is not exotic and will not be described.

`trace_type` re-drives passes only for `T` itself, so an enum reached through a
struct stays incomplete - and `registry_unchecked` must not be used to paper
over it, because an enum recorded with one variant of three reads later as two
variants removed. A partially seen enum is not written down.

**So the rule is quiet:** `trace_type`, failing that `trace_value` from the
declared default, failing that no entry in the document at all - and a leaf with
no entry is described by the name of its type, which is exactly where this
already stands. Tracing cannot make the record worse, only longer, and its
failure costs nothing. Opening a store must not fail because someone's type
would not describe itself.

Two things to hold on to when this is built. The registry is *forward*-looking:
comparing needs the older document to have been written at the time, same as the
shape record. And `serde_reflection`'s own `ContainerFormat` must not be
persisted as-is - that would adopt another crate's data model as this store's
file format; it maps into the document described above.

What it settles, and why it is a track rather than a task:

- **The two weak hashes** stop being the gate, so neither has to be made
  strong. The entry above stays as the record of what they could not see.
- **Metadata carrying no format version** is the same question asked smaller: a
  schema in the store is where a format version would live.
- **`build` running no generated migrations** turns from a silent skip into a
  comparison the store can make: the schema says what shape is expected, and a
  store that does not match it can say so without depending on who collected
  which steps.
- **Reading the schema from the store instead of `inventory`** - which the fork
  below already names - is this, done.

Nothing here should be patched around while this is undecided, which is why
the hash work above was stopped rather than finished.

## The fork under all of it: is the file a store, or a picture of a type

Two models, and the library is currently both.

**A picture of a type.** `to_writer(file, &config)`. The file is nested because
the struct is, a person opening it sees their own shape, and there is nothing to
address: the root is read and written whole. This is what `confy` does and what
the compatibility layer exists to read.

**A store that happens to be text.** Keys are paths, each with its own value,
its own event, its own deletion. VS Code's `settings.json` is this - flat, keys
like `"editor.fontSize"`, no object under `editor`.

We address by path, which promises the second, and write nested, which delivers
the first. Every ambiguity recorded in this file lives on that seam: a
serialized struct and a branch are the same bytes, a leaf and a branch cannot
share a name, `delete` and `delete_prefix` are one call on a document, a cleared
map leaves a node behind, and a scan has to guess how deep a value is.

**Writing the joined path as the key would end it.** The key becomes exactly
`StorePath::as_str()` - the same string the flat engines already use, escaping
included, injectivity already proved. Then a document holds keys, not nodes:

```json
{ "cfg.panels.left": { "width": 800 } }
```

`cfg.panels.left` is a key and `{"width": 800}` is its value; that the value is
an object stops being anyone's business. The picture-of-a-type model does not
die, it moves inside the value, which is where VS Code keeps it too
(`"editor.rulers": [80, 120]`).

What it would close: `Occupied` and both its variants, the empty node after
`clear`, `a_leaf_and_a_branch_coexist_at_one_name`, the depth of a scan, the
difference between `delete` and `delete_prefix` on a document, and `Navigable`
with the four `generic_*` walkers - `TextDocument` becomes a flat map.

What it would cost: every file already written nested needs migrating on open;
`confy`'s files are nested and stay a foreign format to import rather than our
own; toml needs quoted keys (`"editor.fontSize" = 14`), which is legal and
against its idiom; and a path into the interior of a value - `["cfg", "panels",
"left", "width"]` - stops resolving, though `#[amestate(nested)]` already writes
each leaf as its own key and so is unaffected.

### Decided: structure where a schema declares it, flat keys everywhere else

Neither extreme. A document is nested exactly as far as a schema says it is, and
whatever no schema declared is one flat key at the deepest declared point:

```json
{ "app": { "width": 1280, "panel": { "visible": true }, "myplugin.enabled": true } }
```

`app` and `panel` are nested because they are declared nodes. `myplugin.enabled`
is one key because nothing declared it. The ambiguity becomes a decision rather
than a guess: **a node is a level exactly when the schema says so, and anything
else is a value.** Undeclared data cannot be ambiguous at all, since it is never
nested.

That also settles the case that silently corrupted a map: a declared `map`'s
entries are the level below it, so `panels.left` is an entry and `{"width":
800}` is its value, whole. The walk stops there because a role said where the
boundary is, not because of a depth cut.

**What the record has to hold, and it is small.** At each point the reader
answers one question - descend, or take this whole - so the record is a tree of
name and role, and nothing else:

| role | the reader does |
| --- | --- |
| `node` | descends by name |
| `map` | descends one level; each entry is a value |
| `field` | stops; the node is the value |
| no record | nothing was nested here, so the key is a joined remainder - split it |

`FieldDescriptor` already carries exactly this. `type_hash` and `type_name` are
not consulted in any of the four cases, so the layout needs two of the four
layers - path and role - and nothing about types. `AmeType` leaves the critical
path and stays a migration concern.

**The prerequisite, now load-bearing.** Reading the file needs the schema, not
just writing it: without one, `{"panels": {"left": {"width": 800}}}` could be an
entry holding a struct or three levels, and no amount of string handling tells
them apart - the escaped flat remainder reads correctly without a schema, a
struct-valued node does not. So the snapshot has to be read from the store, per
prefix, and "the set it reads is the linked one" above stops being deferrable:
two binaries linking different structs would otherwise read one file into
different data rather than merely refusing different writes.

At read time the file's own snapshot wins - it describes how the file was laid
out. At write time the code's declaration wins. The difference between them is
drift, which is the migration half.

**Two edge rules.** The metadata file is always flat, because reading the data
file needs the schema and the schema lives in the metadata - a file that can
only be read once you have read it is not a design. And a prefix with no
snapshot is read flat: nothing was declared, so nothing was nested.

**Decided: a migration has no layout at all.** Two schemas are live while one
runs - the file was laid out by the old one and the code declares the new - and
they disagree about the same string. If `app.panel` was a `field` holding a
serialized struct and becomes a `node` with `width` under it, the bytes are the
same and only the schema says which reading is right.

Reading by the file's schema and writing by the code's does not survive the
ordinary case: step 2 reads what step 1 wrote, so the write went down in the new
layout and the read looks for it in the old. That is the mainline, not an edge.

So the prefix is flattened for the duration - joined key to value, no levels -
the steps run against that, and the document is laid out once at the end from
the new schema. There is nothing to invent: `MigrationContext` already addresses
by joined key, `ctx.get(key)` and `ctx.set(key)` with `key: &str`, so the flat
view *is* what a step already works with. The whole class of "which schema
applies right now" then does not arise, rather than being answered by a rule.

The alternative was one live schema mutated path by path as the migration
rewrote each. More precise - it leaves paths the migration never touched alone -
but that precision is only worth something while a layout changes in part, and
after this it never does: the document is written out whole.

The cost is the prefix in memory and rewritten entire, which is not a cost on
the engines this applies to. A text engine holds its whole document anyway, and
nobody puts a hundred thousand records in one.

Migration of files already written nested is not a concern - there are no users.

**Done so far:** the metadata file is flat. Its keys were `["meta", prefix]` -
two levels whose second name held the dots itself - and are now one joined key,
`meta.app.panel`. That also ends a quieter oddity: an `as_root` struct's
namespace is the empty string, so its marker was written as a child with no
name, which is exactly what a scan reports as a name no path can hold.

## The text engines take a path apart and put it back on every call

`TextDocument` addresses a node by `&[&str]`, so every `get`, `set` and `delete`
allocates a `Vec<&str>` out of a `StorePath` that already holds the levels, and
the scan walkers allocate one more per child. `generic_scan` then builds a
`StorePath` back out of that slice to compose the child keys.

The second parser is gone: `split_path` cut the joined form by
`str::split('.')`, knew nothing about the escape, and sent `delete_prefix` at a
level that was not there - so the delete removed nothing and returned `Ok(())`.
`delete_prefix` now hands the whole subtree to `delete_subtree`, and the
document walkers compose child keys through `StorePath::try_push`. The tamper
suite that reproduced it is ordinary tests under `tests/`.

The same family, found since and fixed: the scan walkers asked the joined
prefix `!prefix_str.ends_with('.')` before listing the value at the prefix
itself. A trailing dot in the joined form is an escaped one - `cfg.b\.` is a
level called `b.` - so the value at any such path was missing from its own
scan, on the text engines only. Pinned in
`tests/delete_prefix_dotted_keys.rs`.

What is left is the cost, not a defect: `TextDocument` addresses a node by
`&[&str]`, so the levels are taken out of a `StorePath` that already holds them
and put back again per call. The trait should take `&StorePath` and walk it by
`segment_at`, and `scan` should hand back the child's name rather than a joined
string. Nothing outside these three files sees the trait, so it costs the three
document impls and the two walkers.

## One conformance suite for the backends, run against each

Every engine has its own unit tests, written when it was written, and they
overlap by accident rather than by design. Almost every defect found this week
was a difference between engines that no single suite was watching: a prefix
scan that stopped at a level on one and at a character on another, a key with a
separator that survived on the flat engines and split on the tree ones, a
migration cleanup that removed a subtree on one family and nothing on the other.

What is wanted is one set of tests, parameterised by engine, that says what a
store is regardless of which one is underneath - and a per-engine file left with
only what is genuinely particular to it.

`tests/durability_crash.rs` is what one of these looks like: one statement, run
against every engine compiled in. Widening it from redb alone immediately turned
up a difference nothing was watching - see the granularity entry above.

A good part of it belongs as properties rather than examples, because the
statements are universally quantified and the interesting inputs are the ones
nobody thinks to write: a value written at a path reads back at that path and
nowhere else; a scan under a prefix returns exactly the keys written under it;
`delete_prefix` removes exactly the subtree and nothing beside it; a name
holding the separator stays one level through a write, a reopen and a scan.

**After the error model, not before.** Half of what such a suite should pin is
what happens when an operation fails - which error, for which cause - and today
those are not distinguishable enough to assert. Written now it would test the
successes and stay silent about the failures, which is the half that differs.

### The suite draws different inputs every run

`config()` sets `cases: 24`, `failure_persistence: None` and no seed, so which
paths and names a property sees is fresh each time. The recorded divergences
therefore move: two runs an hour apart gave json 2 failures and json 3, and a
property that failed on json passed on toml in one run and the reverse in the
other. A regression is indistinguishable from a different draw, which is the
one thing a suite kept as a gate has to be able to say.

Either pin the seed for the properties that record a divergence, or
`cfg_attr`-ignore those per engine so what is green is green every run. The
second says which engine fails which property in the source, where the reader
is, rather than in whichever run they happen to read.

**Done, and by neither of those.** `failure_persistence` is on: a
counterexample is recorded in `.proptest-regressions` beside the suite and
replayed before the new draws, so what fails once fails every run afterwards.
Two are recorded already, both shrunk to a name that is a lone backslash.
Three toml runs now name the same three properties where the count used to
wander.

Pinning the seed would have frozen the suite into twenty-four examples that
never find anything again - determinism bought by ending the search, which is
the opposite of what a property suite is for. `cfg_attr`-ignoring the three was
dropped for a different reason: they are not accepted divergences. Each waits
on a question open above - the scan's depth, the empty node after a delete,
toml's `with_bytes_de` - and marking them ignored would have decided those by
hiding them.

### What the suite does not reach yet

Ordered by what it costs. **Events**: `StoreOp` appears nowhere in the tests,
and `StoreEvent`'s `old` and `new` bytes are asserted nowhere - one operation
emitting a different op or different bytes per engine is unwatched, and
`text/store.rs` emits a `Delete` for a removal that did not happen.
**Concurrency between two handles**: two handles on one store exist in the
tests but are only ever driven in sequence. **The async surface**: two files,
both through `block_on`. **`is_initialized`**: the happy path only, never
across a failed flush. **Value shapes**: the conformance suite writes `u32` and
one `String`; enums, sequences and nested structs - where the formats differ
most - are never round-tripped.

### Two file-watch tests are load-sensitive

`json_store::store_tests::file_watch_emits_set_for_external_change` and
`..._delete_for_external_removal` fail when the machine is running several test
binaries at once and pass on their own. They wait a fixed interval for the
watcher, so what they measure is the machine as much as the store.

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

**The migration context needs a written-up page of its own.** Its methods carry
doc comments and `StoreBuilder::provide` has a runnable example, but there is
nowhere that explains the shape of a migration as a whole - and it is the part
of the library a person meets exactly once, under pressure, with data they
cannot afford to lose. What it should cover:

- what a step is: a bare `fn` collected at link time, capturing nothing, which
  is why anything from the application arrives through `provide`/`require`
  rather than a closure;
- the difference between `build` and `build_with_report` - only the second
  collects the steps `#[migrate]` generated, which is its own entry above and
  is the first thing that bites;
- reading old data (`AmeData`), the scoped forms (`nested`, `scoped`), and
  which of `get`/`global_get` addresses what;
- that `scan_map` reads a map the step will write back whole, so an entry it
  cannot read is an error rather than a skip;
- what a failing step leaves behind, once migration atomicity above is
  settled - this one has to wait for that answer rather than describe the
  current behaviour, which is on this list.

When the list is empty, turn on `#![deny(missing_docs)]` for the documented
modules so the next undocumented public item cannot land quietly.

## The builder named a file for one engine and opened it with another

Reported from an application built on this, not found here, which is the part
worth keeping: it is reachable by the shortest path the API offers.

```rust
StoreBuilder::for_app(app, config)   // settings.redb, from the default engine
    .backend(Backend::Json)          // changes the engine, not the file
```

`for_app` ends in `new`, which fills in an extension when the path has none,
and it takes it from `default_backend()`. `backend` set only its own field. So
the json engine opened a redb file and failed on its first byte with `stream
did not contain valid UTF-8` - a message about encoding, for a mistake about
which file to open, which is why it cost the reporter a debugging session
rather than a glance. `StoreBuilder::new("app/settings").backend(Json)` does
the same thing without `for_app` anywhere near it.

**Done by remembering who chose the extension.** The builder keeps
`caller_named_extension`, and `backend` re-derives the extension when the
answer is no. An extension the caller spelled is theirs - a `.conf` some other
tool already watches is not renamed because an engine was named - and one this
crate invented belongs to whichever engine actually runs. Four tests in
`store::builder::tests`, including the two-`backend`-calls case.

The application worked around it by rebuilding the path with `etcetera` and the
right extension, ten lines duplicating this crate's logic. Those can go.
