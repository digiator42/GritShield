use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, GenericArgument, PathArguments, Result, Type};

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

    for field in fields {
        let field_name = field.ident.unwrap();
        let field_type = field.ty;

        // Safely peel Arc<T> to find the raw inner dependency type
        let inner_type = extract_inner_arc_type(&field_type).unwrap_or(&field_type);

        // Generate: C: ::gritshield::core::ioc::HasComponent<DependencyType>
        trait_bounds.push(quote! {
            C: ::gritshield::core::ioc::HasComponent<#inner_type>
        });

        // Generate: field_name: container.get_component()
        // If the struct expects Arc<T>, grab it. If it expects T, we assume it's cloned or managed.
        // Usually, components are stored as Arc<T> in compile-time containers.
        static_resolutions.push(quote! {
            #field_name: container.get_component()
        });
    }

    Ok(quote! {
        // Formally wire the structural component entirely at compile time.
        // It remains pristine and untouched by any dynamic `RuntimeInjectable` marker!
        impl #name {
            /// Wires dependencies strictly at compile-time.
            /// Will fail to compile if the provided container lacks required dependencies.
            pub fn compile_time_wire<C>(container: &C) -> std::sync::Arc<Self>
            where
                C: ::gritshield::core::ioc::StrictContainer,
                #(#trait_bounds),*
            {
                std::sync::Arc::new(Self {
                    #(#static_resolutions),*
                })
            }
        }
    })
}

/// Helper function to safely extract the internal type T out of Arc<T> signatures[cite: 6]
fn extract_inner_arc_type(ty: &Type) -> Option<&Type> {
    if let Type::Path(type_path) = ty {
        let segment = type_path.path.segments.last()?;
        if segment.ident == "Arc" {
            if let PathArguments::AngleBracketed(args) = &segment.arguments {
                if let Some(GenericArgument::Type(inner_ty)) = args.args.first() {
                    return Some(inner_ty);
                }
            }
        }
    }
    None
}
