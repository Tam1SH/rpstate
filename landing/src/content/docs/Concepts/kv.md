---
title: "Kv"
sidebar:
  order: 7
---

Reactive values addressed by path, without declaring a struct. For a key set that
is not known at compile time, or where a schema is more ceremony than the job is worth.

```rust
let kv = store.kv();

let width = kv.cell("ui.width", 800u32)?;      // ReactiveCell<u32>
let flags = kv.map::<String, bool>("flags")?;  // ReactiveMap<String, bool>
```

What comes back is an ordinary [`ReactiveCell`](/amethystate/concepts/reactive-cell/) or
`ReactiveMap`, so subscriptions, local delivery and pipelines work exactly as they do for
declared fields:

```rust
width.subscription_with()
    .local(&mut ui)
    .register(|w| resize(*w));
```

The type of a cell comes from its default, so there is nothing to spell out twice.

## Raw access

```rust
kv.set("theme", &"dark")?;
kv.get::<String>("theme")?;
kv.remove("theme")?;
kv.keys("ui")?;             // sorted; values are not read
```

`get` is raw: the type is whatever you ask for at the call site and nothing remembers it.
`cell` does remember, which is the next section.

## What it will not let you do

**Write where a struct lives.** A declared `prefix` belongs to that struct, and writing there
through `Kv` is refused:

```rust
#[amethystate(prefix = "network")]
struct Network { port: u16 }

kv.set("network.port", &"8080")?;   // Err(SchemaOwned)
kv.set("networkish.port", &"8080")?; // fine — different prefix
```

This is not tidiness. Storing a `String` where a `u16` is declared leaves the field's
subscription unable to decode, so it keeps its old value while the store holds something else —
and the next startup fails outright when it reads that path back.

**Use one path as two types.**

```rust
let width = kv.cell("ui.width", 800u32)?;
let oops  = kv.cell("ui.width", String::new())?;  // Err(TypeMismatch)
```

A struct ties a path to a type by declaring it. `Kv` has no declaration, so it records the type
the first time and refuses a second one.

That record lasts for the run: a path written by an earlier run is not checked, and neither is
raw `set`/`get`. It catches a mistake rather than guaranteeing a type.

## What you give up

No versions, no migrations, no drift detection — those belong to the typed structs. If your data
has a shape worth evolving, declare it.

A plugin is not a reason to reach for this: give it its own store file and the typed API, and it
is isolated by construction rather than by rule.
