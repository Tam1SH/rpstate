# Changelog

What changed between releases, in the shape [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
describes, newest first. Versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

This file starts at 0.20.0, which is where it was written. Nothing before that
is recorded here; the commit history is.

## [0.20.0] - 2026-08-24

The number jumps from 0.13.0 because the surface does. A path stopped being a
dotted string, an error stopped being a nested enum, and a cell stopped owning
what it shows - and most code written against 0.13.0 will not compile until
those three are dealt with. The `Breaking` section below is written so a call
site can be fixed by reading it rather than by guessing which failure is
deliberate: every one of them is.

### Breaking

- **A path is a list of levels, not a string with dots in it.** `IntoStorePath`
  is deliberately not implemented for `&str`, because `store.get(["ui",
  "width"])` and `store.get("ui.width")` would look alike and mean different
  things. A name is now a name: one holding the separator stays one level and is
  escaped when the key is written out.

  ```rust
  // before
  store.set("ui.width", &1280u32)?;
  let width: Option<u32> = store.get("ui.width")?;
  // after
  store.set(["ui", "width"], &1280u32)?;
  let width: Option<u32> = store.get(["ui", "width"])?;
  ```

  The same applies to `Store::delete`, `delete_prefix`, `scan_keys`,
  `scan_prefix`, `flush_prefix`, `store::field_with_path`, and the `new(store,
  namespace)` the macro generates for a namespaced struct. A `StorePath` built
  once can be passed by reference wherever a path is wanted.

- **A scan hands back `StorePath`, not `String`.** The caller no longer parses
  what the store just wrote.

  ```rust
  // before
  for key in store.scan_keys("ui")? {
      let path = StorePath::parse_joined(&key)?;
  }
  // after
  for path in store.scan_keys(["ui"])? {}
  ```

  This covers `Store::scan_keys` and `scan_prefix`, `StoreBackend::scan_keys`
  and `scan_prefix`, `Kv::keys`, `MigrationBackendAdapter::scan_prefix` and
  `InspectorBackend::scan_all`. `path.as_str()` is the joined form where a
  string is what a caller wants.

- **Failures are `error-stack` reports.** `StorageResult<T>` is now
  `Result<T, Report<StorageError>>` and `WriteResult<T>` is `Result<T,
  Report<WriteError>>`. A report names the operation and carries the engine's own
  error as the frame below it, with the path, the table and the file attached by
  whoever knew them, so a caller no longer gets `no such table: data` with no
  store and no path.

  `StorageError`'s engine variants (`TextStore`, `RedbStore`, `Sqlite`, `Codec`,
  `Migration`) are gone, replaced by what the store was doing: `Open`, `Read`,
  `Write`, `Delete`, `Scan`, `Flush`, `Codec`, `Meta`, `Migrate`, `Path`,
  `CommitFailed`. `WriteError::StorageError(e)` becomes `WriteError::Storage`,
  and `Path` and `SourceGone` join it.

  ```rust
  // before
  match err { StorageError::RedbStore(e) => .. }
  // after
  match err.current_context() { StorageError::Write => .. }
  ```

  A test asserting on a variant sees less than it used to: `store::one_line`
  renders a report's contexts on one line, which is what the suite here asserts
  on now.

- **`WriteError` loses its generic.** `WriteError<E>`, `FieldError<E>`,
  `ReactiveMapError<E>`, `WriteResult<T, E>` and `ReactiveMapResult<T, E>` each
  drop the storage-error parameter, since the payload it carried is the frame
  below the context now. `ReactiveMap::get` returns `ReactiveMapResult<Option<V>>`
  rather than `ReactiveMapResult<Option<V>, B::Error>`.

- **`WriteError::TypeMismatch` is gone**, along with the check behind it. It
  compared `std::any::type_name` against what the field registry had stored,
  which disagree for anything but a primitive and could not survive a restart
  anyway. What refuses a `Kv` write is ownership of the path; what a path holds
  is the writer's business.

