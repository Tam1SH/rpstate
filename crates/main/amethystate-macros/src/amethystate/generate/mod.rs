mod accessors;
mod data;
mod init;
mod wasm;

use crate::ts_mapping::map_type_to_ts;
use amethystate_macros_core::{MacroArgs, StoreFieldEntry, get_type_ident_str};
use proc_macro2::{Delimiter, TokenStream as TokenStream2, TokenTree};
use quote::format_ident;
use quote::quote;
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, Ident, Token, Visibility};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RpMode {
    Reactive,
    Persistent,
    Both,
}

pub fn generate_code(
    crate_name: TokenStream2,
    vis: &Visibility,
    name: &Ident,
    attrs: &[Attribute],
    prefix: Option<String>,
    entries: &[StoreFieldEntry],
    macro_args: MacroArgs,
) -> TokenStream2 {
    let rp_mode = match macro_args.mode.as_deref() {
        None | Some("reactive") => RpMode::Reactive,
        Some("persistent") => RpMode::Persistent,
        Some("both") => RpMode::Both,
        Some(other) => {
            let err = format!(
                "invalid amethystate mode: \"{}\". Expected one of: \"reactive\", \"persistent\", \"both\"",
                other
            );
            return syn::Error::new(proc_macro2::Span::call_site(), err).to_compile_error();
        }
    };

    if macro_args.target.as_deref() == Some("tauri-wasm") {
        return wasm::generate_wasm_code(crate_name, vis, name, attrs, prefix, entries, macro_args);
    }

    let is_root = prefix.is_some();
    let schema_methods = accessors::schema_methods(&crate_name, entries);
    let fields_impl = data::data_impl(
        &crate_name,
        vis,
        name,
        attrs,
        prefix.clone(),
        entries,
        &macro_args,
        rp_mode,
    );

    let struct_fields = accessors::struct_fields(&crate_name, entries);
    let init_fields = init::init_fields(&crate_name, entries, is_root);
    let node_impl = accessors::node_impl(&crate_name, name, is_root);
    let methods = accessors::methods(&crate_name, entries);
    let scope = accessors::scope(&crate_name, name, prefix.clone());
    let constructor = accessors::constructor(&crate_name, is_root, &init_fields);

    let schema_export =
        generate_schema_export(&crate_name, name, &prefix, macro_args.version, entries);

    let inherent_subs = if matches!(rp_mode, RpMode::Reactive | RpMode::Both) {
        let sub_all_fields = entries.iter().map(|e| {
            let fname = e.ident.as_ref().unwrap();
            if e.nested || e.lookup_node.is_some() {
                quote! {
                    {
                        let cb_clone = cb.clone();
                        scope.watch_scope(self.#fname.subscribe_all(move || cb_clone()));
                    }
                }
            } else if e.get_map_types().is_some() {
                quote! {
                    {
                        let cb_clone = cb.clone();
                        scope.watch(self.#fname.subscribe_any(move |_| cb_clone()));
                    }
                }
            } else {
                quote! {
                    {
                        let cb_clone = cb.clone();
                        scope.watch(self.#fname.subscribe(move |_| cb_clone()));
                    }
                }
            }
        });

        let sub_all_fields_ext = entries.iter().map(|e| {
            let fname = e.ident.as_ref().unwrap();
            if e.nested || e.lookup_node.is_some() {
                quote! {
                    {
                        let cb_clone = cb.clone();
                        scope.watch_scope(self.#fname.subscribe_all_external(move || cb_clone()));
                    }
                }
            } else {
                quote! {
                    {
                        let cb_clone = cb.clone();
                        scope.watch(self.#fname.subscription_with().external().register(move |_| cb_clone()));
                    }
                }
            }
        });

        quote! {
            pub fn subscribe_all<F>(&self, callback: F) -> #crate_name::ReactiveScope
            where
                F: Fn() + Send + Sync + 'static,
            {
                let cb = ::std::sync::Arc::new(callback);
                let mut scope = #crate_name::ReactiveScope::new();

                #(#sub_all_fields)*

                scope
            }

            pub fn subscribe_all_external<F>(&self, callback: F) -> #crate_name::ReactiveScope
            where
                F: Fn() + Send + Sync + 'static,
            {
                let cb = ::std::sync::Arc::new(callback);
                let mut scope = #crate_name::ReactiveScope::new();

                #(#sub_all_fields_ext)*

                scope
            }
        }
    } else {
        quote! {}
    };

    let slice_impl = if is_root {
        let load_fn = match rp_mode {
            RpMode::Persistent => quote! { Self::load_with(store) },
            _ => quote! { Self::new_with(store) },
        };

        let slice_subs = if matches!(rp_mode, RpMode::Reactive | RpMode::Both) {
            quote! {
                fn subscribe_all<F>(&self, callback: F) -> #crate_name::ReactiveScope
                where
                    F: Fn() + Send + Sync + 'static,
                {
                    self.subscribe_all(callback)
                }

                fn subscribe_all_external<F>(&self, callback: F) -> #crate_name::ReactiveScope
                where
                    F: Fn() + Send + Sync + 'static,
                {
                    self.subscribe_all_external(callback)
                }
            }
        } else {
            quote! {
                fn subscribe_all<F>(&self, _callback: F) -> #crate_name::ReactiveScope
                where
                    F: Fn() + Send + Sync + 'static,
                {
                    #crate_name::ReactiveScope::new()
                }

                fn subscribe_all_external<F>(&self, _callback: F) -> #crate_name::ReactiveScope
                where
                    F: Fn() + Send + Sync + 'static,
                {
                    #crate_name::ReactiveScope::new()
                }
            }
        };

        quote! {
            impl #crate_name::AmeStateSlice for #name {
                fn load_slice(store: &#crate_name::Store) -> #crate_name::StorageResult<Self> {
                    #load_fn
                }

                #slice_subs
            }
        }
    } else {
        quote! {}
    };

    let global_new_impl = if is_root {
        quote! {
            impl #name {
                pub fn new() -> #crate_name::StorageResult<Self> {
                    let store = #crate_name::global_store();
                    Self::new_with(&store)
                }
            }
        }
    } else {
        quote! {}
    };

    let fork_fields = entries.iter().map(|e| {
        let fname = e.ident.as_ref().unwrap();
        if e.nested || e.lookup_node.is_some() {
            quote! { #fname: ::std::sync::Arc::new(self.#fname.fork_with_id(new_id)) }
        } else {
            quote! { #fname: self.#fname.fork_with_id(new_id) }
        }
    });

    let fork_impl = if matches!(rp_mode, RpMode::Reactive | RpMode::Both) {
        quote! {
            pub fn fork(&self) -> Self {
                self.fork_with_id(#crate_name::uuid::Uuid::new_v4())
            }

            #[doc(hidden)]
            pub fn fork_with_id(&self, new_id: #crate_name::uuid::Uuid) -> Self {
                Self {
                    __amethystate_instance_id: #crate_name::observability::InstanceGuard::new(
                        new_id,
                        ::std::any::type_name::<Self>(),
                    ),
                    #(#fork_fields,)*
                }
            }
        }
    } else {
        quote! {}
    };

    // Bound per field, so a field type that cannot be printed leaves the struct
    // without Debug instead of failing to compile.
    // Autoref specialisation: a field whose type is Debug prints itself, one
    // that is not prints a placeholder. Without it the bound would be fully
    // concrete and rustc would demand Debug from every field type.
    let debug_fields_auto = entries.iter().map(|e| {
        let fname = e.ident.as_ref().unwrap();
        quote! { .field(stringify!(#fname), (&__AmeW(&self.#fname)).__ame()) }
    });

    match rp_mode {
        RpMode::Reactive | RpMode::Both => {
            quote! {
                #[derive(Clone)]
                #(#attrs)* #vis struct #name {
                    __amethystate_instance_id: ::std::sync::Arc<#crate_name::observability::InstanceGuard>,
                    #(#struct_fields,)*
                }

                impl ::std::fmt::Debug for #name {
                    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        struct __AmeOpaque;
                        impl ::std::fmt::Debug for __AmeOpaque {
                            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                                f.write_str("<opaque>")
                            }
                        }
                        struct __AmeW<'a, T>(&'a T);
                        trait __AmeViaDebug {
                            fn __ame(&self) -> &dyn ::std::fmt::Debug;
                        }
                        impl<'a, T: ::std::fmt::Debug> __AmeViaDebug for __AmeW<'a, T> {
                            fn __ame(&self) -> &dyn ::std::fmt::Debug { self.0 }
                        }
                        trait __AmeViaFallback {
                            fn __ame(&self) -> &dyn ::std::fmt::Debug;
                        }
                        impl<'a, T> __AmeViaFallback for &__AmeW<'a, T> {
                            fn __ame(&self) -> &dyn ::std::fmt::Debug { &__AmeOpaque }
                        }
                        f.debug_struct(stringify!(#name))
                            #(#debug_fields_auto)*
                            .finish()
                    }
                }
                #scope
                impl #name {
                    #constructor #(#schema_methods)* #(#methods)*
                    #fork_impl
                    #inherent_subs
                }
                #global_new_impl
                #node_impl
                #fields_impl
                #schema_export
                #slice_impl
            }
        }
        RpMode::Persistent => {
            quote! {
                #scope
                #fields_impl
                #schema_export
                #slice_impl
            }
        }
    }
}

fn generate_schema_export(
    crate_name: &TokenStream2,
    name: &Ident,
    prefix: &Option<String>,
    version: Option<u32>,
    entries: &[StoreFieldEntry],
) -> TokenStream2 {
    let struct_name_str = name.to_string();
    let prefix_tokens = match prefix {
        Some(p) => quote! { Some(#p) },
        None => quote! { None },
    };

    let field_metas = entries.iter().map(|e| {
        let fname = e.ident.as_ref().unwrap();
        let fname_str = fname.to_string();
        let (ts_type, full_ts_type) = map_type_to_ts(e.ty.clone());

        let ty = &e.ty;
        let rust_type_str = quote!(#ty).to_string();

        let kind_tokens = if e.volatile {
            quote! { #crate_name::tauri::FieldKind::Volatile }
        } else if e.nested {
            let sname = get_type_ident_str(&e.ty);
            quote! { #crate_name::tauri::FieldKind::Nested { struct_name: #sname } }
        } else if let Some(target) = &e.lookup {
            let target_str = target.to_string();
            let mutable = e.export_mut;
            quote! { #crate_name::tauri::FieldKind::Lookup { target_key: #target_str, mutable: #mutable } }
        } else if let Some(target) = &e.lookup_node {
            let target_str = target.to_string();
            let sname = get_type_ident_str(&e.ty);
            quote! { #crate_name::tauri::FieldKind::LookupNode { target_prefix: #target_str, struct_name: #sname } }
        } else if let Some((k, v)) = e.get_map_types() {
            let k_ts = map_type_to_ts(k.clone()).1;
            let v_ts = map_type_to_ts(v.clone()).1;
            let k_rust = quote!(#k).to_string();
            let v_rust = quote!(#v).to_string();
            quote! {
                #crate_name::tauri::FieldKind::ReactiveMap {
                    key_type: #k_ts,
                    value_type: #v_ts,
                    key_rust_type: #k_rust,
                    value_rust_type: #v_rust,
                }
            }
        } else {
            quote! { #crate_name::tauri::FieldKind::Plain }
        };

        quote! {
            #crate_name::tauri::FieldExportMeta {
                name: #fname_str,
                ts_type: #ts_type,
                full_ts_type: #full_ts_type,
                rust_type: #rust_type_str,
                kind: #kind_tokens,
            }
        }
    });

    let data_struct_name = format_ident!("{}_Data", name);
    let version_val = version.unwrap_or(0);

    let schema_entry = quote! {
        #crate_name::inventory::submit! {
            #crate_name::observability::SchemaEntry {
                prefix: #prefix_tokens,
                struct_name: #struct_name_str,
                version: #version_val,
                schema_hash: <#data_struct_name as #crate_name::migration::types::AmeType>::TYPE_HASH,
                fields: <#data_struct_name as #crate_name::migration::fields::AmeStateFields>::FIELDS,
            }
        }
    };

    let tauri_entry = if cfg!(feature = "tauri") {
        quote! {
            #crate_name::inventory::submit! {
                #crate_name::tauri::SchemaExportEntry {
                    prefix: #prefix_tokens,
                    struct_name: #struct_name_str,
                    fields: &[
                        #(#field_metas),*
                    ],
                }
            }
        }
    } else {
        quote!()
    };

    quote! {
        #schema_entry
        #tauri_entry
    }
}

struct MapEntry {
    key: Expr,
    _colon: Token![:],
    value: Expr,
}

impl Parse for MapEntry {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(MapEntry {
            key: input.parse()?,
            _colon: input.parse()?,
            value: input.parse()?,
        })
    }
}

pub(crate) fn parse_default(tokens: &TokenStream2) -> TokenStream2 {
    let mut iter = tokens.clone().into_iter();

    match iter.next() {
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Bracket => {
            let content = g.stream();
            quote! { vec![#content] }
        }
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
            let content = g.stream();

            if content.is_empty() {
                return quote! { ::std::collections::HashMap::default() };
            }

            let parser = Punctuated::<MapEntry, Token![,]>::parse_terminated;
            if let Ok(pairs) = parser.parse2(content)
                && !pairs.is_empty()
            {
                let inserts = pairs.iter().map(|pair| {
                    let k = &pair.key;
                    let v = &pair.value;
                    quote! { __map.insert(::std::convert::Into::into(#k), #v); }
                });

                return quote! {
                    {
                        let mut __map = ::std::collections::HashMap::default();
                        #( #inserts )*
                        __map
                    }
                };
            }

            tokens.clone()
        }
        _ => tokens.clone(),
    }
}
