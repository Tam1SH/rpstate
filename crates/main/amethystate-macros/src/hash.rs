use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

/// The hash of a type, from its fields' names and what each of them hashes as.
///
/// The second half of each pair is the hash expression rather than the type,
/// so a field whose stored form is not its type's can say so.
pub fn gen_recursive_type_hash(
    crate_name: &TokenStream2,
    fields: impl IntoIterator<Item = (String, TokenStream2)>,
) -> TokenStream2 {
    let field_hashes = fields.into_iter().map(|(name, hash)| {
        quote! {
            ^ #crate_name::migration::types::fnv1a(#name.as_bytes())
            ^ #hash
        }
    });

    quote! {
        0u32
        #(#field_hashes)*
    }
}