- **`ReactiveMap::set` and `set_or_create` are renamed, and the surprising name
  is on the surprising operation.** `set` only ever updated an existing key,
  which is not what `HashMap::insert` or `d[k] = v` means, and reaching for it to
  add a key returned `KeyNotFound`. The old names are removed rather than
  redefined, so a call site fails to compile instead of silently changing from a
  strict write to an upsert.

  ```rust
  // before
  map.set_or_create(key, &value)?;   // add or replace
  map.set(key, &value)?;             // replace, error if absent
  map.update(key, |v| *v += 1)?;     // closure
  // after
  map.insert(key, &value)?;
  map.update(key, &value)?;
  map.update_with(key, |v| *v += 1)?;
  ```

  The leptos, yew and dioxus map handles follow: `set_or_create` becomes
  `insert`.

- **A cell is a view on something else, and no longer owns it.** `Field::cell`
  and `ReactiveMap::entry_cell` hold their source weakly. Three consequences: a
  cell left in a UI no longer keeps the database file open and the debouncer
  thread alive; `ReactiveCell::get` returns `Option<T>` and subscribers see
  `&Option<T>`, so a key going away is an event rather than silence; and a write
  to a removed entry fails with `WriteError::SourceGone` instead of recreating
  it, since creating an entry is `insert`'s job.

  ```rust
  // before
  let cell = map.entry_cell(key, 0u32);
  let value: u32 = cell.get();
  // after - a view, and the default is gone with the ownership
  let cell = map.entry_cell(key);
  let value: Option<u32> = cell.get();
  // owning, for: build a struct, hand one value to a widget, drop the struct
  let cell = map.into_entry_cell(key);
  ```

  `Field::into_cell` is the owning form for a field. `Kv::cell` is owning
  already, because nothing else holds the field it builds. The error names the
  remedy, since the failure is otherwise a mystery.

- **`Kv` addresses one name and asks for nesting rather than punctuating it.** A
  dot in a `Kv` path used to mean "next level", which silently split a name that
  was meant whole - a file path, a key with a version in it.

  ```rust
  // before
  kv.set("ui.width", &1280u32)?;      // two levels
  kv.keys("ui")?;
  // after
  kv.namespace("ui").set("width", &1280u32)?;
  kv.set("ui.width", &1280u32)?;      // one name, which happens to hold a dot
  kv.namespace("ui").keys()?;
  ```

  `keys` lost its argument, since a handle already knows where it is rooted.
  `Kv::get`, `set` and `remove` return `WriteResult` rather than
  `StorageResult`. `try_namespace` is the fallible builder beside the panicking
  one, matching `StorePath`.

- **`Kv` ownership is by the path a schema declared, not by the prefix it sits
  under.** A struct with `prefix = "app"` used to take the whole subtree, so
  `app.myplugin.enabled` was refused though nobody had declared it. Two
  directions are refused now and both say which: a path inside a declared one,
  and a declared path under the one being written. A write beside a declared
  field goes through.

- **`StoreBuilder` takes `Duration` where it took milliseconds.**

  ```rust
  // before
  StoreBuilder::new(path).debounce(300).watch_interval(500)
  // after
  StoreBuilder::new(path)
      .debounce(Duration::from_millis(300))
      .watch_interval(Duration::from_millis(500))
  ```

- **A document engine refuses a write it cannot represent instead of overwriting
  what is there.** A tree holds a value at a node or values under it, never both,
  and the walk used to resolve that by replacing whatever stood in its way -
  silently, in both write orders. Writing under a level that holds a plain value
  is refused, and so is writing a non-map value at a level that has children;
  both arrive as `StorageError::Write` over an `Occupied`. The flat engines have
  no such limit and never report it.

- **`kv.set(".", ..)` no longer replaces the whole document.** The `.` sentinel
  was produced by a path parser that no longer exists, and a level named `.` is
  now an ordinary level addressing one value.

- **`SignalSubscription` and `ReactiveScope` are not `Clone`.** The derive copied
  the id and `Drop` cancelled by id, so a clone was a second trigger rather than a
  co-owner: dropping it stopped the original firing while it was still held. A
  second copy of the right to end a subscription is a second way to end it, so
  there is none. Hold one where it belongs, or an `Arc` at the holder that wants
  sharing. This is what stops `amethystate-gpui` building today - `RpView`
  derives `Clone` over a `ReactiveScope` - and that adapter is left for the
  adapter pass.

- **`StoreBackend` changed shape for anyone implementing it.** Every path
  argument is `&StorePath`; `scan_keys` and `scan_prefix` return paths;
  `set_initialized(namespace, InitState)` is required and `mark_initialized` is
  a default method over it.

