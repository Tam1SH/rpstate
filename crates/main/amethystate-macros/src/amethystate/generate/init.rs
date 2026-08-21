use crate::amethystate::generate::accessors::{field_mode, lookup_chain};
use crate::amethystate::generate::{parse_default, path_literal};
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
    let key = e.stored_name();
    let key_path = path_literal(crate_name, &key);

    if let Some(target) = &e.lookup_node {
        let parent = e.parent.as_ref().expect("lookup_node requires parent");
        let chain = lookup_chain(target, parent);
        let target_span = target.span();
        let target_path = path_literal(crate_name, &target.to_string());
        quote_spanned! {target_span=>
            #fname: {
                const _: fn() = || {
                    fn assert_node_type<T>(_: #crate_name::ReadOnly<T>) {}
                    let _ = || assert_node_type(#chain);
                    let _ = #chain;
                };
                let path = <#parent as #crate_name::StateScope>::PATH.join(&#target_path);
                ::std::sync::Arc::new(<#ty as #crate_name::AmeStateNode>::new_node_with_id(store, &path, instance_id)?)
            }
        }
    } else if let Some(target) = &e.lookup {
        let parent = e.parent.as_ref().expect("lookup requires parent");
        let chain = lookup_chain(target, parent);
        let target_span = target.span();
        let target_path = path_literal(crate_name, &target.to_string());
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
                let path = <#parent as #crate_name::StateScope>::PATH.join(&#target_path);
                #crate_name::store::field_with_path::<#ty, #mode>(store, path, #def, instance_id)?
            }
        }
    } else if e.nested {
        if is_root {
            quote! {
                #fname: ::std::sync::Arc::new(#ty::new_with_id(
                    store,
                    <Self as #crate_name::StateScope>::PATH.join(&#key_path),
                    instance_id
                )?)
            }
        } else {
            quote! {
                #fname: ::std::sync::Arc::new(#ty::new_with_id(
                    store,
                    namespace.join(&#key_path),
                    instance_id
                )?)
            }
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
            #fname: #crate_name::store::reactive_map_with_path_only::<#k, #v, _>(
                store,
                #path_expr,
                #def,
                instance_id
            )?
        }
    } else {
        let raw_def = e
            .default
            .as_ref()
            .expect("Default required for leaf fields");
        let def = parse_default(raw_def);

        let path_expr = if is_root {
            quote! { <Self as #crate_name::StateScope>::PATH.join(&#key_path) }
        } else {
            quote! { namespace.join(&#key_path) }
        };

        if e.volatile {
            quote! { #fname: #crate_name::Field::new_volatile_with_id(#path_expr, #def, instance_id) }
        } else {
            quote! { #fname: #crate_name::store::field_with_path(store, #path_expr, #def, instance_id)? }
        }
    }
}
