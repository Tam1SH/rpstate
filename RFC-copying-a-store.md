# Taking a copy of a store while it is open

**Status: designed, not built.** Nothing below exists in the tree yet.

An application that has just been told its last flush failed can do nothing
about it. `close` has closed the store, every read but a `get` from memory
answers `Closed`, and a second `close` answers `Ok` without flushing again. The
docs used to offer "retry, save elsewhere, or not exit yet" as things the caller
could do; none of the three were true, and the sentence is gone from
`Store/opening.md` and from `Store`'s own doc comment.

What would make "save elsewhere" true is a copy taken while the store is still
open. This describes that copy, and the check that it arrived intact.

## The shape

```rust
impl Store {
    /// Writes a copy of this store at `dst`, reads it back, and checks it is
    /// the same bytes.
    pub fn copy_to(&self, dst: impl AsRef<Path>) -> StorageResult<StoreLayout>;
}
```

`dst` is a base name without an extension, the same rule
`StoreBuilder::new` follows: the engine names the files. Naming them at the call
site would mean writing down the `.meta` rule the engine owns, which
`StoreBackend::files_layout` exists to avoid.

The returned `StoreLayout` names what was written, so a caller that wants to
hand the copy to something else does not rebuild the names either.

## Why the buffer does not come into it

The first design compared the two stores by content: scan both and compare
key to value. It is wrong, and expensively so.

`scan_prefix` merges the write buffer into its answer:

```rust
let mut buffered = { let lock = self.inner.pending.lock(); ... };
Ok(utils::merge_buffered(committed, buffered))
```

So a scan of the live store reports writes that have not reached the disk, and
a copy of the disk cannot contain them. The comparison would fail by
construction whenever anything was buffered.

Flushing first does not close it. `write_lock` guards commits, not buffering:
a write that lands after the flush goes into `pending` while the copy is still
being taken, and the next scan reports it.

Comparing the files instead removes the question. The buffer is in memory and
reaches neither side, so neither side sees it. What is compared is what a copy
is: the bytes.

## What it does

Under the store's own `write_lock`, so no commit can interleave:

1. Flush, so what is buffered is on the disk and in the copy.
2. Settle the files for copying - engine's own business, below.
3. Stream each file from `files_layout()` to `dst`, hashing as it goes.
4. Read each written file back and hash it, and compare.
5. Open the copy once.

Writes made during the copy stay in `pending` and go out with the live store's
next flush. They are not in the copy, and the copy does not claim to be later
than the flush it was taken from.

Step 4 is not redundant with step 3. Hashing what was handed to the filesystem
says the right bytes were offered; reading them back says they were kept. That
is the failure the whole call is for.

Step 5 costs one open and catches a destination that is not a place a store can
live. Byte equality proves the copy is the same file; it does not prove the
filesystem under it will hand it back on the next process.

## What each engine has to settle first

The only per-engine part, and it is small:

| engine | before the copy |
| --- | --- |
| redb | nothing - the file is whole between transactions, and `write_lock` holds them off |
| json, toml, ron | render the document, which the flush in step 1 already does |
| sqlite | `PRAGMA wal_checkpoint(TRUNCATE)` |

sqlite runs under `PRAGMA journal_mode = WAL`. Committed data sits in the `-wal`
sidecar until a checkpoint moves it, so copying the file `files_layout()` names would
take a database missing its most recent commits, and say nothing.

`VACUUM INTO` is the usual answer and is the wrong one here: it builds a fresh
file, defragmented and re-paged, which is never byte-identical to the source.
That would cost this design its comparison. A checkpoint keeps sqlite behaving
like the other two - one file, copied as bytes.

## The bug this uncovers

`StoreBackend::files_layout` is documented for "a backup tool, an uninstaller, a test",
and for sqlite it answers `Single { data }` - one file. That is true of a closed
store, because dropping the connection checkpoints, and false of a live one,
where the `-wal` sidecar holds committed data the named file does not.

A backup tool following the doc takes an incomplete copy of a running store and
is told nothing. Fixed either by naming the sidecars in the layout, or by
saying on `files_layout` that it describes a store that is not being written to.
The
second is smaller and matches how a backup is taken anyway.

## Not in scope

**Moving a store to another path.** The mechanism is cheaper than it looks -
every backend already holds its handle behind a swap (`Arc<ArcSwapOption<Database>>`
for redb, `Arc<Mutex<Option<Connection>>>` for sqlite), which is what `close`
uses to let the file go, and subscriptions live in the backend's own
`Arc<RwLock<Vec<SubscriptionEntry>>>` rather than in the engine, so reopening
does not disturb them. What is missing is a swappable `path` and a rule for a
move that half happened. A copy has no such rule to write, because it destroys
nothing.

**A shared lifecycle state machine.** Today the whole mechanism is one latching
`AtomicBool` in the debouncer, and each backend layers its own meaning on it -
which is why a failed `close` reports `Ok` the second time. Worth having before
anything moves, files or handles. A copy does not need it: it takes a lock the
backends already have and changes no state.

**A retryable `close`.** Separate and smaller. `stop_accepting` latches, so a
`close` whose flush failed cannot be attempted again and answers `Ok` instead -
success reported for data that is not on the disk. It does not depend on any of
the above.