- **`StoreExt::decode` drops its `Default` bound and its substitution.** It
  returned `Ok(T::default())` on any decode error, so a map entry of the wrong
  type read back as `Some(0)` and `len()` counted it while a write to the same
  key errored, one line apart.

- **The path helpers are gone**: `join_path`, `split_path`, `normalize_path` and
  `scoped_path`, each of which was a separate implementation of the escaping
  rule. `StorePath` and `IntoStorePath` cover them. `Field::path` returns a
  `StorePath`; `Field::new_volatile`, `ReactiveMap::new` and `intercept` take
  one; the core map operations take `&StorePath`, and `map_set_existing` and
  `map_set_or_create` are `map_update` and `map_insert`.

- **`FieldDescriptor` carries what a declared path is.** It gains `role`
  (`Field`, `Map` or `Node`) and, for a node, its `children`, so the set of
  declared paths is known without opening the store. `FieldDescriptor::leaf(name,
  type_hash, type_name)` builds the ordinary case. `AmeStateNode` gains a
  required `CONSTRUCTION_TERMINATES` const: it is required rather than defaulted
  so a hand-written impl cannot opt out of the cycle check without saying so.

- **`observability::register_field` takes the type rather than its printed name.**

  ```rust
  // before
  register_field(path, instance_id, std::any::type_name::<T>());
  // after
  register_field::<T>(path, instance_id);
  ```

- **A nameless level in `#[amethystate(prefix = ...)]` or `#[amestate(key =
  ...)]` is a compile error.** `prefix = ""`, `"."`, `"a..b"` and `"a."` were
  accepted and the empty level dropped, so a mistyped prefix silently made the
  struct root-scoped. Write `as_root` where that is what was meant. The check
  runs at expansion and points at the attribute, and `StorePath::from_static`
  re-checks in a `const fn`, which catches a hand-written `impl StateScope` too.

- **The text engines' metadata file is flat.** Its keys were `["meta", prefix]`,
  two levels whose second name carried the dots itself; they are one key now,
  joined once: `meta.app.panel`. An `as_root` struct's initialization marker was
  `["__init", ""]` - a child with no name - and is `__init`. A metadata file
  written by 0.13.0 therefore reads as absent: the schema snapshot, the version,
  the migration log and the initialization markers are not found, and a prefix
  that declares migration steps runs them again from the earliest one. Only the
  text engines keep this file; redb and sqlite are unaffected.

### Added

- `StorePath`, the path built from segments and only from segments. It keeps
  both forms - the levels for engines that walk a document tree, the joined and
  escaped string for engines that store a key whole - so reading either borrows
  and allocates nothing. `from_static` is const, so a path the compiler knows is
  checked at expansion and never unwrapped at a call site. `IntoStorePath` is
  what an API asks for where a caller supplies levels; `StorePathError` says
  which level was refused and why. Ten properties pin the design, two of them
  being that distinct sets of levels never share a key and that growing a name
  never makes it a prefix.
- A retry and failure policy for the background flush. A failing flush is
  retried at `retry_interval` (5s by default) and keeps being retried until it
  lands or the store is dropped, because a full disk is usually someone about to
  delete something and a store that gave up could not heal when they did.
  `retry_budget` (60s) bounds the silence rather than the trying: a streak
  outliving it wakes anyone awaiting that flush with a failure and asks
  `on_persist_failure` what writers should be told from then until a flush lands
  - an error each (`AfterGivingUp::Fail`, the default), nothing
  (`AfterGivingUp::Ignore`), or a panic (`AfterGivingUp::Poison`). Reads and the
  buffer are untouched either way. New public types: `RetryPolicy`,
  `AfterGivingUp`, `PersistFailureCallback`, `store::durable::PersistHealth`.
- `StoreBuilder::provide`, and `MigrationContext::provided` / `require` at the
  other end. A `#[migrate]` step is collected at link time as a bare `fn` and
  captures nothing, so anything it needs from the application had no way in
  except a global. One value per type; `require` reports the type asked for and
  lists what was on offer, because the usual cause is providing a `Foo` where the
  step wanted an `Arc<Foo>`.
