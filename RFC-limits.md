# The codec's ceiling is a fact, key depth is a setting, portability is a policy

**Status: partly built.** `key_depth` and the codec ceiling are enforced today.
`portable_across` currently does nothing but lower the depth ceiling to the
lowest of the named engines - the type-level walk of the declared shape and the
value-level check at the write, both described below, are not built.
`landing/src/content/docs/Store/limits.md` documents the built behaviour and
will have to be rewritten when the rest lands.

---

Three things, and calling all of them "the depth limit" is what made the first
draft of this wrong.

**The codec's ceiling is a fact.** `ron` will not read past 64 whatever anyone
configures; `serde_json` stops at 128 less the level its root object spends;
`sonic_rs` under sqlite at 255; `toml` at about 81; `rmp_serde` has none and the
stack ends around 3,200 instead. These were measured, are recorded above, and
are not settings. A write past its own codec's ceiling produces a file that
codec cannot read, so it is refused - always, unconditionally, with nothing to
turn off. There is no number here for anyone to pick.

What is counted is `path_depth + value_depth`, because the budget is shared:
counting segments alone misses that a path of 64 on ron plus any nesting at all
still kills the file, and counting the value alone misses that the path spends
the same allowance. sqlite is the exception in the useful direction - its path
is a `TEXT` key and costs nothing.

**Key depth is configured, on its own.** How deep a path may go is the store's
question rather than the codec's - the application knows the shape of its own
paths, and a cap on them is cheap to check and catches a path that grows without
anyone meaning it to. It also reserves the rest of the shared budget for values,
which is the half nobody thinks about: sixty levels of path on ron leaves four
for whatever is stored there, and a cap turns that into a startup error rather
than a cliff a user's data walks off later.

So one setting, about keys, refused at the moment a path is declared where the
depth is known and the caller is standing there.

**Portability is a policy, and depth is one row of it.** The reason to hold
below your own codec is that a store written on json should still open on ron,
and depth is not the only thing that stops it - it is just the first instance
that happened to be found. The measurements above are already a table of what
each engine cannot hold:

| | refused by |
| --- | --- |
| non-finite floats | json, and sqlite because it carries json |
| `u64::MAX` and anything past `i64` | toml |
| `Option<Option<T>>` kept as two layers | json, sqlite, redb |
| a unit enum variant | ron, through its node type |
| a non-string map key | every text engine |
| the sign of `-0.0` | sqlite |
| depth past the ceiling | all five, at different numbers |

That is the portability surface, and no single number describes it. So:

```rust
StoreBuilder::new(path)
    .portable_across_engines()   // refuse whatever the weakest engine here
                                 // could not give back
```

is one switch over the whole table, not a depth flag - which is why it should be
built as a policy from the start even while depth is the only row implemented.
Off by default, because a store nobody intends to move has no reason to pay for
any of it.

Deriving what it enforces rather than writing constants means it cannot go
quietly stale: adding an engine that loses something new adds a row, and a
store that was portable stops compiling or starts refusing - which is a real
consequence that a hand-written figure would have hidden until someone tried to
open a file.

**A prefix may waive it.** Not to be given more - the codec will not allow that -
but to say *this component is engine-specific and the rest of the store is not*.
The schema snapshot is already per prefix, so the waiver lands on disk with the
rest of the shape and a reader of the file sees which components are portable
without running the program. Drift and migrations are already reckoned per
prefix, so this is the same grain. Paths outside any declared prefix - `Kv`, a
write at an arbitrary path - follow the store.

**Validated at open**, against the engine: a prefix declaring 200 opened on ron
fails at startup naming the prefix and both numbers. Once at build time for the
developer, rather than when a user's data happens to go deep. The declaration is
a budget the author writes, like a version - nothing derives it from the type,
and nothing could.

## What it looks like from the outside

```rust
StoreBuilder::new(path)
    .backend(Backend::Json)
    .limits(|l| {
        l.key_depth(8)
            .portable_across([Backend::Json, Backend::Sqlite])
    })
    .build()?
```

