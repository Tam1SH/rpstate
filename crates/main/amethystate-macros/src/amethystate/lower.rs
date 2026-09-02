//! Parse tree to model.
//!
//! Every question a generator might have asked is answered here, once: which
//! kind a field is, what it is stored under, what it falls back to. Nothing
//! below this reads `syn` for meaning - only for types it prints back out.
//!
//! Answers no question twice, and stops at nothing: what is wrong is gathered
//! and reported together.

use amethystate_macros_core::{MacroArgs, StoreFieldEntry};
use darling::FromField;
use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::{Data, DataStruct, DeriveInput, Fields};

use super::diagnostics::Diagnostics;
use super::model::{
    At, Field, Mode, OnDelete, OnUnreadable, Placement, Rules, Schema, Shape, StoredAs, Target,
};
use super::serde_said::{self, SerdeSaid};

/// Builds the model, adding what it finds wrong to `found` rather than
/// stopping.
///
/// Only a struct that is not one at all ends it here: a field that would not
/// lower is left out and the rest carry on, so checking still sees them and
/// one compile reports the lot.
pub(crate) fn schema(
    input: &DeriveInput,
    args: &MacroArgs,
    found: &mut Diagnostics,
) -> Result<Schema, syn::Error> {
    let said = match serde_said::read(input) {
        Ok(said) => said,
        Err(e) => {
            found.push(e);
            SerdeSaid::none()
        }
    };

    let Data::Struct(DataStruct {
        fields: Fields::Named(named),
        ..
    }) = &input.data
    else {
        found.at(
            input.ident.span(),
            "amethystate can only be used on structs with named fields: it turns each field into \
             a path, and a field with no name has nothing to be called",
        );
        return Err(syn::Error::new(
            input.ident.span(),
            "amethystate can only be used on structs with named fields",
        ));
    };

    let mut fields = Vec::with_capacity(named.named.len());
    for field in &named.named {
        if let Some(lowered) = lower_field(field, &said, found) {
            fields.push(lowered);
        }
    }

    let prefix = prefix_of(args, found);
    let mode = mode_of(args, found);
    let target = target_of(args, found);
    let rules = rules_of(
        args.on_unreadable.as_ref(),
        args.on_delete.as_ref(),
        args.check.as_ref(),
        found,
    );

    let schema = Schema {
        name: input.ident.clone(),
        vis: input.vis.clone(),
        forwarded: serde_said::without_serde(&input.attrs),
        prefix,
        version: args.version.unwrap_or(0),
        mode,
        target,
        rules,
        fields,
    };

    Ok(schema)
}

fn spanned_path(path: Option<&syn::Path>) -> Option<At<syn::Path>> {
    path.map(|p| At::new(p.clone(), p.span()))
}

fn rules_of(
    on_unreadable: Option<&syn::Path>,
    on_delete: Option<&syn::Path>,
    check: Option<&syn::Path>,
    found: &mut Diagnostics,
) -> Rules {
    Rules {
        on_unreadable: variant(
            on_unreadable,
            &[
                ("Refuse", OnUnreadable::Refuse),
                ("UseDefault", OnUnreadable::UseDefault),
            ],
            "`Refuse` is what happens without one: construction fails and names the path. \
             `UseDefault` takes the declared default and carries on, leaving the stored value \
             where it is for somebody to fix, with `try_get` saying so until they do",
            found,
        ),
        on_delete: variant(
            on_delete,
            &[
                ("UseDefault", OnDelete::UseDefault),
                ("Keep", OnDelete::Keep),
            ],
            "`UseDefault` is what happens without one: the field reports its declared default \
             again. `Keep` goes on reporting the last value it held, which is what a value being \
             drawn wants when something else removed the key",
            found,
        ),
        check: spanned_path(check),
    }
}

/// The variant a rule names, as the value it stands for.
///
/// The name is taken from the last segment, so `UseDefault` and
/// `OnUnreadable::UseDefault` both write the same thing and neither needs the
/// type in scope. A name that is neither is refused here rather than becoming
/// whichever variant a later `match` reaches for.
fn variant<T: Copy>(
    written: Option<&syn::Path>,
    allowed: &[(&str, T)],
    hint: &str,
    found: &mut Diagnostics,
) -> Option<At<T>> {
    let path = written?;
    let named = path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default();

    match allowed.iter().find(|(name, _)| *name == named) {
        Some((_, value)) => Some(At::new(*value, path.span())),
        None => {
            found.at(
                path.span(),
                format!("`{named}` is not one of these. {hint}"),
            );
            None
        }
    }
}

