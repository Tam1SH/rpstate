use crate::amethystate::generate::path_parts;
use amethystate_macros_core::StoreFieldEntry;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::Ident;
use syn::spanned::Spanned;

/// The stored type of one field, as it appears in the reactive struct.
pub(crate) fn field_type(crate_name: &TokenStream2, e: &StoreFieldEntry) -> TokenStream2 {
    let ty = &e.ty;

    if e.nested {
        quote! { ::std::sync::Arc<#ty> }
    } else if let Some((k, v)) = e.get_map_types() {
        quote! { #crate_name::ReactiveMap<#k, #v> }
    } else {
        quote! { #crate_name::Field<#ty> }
    }
}

pub(crate) fn struct_fields<'a>(
    crate_name: &'a TokenStream2,
    entries: &'a [StoreFieldEntry],
) -> impl Iterator<Item = TokenStream2> + 'a {
    entries.iter().map(move |e| {
        let fname = e.ident.as_ref().unwrap();
        let fvis = &e.vis;
        let ty = field_type(crate_name, e);

        quote! { #fvis #fname: #ty }
    })
}

pub(crate) fn methods<'a>(
    crate_name: &'a TokenStream2,
    entries: &'a [StoreFieldEntry],
) -> impl Iterator<Item = TokenStream2> + 'a {
    entries.iter().map(move |e| {
        let fname = e.ident.as_ref().unwrap();
        let ty = &e.ty;

        if e.nested {
            quote! { pub fn #fname(&self) -> ::std::sync::Arc<#ty> { self.#fname.clone() } }
        } else if let Some((k, v)) = e.get_map_types() {
            quote! {
                pub fn #fname(&self) -> #crate_name::ReactiveMap<#k, #v> {
                    self.#fname.clone()
                }
            }
        } else {
            quote! {
                pub fn #fname(&self) -> #crate_name::Field<#ty> {
                    self.#fname.clone()
                }
            }
        }
    })
}

/// The types this struct's constructor always constructs in turn.
///
/// A `nested` field is built unconditionally, so those are the edges a cycle
/// can run along. Nothing else is: a map recursing through its value type
/// decodes those values rather than constructing them.
fn construction_edges(crate_name: &TokenStream2, entries: &[StoreFieldEntry]) -> Vec<TokenStream2> {
    entries
        .iter()
        .filter(|e| e.nested)
        .map(|e| {
            let ty = &e.ty;
            quote_spanned! {ty.span()=>
                let _: () = <#ty as #crate_name::AmeStateNode>::CONSTRUCTION_TERMINATES;
            }
        })
        .collect()
}

pub(crate) fn node_impl(
    crate_name: &TokenStream2,
    name: &Ident,
    is_root: bool,
    entries: &[StoreFieldEntry],
) -> TokenStream2 {
    let edges = construction_edges(crate_name, entries);
    let terminates = quote! {
        const CONSTRUCTION_TERMINATES: () = { #(#edges)* };
    };
    let force = quote! {
        const _: () = <#name as #crate_name::AmeStateNode>::CONSTRUCTION_TERMINATES;
    };

    if is_root {
        quote! {
            impl #crate_name::AmeStateNode for #name {
                #terminates

                fn new_node(store: &#crate_name::Store, _path: &#crate_name::store::StorePath) -> #crate_name::StorageResult<Self> {
                    Self::new_with(store)
                }

                fn new_node_with_id(store: &#crate_name::Store, _path: &#crate_name::store::StorePath, instance_id: #crate_name::uuid::Uuid) -> #crate_name::StorageResult<Self> {
                    Self::new_with_id(store, instance_id)
                }
            }

            #force
        }
    } else {
        quote! {
            impl #crate_name::AmeStateNode for #name {
                #terminates

                fn new_node(store: &#crate_name::Store, path: &#crate_name::store::StorePath) -> #crate_name::StorageResult<Self> {
                    Self::new(store, path)
                }

                fn new_node_with_id(store: &#crate_name::Store, path: &#crate_name::store::StorePath, instance_id: #crate_name::uuid::Uuid) -> #crate_name::StorageResult<Self> {
                    Self::new_with_id(store, path, instance_id)
                }
            }

            #force
        }
    }
}

pub(crate) fn scope(
    crate_name: &TokenStream2,
    name: &Ident,
    prefix: Option<String>,
) -> Option<TokenStream2> {
    prefix.map(|p| {
        let (segments, joined) = path_parts(&p);
        quote! {
            impl #crate_name::StateScope for #name {
                const PATH: #crate_name::store::StorePath =
                    #crate_name::store::StorePath::from_static(&[#(#segments),*], #joined);
                const KEY: &'static str = #joined;
            }
        }
    })
}

pub(crate) fn constructor(
    crate_name: &TokenStream2,
    is_root: bool,
    init_fields: &[TokenStream2],
) -> TokenStream2 {
    if is_root {
        quote! {
            pub fn new_with(store: &#crate_name::Store) -> #crate_name::StorageResult<Self> {
                Self::new_with_id(store, #crate_name::uuid::Uuid::new_v4())
            }

            pub fn new_with_id(store: &#crate_name::Store, instance_id: #crate_name::uuid::Uuid) -> #crate_name::StorageResult<Self> {
                use #crate_name::{StoreBackend, StoreExt};
                let __amethystate_guard = #crate_name::observability::InstanceGuard::new(
                    instance_id,
                    ::std::any::type_name::<Self>(),
                );
                let result = Self { __amethystate_instance_id: __amethystate_guard, #(#init_fields,)* };
                store.mark_initialized(&<Self as #crate_name::StateScope>::PATH)?;
                Ok(result)
            }
        }
    } else {
        quote! {
            pub fn new(
                store: &#crate_name::Store,
                namespace: impl #crate_name::store::IntoStorePath,
            ) -> #crate_name::StorageResult<Self> {
                Self::new_with_id(store, namespace, #crate_name::uuid::Uuid::new_v4())
            }

            pub fn new_with_id(
                store: &#crate_name::Store,
                namespace: impl #crate_name::store::IntoStorePath,
                instance_id: #crate_name::uuid::Uuid,
            ) -> #crate_name::StorageResult<Self> {
                use #crate_name::{StoreBackend, StoreExt};
                let namespace = #crate_name::store::to_path(namespace)?;
                let __amethystate_guard = #crate_name::observability::InstanceGuard::new(
                    instance_id,
                    ::std::any::type_name::<Self>(),
                );
                let result = Self { __amethystate_instance_id: __amethystate_guard, #(#init_fields,)* };
                store.mark_initialized(&namespace)?;
                Ok(result)
            }
        }
    }
}
