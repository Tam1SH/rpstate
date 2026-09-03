# RFC: reaching a declared map without holding it

A `ReactiveMap` declared on a struct is resident. `reactive_map_with_path_only`
scans the prefix once at construction and decodes every entry into
`ReactiveMapCore::cache`, and every read afterwards answers from there. That is
the whole point of it - reads are infallible because they never leave memory,
which is what lets a drawing thread call `get` - and nothing here proposes
taking it away.

What it costs is paid at the open, in full, whether or not the program touches
the entries. The performance envelope this library is written against is around
a million records with a hundred thousand already the edge, and the
`parallel_reads` note measures the cost precisely: parsing every stored key and
decoding every value is about four hundred milliseconds of a million-entry
open, and about eighty with the work divided across cores. An application whose
map holds a hundred thousand thumbnails and reads eleven of them pays for a
hundred thousand.

This proposes the smallest way to not pay it, and spends most of its length
arguing about which of two things is being asked for, because they are easy to
confuse and only one of them is worth building.

## What the surrounding design already settled

`RFC-reactive-table.md` has a section called *Residency is a property of the
backing, not of the primitive*, and three of its conclusions bind here.

**One type cannot be both.** The resident side may not block and its reads are
infallible; the non-resident side may block and must be allowed to, so its
reads return `Result`. A type parameterised over a backing has to impose the
union on both, or hide the difference behind an associated `type Read<T>` that
is `T` in one case and `StorageResult<T>` in the other - which compiles and
makes every error message and every doctest a two-headed thing. So whatever is
proposed here is not a `ReactiveMap` with a flag.

**The non-resident keyed collection already exists, and it is `Kv`.**
`Kv::namespace(name)` gives `get`, `set`, `remove` and `keys` over exactly the
level a map occupies, reading through to the store every time and returning
`WriteResult` because it genuinely touches disk.

**The on-disk layout is shared.** `ReactiveMap` at `<path>.<name>`, a
`ReactiveTable` at `<path>.<id>` and `Kv::namespace(name)` all write one segment
per entry under one prefix, sorted by `cmp_names`, seeded through
`is_initialized`. Two views over the same level see the same bytes and no
migration stands between them.

Taken together, those say the answer is not a new collection type. It is a way
to point `Kv` at a level a struct declared - which is exactly what does not work
today.

## The gap: ownership blocks the route the design points at

`Kv::guard` refuses any path a declared struct owns:

```rust
Err(Report::new(WriteError::SchemaOwned {
    path: path.as_str().to_string(),
    declared: declared.as_str().to_string(),
}))
```

That refusal is right and should stay. It is what stops a `String` being
written where a `u16` is declared, where the damage is not merely a wrong value:
the field's subscription fails to decode and keeps its old value, and the next
start fails outright when it reads the path back.

But it means the design's own answer is unreachable for the case that needs it
most. `store.kv().namespace("editor").namespace("thumbnails")` is refused
precisely because `thumbnails` is declared - and a map large enough to want
non-resident access is, in an application, almost always a declared one.

So the missing piece is small and specific: **the owner of a declared path is
the one party entitled to hand out unguarded access to it.** The generated
struct is that owner. A method on it can return the `Kv` that `guard` refuses to
anyone else, because the caller has already proved ownership by holding the
struct.

## Two things are being asked for, and only one pays

**A bypass beside a resident map.** The field stays `ReactiveMap`, and a
generated method reads one entry through the store rather than from the cache.
This does not pay for itself. The cache was filled at the open and is being kept
current by a subscription for as long as the struct lives; reading around it
costs a disk round-trip to learn something already in memory. The only honest
use is reading a key the resident view is not allowed to hold, and no such key
exists - the map holds every entry under its prefix.

**A field that is not resident at all.** Nothing is loaded at the open and the
method is the only access. This is the one that pays, and it cannot be spelled
as an attribute on a `ReactiveMap` field, because `ReactiveMap` *is* the cache -
a non-resident `ReactiveMap` is a `ReactiveMap` with its defining organ removed,
and every method on it would have to change its return type.

**So residency is stated by the field's declared type, not by an attribute.**
The same way `ReactiveMap<K, V>` is recognised today - spelled by name at the
field, refused through an alias - a second spelling means the other thing:

