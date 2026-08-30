---
title: Migrating from confy
sidebar:
  order: 23
---

`amethystate` includes a compatibility adapter that reimplements the `confy` API on top of its own storage backend. You can switch to it by replacing the `confy` import — existing call sites stay unchanged.

## Enabling the adapter

Add the feature flag to your `Cargo.toml`:

```toml
amethystate = { version = "0.20", default-features = false, features = ["toml", "confy-compat-2"] }
```

For projects using confy 0.6:

```toml
amethystate = { version = "0.20", default-features = false, features = ["toml", "confy-compat-0-6"] }
```

Then replace the import:

```rust
// before
use confy;

// after
use amethystate::confy;
```

The API is the same: `confy::load`, `confy::store`, `confy::get_configuration_file_path`, `confy::store_path` all work as before.

## Coexistence with amethystate state

The adapter writes only to the root section of the file — the flat key-value region that `confy` manages. `amethystate`-managed sections (`[network]`, `[ui]`, and so on, corresponding to struct prefixes) are left untouched.

The original `confy` overwrites the entire file on every `store` call. `amethystate::confy` does not — it merges only the root keys, so both can share the same file:

```rust
// Initialize amethystate on the same path confy uses
let file_path = confy::get_configuration_file_path("my-app", None)?;
StoreBuilder::new(&file_path).init_global();

// confy write — touches only root keys
confy::store("my-app", None, &legacy_config)?;

// amethystate write — touches only its own prefix
let mut network = NetworkState::load()?;
network.mutate(|n| n.port = 9090)?;

// both reads still work
let cfg: LegacyConfig = confy::load("my-app", None)?;
assert_eq!(network.port, 9090);
```

## Limitations

`yaml_conf` and `basic_toml_conf` variants are not supported. The upstream crates behind them (`serde_yaml`, `basic-toml`) are archived or unmaintained.

## Migrating to native amethystate state

Once you are comfortable with the adapter, you can move individual structs to `#[amethystate]` one at a time. Use `as_root` to keep the same flat file layout that `confy` produced:

```rust
#[amethystate(mode = "persistent", as_root)]
pub struct AppConfig {
    #[amestate(default = "Unknown".to_string())]
    pub name: String,

    #[amestate(default = true)]
    pub comfy: bool,

    #[amestate(default = 42i64)]
    pub foo: i64,
}
```

This reads from and writes to the root section of the file with no prefix namespace — the same layout `confy` produced. No data migration is needed.

When you are done migrating all structs, drop the `confy-compat` feature and the `confy` import entirely.