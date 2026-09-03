use crate::MigrationContext;
use crate::store::StorageResult;

/// What a declared path is, as far as the store is concerned.
///
/// Not what the value is - that is the value's business and the disk's. This
/// says whether the path holds one value, or is the level a map's entries sit
/// under, or is only a level on the way to other declared paths.
/// Written as a string rather than as a variant, because a `ron::value::Value`
/// has nowhere to put a unit variant and refuses to read one back.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(into = "&'static str", try_from = "String")]
pub enum Role {
    /// One value lives here. Anything under it in a document is the inside of
    /// that value, not a path.
    Field,

    /// A map's entries live one level under here. This path itself holds
    /// nothing.
    Map,

    /// A level on the way to declared paths, holding nothing itself.
    Node,
}

impl Role {
    /// `==` where a `const` needs it, which the derived `PartialEq` cannot
    /// answer.
    pub const fn same(self, other: Self) -> bool {
        self as u8 == other as u8
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Role::Field => "field",
            Role::Map => "map",
            Role::Node => "node",
        }
    }
}

impl From<Role> for &'static str {
    fn from(role: Role) -> Self {
        role.as_str()
    }
}

impl TryFrom<String> for Role {
    type Error = String;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        match name.as_str() {
            "field" => Ok(Role::Field),
            "map" => Ok(Role::Map),
            "node" => Ok(Role::Node),
            other => Err(format!("no such role: {other}")),
        }
    }
}

#[derive(Clone)]
pub struct FieldDescriptor {
    /// Where it is stored, which is its own name unless `path` or `rename_all`
    /// said otherwise. A dot in it is a level.
    pub name: &'static str,

    /// The name in the source, which is what the code calls it. Told apart
    /// from [`name`](FieldDescriptor::name) because a person editing the file
    /// and a person reading the code are looking at different words.
    pub declared: &'static str,

    pub type_hash: u32,
    pub type_name: &'static str,

    pub role: Role,

    /// Whether the path may hold nothing and still be a path - which is not the
    /// same as the path being absent, and is written differently by every
    /// engine that can write it at all.
    ///
    /// Read from the type by [`Probe`](crate::shape::Probe).
    pub optional: bool,

    /// For a [`Role::Node`], the fields that live under it; empty otherwise.
    ///
    /// See [`FieldDescriptor::leaf`] for the ordinary case.
    ///
    /// A static reference rather than a walk, so the set of declared paths is
    /// known without opening the store. It cannot be cyclic: a construction
    /// cycle is refused at compile time by
    /// [`AmeStateNode::CONSTRUCTION_TERMINATES`](crate::AmeStateNode::CONSTRUCTION_TERMINATES).
    pub children: &'static [FieldDescriptor],

    /// Whether this node gives the paths under it a segment of its own.
    ///
    /// A flattened node does not: its children sit at its holder's level, so
    /// anything walking this tree to reach a path has to pass through without
    /// adding [`name`](FieldDescriptor::name). Written as `#[serde(flatten)]`,
    /// and false for everything that is not a [`Role::Node`].
    pub flattened: bool,
}

impl FieldDescriptor {
    /// A path holding one value, which is what most declared paths are.
    pub const fn leaf(name: &'static str, type_hash: u32, type_name: &'static str) -> Self {
        Self {
            name,
            declared: name,
            type_hash,
            type_name,
            role: Role::Field,
            optional: false,
            children: &[],
            flattened: false,
        }
    }
}

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }

    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Whether `fields` puts a path called `name` at its own level.
///
/// Reached through flattened nodes, which contribute no segment: a name a
/// flattened grandchild brings up arrives here as if it were written here.
pub const fn brings(fields: &[FieldDescriptor], name: &str) -> bool {
    let mut i = 0;
    while i < fields.len() {
        if fields[i].flattened {
            if brings(fields[i].children, name) {
                return true;
            }
        } else if same(fields[i].name, name) {
            return true;
        }
        i += 1;
    }
    false
}

/// Whether two sets of fields, flattened into the same level, would land on a
/// name in common.
pub const fn overlap(a: &[FieldDescriptor], b: &[FieldDescriptor]) -> bool {
    let mut i = 0;
    while i < a.len() {
        if a[i].flattened {
            if overlap(a[i].children, b) {
                return true;
            }
        } else if brings(b, a[i].name) {
            return true;
        }
        i += 1;
    }
    false
}

/// Whether a name already spelled at this level is also brought up by `fields`.
pub const fn brings_any(fields: &[FieldDescriptor], names: &[&str]) -> bool {
    let mut i = 0;
    while i < names.len() {
        if brings(fields, names[i]) {
            return true;
        }
        i += 1;
    }
    false
}

pub trait AmeStateFields: Sized {
    const FIELDS: &'static [FieldDescriptor];
    const VERSION: u32;
    const SCHEMA_HASH: u32;
    const PARENT_PREFIX: &'static str;
    const MIGRATION_DEPS: &'static [&'static str];

    fn load_struct(ctx: &mut MigrationContext) -> StorageResult<Self>;

    fn save_struct(&self, ctx: &mut MigrationContext) -> StorageResult<()>;
}
