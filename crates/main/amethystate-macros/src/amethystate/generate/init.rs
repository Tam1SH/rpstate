use crate::amethystate::generate::{parse_default, path_literal};
use amethystate_macros_core::StoreFieldEntry;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;

/// How this field is stored, when its own type is not what stores it.
///
/// The write half is the throwaway `Serialize` serde derives at a
/// `serialize_with` field, made here because the struct serde would have put it
/// in is never encoded. The read half needs no wrapper: an erased deserializer
/// is a `serde::Deserializer`, so the function serde wants can be called with
/// it directly.
pub(crate) fn stored_as(crate_name: &TokenStream2, e: &StoreFieldEntry) -> Option<TokenStream2> {
    let (writes, reads) = (e.writes_with(), e.reads_with());
    if writes.is_none() && reads.is_none() {
        return None;
    }

    let ty = &e.ty;

    let write = match writes {
        Some(write) => quote! {
            Some({
                fn __ame_write(
                    value: &#ty,
                    then: &mut dyn FnMut(&dyn #crate_name::erased_serde::Serialize)
                        -> #crate_name::StorageResult<()>,
                ) -> #crate_name::StorageResult<()> {
                    struct Wrap<'a>(&'a #ty);

                    impl #crate_name::serde::Serialize for Wrap<'_> {
                        fn serialize<S: #crate_name::serde::Serializer>(
                            &self,
                            serializer: S,
                        ) -> ::std::result::Result<S::Ok, S::Error> {
                            #write(self.0, serializer)
                        }
                    }

                    then(&Wrap(value))
                }

                __ame_write as #crate_name::store::WriteAs<#ty>
            })
        },
        None => quote!(None),
    };

    let read = match reads {
        Some(read) => quote! {
            Some({
                fn __ame_read<'de>(
                    deserializer: &mut dyn #crate_name::erased_serde::Deserializer<'de>,
                ) -> ::std::result::Result<#ty, #crate_name::erased_serde::Error> {
                    #read(deserializer)
                }

                __ame_read as #crate_name::store::ReadAs<#ty>
            })
        },
        None => quote!(None),
    };

    Some(quote! {
        #crate_name::store::StoredAs { write: #write, read: #read }
    })
}

pub(crate) fn init_fields(
    crate_name: &TokenStream2,
    entries: &[StoreFieldEntry],
    is_root: bool,
    on_unreadable: Option<&str>,
    on_delete: Option<&str>,
) -> Vec<TokenStream2> {
    entries
        .iter()
        .map(|e| {
            let unreadable = match variant(e.on_unreadable.as_ref())
                .as_deref()
                .or(on_unreadable)
            {
                Some("UseDefault") => quote!(#crate_name::store::OnUnreadable::UseDefault),
                Some(_) => quote!(#crate_name::store::OnUnreadable::Refuse),
                None => quote!(__ame_on_unreadable),
            };

            let deleted = match variant(e.on_delete.as_ref()).as_deref().or(on_delete) {
                Some("UseDefault") => quote!(#crate_name::store::OnDelete::UseDefault),
                Some(_) => quote!(#crate_name::store::OnDelete::Keep),
                None => quote!(__ame_on_delete),
            };

            init_field(crate_name, e, is_root, &unreadable, &deleted)
        })
        .collect::<Vec<_>>()
}

fn variant(written: Option<&syn::Path>) -> Option<String> {
    written
        .and_then(|path| path.segments.last())
        .map(|segment| segment.ident.to_string())
}

fn init_field(
    crate_name: &TokenStream2,
    e: &StoreFieldEntry,
    is_root: bool,
    unreadable: &TokenStream2,
    deleted: &TokenStream2,
) -> TokenStream2 {
    let fname = e.ident.as_ref().unwrap();
    let ty = &e.ty;
    let key = e.stored_name();
    let key_path = path_literal(crate_name, &key);

    if e.nested {
        let under = match (is_root, e.flatten) {
            (true, false) => quote!(<Self as #crate_name::StateScope>::PATH.join(&#key_path)),
            (true, true) => quote!(<Self as #crate_name::StateScope>::PATH.clone()),
            (false, false) => quote!(namespace.join(&#key_path)),
            (false, true) => quote!(namespace.clone()),
        };

        quote! {
            #fname: ::std::sync::Arc::new(#ty::new_with_id_under(
                store,
                #under,
                instance_id,
                #unreadable,
                #deleted
            )?)
        }
    } else if let Some((k, v)) = e.get_map_types() {
        let def = e
            .default
            .as_ref()
            .map(parse_default)
            .unwrap_or_else(|| quote!(::std::collections::HashMap::new()));

        let path_expr = if is_root {
            quote! { <Self as #crate_name::StateScope>::PATH.join(&#key_path) }
        } else {
            quote! { namespace.join(&#key_path) }
        };

        quote! {
            #fname: #crate_name::store::reactive_map_with_path_only::<#k, #v>(
                store,
                #path_expr,
                #def,
                instance_id
            )?
        }
    } else {
        let def = e
            .default
            .as_ref()
            .map(parse_default)
            .unwrap_or_else(|| quote! { <#ty as ::std::default::Default>::default() });

        let path_expr = if is_root {
            quote! { <Self as #crate_name::StateScope>::PATH.join(&#key_path) }
        } else {
            quote! { namespace.join(&#key_path) }
        };

        if e.volatile {
            quote! { #fname: #crate_name::Field::new_volatile_with_id(#path_expr, #def, instance_id) }
        } else {
            let checked = match &e.check {
                Some(check) => quote_spanned! {check.span()=> .check(#check) },
                None => quote! {},
            };

            let stored_as = match stored_as(crate_name, e) {
                Some(how) => quote! { .stored_as(#how) },
                None => quote! {},
            };

            let rules = quote! {
                #crate_name::store::ReadRules::new()
                    .on_unreadable(#unreadable)
                    .on_delete(#deleted)
                    #checked
                    #stored_as
            };

            quote! { #fname: #crate_name::store::field_with_path_under(store, #path_expr, #def, instance_id, #rules)? }
        }
    }
}