/// The place this schema's fields hang under.
///
/// `as_root` and `prefix` are two ways of saying it, and saying both leaves
/// nothing to decide which won.
fn prefix_of(args: &MacroArgs, found: &mut Diagnostics) -> Option<Placement> {
    match (&args.prefix, args.as_root) {
        (Some(prefix), true) => {
            found.at(
                prefix.span(),
                "`as_root` puts these fields at the top of the store and `prefix` puts them under \
                 a name, so the two cannot both be true. Drop one",
            );
            None
        }
        (Some(prefix), false) => Some(Placement::Under(At::new(
            prefix.as_ref().clone(),
            prefix.span(),
        ))),
        (None, true) => Some(Placement::Root),
        (None, false) => None,
    }
}

fn mode_of(args: &MacroArgs, found: &mut Diagnostics) -> Mode {
    match args.mode.as_deref() {
        None | Some("reactive") => Mode::Reactive,
        Some("persistent") => Mode::Persistent,
        Some("both") => Mode::Both,
        Some(other) => {
            found.at(
                Span::call_site(),
                format!(
                    "`{other}` is not a mode. `reactive` gives fields that watch the store, \
                     `persistent` gives a struct that loads and saves whole, and `both` gives \
                     each of them"
                ),
            );
            Mode::Reactive
        }
    }
}

fn target_of(args: &MacroArgs, found: &mut Diagnostics) -> Target {
    match args.target.as_deref() {
        None | Some("native") => Target::Native,
        Some("tauri-wasm") => Target::TauriWasm,
        Some(other) => {
            found.at(
                Span::call_site(),
                format!(
                    "`{other}` is not a target. `native` builds against a store this process \
                     holds, and `tauri-wasm` against one on the other side of a Tauri command"
                ),
            );
            Target::Native
        }
    }
}

fn lower_field(field: &syn::Field, said: &SerdeSaid, found: &mut Diagnostics) -> Option<Field> {
    let mut entry = match StoreFieldEntry::from_field(field) {
        Ok(entry) => entry,
        Err(e) => {
            found.push(e.into());
            return None;
        }
    };

    let ident = entry.ident.clone()?;

    if let Some(from_serde) = said.of(&ident)
        && let Err(e) = serde_said::fold_into(from_serde, &mut entry)
    {
        found.push(e);
        return None;
    }

    let stored = At::new(
        entry.stored_name(),
        entry
            .key
            .as_ref()
            .map_or_else(|| ident.span(), darling::util::SpannedValue::span),
    );

    let rules = rules_of(
        entry.on_unreadable.as_ref(),
        entry.on_delete.as_ref(),
        entry.check.as_ref(),
        found,
    );

    let lowered = Field {
        shape: shape_of(&entry, found),
        rules,
        vis: entry.vis.clone(),
        ty: entry.ty.clone(),
        ident,
        stored,
    };

    Some(lowered)
}

/// Which of the four kinds this field is.
///
/// Decided once, here, so that everything downstream matches rather than asks.
fn shape_of(entry: &StoreFieldEntry, found: &mut Diagnostics) -> Shape {
    let default = entry.default.as_ref().map(super::generate::parse_default);

    if entry.volatile {
        let named = entry
            .ident
            .as_ref()
            .map_or_else(|| entry.stored_name(), syn::Ident::to_string);

        if entry.nested {
            found.at(
                entry.ty.span(),
                format!(
                    "`{named}` is both `volatile` and `nested`, and the two want opposite things: \
                     `nested` gives a struct whose every field is a path, and `volatile` gives no \
                     path at all. A struct this process holds and never stores is a plain field \
                     of that type"
                ),
            );
        } else if entry.get_map_types().is_some() {
            found.at(
                entry.ty.span(),
                format!(
                    "`{named}` is a `volatile` map, and a `ReactiveMap` is entries under a path: \
                     it is built against the store and has nowhere else to keep them. A map this \
                     process holds and never stores is a plain field over `HashMap`"
                ),
            );
        }

        return Shape::Volatile {
            default: default.unwrap_or_else(|| fallback(&entry.ty)),
        };
    }

    if entry.nested {
        return Shape::Node {
            flattened: entry.flatten,
        };
    }

    if let Some((key, value)) = entry.get_map_types() {
        return Shape::Map {
            key: key.clone(),
            value: value.clone(),
            default,
        };
    }

    let stored_as = match (entry.writes_with(), entry.reads_with()) {
        (None, None) => None,
        (write, read) => Some(StoredAs { write, read }),
    };

    Shape::Leaf {
        default: default.unwrap_or_else(|| fallback(&entry.ty)),
        stored_as,
    }
}

/// What a field with nothing written falls back to.
fn fallback(ty: &syn::Type) -> proc_macro2::TokenStream {
    quote::quote! { <#ty as ::std::default::Default>::default() }
}
