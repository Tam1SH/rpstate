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

## Sync and async durable writes commit different amounts

`Durable::set` calls `flush_prefix`; `Durable::set_async` calls `flush_async`,
which every backend implements as a full `save_now` of the whole buffer. The
same pair on `ReactiveCell`, `ReactiveMap` and `Kv` behaves the same way.

Distinct from the `flush_prefix` entry at the top, which is about how much rides
along with a prefix flush. This one is that the two forms of the same operation
have different granularity, and the documentation of the sync form is spent
explaining a granularity the async form does not have.

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
- The dioxus and leptos pages name the provider component `amethystateProvider`;
  it is `AmeStateProvider`. The dioxus page uses both.

Rustdoc has its own: the macro's own documentation gives constructors that do
not exist (`new(&Arc<Store>)` where the real one takes no arguments and the
store is already `Arc`-backed), and says `default` is required on leaf fields
where the code falls back to `Default::default()`. `Kv::set` and `Kv::remove`
open by promising the durability their `Durable` counterparts provide.

`Concepts/observability.md` promises `location` is the caller's `file:line`.
`#[track_caller]` is on the direct `subscribe` methods but on none of the
`Watch` builder's, so every subscription made the way the subscriptions chapter
teaches logs a line inside this library.

## The text engines take a path apart and put it back on every call

`TextDocument` addresses a node by `&[&str]`, so every `get`, `set` and `delete`
allocates a `Vec<&str>` out of a `StorePath` that already holds the levels, and
the scan walkers allocate one more per child. `generic_scan` then builds a
`StorePath` back out of that slice to compose the child keys.

`split_path` parses the joined form a third way - by `str::split('.')`, which
knows nothing about the escape - and `delete_prefix` still goes through it.
`tests/delete_prefix_dotted_keys.rs` says a dotted name does get deleted with
its subtree today, so this is a disagreement between two parsers rather than a
reproduced failure; it stays a hazard while both exist.

`normalise_parts` maps `["."]` to the root, left from when `"."` was the
sentinel for the whole document. A level genuinely named `.` is a path now, and
that mapping silently sends it to the root instead.

The trait should take `&StorePath` and walk it by `segment_at`, `scan` should
hand back the child's name rather than a joined string, and `split_path` should
go. Nothing outside these three files sees the trait, so it costs the three
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

Durability is the clearest case of a statement written against one engine.
`tests/durability_crash.rs::a_durable_write_survives_a_crash_and_a_plain_one_does_not`
names a property every store has to have, and runs it against redb alone - so
whether a text engine or sqlite keeps a durable write across a crash is
currently unasserted, and the answer differs by engine.

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
