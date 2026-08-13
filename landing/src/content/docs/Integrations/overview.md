---
title: Overview
sidebar:
  order: 10
---

`amethystate` supports multiple GUI frameworks. Which mode to use — reactive or persistent-only — depends on the execution model of the framework.

## Execution models

### Retain-mode and TEA (egui, ratatui, iced, Xilem)

These frameworks have an explicit event loop. Either they redraw every frame (immediate-mode: egui, ratatui), or all state changes flow through a message → update → render cycle (TEA/MVU: iced, Xilem). In both cases the framework decides when to read state, and there is no place to attach subscriptions in a natural way.

**Persistent-only mode** is the right fit. Fields are plain Rust types, you mutate them in `update` or `show`, and call `save_lazy()` when done.

One caveat: persistent-only state does not observe external changes. If another thread, another process, or a manually edited file changes the underlying store while the app is running, the loaded struct will not update. If you need that, use reactive mode and call `.get()` at the start of each frame or update cycle — the framework loop naturally polls the latest value.

- [egui / iced / ratatui](./retain-mode)


### Property bindings (Slint, GTK 4)
Both frameworks own their UI properties. The bridge is bidirectional: subscribe to a Field<T> and push changes into the framework's property system, with optional back-propagation from UI callbacks into the field.
Reactive mode is required.

- [Slint](./slint)
- [GTK 4](./gtk4)

### Signal-based (Dioxus, Leptos, Yew)

These frameworks use fine-grained signals: components subscribe to sources and re-render only when their inputs change. The integration pattern is the same across all three — subscribe to a `Field<T>` and write the new value into a framework signal. Components read the signal, not the `Field<T>` directly.

The signals differ in ownership model: Dioxus and Leptos use arena-allocated `Copy` handles; Yew uses RC-based handles that are passed by clone. The bridge looks slightly different in each case but the concept is identical.

**Reactive mode** is required.

- [Dioxus](./dioxus)
- [Leptos](./leptos)
- [Yew](./yew)

### Hooks with a UI-thread marshaller (windows-reactor)

Reactor re-renders components from hook state on a single UI thread, and the host exposes a marshaller for getting work onto that thread. That covers the whole problem the other bridges solve by hand: a subscription writes the new value into a hook slot through the marshaller, and the framework re-renders. No channel, no background task, no arena.

**Reactive mode** is required.

- [windows-reactor](./windows-reactor)

### Webview bridge (Tauri)

Tauri splits the application into a Rust backend and a frontend communicating over commands and events. `amethystate` provides a dedicated plugin that handles this boundary — state is loaded on the Rust side, and generated bindings expose it to the frontend. Both TypeScript and Rust frontend clients are supported.

- [Tauri](./tauri)

### GPUI

GPUI uses an entity model with deferred notification. Mutations are applied inside entity update closures, and the framework notifies dependents after the closure returns. This does not compose directly with synchronous `Field<T>` subscriptions, so the bridge goes through an async channel: a background task holds a `WeakEntity`, subscribes to reactive fields, and schedules updates via `AsyncApp`.

**Reactive mode** is required.

- [GPUI](./gpui)