```rust
#[amethystate(prefix = "editor")]
pub struct Editor {
    #[amestate(default = {})]
    pub open_files: ReactiveMap<String, Tab>,   // resident, as now

    pub thumbnails: LazyMap<String, Thumbnail>, // nothing loaded at the open
}
```

An attribute would be the wrong shape twice over: it would put a fact about the
type somewhere other than the type, and it would leave the field's methods
promising infallible reads they can no longer make.

## What is lazy, and what is not

Worth being exact, because "lazy" hides a difference that decides how much this
buys.

**The walk is `scan_keys` and then a read per key, and not `scan_prefix`.** That
is a design decision and not an implementation detail, because `scan_prefix`
returns `Vec<(StorePath, Raw)>` - the bytes of every entry, materialised, in one
call. A walk built on it would have read everything before yielding its first
item, and the only thing left lazy would be the decode.

`scan_keys` returns the keys alone, sorted, without a value. Reading one
afterwards is `get_raw`, which on redb opens a read transaction and asks the
table for that one key. So a walk stopped after thirty entries has read thirty
entries, and that is where the saving is.

### It is not the same saving on every engine

Worth stating plainly, because the answer differs by more than a constant.

**redb and sqlite** hold entries on disk and read them one key at a time.
Nothing about the ninety thousand not asked for is touched - not read, not
parsed, not decoded, not held.

**json, toml and ron** parse the whole file into a document at the open and keep
it: `StoreFile { doc: Arc<RwLock<D>> }`. That is inherent to the formats rather
than a choice - there is no reading one key of a json file without parsing the
json. So the parse is paid whatever this type does, and what a lazy walk saves
there is the second half: turning parsed nodes into `Thumbnail`s, and holding
ninety thousand of them afterwards.

So on the text engines `LazyMap` saves the typed decode and the retained typed
values, and cannot save the parse. That is a smaller win, and for a map large
enough to want this type it is also a sign the store is on the wrong engine: a
format that must be read whole is not the one to put ninety thousand
thumbnails in.

Which makes the honest framing not "this type is weak on text" but **this type
is where the engine choice starts to matter**. A settings file of forty keys on
json is right; a large keyed collection wants redb or sqlite, and `LazyMap` is
what lets the difference show.

## Shape

**The names are `ReactiveMap`'s.** Not approximately - the same ones, in the
same order of arguments, meaning the same thing. What differs is the return
type, and that difference is the whole content of the type: one answers from
memory and cannot fail, the other goes to the store and can.

Keeping them identical is what makes changing a field's mind cheap. The edit is
not a rename and not a restructuring; it is the compiler walking you through
call sites adding or removing a `?`, and nothing else moves.

```rust
pub struct LazyMap<K, V> { /* store handle, prefix, types */ }

impl<K: ReactiveMapKey, V: ReactiveMapValue> LazyMap<K, V> {
    pub fn get<Q>(&self, key: &Q) -> StorageResult<Option<V>>;
    pub fn contains_key<Q>(&self, key: &Q) -> StorageResult<bool>;

    /// The keys, in stored order, without decoding a value.
    pub fn keys(&self) -> StorageResult<impl Iterator<Item = K>>;

    /// Every entry, in stored order, decoding each as it is reached.
    pub fn entries(&self) -> StorageResult<impl Iterator<Item = StorageResult<(K, V)>>>;

    pub fn len(&self) -> StorageResult<usize>;
    pub fn is_empty(&self) -> StorageResult<bool>;

    pub fn insert(&self, key: K, value: &V) -> StorageResult<()>;
    pub fn update<Q>(&self, key: &Q, value: &V) -> StorageResult<()>;
    pub fn remove<Q>(&self, key: &Q) -> StorageResult<Option<V>>;
    pub fn clear(&self) -> StorageResult<()>;

    pub fn subscribe_any<F>(&self, callback: F) -> SignalSubscription;
    pub fn subscribe_key<F>(&self, key: K, callback: F) -> SignalSubscription;

    pub fn path(&self) -> &StorePath;
    pub fn fork(&self) -> Self;
    pub fn durable(&self) -> Durable<'_, Self>;
}
```

Four of them mean something slightly different on this side, and each is worth
saying out loud rather than discovering:

