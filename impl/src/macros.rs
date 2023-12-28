use proc_macro2::TokenStream;
use quote::quote;

pub fn impl_try_from_for_payload(input: syn::DeriveInput) -> TokenStream {
    let name = input.ident;
    let implementations = match input.data {
        syn::Data::Enum(ref e) => e
            .variants
            .iter()
            .map(|v| {
                let variant_name = &v.ident;
                println!("variant_name: {:?}", variant_name);

                let fields = match v.fields {
                    syn::Fields::Unnamed(ref f) => &f.unnamed,
                    _ => panic!("Only unnamed fields are supported"),
                };

                fields
                    .iter()
                    .map(|f| {
                        let field_name;
                        let ft = &f.ty;
                        if let syn::Type::Path(ref p) = ft {
                            println!("p: {:?}", p);
                            if let Some(ident) = p.path.get_ident() {
                                field_name = ident;
                                println!("ident: {:?}", ident);
                            } else {
                                panic!("Only named fields are supported");
                            }
                        } else {
                            panic!("Only named fields are supported");
                        }
                        quote! {
                            impl std::convert::TryFrom<#name> for #field_name {
                                type Error = RuntimeError;

                                fn try_from(payload: #name) -> Result<Self, Self::Error> {
                                    match payload {
                                        #name::#variant_name(payload) => Ok(payload),
                                        _ => Err(RuntimeError::UnprocessableEntity {
                                            name: "payload".to_string(),
                                        }),
                                    }
                                }
                            }
                        }
                    })
                    .collect::<Vec<proc_macro2::TokenStream>>()
            })
            .flatten()
            .collect::<Vec<proc_macro2::TokenStream>>(),
        _ => panic!("Only enums are supported"),
    };

    let output = quote! {
        #(#implementations)*
    };

    output.into()
}

// macro_rules! impl_try_from {
//     ($name:ident, $variant:ident) => {
//         impl std::convert::TryFrom<$name> for $variant {
//             type Error = RuntimeError;
//
//             fn try_from(payload: $name) -> Result<Self, Self::Error> {
//                 match payload.payload {
//                     $name::$variant(payload) => Ok(payload),
//                     _ => Err(RuntimeError::UnprocessableEntity {
//                         name: "event_type".to_string(),
//                     }),
//                 }
//             }
//         }
//     };
// }