- `Kv::namespace`, `try_namespace` and `prefix`, and `Kv::clear` /
  `reset_to_defaults`, which drop what no schema declared and keep what one does,
  returning a `Cleared` saying what went and what stayed. `reset_to_defaults`
  clears the initialization marker before dropping the values rather than after,
  so failing in between leaves the values rather than the mark.
- `Field::into_cell` and `ReactiveMap::into_entry_cell`, the owning forms of the
  two views, plus `ReactiveMap::path` and `instance_id`.
- `InitState` and `StoreBackend::set_initialized`, so a namespace's mark can be
  cleared and not only set.
- `WriteError::intercepted`, carrying the sentence an interceptor's refusal gave.
  All four call sites used to throw it away with `map_err`, which made a filter
  turning a value down and interceptors recursing past the depth guard - a bug in
  the code that installed them - render identically, as did a refused `insert` and
  a refused `clear`.
- `store::one_line`, `Occupied`, `IntoStorageReport` and the `Attempted::doing`
  helper the engines attach their file and operation with.
- `store::load_map`, `store::to_path`, `store::entry_path` and
  `StorePath::entry_name`, each replacing a sequence several call sites had
  spelled out for themselves.
- `#[track_caller]` on the subscription builders (`register`,
  `register_with_source`, `stream`, `watch_raw`) and on the panicking path
  builders (`StorePath::segment`, `from_segments`, `push`, `Kv::namespace`). A
  subscription made the way the subscriptions chapter teaches used to log a
  location inside this library; a refused name used to point into it.
- `amethystate_core::test_utils::TempPath`, which deletes its file on drop along
  with whatever the backend wrote beside it - a text backend's sibling backup, a
  lock, a journal. Declare it before the store: drop order is reverse
  declaration, and on Windows an open file cannot be deleted.
- A `golden` cargo feature gating the trybuild and macrotest goldens, which
  compile a crate per case and cost more than the rest of the suite put together.
  Off by default so an ordinary run stays quick; CI turns it on, which is what
  keeps the goldens from rotting.
- Rustdoc on every public item people actually reach for, most of it carrying an
  example that asserts rather than describes. Several were written to pin down
  behaviour that had never been stated: that `len` counts buffered writes and
  costs a scan, that `keys` sorts by the key's string form so numeric keys come
  back 10, 100, 9, that a subscription ends when its handle drops and an
  interceptor does not, and that an `async` write does nothing at all until
  awaited, the write included.
- `AGENTS.md`, a map of where the book lives, what sits in which crate, that
  `ci.ps1` is the authoritative check set, and which goldens are regenerated
  rather than hand-edited.

### Fixed

- What is still buffered when the store is dropped is written. The debouncer's
  inner wait returned on a closed channel instead of breaking, so the last quiet
  period was skipped - which is the one case dropping the store exists to cover.
  It was masked in-process by every backend flushing from its own `Drop`.
- A listing says what it holds and holds it in the store's order.
  `ReactiveMap::keys` and `entries` sorted by the bare name and their doc claimed
  that agreed with the store; a flat engine sorts by the joined key, where a name
  is escaped, and escaping does not preserve order - so `a.b` sorts before `a1b`
  by name and after it by key. The comparator lives beside the join and is pinned
  to it by a property.
- A scan no longer refuses a whole prefix over one name it cannot address. A
  document may hold `{"": 1}` through a text editor; that entry is passed over
  and logged, and everything else still lists. A key that will not parse is
  carried up with the key attached rather than disappearing.
- Three places stopped substituting a plausible value for a failure:
  `StoreExt::decode` returned `T::default()` on any decode error;
  `TextDocument::scan` and the two document walkers dropped an entry whose name
  cannot be a level, which made `delete_prefix` walk past it and `len` wrong; and
  `MigrationContext::scan_map` skipped an entry it could not read, which - since
  a migration writes the map back whole - was a delete, once, against data with
  no other copy.
- A delete of a path that held nothing emits nothing. Every engine used to send a
  `Delete` with no old value and no new one, so a subscriber acted on a removal
  that had not happened, and a document was scheduled for a flush it did not
  need.
- The text engines' scan walkers asked the joined prefix whether it ended in a
  dot before listing the value at the prefix itself, and a trailing dot there is
  an escaped one - `cfg.b\.` is a level called `b.` - so that value was missing
  from its own scan.
