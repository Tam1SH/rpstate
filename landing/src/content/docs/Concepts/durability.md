---
title: "Durability: availability over consistency"
sidebar:
  label: Durability
  order: 9
---

Most storage documentation opens with what it guarantees. This page opens with what it gives up, because that is the part you need in order to decide whether `amethystate` suits what you are building.

A user interface touches its state without restraint. A list re-reads its settings while it lays itself out. A slider writes on every frame you drag it. Nobody writes that code carefully, and nobody should have to. Reading or writing state is touching a value that already lives in memory, and it should cost about that much.

A store that went to disk on each of those touches would make the interface stutter, and you would end up working around your own state layer: caching it by hand, batching writes yourself, dreading the frame where it decides to flush. A persistence library that has to be avoided in the hot path is not doing its job.

So `amethystate` picks a side. The parallel with CAP is loose but honest: faced with the same kind of choice, this library sacrifices consistency to stay readable and writable at frame rate. Consistency here means the agreement between what you read and what is durably stored — and that agreement is exactly what gets traded away. What you read is always the truth about your application's state. It is not always the truth about what is on disk.

Everything below is the shape of that decision: what it buys, what it costs, and where you can buy the guarantee back when you need it.

## How it works

Writes do not reach disk when you make them. `field.set()` puts the value in an in-memory buffer, notifies subscribers, and returns. A background timer flushes the buffer to storage a little later.

Reads are cheap for the same reason. `get()` looks in that buffer first and answers from it, so a value you have been writing every frame is read back from memory, not from storage.

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