Both in the same closure, because both answer the same question - what this
store will refuse to hold - and because a builder that grows a method per idea
is already an entry in this file. `backend` and `build` stay at the top level;
everything that configures goes into a group and reads as one chain, the way
`file_write` and `located` already do.

`portable_across` takes the set rather than meaning *all*, because "all" is a
moving target - it changes under a store when an engine is added - and because
the honest requirement is usually narrower. A desktop application that ships
json and a mobile one that ships sqlite need those two and have no opinion about
ron. With no argument it means every engine this build has, which is the strict
reading for someone who wants it.

**Portability is mostly a question about types, so it is answered once.** The
schema on disk already records what every path is; the set of engines is known
at `build()`. So a store walks its own declared shape at startup and refuses
what no member of the set can hold, naming the field, and finding that out at
startup is the difference between a bug and a support ticket.

**What is unportable by type is narrower than it looks, and the check has to be
measured rather than reasoned about.** `HashMap<u64, _>` was the example here
until it was run: all three text engines take it and give back `u64` keys,
because serde spells the key the way each format allows - `"10"` in JSON, `10`
in RON and in a TOML inline table. RON in particular is a Rust format and
refuses very little, so for any set containing RON the by-type check has almost
nothing left to say. It earns its place on narrower sets - JSON and TOML
together, say - and every type it refuses needs a run behind it before the
refusal is written, or this check becomes a list of guesses that fail closed.

What is left for the write is the residue that depends on the value rather than
the type: a particular `f64` that is `NaN`, a particular `u64` past `i64::MAX`.
Refusing every `f64` in a portable store would be absurd - almost every `f64` is
fine - so the type check refuses only what is unportable in all its inhabitants,
and the write catches the rest. That is the same split as the gate above, and it
should be the same code.

Three refusals, three moments, and each says what to do next:

```
a path is deeper than this store allows
├╴path: ui.panels.left.tree.node.style.color.fg.alpha
├╴levels: 9, and the limit is 8
├╴set by: limits(|l| l.key_depth(8))
╰╴note: what is stored here spends the same budget - json reads 127 levels in all

a value cannot be read back from where it was put
├╴path: doc.tree
├╴the path spends 2 levels and the value adds 126
├╴json reads at most 127
╰╴note: a deeper value is written without complaint and the file will not open again

this value is not portable, and this store asked to be
├╴path: ui.ratio
├╴value: NaN
├╴json writes it as null and reads back nothing, and sqlite carries json
╰╴note: keep it by waiving portability for this prefix, or drop it from limits
```

The last line of each is the part that is usually missing. A refusal with no way
out is a wall, and the way out here is always one of three: change the value,
waive the prefix, or drop the claim.

## Where each half is enforced

The ceiling is enforced **at the write**, by the counting serializer below, and
it belongs to whichever codec is running. Nothing about it is deferred and
nothing checks it against a setting, because there is no setting to check it
against. redb is the one that most needs this: it has no ceiling of its own, so
without an imposed one a deep value commits and then kills every process that
opens the file afterwards.

The refusal names the codec, its ceiling, the path, and how much of the budget
the path spent - a caller told only "too deep" has to find the rest by
experiment, and the path's share is the half they would not think of.

Key depth is enforced **where a path is declared**, which is the earliest
moment it is known and the only one where the caller is still standing next to
the mistake.

Portability is settled **at compile time** for what it can be - the set of
engines the build has is known before the store exists - and at the write for
the rest, since whether a particular value is representable everywhere is a
question about that value.

What all three deliberately avoid is a number the caller supplies and the store
checks later. That would have to wait for `build()`, because `backend()` is
ordinarily called after the setting and `default_backend()` answers until it -
which is the extension bug recorded further down, where the builder named a file
for one engine and opened it with another for exactly that reason. Nothing here
takes a figure that only the engine can judge, so nothing here has to wait.

## What a non-finite float does today

The first row of the portability table, measured end to end.
`tests/non_finite_float.rs` writes `f64::NAN` through a field on every engine.

