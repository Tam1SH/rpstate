//! What this binary declares, collected at link time.
//!
//! One entry per `#[amethystate]` struct, submitted by the macro wherever the
//! struct is written and gathered by [`inventory`](https://docs.rs/inventory)
//! without anything registering it by hand.
//!
//! Not reporting. The migration engine walks these to know what shape the code
//! says it has, [`MigrationSet`](crate::migration::set::MigrationSet) falls
//! back to them for a prefix nobody handed it steps for, and
//! [`Kv`](crate::store::Kv) asks them what a namespace may not overwrite - so
//! a store opening at all depends on this being right, which is a different
//! job from showing a person what a field holds.
//!
//! What it does *not* say is what actually opened. A struct compiled in and
//! never constructed has claimed nothing, and an entry here is not evidence
//! that it did - see `RFC-the-ownership-tree.md`.

use crate::migration::fields::FieldDescriptor;
use amethystate_core::path::StorePath;

pub struct SchemaEntry {
    /// Where the struct's fields live. `None` for a struct that has no place
    /// of its own - one built under a namespace given at runtime.
    pub prefix: Option<StorePath>,
    pub struct_name: &'static str,
    pub version: u32,
    pub fields: &'static [FieldDescriptor],
}

inventory::collect!(SchemaEntry);
