# RFC: what a call can fail with, said by its type

**Status: built, except the migration sets.**

An error today names the *verb* and puts the *nouns* in a bag. A caller that
wanted to tell one refused open from another used to write this:

```rust
let refused = StrictUi::new_with(&store).unwrap_err();

let failed_at = facts::all::<Key, _>(&refused).next();
let said = facts::all::<Refused, _>(&refused).next();

match (refused.current_context(), failed_at, said) {
    (StorageError::Read, Some(Key(at)), Some(Refused(why))) => {
        eprintln!("{at} will not do: {why}")
    }
    _ => return Err(refused.into()),
}
```

Every part of that is the caller rebuilding something the library already knew
and threw away.

## What is wrong with it

**The enum mixes two axes.** `Open`, `Read`, `Write`, `Delete`, `Scan`, `Flush`,
`Codec`, `Meta` and `Migrate` say what the store was *doing*. `Path`, `Depth`,
`CommitFailed`, `Claimed`, `Closed`, `Notify` and `Reentrant` say what *went
wrong*. Sixteen variants along two axes, with nothing marking which is which.

**The verb is often the wrong one.** A struct that would not open reports
`Read`, because a read was the last thing attempted. The caller matching on it
is matching on an implementation detail of the constructor.

**The nouns are `Any`.** `facts::all::<Key, _>` is a downcast over the report's
frames. Nothing says which facts a given failure carries, so `Some(Key(at))` in
that pattern is a guess the compiler cannot check: change what is attached and
the arm silently stops matching and falls through to `_`.

**`current_context()` is a dependency showing through.** The shape of the public
API is `error_stack`'s, and a caller has to learn it to ask one question.

**The catch-all is load-bearing.** `_ => return Err(..)` is where a caller that
wanted "refused" and met a slightly different combination ends up, without
noticing.

**And `?` into `anyhow` does not compile.** `error-stack` provides
`From<Report<C>> for Box<dyn Error + Send + Sync>` but deliberately no
`impl Error for Report<C>`, and `anyhow::Error: From<E>` wants `E: Error`. So
the way out for a caller who does not care is `.map_err(|e| e.into_error())` at
every call. Giving up should be free.

## The rule

**Each point in the API fails with the set that is possible there, and nothing
else.** Not one enum for the library: `Kv::set` cannot fail on a flush, a
struct's constructor cannot fail on a path the macro already checked, and
neither should have to be excluded by hand.

Then `match` without a `_` arm means something, and adding a failure mode to an
operation is a compile error at every caller that was handling them all - which
is the point of saying it in the type.

**A variant carries everything known where it was raised.** Not a path alone
where the owner is also known, not a reason alone where the place is. More is
better: the caller can ignore a field, and cannot invent one.

The shape already exists in the crate and is the one to copy:

```rust
pub struct Disagreement {
    pub at: StorePath,
    pub reason: Reason,
}

pub enum Reason {
    WillNotRead(Arc<str>),
    Refused(Arc<str>),
    Occupied(Arc<str>),
}
```

## Giving up is free

Every set is an ordinary `std::error::Error`:

- `Send + Sync + 'static`, so `anyhow` and `eyre` take it;
- `Display` writing one line, which is what a `{:#}` chain shows;
- `source()` chaining down to the cause.

```rust
// Take it apart.
match StrictUi::new_with(&store) {
    Ok(ui) => ui,
    Err(OpenStruct::Refused { at, said })     => return warn(at, said),
    Err(OpenStruct::WillNotRead { at, said }) => return warn(at, said),
    Err(OpenStruct::Claimed(taken))           => panic!("{taken}"),
    Err(OpenStruct::NotAPath(why))            => return Err(why.into()),
    Err(OpenStruct::Store(disk))              => return Err(disk.into()),
}

// Or do not.
let ui = StrictUi::new_with(&store)?;   // anyhow, eyre, Box<dyn Error>, all of them
```

**The report is not lost - and where the numbers are the diagnosis, it is
carried whole.** `TooDeep`, `WillNotEncode` and `WillNotRead` hold their
`Report<StorageError>` rather than a rendered sentence: which budget ran out
and by how much, what the codec choked on, how many bytes it found, are all
attachments, and a variant that flattened them to a line would be the same loss
the bag of facts was. `Display` renders the chain; the field is there for
whoever wants the tree.