**`len` counts what the store holds.** `ReactiveMap::len` is documented as how
many entries the map holds - answered from the projection, counting buffered
writes that have not landed. It is a statement about the view. Here there is no
view, so it is a statement about the store, and a write not yet flushed is not
in it. Both are honest; they are answers to different questions.

**`update_with` and `modify` are not offered.** On the resident side they are a
read, a change and a write against a projection nobody else is touching between
the three. Here the read is a round-trip and so is the write, and nothing holds
the entry still in between. Offering them under the same names would be offering
the same guarantee, which is the one thing the type cannot give. Read, change
and write, and know that you did.

**`intercept` is not offered either.** An interceptor that inspects the
collection - refusing a duplicate, capping a size - is cheap against a
projection and a full scan against a store. The mechanism would work and the
cost would be a lie.

**`entries` yields `StorageResult` per entry, not one for the walk.** One
undecodable entry among ninety thousand should not end the walk, and the caller
is the one who knows whether to skip it or stop. `ReactiveMap` decided this at
construction instead, which is a thing it can do because it decodes everything
up front.

Every read returns `StorageResult`, and that is the point rather than a wart: it
is how the type says it touches the disk, which is the thing the resident type
promises never to do.

**`keys` is ordered, and that is worth stating rather than leaving implied.**
`scan_keys` returns the keys under a prefix already sorted, without decoding a
value, and the order is `cmp_names` - the escaped form compared as bytes, and
nothing cleverer. So the order is a property of the key, fixed when an entry is
named.

That makes an ordered window one sorted scan and a slice:

```rust
let keys = editor.thumbnails().keys()?;      // nothing decoded

for key in &keys[7..34] {
    draw(editor.thumbnails().get(key)?);      // twenty-seven decoded
}
```

and the position of a key that changed a `binary_search` in the same list. An
application wanting an order other than the one its keys already have gets it
by naming entries so their order is the one it wants - a fixed-width counter, a
timestamp, a UUIDv7, which is lexicographically ordered by time.

What no keyed collection answers is re-sorting the same entries by their
*content* at runtime, because sorting by a value means reading the values. The
persistent answer to that is a second prefix whose keys order the same entries,
which is another map and not another primitive.

**Not here:** an ordering other than the stored one, and a `MapChange` carrying
a decoded `old_value` for every change whether or not anyone wanted it. The
first is answered by naming entries so their order is the one you want; the
second by handing the change over undecoded and letting the callback decode the
half it needs.

## What it is like to use

### The case it exists for

An editor keeps a thumbnail per file it has ever opened. Ninety thousand of
them; the sidebar shows thirty.

Today the declaration is a resident map, and the open pays for ninety thousand
decodes before the window is drawn:

```rust
#[amethystate(prefix = "editor")]
pub struct Editor {
    #[amestate(default = {})]
    pub thumbnails: ReactiveMap<PathBuf, Thumbnail>,
}
```

The change is one line, at the field:

```rust
    pub thumbnails: LazyMap<PathBuf, Thumbnail>,
```

and the open stops touching them. Drawing the sidebar becomes a read per row:

```rust
for file in visible_rows {
    let Some(thumb) = editor.thumbnails().get(file)? else {
        continue;
    };
    draw(thumb);
}
```

Thirty decodes instead of ninety thousand, and the ninety thousand that were
never asked for are never paid for.

### What the `?` is telling you

Every read on the resident map is infallible - `map.get(&k)` hands back a value
or `None` and cannot fail, because it never leaves memory. Every read here
returns `StorageResult`, and that is the type saying it touches the disk.

Which is the one thing to think about before switching a field. On a drawing
thread, thirty reads through the store is thirty reads through the store: on
redb and sqlite that is a transaction each, on the text engines it is a read
lock over the in-memory document. The library's own rule is that the drawing
thread should not touch the disk, and this type is the exception you take
deliberately, for a collection where the alternative is worse.

The honest shape of the decision: **resident is right until the open costs more
than the reads do.** A map of a hundred entries read once a frame should stay
resident. A map of ninety thousand read thirty at a time should not.

### What you give up, and what you do instead

A resident map notifies per entry, with the old value and the new one, decoded:

```rust
let scope = editor.open_files().subscribe_any(|change: &MapChange<String, Tab>| {
    match change {
        MapChange::Update { key, old_value, new_value, .. } => repaint(key),
        MapChange::Remove { key, old_value, .. } => forget(key),
        _ => {}
    }
});
```

