---
title: What serde says here
sidebar:
  order: 5
---

The library stores whatever serde will carry, so a stored type is written in two
vocabularies at once. This page is about where they meet: which serde attributes
this library reads as its own, which it refuses, and which are none of its
business.

The split that decides everything is not which attribute but which kind of type
it is on.

**A struct whose fields are paths** - one with `#[amethystate]`, or embedded with
`nested` - never goes through serde whole. Its fields are stored one at a time,
each at a path, and the struct itself is encoded nowhere. So an attribute here
is read as a statement about paths, or refused.

**A leaf** - anything stored as one value at one path - is the opposite. The
store hands it to the codec and takes back bytes, and never looks inside.

## On a struct whose fields are paths

### Where a field goes

`rename` names the place a field is stored at, and `rename_all` on the struct
says it once for all of them. A dot in that name is a level, so a field can be
put anywhere under the prefix rather than only renamed:

<!-- shown: a struct that says where its fields go -->
```rust
#[amethystate(prefix = "net")]
#[serde(rename_all = "camelCase")]
pub struct NetState {
    #[amestate(default = 8080u16)]
    pub listen_port: u16,

    #[serde(rename = "tls.enabled")]
    #[amestate(default = false)]
    pub tls: bool,
}
```
<!-- /shown -->

That writes `net.listenPort` and `net.tls.enabled`.

The dot is worth pausing on. To serde, `"tls.enabled"` would be one field name
that happens to contain a dot; here it is two levels. Nothing is ambiguous,
because this struct never reaches serde - but somebody reading the attribute
with only serde in mind will read it the other way.

### A field whose paths sit at its holder's level

`flatten` on a `nested` field says its fields are stored here, without a segment
named after it:

<!-- shown: a nested struct whose fields sit at their holder's level -->
```rust
#[amethystate(prefix = "editor")]
pub struct Editor {
    #[serde(flatten)]
    #[amestate(nested)]
    pub window: Window,
}
```
<!-- /shown -->

That writes `editor.width`, not `editor.window.width`.

Two flattened children that spell a field the same way are a compile error
naming both, since each stores its fields at this level and the two would write
over each other. So is a flattened child whose field is spelled the same as one
written beside it.

**Flatten moves paths, so adding it to something already shipped is a
migration.** The data stays where the old build wrote it while the new build
reads a level up, and what a person sees is their settings gone back to
defaults. Same for `rename`, and for `rename_all`, which does it to every field
at once.

### Defaults

`#[serde(default)]` and `#[serde(default = "some_fn")]` are read as
`#[amestate(default = ..)]`: both are the value for an absence. Writing both is
a compile error naming both, because the two could disagree and nothing would
say which won.

`amestate` takes an expression where serde takes the path to a function, which
is the only reason it is still here:

```rust
#[amestate(default = 8080)]
pub port: u16,
```

says what serde needs a `fn default_port() -> u16 { 8080 }` to say.

### What is refused

Each of these is a compile error where it is written, in a sentence saying what
to write instead.

| written | why not, and what instead |
| --- | --- |
| `deny_unknown_fields` | describes one encoded value, and there is none. A path nobody declared is not a key inside anything; `on_unreadable` is the per-field answer |
| `tag`, `tag` + `content`, `untagged` | say how a variant names itself inside one encoded value, and there is none |
| `transparent` | makes a struct encode as its one field, and this struct encodes as nothing |
| `from`, `into`, `try_from` | convert the whole struct on its way through serde, and it never goes through whole. A conversion of one value belongs on that field's type |
| `remote` | writes the serde impls for a type from another crate; this one is yours |
| `skip`, `skip_serializing`, `skip_deserializing` | ask to be left out of an encoded value that does not exist. `#[amestate(volatile)]` is what a field can be here - and it says more: no path, nothing in the schema, nothing for a migration to carry |
| `skip_serializing_if` | would mean the path is not written, so whatever is already there stays - and setting the field to the very thing the rule names would fail to clear it |
| `alias` | names a second spelling to read, and a path is looked for under one name. A `#[migrate]` step with `#[rename(old => new)]` moves the data once at the open and is done |
| `getter` | reads through a function, which serde does for a type it does not own |
| `borrow` | borrows from the input, and a value comes back from the engine owned, one path at a time |
| `serialize_with`, `deserialize_with`, `with` | make one field encode differently from how its own type encodes, and a path holds one value with nothing beside it to say which of the two is there. A second form of a type is a second type: a newtype whose own `Serialize` does it — which is also how a type from another crate gets one |

The attributes are read with serde's own parser, so what serde itself calls a
contradiction comes back in serde's words.

## Inside a leaf

Nothing above applies. The store writes the whole value at one path and reads it
back, and what it is told in between is between you and serde:

<!-- shown: a leaf, where serde answers to nobody here -->
```rust
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, AmeType)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Endpoint {
    pub host_name: String,

    #[serde(default)]
    pub port: u16,
}

#[amethystate(prefix = "svc")]
pub struct Service {
    #[amestate(default = Endpoint { host_name: "localhost".to_string(), port: 443 })]
    pub upstream: Endpoint,
}
```
<!-- /shown -->

`Endpoint` lands whole at `svc.upstream`, kebab-cased and strict about unknown
fields, and the store has no opinion about any of it.

Two things still cost something, and it is worth knowing which:

**A rename inside a leaf is a migration nothing here will notice.** The bytes
move and the Rust name does not. A store written by the old build has the old
field name inside the value, and the new build will not find it.

**`deny_unknown_fields` turns every field a later version adds into a hard read
failure for an older build.** proto3 dropped unknown fields in its first release
and reversed it in 3.5.0 for the same reason. It is your value and your call.

And two habits worth keeping, for the same reason any format keeps them:

**`#[serde(default)]` on every field added after the first release.** Serde
already ignores a field it does not know, so a value written by a newer build
reads on an older one for free. The other direction is not free: a field with no
default fails the whole value when it is missing, so a leaf stored before the
field existed stops reading.

**Named fields rather than a tuple.** A tuple struct serialises positionally, so
a field added in the middle silently reassigns every field after it.

## What a format will carry is a different list

None of this is about what an engine can hold. That is measured, and lives in
[Limitations](/amethystate/limitations/): ron will not carry an enum, json and
sqlite will not carry a `NaN`, toml has no room past `i64`. Those refuse a
*value*; this page is about a *type* saying something the store cannot record.

A type can pass one list and fail the other.
