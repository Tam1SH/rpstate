---
title: Introduction
sidebar:
  order: 1
---

`amethystate` holds a Rust GUI application's state — reactive in memory, saved
on disk.

You describe the state you want in one struct. Saving it, making it reactive,
migrating it when it changes — amethystate does the rest.

## The moving parts

### How it is stored

Everything lives at a path: dotted levels that nest the way a directory does. A
struct's `prefix` decides where it sits, and each field becomes a path under
that prefix. The path is the identity, and it is settled where the struct is
declared.

What that becomes on disk is the engine's business, and there are five of them
— see [Installation](/amethystate/getting-started/installation/). redb and
SQLite get by with one file; the text engines put the data in one file and what
they know about the schema in the one beside it.

### How it is saved

A write changes memory at once: the next read returns the new value, and
subscribers hear about it. It reaches disk after a pause, so a burst of writes
settles into one flush. Writing the value already stored gets no further — a
slider that rounds to the step it was already on costs nothing.

On close the store writes down everything it was holding, so a graceful exit
loses nothing. A write that cannot wait asks for the disk directly — see
[Durability](/amethystate/concepts/durability/).

### How change travels

A write wakes that field's subscribers. In collections whose keys are unknown
at compile time each entry sits at its own path, so you can watch one entry or
the whole collection.

State comes in two kinds, and they differ here alone:

- **Reactive** — a field is a handle: you read it, write it, subscribe to it.
  The rest of the book assumes this one.
- **Persistent-only** — ordinary fields on an ordinary struct, saved when you
  say so. For frameworks that own their update loop, and for state nobody needs
  to watch. It does not see changes made elsewhere.

### The schema is on disk too

Beside the data sits a record of which version of each struct wrote it and what
its fields looked like. Opening the store checks that against the running code.

Version went up — the struct goes through the steps you declared. Fields
changed and the version did not — that is drift: the mismatch is reported with
a diff and startup carries on. Refusing to start over a renamed field is worse
than saying so.

Which steps run depends on how the store was opened, and this is the one thing
worth reading twice.
[`StoreBuilder::build`](/amethystate/migrations/manual/) runs only the steps
handed to it.
[`build_with_migration`](/amethystate/migrations/defining-steps/) also collects
every `#[migrate]` in the binary and reports what the pass did. Open a binary
full of `#[migrate]` steps with `build` and it migrates nothing, and says not a
word about it.