`old_value` comes out of the projection, and there is no projection here. But
the store's own subscription takes `SubscriptionKind::Prefix`, and a
`StoreEvent` already carries `path`, `op`, and both `old` and `new` **as bytes**.
So a `LazyMap` can offer the same notification with the same laziness - you are
told which key changed without anything being decoded, and you decode the halves
you actually want:

```rust
let scope = editor.thumbnails().subscribe(|at: &PathBuf, change| {
    // nothing has been decoded yet
    if !visible(at) {
        return Ok(());
    }
    repaint(at, change.new()?);   // decoded here, and only here
});
```

That is a better fit for the case anyway: a sidebar showing thirty rows does not
want ninety thousand entries decoded so it can discover that the one that
changed is off-screen.

What genuinely goes away is the collection-level answers residency buys - a
`len` that counts buffered writes, an ordering other than stored order, one
notification for the collection rather than per path. Those are the subject of
`RFC-reactive-table.md`, and wanting them is wanting a different primitive.

### Both in one struct

Nothing stops a struct having one of each, and a settings struct with a large
side-table is the ordinary shape:

```rust
#[amethystate(prefix = "editor")]
pub struct Editor {
    #[amestate(default = 14u32)]
    pub font_size: u32,

    #[amestate(default = {})]
    pub open_files: ReactiveMap<String, Tab>,   // a dozen, resident

    pub thumbnails: LazyMap<PathBuf, Thumbnail>, // ninety thousand, not
}
```

`subscribe_all` walks the fields that have something to subscribe to, and skips
the one that does not - so a struct-wide "something changed, redraw" keeps
working and keeps not loading the thumbnails.

### Changing your mind later

Switching a field between the two is a **type change and not a data change**.
Both write one segment per entry under one prefix, sorted by `cmp_names`, seeded
through `is_initialized`; two views over the same level see the same bytes. So
the store written by the resident build is read by the lazy build and back
again, with no migration and no version bump.

What does change is the code around the field: every read grows a `?`, or loses
one. That is the compiler's work to point out, field by field, and there is no
silent half-state where some call sites were updated and others were not.

## What the generated struct does with it

The field's declared type decides four things, and all four already have a place
in the model rewritten this cycle. `Shape` gains a variant beside `Map`, and
every generator matches on it exhaustively:

- **the struct's field type** - `LazyMap<K, V>` rather than `ReactiveMap<K, V>`
- **the constructor** - claims the path and marks it initialised, and does not
  call `load_map`
- **the getter** - hands back a clone, as the others do
- **`subscribe_all`** - skips it, there being nothing to subscribe to

The two that need deciding are on disk.

**`_Data`.** A map is an `IndexMap<K, V>` in the snapshot today. A `LazyMap`
materialising there defeats the purpose - but only where the whole struct is
being materialised on purpose: `__ame_to_data`, a `mode = "persistent"` load, a
migration step. Those are explicit whole-data operations and are allowed to
load. The alternative - leaving the field out of `_Data` - would put it beyond
the reach of every migration, which is worse.

**The schema descriptor.** `Role::Map`, unchanged. The layout on disk is the
same one segment per entry under one prefix, so the schema should say map,
because that is what is written. A view is not a shape.

## What this does not settle

**Whether the resident default is right.** It is, for what the library is for -
a settings struct with eleven fields and two maps of a dozen entries pays
nothing measurable, and infallible reads are worth a great deal on a drawing
thread. This adds a second answer for the case where the default stops being
free; it does not move the default.

**Seeding.** `#[amestate(default = {..})]` on a resident map writes its entries
at the first open, through `is_initialized`. A `LazyMap` could do the same - the
mechanism is the same `is_initialized` flag and the same one-segment-per-entry
write - but a declared default on a collection nothing loads is a strange thing
to want, and it is left out until somebody wants it.

**Ownership of entries beneath.** `guard` refuses a declared path to `Kv`, and
the method proposed here is that refusal's one exemption. Whether the exemption
should be the whole `Kv` surface or the narrower `LazyMap` above is the last
open question: `Kv` is more powerful and less typed, `LazyMap` says `K` and `V`
and cannot be pointed at the wrong level. This proposes `LazyMap` on those
grounds.
