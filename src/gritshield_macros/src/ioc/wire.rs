use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, Result};
use crate::core_parser::unwrap_arc_type;

pub fn expand_grit_wire(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;

    // Ensure we are working with a struct
    let fields = match input.data {
        Data::Struct(data_struct) => match data_struct.fields {
            Fields::Named(fields_named) => fields_named.named,
            _ => {
                return Err(Error::new_spanned(
                    name,
                    "GritWire can only be derived on structs with named fields.",
                ))
            }
        },
        _ => {
            return Err(Error::new_spanned(
                name,
                "GritWire can only be derived on structs.",
            ))
        }
    };

    let mut trait_bounds = vec![];
    let mut static_resolutions = vec![];
    let mut bindings = vec![];

    for field in fields {
        let field_name = field.ident.unwrap();
        let field_type = field.ty;

        let (is_arc, inner_type) = unwrap_arc_type(&field_type);

        trait_bounds.push(quote! {
            C: ::gritshield::core::ioc::HasComponent<#inner_type>
        });

        // Bind every component through an explicitly typed local before the
        // struct literal. Calling `container.get_component()` inline is
        // ambiguous as soon as `C` has more than one `HasComponent<_>` bound:
        // nothing in the expected type of a non-Arc field tells the compiler
        // which trait's method to pick, and the derive failed with
        // "type annotations needed" for any struct mixing Arc and plain fields.
        let binding = quote::format_ident!("__wired_{}", field_name);
        bindings.push(quote! {
            let #binding: ::std::sync::Arc<#inner_type> =
                <C as ::gritshield::core::ioc::HasComponent<#inner_type>>::get_component(container);
        });

        if is_arc {
            static_resolutions.push(quote! {
                #field_name: #binding
            });
        } else {
            static_resolutions.push(quote! {
                #field_name: (*#binding).clone()
            });
        }
    }

    Ok(quote! {
        // Formally wire the structural component entirely at compile time.
        // It remains pristine and untouched by any dynamic `RuntimeInjectable` marker!
        impl #name {
            /// Wires dependencies strictly at compile-time.
            /// Will fail to compile if the provided container lacks required dependencies.
            pub fn wire<C>(container: &C) -> std::sync::Arc<Self>
            where
                C: ::gritshield::core::ioc::StrictContainer,
                #(#trait_bounds),*
            {
                #(#bindings)*
                std::sync::Arc::new(Self {
                    #(#static_resolutions),*
                })
            }
        }
    })
}