**`StorageError` stops trying to be the whole story** and becomes what it
honestly is - the disk's: `Open`, `Read`, `Write`, `Delete`,
`Scan`, `Flush`, `Codec`, `Meta`, `Migrate`, with the facts attached as they are
now. It lives inside the `Store(..)` variant of every set and is reachable
through `source()`, so anything wanting the tree of attachments still has it.
The reasons that were mixed into it - `Claimed`, `Closed`, `Path`, `Depth`,
`Reentrant`, `CommitFailed` - move into the sets where they are actually
possible, and the two axes come apart without anybody prising them.

## The sets

Ten, over roughly forty public fallible functions. The rest of the functions
share a set with one of these.

| set | raised by | beyond `Store(..)` |
| --- | --- | --- |
| `OpenStruct` | `new`, `new_with`, `new_with_id`, `new_with_id_under` | a declared check refused a stored value; the bytes will not read as the field's type; the place is claimed by another owner; seeding found something already there |
| `OpenStore` | `build`, `located`, `beside_the_executable` | the directory cannot be used; the file is held by another process; the engine would not open it |
| `OpenStoreMigrating` | `build_with_migration` | everything `OpenStore` has, and a step that failed - which prefix, which version, and what it said |
| `ReadValue` | `Store::get`, `Kv::get`, `Store::decode` | the name is not a level; the bytes will not read as the asked-for type; the store is closed |
| `WriteValue` | `Store::set`, `Store::delete`, `Field::set`, `ReactiveCell::set`/`update`/`modify`, `ReactiveMap::insert` | not a level; the value will not encode; past the depth or size the store allows; closed |
| `KvWrite` | `Kv::set`, `Kv::remove`, `Kv::clear`, `Kv::reset_to_defaults` | everything `WriteValue` has, and: a declared struct owns that place - which struct, and which of its paths |
| `ScanKeys` | `Store::scan_keys`, `Kv::keys` | a stored key will not read back as a path, and the key as it sits on disk; closed |
| `LoadMap` | `Kv::map`, a map field's constructor | everything `ReadValue` has, and: a key sits deeper than an entry, so it belongs to whatever claimed that level; a name will not read as the map's key type |
| `RunStep` | the eleven `MigrationContext` methods | nothing was provided for a `require`; a value will not decode into the shape the step asked for; the key is not a path |
| `Flush` | `close`, `save_now`, `flush_prefix` | the flush did not land, and what was still buffered; already closed |

Every name is a verb and its object, the way `OpenStruct` is. Two of them owe
that shape to a collision as well as to symmetry: `Read` and `Write` are in
`std::io`, in the prelude of half the files that would import these, and a set
a caller cannot `use` without renaming it is a set they will not `match` on.

`OpenStoreMigrating` is its own type rather than a `Migration` variant on
`OpenStore`, and `KvWrite` is its own rather than a `Declared` variant on
`WriteValue`, for the same reason: a variant that one caller can never see is a
variant every caller has to read past.

**Reused, already the right shape:** `StorePathError`, `Reason`,
`Disagreement`, `Collision`, `Cleared`. No new vocabulary is needed - `Reason`
is what a field says about a value the store disagrees with, and it is now
written once: the private `Unread` and the hand-written translation between the
two in `reactive/field.rs` are gone.

## What this costs

Not the 293 places that construct an error. Those keep `Report<StorageError>`:
the sets appear at ten public boundaries, where the report is wrapped.

- ten enums, with `Error`, `Display` and `source`;
- ten wrapping points;
- 73 places that take an error apart, most of them documentation and the book
  blocks that are checked against it, in two locales.

## Order

1. **`OpenStruct`. Done.** The worst call site was its, the pattern to copy is
   next to it, and it is where the shared `Reason` vocabulary comes out into
   the open - along with the duplication between `Reason` and `Unread`.
2. **`KvWrite` and `WriteValue`. Done.** Two sets that were meant to differ by
   one variant, which was the test of whether the rule survives contact.
3. **`ReadValue`, `ScanKeys`, `Flush` and `OpenStore`. Done.** Driven by
   rewriting every test to `anyhow::Result`, which turns the whole suite into
   the checklist: a call still on `Report<StorageError>` does not compile there.
   `RunStep`, `OpenStoreMigrating` and `LoadMap` are what is left.