- toml reached a child through `Item`'s `Index`, which inserts the key it is
  asked for, so walking to an absent path built the levels on the way. It also
  addresses through `as_table_like` now, so an inline table is written into
  rather than replaced by one.
- `delete_prefix` takes the subtree in one call. It used to scan one level deep
  and delete each result, which meant deleting branches.
- The seeding write in `field_with_path` is nobody's request, so it no longer
  fails a whole struct: a field whose parent is occupied keeps its default in
  memory, leaves the file alone, and says so with the whole chain on one line.
- `Kv::guard` covers `as_root` structs, which it did not, so `Kv` could write
  over their fields with any type and the struct would stop constructing.
- A construction cycle between nodes is refused at compile time rather than
  recursing at startup.

### Performance

- `ReactiveMap`'s reads come from the projection it already builds at
  construction. On ten thousand entries `len` goes from 386 ms to 620 ns and
  stops depending on size at all, `keys` from 47 ms to 2.9 ms, `entries` from
  61 ms to 3.2 ms, and `get` to 42 ns. The cost lands on small maps: a scan of
  ten entries is 8.7 us against 4.2 us, because `DashMap::iter` touches every
  shard whatever the size.
- Folding the write buffer into a scan is a lookup rather than a search. The
  fold searched a `Vec` linearly for every pending key and removed with `retain`,
  another full pass each, while iterating a `HashMap` it never looked anything up
  in. Measured on redb with the whole map still buffered: at 10 000 entries
  `scan_keys` 344.6 ms to 5.09 ms and `scan_prefix` 394.1 ms to 8.13 ms; at
  1 000, `scan_keys` 4.14 ms to 0.32 ms. Ten times the size cost 83 times the
  time before and 16 times after.
- The map projection is a `DashMap` rather than an `Arc<Mutex<HashMap>>`, which
  serialised readers against each other. That was harmless while almost nothing
  read it and is the hot path now that reads come from it.
- `Field` and `ReactiveMap` are `Arc<Inner>` handles, so cloning one is a single
  atomic increment and rebuilding a primitive from the `Weak` inside a writer
  closure costs nothing.

### Internal

- `backend_conformance.rs` states what a store is - twenty-eight numbered
  properties, twenty-nine tests since property 2 is two statements, each run
  against whichever engine the build enabled. Almost every defect found this
  month was a difference between engines that no single suite was watching. When
  the document walk stopped destroying what it could not hold, json and ron went
  from 19 passing to 24 and toml to 22, with redb and sqlite at 26 and nothing
  failing.
- The tamper suite - stores broken the way a person or another tool would break
  them - moved into the tree as ordinary tests. What still fails is marked
  `#[ignore = "known: ... - see TODO.md"]`, which compiles, reads as the
  specification of what should happen, and does not colour the build.
- `errors_are_not_swallowed.rs` pins the substitutions that used to answer a
  failure with a plausible value, and `error_reports.rs` snapshots what a report
  renders as, with the source location, the temporary path and the backtrace
  taken out so another machine reads the same thing.
- A pass over the suite for assertions that could not fail: a comparison that
  sorted both sides and tested membership, a pipeline that died before the write
  it was watching for, an interceptor branch the depth limit made unreachable, a
  file read with `unwrap_or_default` so the assertion passed when there was no
  file.
- Every test seeding a json, toml or ron document now names the engine it seeds.
  Built without one, a store takes `default_backend()`, which prefers redb - so
  wherever redb was also compiled in, eight files were asserting about a redb
  database sitting beside the document they wrote.
- `proptest-regressions` is checked in, so the two cases that found the escape
  bug in `parse_joined` run first from now on.
- Benches: `reactive_map_bench` measures the map against map size, and
  `store_scan_buffered` calls the store's own scan with everything still in the
  buffer, which the map groups cannot reach now that they answer from the
  projection.
- `TODO.md` is where a finding lands with the numbers or the code that produced
  it, and entries describing code that no longer exists are marked done rather
  than left standing. The two working notes in `docs/` were aimed at 0.10.0 and
  had not been acted on since; git keeps them.

[0.20.0]: https://github.com/uniproc-dev/amethystate/compare/v0.13.0...v0.20.0
