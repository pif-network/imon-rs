extern crate proc_macro;
extern crate syn;

use proc_macro::TokenStream;
mod macros;

#[allow(unused_variables)]
#[proc_macro_derive(TryFromPayload)]
pub fn derive_try_from_payload(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    macros::impl_try_from_for_payload(input).into()
}
