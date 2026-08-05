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
| `nested` | flag | Marks field as an embedded `#[amethystate]` struct. |
| `volatile` | flag | In-memory only. Never read from or written to the store. Resets to default on every restart. |
| `export_mut` | flag | Allows this field to be mutated via `lookup` from other structs. |
| `key` | `String` | Overrides the storage key. Defaults to the field name. |
| `lookup` | `String` | Links to a field in a `parent` struct. Supports dot-notation. |
| `lookup_node` | `String` | Links to a nested struct node in a `parent` struct. |
| `parent` | `Type` | The source struct for `lookup` or `lookup_node`. |

## #[derive(AmeType)]

The `#[derive(AmeType)]` macro is used to implement schema tracking for plain Rust structs (e.g., custom data types used as fields inside your `#[amethystate]` containers).

It calculates a unique compile-time `TYPE_HASH` representing the structure of your data.

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

## Cross-struct references

Fields can share storage with fields in another struct via `lookup`. References are verified at compile time — a wrong field name or type mismatch is a compile error.

```rust
#[amethystate(prefix = "net")]
pub struct NetworkState {
    #[amestate(default = 8080, export_mut)]
    pub port: u16,

    #[amestate(default = "127.0.0.1".to_string())]
    pub host: String,
}

#[amethystate(prefix = "ui")]
pub struct UiState {
    // Read-write link
    #[amestate(lookup = "port", parent = NetworkState, export_mut)]
    pub proxy_port: u16,

    // Read-only link
    #[amestate(lookup = "host", parent = NetworkState)]
    pub proxy_host: String,
}
```

Compile-time guarantees:

- Wrong field name → `no associated item named '__schema_field_porrt'`
- Wrong type → `TypeCheck<String>` is not implemented for `ReadOnly<u16>`
- Writing a read-only link → `no method named 'perform_set' found for ReadOnly<T>`

`lookup_node` links an entire nested struct instead of a single field:

```rust
#[amethystate(prefix = "ui.inspector")]
pub struct InspectorState {
    #[amestate(lookup_node = "db", parent = SystemSettings)]
    pub db_view: DatabaseConfig,
    // accessed as state.db_view().host().get()
}
```

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