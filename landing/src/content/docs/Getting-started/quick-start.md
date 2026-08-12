---
title: Quick Start
sidebar:
  order: 3
---

## Defining state

Use the `#[amethystate]` macro to declare a state struct. The `prefix` sets the namespace under which fields are stored.

```rust,ignore
use amethystate::amethystate;

#[amethystate(prefix = "network")]
pub struct NetworkState {
    #[amestate(default = "127.0.0.1".to_string())]
    pub host: String,

    #[amestate(default = 8080)]
    pub port: u16,
}
```

## Initializing the store

### Global store

The simplest approach. Initialize once at startup, then access state anywhere without passing the store around.

```rust,ignore
use amethystate::{IntoGlobalStore, StoreBuilder};

// From a path string
"./app.redb".init_global();

// Platform config directory (XDG on Linux, AppData on Windows, Application Support on macOS)
StoreBuilder::for_app("my-app", "settings")?.init_global();

// With options
StoreBuilder::for_app("my-app", "settings")?
    .debounce(500)
    .init_global();

let state = NetworkState::new().unwrap();
```

### Explicit store

If you prefer to manage the store lifetime yourself:

```rust,ignore
use amethystate::StoreBuilder;

fn main() -> amethystate::Result<()> {
    let store = StoreBuilder::new("./app.redb")
        .debounce(500)
        .build()?;

    let state = NetworkState::new_with(&store)?;
    Ok(())
}
```

## Reading and writing

```rust,ignore
// Read
println!("{}", state.host().get());

// Write — persists to buffer immediately, flushes to disk debounced
state.port().set(9090)?;

// Subscribe to changes
let _sub = state.port().subscribe(|p| {
    println!("port changed to {p}");
});
```

## Persistent-only mode

For frameworks that own their update loop (egui, iced, ratatui), use `mode = "persistent"`. Fields become plain Rust types with no reactive overhead — a confy-like API.

Note that persistent-only state does not observe external changes. If another part of the application writes to the same store, or the underlying file is modified externally, the loaded struct will not update. Use reactive mode if you need that.

```rust
#[amethystate(prefix = "network", mode = "persistent")]
pub struct NetworkState {
    #[amestate(default = "127.0.0.1".to_string())]
    pub host: String,

    #[amestate(default = 8080)]
    pub port: u16,
}
```

```rust,ignore
let mut state = NetworkState::load_with(&store)?;

// Direct field mutation
state.port = 9090;
state.save_lazy()?; // debounced background flush
state.save()?;      // immediate flush

// Block mutation — immediate flush
state.mutate(|d| {
    d.host = "0.0.0.0".to_string();
    d.port = 443;
})?;

// Block mutation — debounced background flush
state.mutate_lazy(|d| {
    d.host = "0.0.0.0".to_string();
    d.port = 443;
})?;
```

## Reactive Maps

`ReactiveMap<K, V>` is a persistent dynamic collection where each entry is stored as an individual key.

```rust,ignore
#[derive(Debug, Clone, Serialize, Deserialize, Default, AmeType)]
pub struct AlertThresholds {
    pub warning: u64,
    pub critical: u64,
}

#[amethystate(prefix = "sys")]
pub struct SystemSettings {
    #[amestate(default = {
        "cpu": AlertThresholds { warning: 70, critical: 90 },
        "mem": AlertThresholds { warning: 80, critical: 95 }
    })]
    pub limits: ReactiveMap<String, AlertThresholds>,
}
```

```rust,ignore
let state = SystemSettings::new()?;

// Insert or update
state.limits().set_or_create("gpu".into(), &AlertThresholds { warning: 60, critical: 85 })?;

// Lookup
let cpu = state.limits().get(&"cpu".into())?;

// Iterate — always sorted by key
for (key, val) in state.limits().entries()? {
    println!("{key}: {val:?}");
}

// Subscribe to any change
let _sub = state.limits().subscribe_any(|change| {
    println!("{change:?}");
});
```

## Derived pipelines

Pipelines derive a value from one or more reactive fields. They recompute automatically when any input changes.

```rust
use amethystate::IntoPipeline;

let address = (state.host(), state.port())
    .pipe()
    .map(|(host, port)| format!("{host}:{port}"));

println!("{}", address.get()); // "127.0.0.1:8080"
let mut scope = ReactiveScope::new();

address.subscribe(|addr| {
    println!("address changed: {addr}");
}).watch(&mut scope);

```