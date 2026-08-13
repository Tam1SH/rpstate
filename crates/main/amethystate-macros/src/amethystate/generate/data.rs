use super::RpMode;
use crate::amethystate::generate::parse_default;
use amethystate_macros_core::{MacroArgs, StoreFieldEntry};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::Ident;

pub(crate) fn persistent_fields(entries: &[StoreFieldEntry]) -> Vec<&StoreFieldEntry> {
    entries
        .iter()
        .filter(|e| e.lookup.is_none() && e.lookup_node.is_none() && !e.volatile)
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn data_impl(
    crate_name: &TokenStream2,
    vis: &syn::Visibility,
    name: &Ident,
    attrs: &[syn::Attribute],
    prefix: Option<String>,
    entries: &[StoreFieldEntry],
    macro_args: &MacroArgs,
    rp_mode: RpMode,
) -> TokenStream2 {
    let mut p_fields = persistent_fields(entries);

    p_fields.sort_by(|a, b| {
        a.ident
            .as_ref()
            .unwrap()
            .to_string()
            .cmp(&b.ident.as_ref().unwrap().to_string())
    });

    let data_struct_name = format_ident!("{}_Data", name);

    let data_fields = p_fields.iter().map(|e| {
        let fname = e.ident.as_ref().unwrap();
        let ty = &e.ty;
        if e.nested {
            quote! { pub #fname: <#ty<#crate_name::DefaultStore> as #crate_name::AmeState>::Data }
        } else if let Some((k, v)) = e.get_map_types() {
            quote! { pub #fname: ::std::collections::HashMap<#k, #v> }
        } else {
            quote! { pub #fname: #ty }
        }
    });

    let version_val = macro_args.version.unwrap_or(0);

    let field_descriptors = p_fields.iter().map(|e| {
        let fname_str = e.ident.as_ref().unwrap().to_string();
        let ty = &e.ty;
        let type_name = quote!(#ty).to_string().replace(" ", "");

        if e.nested {
            quote! {
                #crate_name::migration::fields::FieldDescriptor {
                    name: #fname_str,
                    type_hash: 0xDEADBEEF ^ < <#ty<#crate_name::DefaultStore> as #crate_name::AmeState>::Data as #crate_name::migration::types::AmeType>::TYPE_HASH,
                    type_name: #type_name,
                }
            }
        } else if let Some((k, v)) = e.get_map_types() {
            quote! {
                #crate_name::migration::fields::FieldDescriptor {
                    name: #fname_str,
                    type_hash: <::std::collections::HashMap<#k, #v> as #crate_name::migration::types::AmeType>::TYPE_HASH,
                    type_name: #type_name,
                }
            }
        } else {
            quote! {
                #crate_name::migration::fields::FieldDescriptor {
                    name: #fname_str,
                    type_hash: <#ty as #crate_name::migration::types::AmeType>::TYPE_HASH,
                    type_name: #type_name,
                }
            }
        }
    });

    let load_fields = p_fields.iter().map(|e| {
        let fname = e.ident.as_ref().unwrap();
        let key = e.key.clone().unwrap_or_else(|| fname.to_string());
        let ty = &e.ty;

        if e.nested {
            quote! {
                #fname: {
                    let mut sub_ctx = ctx.scoped(#key);
                    < <#ty as #crate_name::AmeState>::Data as #crate_name::migration::fields::AmeStateFields>::load_struct(&mut sub_ctx)?
                }
            }
        } else if let Some((k, v)) = e.get_map_types() {
            quote! {
                #fname: ctx.scan_map::<#k, #v>(#key)?
            }
        } else {
            let fallback = e
                .default
                .as_ref()
                .map(parse_default)
                .unwrap_or_else(|| quote! { <#ty as ::std::default::Default>::default() });
            quote! {
                #fname: ctx.get::<#ty>(#key)?.unwrap_or_else(|| #fallback)
            }
        }
    });

    let save_fields = p_fields.iter().map(|e| {
        let fname = e.ident.as_ref().unwrap();
        let key = e.key.clone().unwrap_or_else(|| fname.to_string());

        if e.nested {
            quote! {
                {
                    let mut sub_ctx = ctx.scoped(#key);
                    self.#fname.save_struct(&mut sub_ctx)?;
                }
            }
        } else if e.get_map_types().is_some() {
            quote! {
                for (k, v) in &self.#fname {
                    let full_key = format!("{}.{}", #key, k);
                    ctx.set(&full_key, v)?;
                }
            }
        } else {
            quote! { ctx.set(#key, &self.#fname)?; }
        }
    });

    let store_load_fields = p_fields.iter().map(|e| {
        let fname = e.ident.as_ref().unwrap();
        let key = e.key.clone().unwrap_or_else(|| fname.to_string());
        let ty = &e.ty;
        if e.nested {
            let data_ty = get_data_type(ty);
            quote! {
                #fname: <#data_ty>::__amethystate_load_from(
                    store,
                    &#crate_name::join_path(prefix, #key),
                )?
            }
        } else if let Some((k, v)) = e.get_map_types() {
            quote! {
                #fname: {
                    let path = #crate_name::join_path(prefix, #key);
                    let raw = <S as #crate_name::Store>::scan_prefix(
                        store,
                        &format!("{}.", path),
                    )?;
                    let mut map = ::std::collections::HashMap::<#k, #v>::new();
                    for (stored_path, bytes) in raw {
                        if let Some(k_str) = stored_path.strip_prefix(&format!("{}.", path))
                            && let Ok(kv) = <#k as ::std::str::FromStr>::from_str(k_str)
                        {
                            let vv = <S as #crate_name::Store>::decode::<#v>(
                                store,
                                &bytes,
                            )?;
                            map.insert(kv, vv);
                        }
                    }
                    map
                }
            }
        } else {
            let fallback = e
                .default
                .as_ref()
                .map(parse_default)
                .unwrap_or_else(|| quote! { <#ty as ::std::default::Default>::default() });
            quote! {
                #fname: <S as #crate_name::Store>::get::<#ty>(
                    store,
                    &#crate_name::join_path(prefix, #key),
                )?.unwrap_or_else(|| #fallback)
            }
        }
    });

    let store_save_fields = p_fields.iter().map(|e| {
        let fname = e.ident.as_ref().unwrap();
        let key = e.key.clone().unwrap_or_else(|| fname.to_string());

        if e.nested {
            quote! {
                self.#fname.__amethystate_save_to(store, &#crate_name::join_path(prefix, #key))?;
            }
        } else if e.get_map_types().is_some() {
            quote! {
                {
                    let path = #crate_name::join_path(prefix, #key);
                    for (k, v) in &self.#fname {
                        let full_path = format!("{}.{}", path, k);
                        <S as #crate_name::Store>::set(store, &full_path, v)?;
                    }
                }
            }
        } else {
            quote! {
                <S as #crate_name::Store>::set(
                    &store,
                    &#crate_name::join_path(prefix, #key),
                    &self.#fname,
                )?;
            }
        }
    });

    let fields_for_hash = p_fields
        .iter()
        .map(|e| {
            let fname_str = e.ident.as_ref().unwrap().to_string();
            let ty = &e.ty;
            let field_ty = if e.nested {
                quote! { <#ty<#crate_name::DefaultStore> as #crate_name::AmeState>::Data }
            } else if let Some((k, v)) = e.get_map_types() {
                quote! { ::std::collections::HashMap<#k, #v> }
            } else {
                quote! { #ty }
            };
            (fname_str, field_ty)
        })
        .collect::<Vec<_>>();

    let recursive_hash_expr = crate::hash::gen_recursive_type_hash(crate_name, fields_for_hash);

    let prefix_expr = prefix.clone().unwrap_or_default();
    let deps = migration_deps(crate_name, entries);
    let is_root = prefix.is_some();

    let persistent_wrapper_tokens = match rp_mode {
        RpMode::Reactive => quote! {},
        RpMode::Persistent => {
            quote! {
                #[derive(Clone)]
                #(#attrs)* #vis struct #name<S: #crate_name::Store = #crate_name::DefaultStore> {
                    inner: #data_struct_name,
                    store: S,
                    prefix: ::std::sync::Arc<str>,
                }

                impl<S: #crate_name::Store> ::std::fmt::Debug for #name<S> {
                    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        f.debug_struct(stringify!(#name))
                            .field("inner", &self.inner)
                            .finish()
                    }
                }

                impl<S: #crate_name::Store> ::std::ops::Deref for #name<S> {
                    type Target = #data_struct_name;

                    fn deref(&self) -> &Self::Target {
                        &self.inner
                    }
                }

                impl<S: #crate_name::Store> ::std::ops::DerefMut for #name<S> {
                    fn deref_mut(&mut self) -> &mut Self::Target {
                        &mut self.inner
                    }
                }

                impl<S: #crate_name::Store> #name<S> {
                    pub fn save_lazy(&self) -> #crate_name::StorageResult<()> {
                        self.inner.__amethystate_save_to(&self.store, &self.prefix)
                    }

                    pub fn mutate_lazy(&mut self, f: impl FnOnce(&mut #data_struct_name)) -> #crate_name::StorageResult<()> {
                        f(&mut self.inner);
                        self.save_lazy()
                    }

                    pub fn mutate(&mut self, f: impl FnOnce(&mut #data_struct_name)) -> #crate_name::StorageResult<()> {
                        f(&mut self.inner);
                        self.save()
                    }

                    pub fn save(&self) -> #crate_name::StorageResult<()> {
                        self.save_lazy()?;
                        <S as #crate_name::Store>::flush_prefix(&self.store, &self.prefix)
                    }

                    pub fn load_with(store: &S) -> #crate_name::StorageResult<Self> {
                        Ok(Self {
                            inner: #data_struct_name::__amethystate_load_from(store, #prefix_expr)?,
                            store: store.clone(),
                            prefix: ::std::sync::Arc::from(#prefix_expr),
                        })
                    }
                }

                impl #name<#crate_name::DefaultStore> {
                    pub fn load() -> #crate_name::StorageResult<Self> {
                        let store = #crate_name::global_store();
                        Self::load_with(&store)
                    }
                }
            }
        }
        RpMode::Both => {
            let persisted_struct_name = format_ident!("{}_Persistent", name);
            quote! {
                #[allow(non_camel_case_types)]
                #[derive(Clone)]
                pub struct #persisted_struct_name<S: #crate_name::Store = #crate_name::DefaultStore> {
                    inner: #data_struct_name,
                    store: S,
                    prefix: ::std::sync::Arc<str>,
                }

                impl<S: #crate_name::Store> ::std::fmt::Debug for #persisted_struct_name<S> {
                    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        f.debug_struct(stringify!(#persisted_struct_name))
                            .field("inner", &self.inner)
                            .finish()
                    }
                }

                impl<S: #crate_name::Store> ::std::ops::Deref for #persisted_struct_name<S> {
                    type Target = #data_struct_name;

                    fn deref(&self) -> &Self::Target {
                        &self.inner
                    }
                }

                impl<S: #crate_name::Store> ::std::ops::DerefMut for #persisted_struct_name<S> {
                    fn deref_mut(&mut self) -> &mut Self::Target {
                        &mut self.inner
                    }
                }

                impl<S: #crate_name::Store> #persisted_struct_name<S> {
                    pub fn save_lazy(&self) -> #crate_name::StorageResult<()> {
                        self.inner.__amethystate_save_to(&self.store, &self.prefix)
                    }

                    pub fn mutate_lazy(&mut self, f: impl FnOnce(&mut #data_struct_name)) -> #crate_name::StorageResult<()> {
                        f(&mut self.inner);
                        self.save_lazy()
                    }

                    pub fn mutate(&mut self, f: impl FnOnce(&mut #data_struct_name)) -> #crate_name::StorageResult<()> {
                        f(&mut self.inner);
                        self.save()
                    }

                    pub fn save(&self) -> #crate_name::StorageResult<()> {
                        self.save_lazy()?;
                        <S as #crate_name::Store>::flush_prefix(&self.store, &self.prefix)
                    }
                }

                impl<S: #crate_name::Store> #name<S> {
                    pub fn load_with(store: &S) -> #crate_name::StorageResult<#persisted_struct_name<S>> {
                        Ok(#persisted_struct_name {
                            inner: #data_struct_name::__amethystate_load_from(store, #prefix_expr)?,
                            store: store.clone(),
                            prefix: ::std::sync::Arc::from(#prefix_expr),
                        })
                    }
                }

                impl #name<#crate_name::DefaultStore> {
                    pub fn load() -> #crate_name::StorageResult<#persisted_struct_name<#crate_name::DefaultStore>> {
                        let store = #crate_name::global_store();
                        Self::load_with(&store)
                    }
                }
            }
        }
    };

    let gen_load_save_helpers = !(is_root && matches!(rp_mode, RpMode::Reactive));

    let load_save_helpers = if gen_load_save_helpers {
        quote! {
            #[doc(hidden)]
            pub fn __amethystate_load_from<S: #crate_name::Store>(
                store: &S,
                prefix: &str,
            ) -> #crate_name::StorageResult<Self> {
                Ok(Self {
                    #(#store_load_fields,)*
                })
            }

            #[doc(hidden)]
            pub fn __amethystate_save_to<S: #crate_name::Store>(
                &self,
                store: &S,
                prefix: &str,
            ) -> #crate_name::StorageResult<()> {
                #(#store_save_fields)*
                Ok(())
            }
        }
    } else {
        quote! {}
    };

    // Persistent mode hands this struct to the user, so it derives Debug and
    // field types have to be printable. In reactive mode it only carries the
    // schema, so deriving it there would impose Debug for nothing.
    let data_debug = match rp_mode {
        RpMode::Reactive => quote! {},
        RpMode::Persistent | RpMode::Both => quote! { , Debug },
    };

    quote! {
        #[derive(#crate_name::serde::Serialize, #crate_name::serde::Deserialize, Default, Clone #data_debug)]
        #[serde(crate = "::amethystate::serde")]
        #[doc(hidden)]
        #[allow(non_camel_case_types)]
        pub struct #data_struct_name {
            #(#data_fields,)*
        }

        #persistent_wrapper_tokens

        impl #data_struct_name {
            #load_save_helpers
        }

        impl #crate_name::migration::types::AmeType for #data_struct_name {
            const TYPE_HASH: u32 = #recursive_hash_expr;
            const TYPE_NAME: &'static str = stringify!(#data_struct_name);
        }

       impl #crate_name::migration::fields::AmeStateFields for #data_struct_name {
            const FIELDS: &'static [#crate_name::migration::fields::FieldDescriptor] = &[
                #(#field_descriptors),*
            ];
            const VERSION: u32 = #version_val;
            const SCHEMA_HASH: u32 = #crate_name::migration::types::schema_hash(Self::FIELDS);
            const PARENT_PREFIX: &'static str = #prefix_expr;
            const MIGRATION_DEPS: &'static [&'static str] = &[ #(#deps),* ];

            fn load_struct(ctx: &mut #crate_name::MigrationContext) -> #crate_name::StorageResult<Self> {
                Ok(Self {
                    #(#load_fields,)*
                })
            }

            fn save_struct(&self, ctx: &mut #crate_name::MigrationContext) -> #crate_name::StorageResult<()> {
                #(#save_fields)*
                Ok(())
            }
        }

        impl<S: #crate_name::Store> #crate_name::AmeState for #name<S> {
            type Data = #data_struct_name;
        }
    }
}

fn migration_deps(crate_name: &TokenStream2, entries: &[StoreFieldEntry]) -> Vec<TokenStream2> {
    entries
        .iter()
        .filter_map(|e| e.parent.as_ref())
        .map(|p| quote! { <#p as #crate_name::StateScope>::PREFIX })
        .collect::<Vec<_>>()
}

fn get_data_type(ty: &syn::Type) -> proc_macro2::TokenStream {
    if let syn::Type::Path(type_path) = ty {
        let mut path = type_path.path.clone();
        if let Some(last) = path.segments.last_mut() {
            last.arguments = syn::PathArguments::None;
            last.ident = quote::format_ident!("{}_Data", last.ident);
        }
        quote::quote! { #path }
    } else {
        quote::quote! { #ty }
    }
}
