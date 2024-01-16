use proc_macro2::TokenStream;
use quote::quote;

/// Implements `TryFrom` for an enum of payload.
pub fn impl_try_from_for_payload(input: syn::DeriveInput) -> TokenStream {
    let name = input.ident;
    let implementations = match input.data {
        syn::Data::Enum(ref e) => e
            .variants
            .iter()
            .flat_map(|v| {
                let variant_name = &v.ident;
                println!("variant_name: {:?}", variant_name);

                let fields = match v.fields {
                    syn::Fields::Unnamed(ref f) => &f.unnamed,
                    // ignore other variants
                    _ => return Vec::new(),
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
            .collect::<Vec<proc_macro2::TokenStream>>(),
        _ => panic!("Only enums are supported"),
    };

    let output = quote! {
        #(#implementations)*
    };

    output
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

// write test for impl_try_from_for_payload
#[test]
fn test_should_generate_impl_try_from() {
    let input = syn::parse_quote! {
        enum SudoUserRpcEventPayload {
            RegisterRecord(RegisterRecordPayload),
        }
    };
    let output = impl_try_from_for_payload(input);
    let expected = quote! {
        impl std::convert::TryFrom<SudoUserRpcEventPayload> for RegisterRecordPayload {
            type Error = RuntimeError;

            fn try_from(payload: SudoUserRpcEventPayload) -> Result<Self, Self::Error> {
                match payload {
                    SudoUserRpcEventPayload::RegisterRecord(payload) => Ok(payload),
                    _ => Err(RuntimeError::UnprocessableEntity {
                        name: "payload".to_string(),
                    }),
                }
            }
        }
    };
    assert_eq!(output.to_string(), expected.to_string());
}