Each step is finished when a test does `?` into both `anyhow` and
`Box<dyn Error + Send + Sync>` and a `match` with no `_` arm compiles.
`tests/what_an_open_can_fail_with.rs` is that test for step 1.

### What step 1 changed on the way

`Claimed` carries four things rather than two: `at` and `wanted_by` for the
declaration that was turned down, `held_at` and `held_by` for the one standing.
The two paths differ whenever one declaration reaches the other through an
ancestor - `root.b` holding `root.b.x` - and the pair is the whole diagnosis.

They live in `Taken`, boxed, rather than as four fields on the variant. Four
paths and names come to exactly the 128 bytes at which a large `Err` starts
widening every `Result` on the way out, and `Claimed` is the rarest of the
five. `Owners::take` refuses with the same `Box<Taken>`, and `OpenStruct` takes
it by `From`; `Owners::claim` stays, wrapping it into a report, for the callers
still on `Report<StorageError>`.

`refused_or_default` and `refused_struct_or_kept` now refuse with
`OpenStruct::Refused` rather than a `Report<StorageError>` of `Read`, which is
what made a check's verdict reach a persistent `load_with` as a disk failure.

### What step 2 changed on the way

**`StorageError` moved into `amethystate-core`**, and `AmeBackendSync` lost its
associated `Error` for it. It had exactly one implementor, and keeping the
error behind an associated type meant a set built in core could not say
`Store(Report<StorageError>)` - so a write would have had to either lose the
report or be classified twice, once in core and once at the boundary. It is now
`StorageResult` throughout, and `WriteValue` lives beside the ops that raise it.
`AmeBackendAsync` keeps its own `Error`: a client talking over a wire really
does fail in its own vocabulary, and `WriteValue::from_backend` folds it in
under the operation it was carrying.

**They differ by more than one variant, and that is the honest answer.**
`WriteValue` has `Intercepted`, `Absent` and `SourceGone`, which a raw write
cannot reach; `KvWrite` has `Declared`, which a field write cannot. Four
variants are shared. Written as one set, every caller would read past three or
four arms that cannot fire where they are.

**`Reason` replaced the private `Unread`** - the duplication step 1 flagged -
and gained `Closed`. `Field::try_get` now returns `Result<T, Disagreement>`,
which is what it always was: not a write, and not the store's failure, but what
this field and the store do not agree about, at a path, for one of four
reasons. `Disagreement` is an ordinary `Error`, so `?` still works.

`#[non_exhaustive]` goes on `StorageError`, `Occupied` and `Reason` and on none
of the sets. The sets exist so a `match` can be complete and so gaining a way
to fail breaks every caller that was handling them all; `non_exhaustive` forces
a `_` arm and takes exactly that away. The other three are lists that grow with
the engines and are read rather than dispatched on.

`tests/what_a_write_can_fail_with.rs` is the finishing test for this step.

### What step 3 changed on the way

**The boundary is `StoreExt` and `Store`, and the plumbing under it is
`StoreBackend`.** The engine trait keeps `StorageResult`, because it is where
the facts are attached and there are hundreds of those; the typed surface over
it hands back the sets.

**What made that cheap is `From<Set> for Report<StorageError>`** - the orphan
rule allows it, since the trait's parameter is ours. Every internal `?` that
crossed from a set back into a report kept compiling, so a change that could
have touched ~400 call sites touched about thirty. It goes both ways at the
boundary and is the honest statement of the split: outside the library you meet
a set, inside it everything is still a report.

**`to_path` returns `StorePathError` rather than a report.** Wrapping it in
`StorageError::Path` was the reason a `set(["ui", ""])` came back as
`Store(..)` instead of `NotAPath`.

**A codec's refusal is not the outermost frame.** A read that finds the wrong
type says `Read` on top and `Codec` below, so `ReadValue::from_store` asks
`will_not_read` - which already existed for exactly this question, and is now
`pub` because two boundaries need it rather than one.

`tests/what_a_read_can_fail_with.rs` is the finishing test for this step: four
exhaustive matches, and one function that opens, writes, flushes, reads and
closes through `anyhow::Result` and again through `Box<dyn Error + Send +
Sync>`.
