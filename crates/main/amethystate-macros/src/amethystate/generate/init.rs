use crate::amethystate::generate::accessors::{field_mode, lookup_chain};
use crate::amethystate::generate::parse_default;
use amethystate_macros_core::StoreFieldEntry;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};

pub(crate) fn init_fields(
    crate_name: &TokenStream2,
    entries: &[StoreFieldEntry],
    is_root: bool,
) -> Vec<TokenStream2> {
    entries
        .iter()
        .map(|e| init_field(crate_name, e, is_root))
        .collect::<Vec<_>>()
}

fn init_field(crate_name: &TokenStream2, e: &StoreFieldEntry, is_root: bool) -> TokenStream2 {
    let fname = e.ident.as_ref().unwrap();
    let ty = &e.ty;
    let key = e.key.clone().unwrap_or_else(|| fname.to_string());

    if let Some(target) = &e.lookup_node {
        let parent = e.parent.as_ref().expect("lookup_node requires parent");
        let chain = lookup_chain(target, parent);
        let target_span = target.span();
        let target_str = target.to_string();
        quote_spanned! {target_span=>
            #fname: {
                const _: fn() = || {
                    fn assert_node_type<T>(_: #crate_name::ReadOnly<T>) {}
                    let _ = || assert_node_type(#chain);
                    let _ = #chain;
                };
                let parent_prefix = <#parent<S> as #crate_name::StateScope>::PREFIX;
                let path = if parent_prefix == "." {
                    #target_str.to_string()
                } else {
                    format!("{}.{}", parent_prefix, #target_str)
                };
                ::std::sync::Arc::new(<#ty<S> as #crate_name::AmeStateNode<S>>::new_node_with_id(store, &path, instance_id)?)
            }
        }
    } else if let Some(target) = &e.lookup {
        let parent = e.parent.as_ref().expect("lookup requires parent");
        let chain = lookup_chain(target, parent);
        let target_span = target.span();
        let target_str = target.to_string();
        let def = e
            .default
            .as_ref()
            .map(parse_default)
            .unwrap_or_else(|| quote!(::std::default::Default::default()));

        let mode = field_mode(crate_name, e);
        let write_check = if e.export_mut {
            quote_spanned! { target.span() =>
                fn assert_writable<T>(_: #crate_name::Writable<T>) {}
                assert_writable(#chain);
            }
        } else {
            quote!()
        };

        quote_spanned! { target_span =>
            #fname: {
                const _: fn() = || {
                    #write_check
                    trait TypeCheck<T> {}
                    impl<T> TypeCheck<T> for #crate_name::ReadOnly<T> {}
                    impl<T> TypeCheck<T> for #crate_name::Writable<T> {}
                    fn assert_field_type_matches_lookup<T, M: TypeCheck<T>>(_: M) {}
                    assert_field_type_matches_lookup::<#ty, _>(#chain);
                };
                let parent_prefix = <#parent as #crate_name::StateScope>::PREFIX;
                let path = if parent_prefix == "." {
                    #target_str.to_string()
                } else {
                    format!("{}.{}", parent_prefix, #target_str)
                };
                #crate_name::store::field_with_path::<#ty, _, #mode>(store, ::std::sync::Arc::from(path), #def, instance_id)?
            }
        }
    } else if e.nested {
        if is_root {
            quote! {
                #fname: {
                    let prefix = <Self as #crate_name::StateScope>::PREFIX;
                    let path = if prefix == "." {
                        #key.to_string()
                    } else {
                        format!("{}.{}", prefix, #key)
                    };
                    ::std::sync::Arc::new(#ty::<S>::new_with_id(
                        store,
                        &path,
                        instance_id
                    )?)
                }
            }
        } else {
            quote! { #fname: ::std::sync::Arc::new(#ty::<S>::new_with_id(store, &format!("{}.{}", namespace, #key), instance_id)?) }
        }
    } else if let Some((k, v)) = e.get_map_types() {
        let def = e
            .default
            .as_ref()
            .map(parse_default)
            .unwrap_or_else(|| quote!(::std::collections::HashMap::new()));

        if is_root {
            quote! {
                #fname: #crate_name::store::reactive_map::<Self, #k, #v, S>(store, #key, #def, instance_id)?
            }
        } else {
            quote! {
                #fname: #crate_name::store::reactive_map_with_path_only::<#k, #v, _, _>(
                    store,
                    ::std::sync::Arc::from(format!("{}.{}", namespace, #key)),
                    #def,
                    instance_id
                )?
            }
        }
    } else {
        let raw_def = e
            .default
            .as_ref()
            .expect("Default required for leaf fields");
        let def = parse_default(raw_def);

        let path_expr = if is_root {
            quote! { #key.to_string() }
        } else {
            quote! { format!("{}.{}", namespace, #key) }
        };

        if e.volatile {
            let path_expr = if is_root {
                quote! {
                    {
                        let prefix = <Self as #crate_name::StateScope>::PREFIX;
                        if prefix == "." {
                            #key.to_string()
                        } else {
                            format!("{}.{}", prefix, #key)
                        }
                    }
                }
            } else {
                quote! { format!("{}.{}", namespace, #key) }
            };
            quote! { #fname: #crate_name::Field::new_volatile_with_id(::std::sync::Arc::from(#path_expr), #def, instance_id) }
        } else if is_root {
            quote! { #fname: #crate_name::store::field::<Self, #ty, S>(store, #key, #def, instance_id)? }
        } else {
            quote! { #fname: #crate_name::store::field_with_path(store, ::std::sync::Arc::from(#path_expr), #def, instance_id)? }
        }
    }
}
