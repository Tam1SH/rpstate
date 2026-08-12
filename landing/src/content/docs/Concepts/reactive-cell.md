---
title: "ReactiveCell<T>"
sidebar:
  label: ReactiveCell
  order: 6
---

The one type every reactive value can become. A field, a map entry, or a plain in-memory value
all erase into it, so code that just needs "a `u64` I can read, write and watch" does not have to
name which of the three it got — or carry the store backend and access mode in its own signature.

```rust
use std::collections::HashMap;

// all three are ReactiveCell<u64>
let width      = state.sidebar_width().cell();
let cpu_column = state.widths().entry_cell("cpu".into(), 110);
let dragging   = ReactiveCell::new(0u64);

let mut columns: HashMap<String, ReactiveCell<u64>> = HashMap::new();
columns.insert("cpu".into(), cpu_column);
columns.insert("dragging".into(), dragging);
```

## Writes always land

A cell writes through to wherever its value actually lives. There is no way to obtain one whose
writes go into a cache and quietly disappear.

```rust
cell.set(200)?;              // reaches the store
cell.update(|w| w + 10)?;    // read, transform, write — not atomic
cell.modify(|w| *w += 10)?;
```

`set` returns a `Result` and fails if the write is refused — by an interceptor, or by the store
itself. The cache is not touched on the way in: it is updated when the store reports what it
committed, so a refused write never shows up in `get()`.

## Reading and subscribing

```rust
let current = cell.get();

let _sub = cell.subscribe(|w| println!("width → {w}"));
```

`get()` reads a cache held by the cell, so it costs the same as reading the primitive directly —
cheap enough for a render loop that reads every frame.

Cells also implement `Reactive`, so they work in pipelines:

```rust
let label = (cell_a, cell_b)
    .pipe()
    .map(|(a, b)| format!("{a}×{b}"));
```

## Where they come from

| from | how | persisted |
|---|---|---|
| `Field` | `field.cell()` | yes, unless the field is volatile |
| map entry | `map.entry_cell(key, default)` | yes |
| nothing | `ReactiveCell::new(value)` | no |

`cell()` exists only on writable fields, so a read-only field cannot produce one.

For a map entry, `default` stands in while the key is absent, and again if it is later removed.

A cell keeps alive whatever feeds it, so it is safe to build one and drop the field or map handle
it came from — `get()` goes on seeing changes.
