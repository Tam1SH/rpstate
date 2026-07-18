use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

pub fn gen_recursive_type_hash(
    crate_name: &TokenStream2,
    fields: impl IntoIterator<Item = (String, TokenStream2)>,
) -> TokenStream2 {
    let field_hashes = fields.into_iter().map(|(name, ty)| {
        quote! {
            ^ #crate_name::migration::types::fnv1a(#name.as_bytes())
            ^ <#ty as #crate_name::migration::types::AmeType>::TYPE_HASH
        }
    });

    quote! {
        0u32
        #(#field_hashes)*
    }
}