Three of five carry it intact. TOML and RON have `nan` and `inf` in their
grammars, and msgpack under redb has no trouble either. The two that cannot
spell it are json and sqlite - sqlite because it encodes with `sonic_rs`, which
answers the way `serde_json` does. So the split follows the **codec**, and the
codec is not visible in the engine's name: reading the list as "the text ones"
is what hid this, since two of the three text engines are fine and one of the
two binary ones is not.

`set` returns `Ok`. The value reaches the file as `null`. The store event
carries `null`, the field's subscription cannot decode it, and it logs and
**leaves the signal alone** - so the handle goes on reporting the value it held
before. A field last set to `5.0` says `5.0` about a store holding `null`.
Meanwhile a typed `Store::get` of the same path answers `Err(Codec(..))`,
because that path propagates the codec failure. The same bytes read two ways
give two different answers, and neither says what happened at the write.

A stale value is worse here than a substituted default would be: a default is a
value nobody wrote, while a stale one is indistinguishable from a write that
worked.

This is the value-level half of `portable_across` that is not built. When it
is, a non-finite `f32` or `f64` is refused at the write whenever the named
engines include one whose codec writes `null` for it - failing where the caller
is standing, rather than surfacing as a confident handle three steps later.
`landing/src/content/docs/Limitations/` says nothing about any of this and
should.

The upstream half is not waiting on anything: `serde-rs/json#202` has been open
since January 2017, and JSON having no non-finite floats is a property of the
format rather than of the crate.

Separately, and not about floats: any decode failure leaves a handle reporting
the past, and `StoreExt::decode` returning an error while a subscription
silently keeps the old value is a split worth settling on its own.

## How the depth is measured without building anything

Not by inspecting the value - by the time it reaches the engine the type is
gone, it is a `&dyn erased_serde::Serialize`, and a five-level struct is
indistinguishable from a five-level tree. Nor by building the node and walking
it: building is the dangerous act, and on redb it is what overflows the stack.

Serde is a push protocol and the engine is on the receiving end. A counting
serializer wrapper is enough:

```
serialize_seq / _map / _struct  -> depth += 1; if depth > limit, Err
end                             -> depth -= 1
```

Nothing is allocated, no node is built, the type is never needed, and the stack
at the point of refusal is `limit` frames deep by construction. `erased_serde`
exists precisely to put a `&mut dyn Serializer` under a `&dyn Serialize`.

`serde_json` already does exactly this on the **read** side - `check_recursion!`
against `RECURSION_LIMIT`. What is missing is the same thing on the write side,
on all five. The path is the other half and costs `path.len()`.

## Recursive types, and why the limit is affordable

A recursive type's depth is fixed by data rather than by code, so any limit is a
cliff on someone's data rather than an error in their program. That is the one
real cost, and it is small: recursive *types* are ordinary, deep recursive
*persisted data* is not. A file browser persists which nodes are expanded, not
the tree; nested layout is bounded by what a person can stand to look at.

And a graph does not need depth. Stored as edges - `ReactiveMap<NodeId, Node>`
with `Vec<NodeId>` inside - any graph is two levels, whatever its diameter. An
adjacency **list** rather than a matrix, since a UI tree's children are ordered.
The flat form is also strictly more expressive: a nested value cannot hold a
cycle at all, while an edge set holds one for free. The nested form wins on
exactly one point, that derive writes it for you.

So a tree deep enough to meet the limit wanted to be an edge set anyway - not to
satisfy the limit, but because a nested blob rewrites and re-notifies the whole
tree on every change to any node, which is the opposite of what a reactive store
is for. The refusal message should say this, and say that bytes or a string are
one level if reactivity inside is not wanted.

What the store does not have is any understanding of the ids inside those
values: no cascade on delete, no notification through a reference, no rewriting
of references by a migration, nothing against a cycle. That is a foreign key,
it is a database feature, and it is a note about where this library ends rather
than a task. Ordering over rows (`RFC-reactive-table.md`) is wanted by everyone
drawing a list; reference integrity is wanted by whoever has a graph. Different
weights, and they should not be added together.
