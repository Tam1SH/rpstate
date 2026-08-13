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

## Configuring a subscription

`subscribe` covers the common case. Anything beyond it — ignoring your own writes, seeing who made a change, running the callback on your own thread — goes through `subscription_with()`:

```rust
field.subscription_with()
    .external()                 // skip writes made through this handle
    .local(&mut ui)             // run the callback where ui is drained
    .register(|value| ...);     // or .register_with_source(|value, who| ...)
```

Each link is optional and they compose freely. `register` returns a subscription handle you must keep, exactly like `subscribe`.

## Callbacks on your own thread

A plain `subscribe` callback must be `Send + Sync`, because a change made to the store file outside the process is delivered from a background watcher thread. That rules out `Rc` state and most GUI context handles.

`.local()` queues the change instead, and runs the callback when you drain — on whatever thread drains:

```rust
let mut ui = LocalScope::new();

state.port()
    .subscription_with()
    .local(&mut ui)
    .register(move |port| label.set_text(&port.to_string()));

// once a frame
ui.drain();
```

The scope is neither `Send` nor `Sync`, so a callback registered on it cannot reach another thread — the compiler will not let the scope move. Dropping the scope ends every subscription in it.

Most GUI frameworks give you hooks rather than a place to own things, so keep one `LocalScope` in the context the framework already threads through your app, and drain it once a frame. Anything in the tree can then register into it without inventing an owner of its own.

Changes **coalesce**: however many arrived since the last drain, the callback sees the newest once. That is what a frame wants from a value. For map changes, which are events rather than a state, add `.every()` to keep them all.

A callback that writes to what it listens to cannot spin: its own write lands in the next drain, not the current one.

### If you already have an event loop

`changed()` waits until something is queued, so you can drive the drain from a loop instead of a frame:

```rust
loop {
    ui.changed().await;
    ui.drain();
}
```

Or skip callbacks entirely and take a `Stream`:

```rust
let mut ports = state.port().subscription_with().stream();

while let Some(port) = ports.next().await {
    label.set_text(&port.to_string());
}
```

A stream yields every change rather than coalescing — it is a sequence, and you can coalesce downstream if you want to. Dropping it ends the subscription.

## clone vs fork

`clone()` and `fork()` both give you a new handle to the same field, but they differ in one thing: `instance_id`.

**`clone()`** preserves the same `instance_id`. Both the original and the clone are considered the same actor — an `external` subscription on one will not fire for writes from the other.

**`fork()`** assigns a new `instance_id`. The fork is a distinct actor, so an `external` subscription on the original does fire for writes from the fork, and vice versa.

```rust
let a = state.port();
let b = state.port().clone(); // same instance_id as a
let c = state.port().fork();  // new instance_id
```

## Ignoring your own writes

`subscribe` fires on every write, including ones made through the same handle. If a component writes to a field and subscribes to it, it gets its own writes back. That is usually fine.

`.external()` filters them out, so the subscription only fires when somebody else made the change:

```rust
let state = ConnectionState::new()?;
let watcher = state.fork();

let _sub = state.port()
    .subscription_with()
    .external()
    .register(|_| redraw());

state.port().set(8080)?;   // silent — same instance_id
watcher.port().set(9090)?; // fires
```

A typical use is a background thread writing while the UI reacts, without redrawing on its own writes:

```rust
let watcher = state.fork();

thread::spawn(move || {
    loop {
        watcher.latency_ms().set(measure_ping())?;
        thread::sleep(Duration::from_secs(1));
    }
});

let _sub = state.latency_ms()
    .subscription_with()
    .external()
    .register(|ms| ui.update_latency(*ms));
```

A change made outside the process — the store file edited by hand, say — has no `instance_id`, so it is nobody's own write and reaches `external` subscribers too.

On a map, `.external()` filters `Update` only. A key appearing or disappearing changes what the map holds and goes to everyone, including the handle that caused it. See [ReactiveMap subscriptions](#reactivemap-subscriptions).

## ReactiveMap iteration order

`entries()` is **sorted by key** — the same on every backend, and stable across writes.

```rust
// inserted as zulu, alpha, mike
for (key, _) in state.limits().entries()? {
    println!("{key}");   // alpha, mike, zulu
}
```

Insertion order is not recorded. If your UI needs an order of its own — table columns, steps in
a list — keep that list yourself and use the map for lookup.

## ReactiveMap subscriptions

`ReactiveMap` follows the same pattern, with `.key()` to narrow to one entry:

```rust
// any change to the map
let _sub = state.limits().subscribe_any(|change| {
    println!("{change:?}");
});

// only changes to a specific key
let _sub = state.limits().subscribe_key("cpu".into(), |change| {
    println!("cpu limits changed");
});

// one key, other people's changes only, delivered on your thread
state.limits()
    .subscription_with()
    .key("cpu".into())
    .external()
    .local(&mut ui)
    .every()
    .register(|change| println!("{change:?}"));
```

Map changes are events rather than a state, so pair `.local()` with `.every()` unless dropping the intermediate ones is what you want.

### What `external` filters, and what it does not

The map variants filter `Update` and nothing else. `Insert`, `Remove` and `Clear` are delivered
to every subscriber regardless of who caused them, including the actor that caused them.

The distinction is between editing a value and changing what the map holds. A value you wrote
yourself is your own business — you already know about it. But a key appearing or disappearing
changes the shape of the map, and a view listing the keys has to rebuild whether or not it was
the one that added the key.

This has one consequence worth knowing about:

```rust
let limits = state.limits();
let _sub = limits.subscription_with().external().register(|change| {
    println!("{change:?}");
});

limits.set_or_create("cpu".into(), &80)?;  // Insert — delivered to you
limits.set_or_create("cpu".into(), &90)?;  // Update — filtered out
```

`set_or_create` is an `Insert` the first time and an `Update` after that, so whether your own
call comes back to you depends on whether the key already existed. If you need every change
including your own, use `subscribe_any`; if you need none of your own, compare
`change.source()` against your `instance_id` yourself.
