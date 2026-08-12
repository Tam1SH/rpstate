---
title: Fields & Subscriptions
sidebar:
  order: 5
---

## Reading and writing

A reactive field exposes three core operations:

```rust
state.port().get()           // read current value
state.port().set(9090)?      // write and persist
state.port().update(|p| p + 1)?  // read-modify-write
state.port().modify(|p| *p += 1)?  // in-place mutation
```

## Subscriptions

`subscribe` fires on every `set()`, regardless of who wrote the value:

```rust
let _sub = state.port().subscribe(|p| {
    println!("port is now {p}");
});
```

Subscriptions are RAII handles — the callback is unregistered when the handle is dropped. Store it as long as you need it:

```rust
struct MyComponent {
    _sub: SignalSubscription,
}
```

For managing multiple subscriptions at once, use `ReactiveScope`:

```rust
use amethystate::ReactiveScope;

let mut scope = ReactiveScope::new();

state.port().subscribe(|p| println!("{p}")).watch(&mut scope);
state.host().subscribe(|h| println!("{h}")).watch(&mut scope);

scope.clear(); // drops all subscriptions at once
```

## Send + Sync requirement

Callbacks must be `Send + Sync` because external changes — for example when the underlying file is modified outside the process — are delivered from a background watcher thread.

For frameworks that don't support `Send + Sync` callbacks directly, bridge via a channel:

```rust
let (tx, rx) = std::sync::mpsc::channel();

let _sub = state.port().subscribe(move |val| {
    let _ = tx.send(val);
});

// drain rx in your framework's event loop
```

## clone vs fork

`clone()` and `fork()` both give you a new handle to the same field, but they differ in one thing: `instance_id`.

**`clone()`** preserves the same `instance_id`. Both the original and the clone are considered the same actor — `subscribe_external` on one will fire for writes from the other.

**`fork()`** assigns a new `instance_id`. The fork is a distinct actor. `subscribe_external` on the original will fire for writes from the fork, and vice versa.

```rust
let a = state.port();
let b = state.port().clone(); // same instance_id as a
let c = state.port().fork();  // new instance_id
```

## subscribe vs subscribe_external

`subscribe` fires on every write — including writes made by the same handle. If a component writes to a field and subscribes to it, it will receive its own writes back. This is fine for most cases.

`subscribe_external` filters out writes from the same `instance_id`. It only fires when another actor made the change:

```rust
let state = ConnectionState::new()?;
let watcher = state.fork();

// fires only when watcher (or anyone else) writes — not when state writes
let _sub = state.port().subscribe_external(|p| {
    redraw();
});

state.port().set(8080)?;   // silent — same instance_id
watcher.port().set(9090)?; // fires
```

A typical pattern is a background thread writing, and the UI reacting without spurious redraws:

```rust
let watcher = state.fork();

thread::spawn(move || {
    loop {
        watcher.latency_ms().set(measure_ping())?;
        thread::sleep(Duration::from_secs(1));
    }
});

// UI subscribes externally — only redraws when the background thread writes
let _sub = state.latency_ms().subscribe_external(|ms| {
    ui.update_latency(ms);
});
```

`subscribe_external` also fires when the value is changed from outside the process entirely — for example if the store file is edited externally. Those changes have no `instance_id` and are always delivered to all subscribers including `subscribe_external`.

## ReactiveMap subscriptions

`ReactiveMap` follows the same pattern with `subscribe_any`, `subscribe_key`, `subscribe_any_external`, and `subscribe_key_external`:

```rust
// any change to the map
let _sub = state.limits().subscribe_any(|change| {
    println!("{change:?}");
});

// only changes to a specific key
let _sub = state.limits().subscribe_key("cpu".into(), |change| {
    println!("cpu limits changed");
});

// external changes only (same fork semantics as Field)
let watcher = state.limits().fork();
let _sub = state.limits().subscribe_any_external(|change| {
    println!("external change: {change:?}");
});
```

### What `_external` filters, and what it does not

The map variants filter `Update` and nothing else. `Insert`, `Remove` and `Clear` are delivered
to every subscriber regardless of who caused them, including the actor that caused them.

The distinction is between editing a value and changing what the map holds. A value you wrote
yourself is your own business — you already know about it. But a key appearing or disappearing
changes the shape of the map, and a view listing the keys has to rebuild whether or not it was
the one that added the key.

This has one consequence worth knowing about:

```rust
let limits = state.limits();
let _sub = limits.subscribe_any_external(|change| {
    println!("{change:?}");
});

limits.set_or_create("cpu".into(), &80)?;  // Insert — delivered to you
limits.set_or_create("cpu".into(), &90)?;  // Update — filtered out
```

`set_or_create` is an `Insert` the first time and an `Update` after that, so whether your own
call comes back to you depends on whether the key already existed. If you need every change
including your own, use `subscribe_any`; if you need none of your own, compare
`change.source()` against your `instance_id` yourself.

### `clear` reports twice

`clear()` deletes each key through the store, and the map rebuilds a `Remove` from each of
those deletions on top of the `Clear` itself. A subscriber to a map holding two keys sees three
events: `Remove`, `Remove`, `Clear`. Match on the variants you care about rather than assuming
one call produces one event.
