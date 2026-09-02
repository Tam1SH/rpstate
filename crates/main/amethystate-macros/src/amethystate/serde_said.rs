use amethystate_macros_core::StoreFieldEntry;
use darling::util::SpannedValue;
use serde_derive_internals::ast::{Container, Data};
use serde_derive_internals::attr::{Default as SerdeDefault, TagType};
use serde_derive_internals::{Ctxt, Derive};
use std::collections::HashMap;
use syn::spanned::Spanned;
use syn::{DeriveInput, Error, Ident};

/// The default serde was given for a field.
pub enum SaidDefault {
    /// `#[serde(default)]`
    Unit,
    /// `#[serde(default = "path")]`
    Path(syn::ExprPath),
}

/// What serde was told about one field, in the vocabulary this macro answers in.
pub struct FieldSaid {
    /// The name serde would store this field under, when it is not the field's
    /// own - `rename` on the field, or `rename_all` on the struct.
    pub renamed_to: Option<String>,
    pub default: Option<SaidDefault>,
    pub flatten: bool,
    pub span: proc_macro2::Span,
}

pub struct SerdeSaid {
    pub fields: HashMap<Ident, FieldSaid>,
}

impl SerdeSaid {
    pub fn of(&self, field: &Ident) -> Option<&FieldSaid> {
        self.fields.get(field)
    }
}

fn refuse(at: proc_macro2::Span, what: &str, why: &str) -> Error {
    Error::new(at, format!("`#[serde({what})]` {why}"))
}

/// Reads the `#[serde(..)]` attributes on a struct whose fields become paths.
///
/// Serde's own parser rather than another one beside it: `rename_all` has
/// already been folded into each field's name by the time this returns, and a
/// contradiction serde itself refuses comes back in serde's words, with serde's
/// spans.
pub fn read(input: &DeriveInput) -> Result<SerdeSaid, Error> {
    let cx = Ctxt::new();
    let container = Container::from_ast(&cx, input, Derive::Deserialize);
    cx.check()?;

    let Some(container) = container else {
        return Ok(SerdeSaid {
            fields: HashMap::new(),
        });
    };

    let at = input
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("serde"))
        .map_or_else(|| input.ident.span(), |attr| attr.span());
    let attrs = &container.attrs;

    if attrs.deny_unknown_fields() {
        return Err(refuse(
            at,
            "deny_unknown_fields",
            "describes one encoded value, and this struct is not one: its fields are separate paths, and a path nobody declared is not a key inside anything. What answers here is `on_unreadable`, per field",
        ));
    }

    match attrs.tag() {
        TagType::External => {}
        TagType::Internal { .. } => {
            return Err(refuse(at, "tag = ..", TAGGING));
        }
        TagType::Adjacent { .. } => {
            return Err(refuse(at, "tag = .., content = ..", TAGGING));
        }
        TagType::None => {
            return Err(refuse(at, "untagged", TAGGING));
        }
    }

    if attrs.transparent() {
        return Err(refuse(
            at,
            "transparent",
            "makes a struct encode as its one field, and this struct encodes as nothing: each field is stored at a path of its own",
        ));
    }

    if attrs.remote().is_some() {
        return Err(refuse(
            at,
            "remote = ..",
            "writes the serde impls for a type from another crate, and this one is yours: the fields here become paths rather than an impl",
        ));
    }

    if input.ident != attrs.name().serialize_name() {
        return Err(refuse(
            at,
            "rename = ..",
            "names the type inside an encoded value, and this type is never encoded. Where its *fields* go is `rename` on each of them, or `rename_all` here",
        ));
    }

    if attrs.expecting().is_some() {
        return Err(refuse(
            at,
            "expecting = ..",
            "is what serde says when a value will not read as this struct, and no value is ever read as this struct: each field arrives from its own path. A value that will not read is `on_unreadable`'s business",
        ));
    }

    if attrs.ser_bound().is_some() || attrs.de_bound().is_some() {
        return Err(refuse(
            at,
            "bound = ..",
            "puts a where clause on the serde impls, and none are written for this struct",
        ));
    }

    for (what, ty) in [
        ("from = ..", attrs.type_from()),
        ("try_from = ..", attrs.type_try_from()),
        ("into = ..", attrs.type_into()),
    ] {
        if ty.is_some() {
            return Err(refuse(
                at,
                what,
                "converts the whole struct on its way through serde, and this struct never goes through serde whole. A conversion of one value belongs on that field's type",
            ));
        }
    }

    let Data::Struct(_, fields) = &container.data else {
        return Ok(SerdeSaid {
            fields: HashMap::new(),
        });
    };

    let mut said = HashMap::new();

    for field in fields {
        let Some(ident) = field.original.ident.clone() else {
            continue;
        };
        let span = field.original.span();
        let attrs = &field.attrs;

        if attrs.skip_serializing() || attrs.skip_deserializing() {
            return Err(Error::new(
                span,
                format!(
                    "`{ident}` is left out of the encoded value by `#[serde(skip)]`, and this struct has no encoded value to be left out of. What a field can be here is `#[amestate(volatile)]`: no path, nothing in the schema, nothing for a migration to carry, and the declared default again on every start. That is more than `skip` asks for, so it is written rather than assumed"
                ),
            ));
        }

        if attrs.skip_serializing_if().is_some() {
            return Err(Error::new(
                span,
                format!(
                    "`{ident}` would be left out of the encoded value when a rule says so, and this struct has no encoded value. Left out here would mean the path is not written - so the value already at it stays, and setting `{ident}` to the very thing the rule names would fail to clear it. An `Option` field is already an absence the store knows how to keep, and `on_delete` says what a path going away does"
                ),
            ));
        }

        if attrs.ser_bound().is_some() || attrs.de_bound().is_some() {
            return Err(Error::new(
                span,
                format!(
                    "`{ident}` puts a where clause on the serde impls, and none are written for this struct"
                ),
            ));
        }

        if attrs.getter().is_some() {
            return Err(Error::new(
                span,
                format!(
                    "`{ident}` is read through a getter, which serde does for a type it does not own. This struct is yours and its fields are read as fields"
                ),
            ));
        }

        if !attrs.borrowed_lifetimes().is_empty() {
            return Err(Error::new(
                span,
                format!(
                    "`{ident}` borrows from the input it is read out of, and there is no such input to borrow from: a value comes back from the engine owned, one path at a time"
                ),
            ));
        }

        if attrs.serialize_with().is_some() || attrs.deserialize_with().is_some() {
            return Err(Error::new(
                span,
                format!(
                    "`{ident}` would encode differently from how its own type encodes, and the store keeps one value per path with nothing beside it to say which of the two is there. A type is stored the way it serialises, so a second form of it is a second type - a newtype whose own `Serialize` does this, which is also the answer for a type from another crate you cannot write one for"
                ),
            ));
        }

        let name = attrs.name();

        // Every field's alias set holds its own read name, put there by serde
        // when the container's `rename_all` rules are folded in. What was
        // written is what is left over.
        if attrs
            .aliases()
            .iter()
            .any(|alias| alias != name.deserialize_name())
        {
            return Err(Error::new(
                span,
                format!(
                    "`{ident}` names another spelling to read, and nothing here reads it: a path is written under one name and looked for under that name. A store written before a rename is moved by a `#[migrate]` step with `#[rename(old => new)]`, which runs at the open and converges - an alias would have to be kept for good instead"
                ),
            ));
        }

        if name.serialize_name() != name.deserialize_name() {
            return Err(Error::new(
                span,
                format!(
                    "`{ident}` is written under `{}` and read under `{}`, and a stored path is one name. Name it once",
                    name.serialize_name(),
                    name.deserialize_name(),
                ),
            ));
        }

        let renamed_to =
            (ident != name.serialize_name()).then(|| name.serialize_name().to_string());

        let default = match attrs.default() {
            SerdeDefault::None => None,
            SerdeDefault::Default => Some(SaidDefault::Unit),
            SerdeDefault::Path(path) => Some(SaidDefault::Path(path.clone())),
        };

        said.insert(
            ident,
            FieldSaid {
                renamed_to,
                default,
                flatten: attrs.flatten(),
                span,
            },
        );
    }

    Ok(SerdeSaid { fields: said })
}

