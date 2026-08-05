---
title: Typescript
---

`amethystate` provides a TypeScript package for Tauri apps with a plain TypeScript or JavaScript frontend. The package ships `Field`, `ReadonlyField`, and `ReactiveMapField` — the primitive classes that generated bindings are built on top of.

## Installation

```sh
npm install amethystate
```

`@tauri-apps/api` is a peer dependency and must already be present in your project.

## Codegen

Generated bindings are a single TypeScript file that imports from `amethystate` and exposes typed classes for each of your state slices.

**1. Add the binary target and dependency to your Tauri crate:**

```toml
# src-tauri/Cargo.toml
[[bin]]
name = "codegen"
path = "src/bin/codegen.rs"

[dependencies]
amethystate-codegen = "*"
```

**2. Create `src/bin/codegen.rs`:**

```rust
#[allow(unused_imports)]
use your_crate_with_amethystate_types as _;

amethystate_codegen::amethystate_codegen_main!(
    ts_out = "../src/bindings/amethystate.ts",
);
```

**3. Run:**

```sh
cargo run --bin codegen
```

## Using generated bindings

Each root struct becomes a class with a static `load()` method. Call it once on startup before rendering your UI.

```ts
import { AppSettings } from "./bindings/amethystate";

const settings = await AppSettings.load();
```

`load()` bulk-reads all keys under the slice's prefix over a single IPC call and wires up subscriptions so the local cache stays in sync with the backend.

## Reading and writing fields

Each field is a `Field<T>` instance with two access patterns:

```ts
// synchronous — reads from the local in-memory cache
const name = settings.username.value;

// optimistic write — updates cache immediately, persists asynchronously
settings.username.value = "Alice";

// async — reads directly from the persistent store (transaction-safe)
const name = await settings.username.get();

// async write — queues a write to the store
await settings.username.set("Alice");
```

`value` getter/setter is the typical choice for UI bindings. Use the async methods when you need a guarantee that the value is consistent with the backend, or want explicit control over when the write is queued.

## Subscriptions

```ts
const unsubscribe = settings.username.subscribe((val) => {
    console.log("username changed:", val);
});

// later
unsubscribe();
```

## Flushing to disk

Writes are debounced in the background. To guarantee immediate persistence — for example before the app closes — call `save()` on the slice:

```ts
await settings.save();
```

## ReactiveMap

Map fields expose synchronous and async access plus subscriptions per-key or for the entire map:

```ts
// async
await settings.env.set("HTTP_PROXY", "http://localhost:8080");
const val = await settings.env.get("HTTP_PROXY");

// synchronous (in-memory cache)
settings.env.setSync("HTTP_PROXY", "http://localhost:8080");
const val = settings.env.getSync("HTTP_PROXY");

// iterate current entries
for (const [key, val] of settings.env.entries) {
    console.log(key, val);
}

// subscribe to any change
const unsub = settings.env.subscribeAny((change) => {
    if (change.type === "Insert") { /* ... */ }
    if (change.type === "Update") { /* ... */ }
    if (change.type === "Remove") { /* ... */ }
    if (change.type === "Clear")  { /* ... */ }
});

// subscribe to a specific key
const unsub = settings.env.subscribeKey("HTTP_PROXY", (val) => {
    console.log("proxy changed:", val);
});
```

## Cleanup

Call `destroy()` when a slice is no longer needed to unregister all subscriptions:

```ts
settings.destroy();
```

## Examples

- [`tauri-settings`](https://github.com/uniproc-dev/amethystate/tree/master/examples/tauri-settings) — TypeScript frontend