---
title: Defining Structs
sidebar:
  order: 4
---

## The `#[amethystate]` macro

The `#[amethystate]` macro transforms a plain Rust struct into a persistent state container.

### Struct attributes

```rust
#[amethystate(prefix = "network", version = 1, mode = "reactive", as_root)]
pub struct NetworkState { ... }
```

| Attribute | Type | Description |
|-----------|------|-------------|
| `prefix` | `String` | Namespace path in the store. Required for root structs. |
| `version` | `u32` | Schema version for migrations. Defaults to `0`. |
| `mode` | `String` | Code generation mode: `"reactive"` (default), `"persistent"`, or `"both"`. |
|`as_root`| `flag` | If specified, fields are written directly to the store root without a namespace. |
| `on_unreadable` | variant | What opening does about a stored value that will not decode. `Refuse` (the default) or `UseDefault`. |
| `on_delete` | variant | What a field does when its key is deleted under it. `Keep` (the default) or `UseDefault`. |

Structs without `prefix` are nested components, intended to be embedded in other structs via `nested`.

A `prefix` claims the place it names and everything under it, so two structs
cannot be declared over the same place - the second one to open is refused. That
is a whole subject of its own, and the one that decides how a `prefix` and a
dotted `key` interact: [Who owns which place](/amethystate/concepts/claims/).

### Field attributes

Field attributes are optional. A field with no `#[amestate]` annotation uses `Default::default()` as its value and the field name as its storage key.

```rust
#[amethystate(prefix = "app")]
pub struct AppState {
    pub counter: u32, // no annotation — uses Default::default(), stored as "app.counter"

    #[amestate(default = 8080)]
    pub port: u16,
}
```

| Attribute | Type | Description |
|-----------|------|-------------|
| `default` | `Expr` | Initial value on first run. If omitted, uses `Default::default()`. |
| `key` | `String` | Overrides the storage key. Defaults to the field name. |
| `nested` | flag | Marks field as an embedded `#[amethystate]` struct. |
| `volatile` | flag | In-memory only. Never read from or written to the store. Resets to default on every restart. |
| `on_unreadable` | variant | This field's answer, overriding the struct's. |
| `on_delete` | variant | The same for a deleted key. |

Those six are the whole set; anything else is a compile error naming the six.

### What a value going wrong does

Three moments, each with its own answer.

**Opening.** A declared path holding something that will not decode into the
field's type refuses construction and names the path. That is `Refuse`, the
default. `UseDefault` is for the application that has to start anyway: the field
takes its declared default, the stored value stays on disk for somebody to fix,
and [`try_get`](/amethystate/primitives/field/) answers `Err` from construction
until a change decodes.

<!-- shown: a struct that opens over a value it cannot read -->
```rust
#[amethystate(prefix = "mixed", on_unreadable = UseDefault)]
pub struct Mixed {
    #[amestate(default = 8080u16)]
    pub port: u16,

    #[amestate(default = "".to_string(), on_unreadable = Refuse)]
    pub licence: String,
}
```
<!-- /shown -->

**A field may tighten what its struct wrote.** Above, the settings open with a
broken `port`, and a `licence` that will not read stops the whole thing.
`Refuse` on the struct with `UseDefault` on a field is a compile error naming
the field. A `nested` struct inherits its holder's answer, tightens it the same
way, and is checked against the holder while it compiles.

**A key deleted under a live field.** The field goes on reporting what it last
held: that is what was on screen a moment ago, and the declared default is a
compile-time guess. `UseDefault` asks for the guess:

<!-- shown: a field that wants the default back when its key goes -->
```rust
#[amethystate(prefix = "mixed_delete")]
pub struct MixedDelete {
    #[amestate(default = 800u32)]
    pub width: u32,

    #[amestate(default = 600u32, on_delete = UseDefault)]
    pub height: u32,
}
```
<!-- /shown -->

**A live change that will not decode.** The field keeps the last value the store
agreed with and no subscriber is called. `try_get` reports it, and clears itself
as soon as a change decodes. There is nothing to declare here.

## #[derive(AmeType)]

`#[derive(AmeType)]` is what lets a plain Rust struct be used as the value of an
`#[amethystate]` field. It computes a compile-time `TYPE_HASH` from the type's
shape, and that number is what the migration pass compares to notice that a
declaration has changed since the data was written.

The hash is a summary, not an identity: distinct shapes can land on the same
number, and where they do, a change goes unnoticed and no drift is reported.
Bumping `version` when a shape changes is the thing that does not depend on it.

```rust
#[derive(Debug, AmeType)]
pub struct CustomEndpoint {
    pub host: String,
    pub port: u16,
}
```

## Volatile fields

Volatile fields live in memory only and reset to their default on every restart. Useful for transient UI state that should not persist.

```rust
#[amethystate(prefix = "app")]
pub struct AppState {
    #[amestate(default = 8080)]
    pub port: u16,

    #[amestate(default = false, volatile)]
    pub loading: bool, // always starts as false, never written to disk
}
```

## Nested structs

Structs without a `prefix` are components — they have no storage namespace of their own and are embedded into a parent struct via `nested`. The parent's prefix is prepended to all nested fields.

```rust
#[amethystate]
pub struct DatabaseConfig {
    #[amestate(default = "localhost".to_string())]
    pub host: String,
}

#[amethystate(prefix = "sys")]
pub struct SystemSettings {
    #[amestate(nested)]
    pub db: DatabaseConfig, // stored as "sys.db.host"
}
```

## Sharing one place between two structs

Two structs cannot both declare the same place - the second to open is refused.
Where one value has to be reachable from two sides, address it by path from the
one that did not declare it: [Kv](/amethystate/primitives/kv/) reads and writes
anywhere no struct has claimed, and
[Who owns which place](/amethystate/concepts/claims/) is what decides where that
line falls.

## Root-level storage (`as_root`)

By default, all fields are stored under the struct's `prefix`. With `as_root`, fields are written directly to the store root with no namespace.

```rust
#[amethystate(mode = "persistent", as_root)]
pub struct AppConfig {
    #[amestate(default = "acme".to_string())]
    pub name: String,

    #[amestate(default = false)]
    pub verbose: bool,
}
```

This produces a file like:

```toml
name = "acme"
verbose = false
```

That is the shape to ask for when the file is read by something other than this
crate — a config somebody edits by hand, or one whose keys another program
already expects at the top level. Root fields are claimed like any others, so
two structs reaching for the same key still collide:
[Who owns which place](/amethystate/concepts/claims/).