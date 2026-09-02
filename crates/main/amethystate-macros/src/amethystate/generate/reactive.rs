//! What only the watching half of a struct has: subscriptions over every
//! field at once, a copy under a new identity, and a `Debug` that prints what
//! it can.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

use super::accessors;
use crate::amethystate::model::{Schema, Shape};

/// The struct callers hold: an identity, and one watching field per declared
/// field.
///
/// The identity is what tells a change this handle made from one another
/// handle made, so it is part of the struct rather than of any field.
pub(crate) fn declaration(crate_name: &TokenStream2, schema: &Schema) -> TokenStream2 {
    if !schema.mode.watches() {
        return quote! {};
    }

    let (vis, name, attrs) = (&schema.vis, &schema.name, &schema.forwarded);
    let fields = accessors::struct_fields(crate_name, &schema.fields);

    quote! {
        #[derive(Clone)]
        #(#attrs)* #vis struct #name {
            __amethystate_instance_id: ::std::sync::Arc<#crate_name::observability::InstanceGuard>,
            #(#fields,)*
        }
    }
}

/// Everything callers reach through the struct itself.
pub(crate) fn inherent(crate_name: &TokenStream2, schema: &Schema) -> TokenStream2 {
    if !schema.mode.watches() {
        return quote! {};
    }

    let name = &schema.name;
    let constructor = accessors::constructor(crate_name, schema);
    let methods = accessors::methods(crate_name, schema);
    let refused = accessors::refused_marker(schema);
    let forking = fork(crate_name, schema);
    let watching = subscriptions(crate_name, schema);

    quote! {
        impl #name {
            #constructor
            #methods
            #refused
            #forking
            #watching
        }
    }
}

/// `subscribe_all` and `subscribe_all_external`, which reach every field
/// including the ones inside nested structs.
pub(crate) fn subscriptions(crate_name: &TokenStream2, schema: &Schema) -> TokenStream2 {
    if !schema.mode.watches() {
        return quote! {};
    }

    let fields = &schema.fields;

    let each = fields.iter().map(|field| {
        let fname = &field.ident;
        match field.shape {
            Shape::Node { .. } => quote! {
                {
                    let cb_clone = cb.clone();
                    scope.watch_scope(self.#fname.subscribe_all(move || cb_clone()));
                }
            },
            Shape::Map { .. } => quote! {
                {
                    let cb_clone = cb.clone();
                    scope.watch(self.#fname.subscribe_any(move |_| cb_clone()));
                }
            },
            Shape::Leaf { .. } | Shape::Volatile { .. } => quote! {
                {
                    let cb_clone = cb.clone();
                    scope.watch(self.#fname.subscribe(move |_| cb_clone()));
                }
            },
        }
    });

    let each_external = fields.iter().map(|field| {
        let fname = &field.ident;
        match field.shape {
            Shape::Node { .. } => quote! {
                {
                    let cb_clone = cb.clone();
                    scope.watch_scope(self.#fname.subscribe_all_external(move || cb_clone()));
                }
            },
            _ => quote! {
                {
                    let cb_clone = cb.clone();
                    scope.watch(self.#fname.subscription_with().external().register(move |_| cb_clone()));
                }
            },
        }
    });

    quote! {
        pub fn subscribe_all<F>(&self, callback: F) -> #crate_name::ReactiveScope
        where
            F: Fn() + Send + Sync + 'static,
        {
            let cb = ::std::sync::Arc::new(callback);
            let mut scope = #crate_name::ReactiveScope::new();

            #(#each)*

            scope
        }

        pub fn subscribe_all_external<F>(&self, callback: F) -> #crate_name::ReactiveScope
        where
            F: Fn() + Send + Sync + 'static,
        {
            let cb = ::std::sync::Arc::new(callback);
            let mut scope = #crate_name::ReactiveScope::new();

            #(#each_external)*

            scope
        }
    }
}

/// A copy of this struct under a new identity.
///
/// Every field forks in turn, so what comes back watches the same paths and
/// answers to a different id - which is how a change made through one handle
/// is told apart from a change made through another.
pub(crate) fn fork(crate_name: &TokenStream2, schema: &Schema) -> TokenStream2 {
    if !schema.mode.watches() {
        return quote! {};
    }

    let each = schema.fields.iter().map(|field| {
        let fname = &field.ident;
        match field.shape {
            Shape::Node { .. } => {
                quote! { #fname: ::std::sync::Arc::new(self.#fname.fork_with_id(new_id)) }
            }
            _ => quote! { #fname: self.#fname.fork_with_id(new_id) },
        }
    });

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
                #(#each,)*
            }
        }
    }
}

/// `Debug` for the struct, printing each field that can be printed.
///
/// A field whose value does not implement `Debug` is not a reason for the
/// struct to lose its own: the two traits below pick the real one where there
/// is one and `<opaque>` where there is not, by the trick that an inherent
/// method on `&T` is reached only when the one on `T` does not apply.
pub(crate) fn debug(schema: &Schema) -> TokenStream2 {
    if !schema.mode.watches() {
        return quote! {};
    }

    let name = &schema.name;
    let each = schema.fields.iter().map(|field| {
        let fname = &field.ident;
        quote! { .field(stringify!(#fname), (&__AmeW(&self.#fname)).__ame()) }
    });

    quote! {
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
                    #(#each)*
                    .finish()
            }
        }
    }
}
