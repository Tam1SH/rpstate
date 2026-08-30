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

Those four are the whole set; anything else is a compile error naming the four.

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

By default, all fields are stored under the struct's `prefix`. With `as_root`, fields are written directly to the store root with no namespace — the same layout that `confy` produces.

```rust
#[amethystate(mode = "persistent", as_root)]
pub struct AppConfig {
    #[amestate(default = "legacy".to_string())]
    pub name: String,

    #[amestate(default = false)]
    pub comfy: bool,
}
```

This produces a file like:

```toml
name = "legacy"
comfy = false
```

The primary use case is coexistence with or migration from an existing `confy`-managed file. See [Migrating from confy](/amethystate/migrations/confy-compat/).