const TAGGING: &str = "says how a variant names itself inside one encoded value. This struct's fields are separate paths, so there is no value for a tag to sit in";

/// Reads what serde was told into the field, where the two say the same thing.
///
/// Said twice is refused naming both, because the two could disagree and
/// nothing would say which won.
pub fn fold_into(said: &FieldSaid, entry: &mut StoreFieldEntry) -> Result<(), Error> {
    let named = entry
        .ident
        .as_ref()
        .map_or_else(|| entry.stored_name(), Ident::to_string);

    if let Some(name) = &said.renamed_to {
        if entry.volatile {
            return Err(Error::new(
                said.span,
                format!(
                    "`{named}` would be stored as `{name}`, and it is stored nowhere: `#[amestate(volatile)]` keeps it in memory and gives it no path. Drop one of the two"
                ),
            ));
        }
        entry.key = Some(SpannedValue::new(name.clone(), said.span));
    }

    if let Some(default) = &said.default {
        if entry.default.is_some() {
            return Err(Error::new(
                said.span,
                format!(
                    "`{named}` has a default twice, from `#[serde(default)]` and from `#[amestate(default)]`. Both are the value for an absence, so write one"
                ),
            ));
        }
        entry.default = Some(match default {
            SaidDefault::Unit => quote::quote!(::core::default::Default::default()),
            SaidDefault::Path(path) => quote::quote!(#path()),
        });
    }

    if said.flatten {
        if !entry.nested {
            return Err(Error::new(
                said.span,
                format!(
                    "`{named}` is stored as one value at one path, and `#[serde(flatten)]` merges what a field holds into what holds it. There is nothing here to merge into: the store hands this value to the codec whole and never looks inside. Flatten belongs on an `#[amestate(nested)]` field, whose own fields are paths"
                ),
            ));
        }

        if let Some(key) = &entry.key {
            return Err(Error::new(
                said.span,
                format!(
                    "`{named}` is both named and unnamed: `#[serde(rename = \"{}\")]` says where it sits, and `#[serde(flatten)]` says it sits nowhere of its own",
                    key.as_ref()
                ),
            ));
        }

        entry.flatten = true;
    }

    Ok(())
}

/// The attributes to forward, which is every one that is not serde's.
///
/// Serde's are read here and answered here; forwarding them puts `#[serde(..)]`
/// on a struct with no serde derive, which the compiler reports as the
/// attribute not existing.
pub fn without_serde(attrs: &[syn::Attribute]) -> Vec<syn::Attribute> {
    attrs
        .iter()
        .filter(|attr| !attr.path().is_ident("serde"))
        .cloned()
        .collect()
}
