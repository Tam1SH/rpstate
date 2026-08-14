---
title: Durability
sidebar:
  order: 9
---

Writes do not reach disk when you make them. `field.set()` puts the value in an in-memory buffer, notifies subscribers, and returns. A background timer flushes the buffer to storage a little later.

This is what makes writing cheap enough to do at interface speed — on every frame of a drag, on every keystroke — instead of paying a disk barrier each time. The cost is a window in which your data lives only in memory.

## What you get

**Reads see your own writes.** A value you just wrote is immediately visible through `get()`, even before it reaches disk. The buffer is consulted first.

**Flushes are atomic.** Everything buffered goes to storage in a single transaction. Storage never holds a half-written batch.

**Clean shutdown loses nothing.** Dropping the store flushes it. A process that exits normally has everything on disk.

## What you lose

**A crash loses the buffer.** A process killed by a signal, aborting on panic, or cut off by power loss loses everything written since the last flush. Destructors do not run in those cases.

**The window is the debounce interval** — 300 ms by default:

```rust
let store = StoreBuilder::new("app.db")
    .debounce(50)
    .build()?;
```

A smaller value narrows the window and flushes more often. A larger one widens it and flushes less. This is the only knob, and it controls exactly this trade.

**A notification does not mean the value is stored.** Subscribers are called during `set()`, before the flush. A subscriber can observe a value that a later crash erases. If your callback does something irreversible outside the process — sends a request, writes another file — do not treat the event as proof the value survived:

```rust
state.port().subscribe(|port| {
    // The value is live, but not yet on disk.
    println!("port is now {port}");
});
```

## Forcing a flush

Call `save_now()` to write everything, or `flush_prefix()` to write one branch. Both return after storage has committed:

```rust
store.set("settings.port", &8080)?;
store.save_now()?;
// Now it is on disk.
```

Reach for these at points where losing the last few hundred milliseconds actually matters — before launching an external process, after a step the user cannot repeat. Calling `save_now()` after every write gives back the cheap writes you came for.

## Everything follows these rules

There is no separate path with immediate durability. Every write lives by the same terms, including the bookkeeping `amethystate` does for itself, such as marking a namespace as initialized. That uniformity is deliberate: it is what lets a value and the metadata describing it land in the same transaction, so a crash can never leave one without the other.
