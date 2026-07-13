use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, GenericArgument, PathArguments, Type, DeriveInput};
use syn::{Data, Fields};
use syn::{ImplItem, ItemImpl, Pat};

pub fn expand_component(input: ItemImpl) -> TokenStream {
    let self_ty = &input.self_ty;

    let mut constructor_found = false;
    let mut dependency_resolutions = vec![];
    let mut constructor_args = vec![];
    let mut constructor_ident = None;
    let mut dependency_inner_types: Vec<Type> = vec![];

    // Look inside the impl block for the constructor method (e.g., `pub fn new(...)`)
    for item in &input.items {
        if let ImplItem::Fn(method) = item {
            if method.sig.ident == "new" {
                constructor_found = true;
                constructor_ident = Some(&method.sig.ident);

                // Parse the constructor's parameters to generate dependency resolution logic
                for arg in &method.sig.inputs {
                    if let FnArg::Typed(pat_type) = arg {
                        if let Pat::Ident(pat_ident) = &*pat_type.pat {
                            let arg_name = &pat_ident.ident;
                            let arg_type = &pat_type.ty;

                            let is_arc = extract_inner_arc_type(arg_type).is_some();
                            // Peek inside Arc<T> to extract T for CONTEXT.resolve::<T>()
                            let inner_type = extract_inner_arc_type(arg_type).unwrap_or(arg_type);

                            dependency_resolutions.push(quote! {
                                let #arg_name = ::gritshield::core::ioc::CONTEXT.resolve::<#inner_type>().expect(
                                    std::concat!(
                                        "Critical Bootstrap DI Fault: Failed to resolve dependency '",
                                        std::stringify!(#inner_type),
                                        "' required by component '",
                                        std::stringify!(#self_ty),
                                        "'",
                                        ", use AutoWire::component() to register it"
                                    )
                                );
                            });

                            if is_arc {
                                constructor_args.push(quote! { #arg_name });
                            } else {
                                // If it's a plain type (like String), clone it out of the resolved Arc<T> automatically!
                                constructor_args.push(quote! { (*#arg_name).clone() });
                            }

                            dependency_inner_types.push(inner_type.clone());
                        }
                    }
                }
                break; // Found our 'new' constructor, stop scanning
            }
        }
    }

    // Fallback error if the developer didn't provide a `new` function
    if !constructor_found {
        panic!(
            "Framework compilation error: #[component] requires an associated `pub fn new(...)` constructor method inside the impl block for '{}'.", 
            quote! { #self_ty }.to_string()
        );
    }

    let edge_submissions = dependency_inner_types.iter().map(|inner_type| {
        quote! {
            ::gritshield::inventory::submit! {
                ::gritshield::core::ioc::DependencyEdge {
                    component: std::stringify!(#self_ty),
                    requires: std::stringify!(#inner_type),
                }
            }
        }
    });

    let expanded = quote! {
        // Keep the original impl block untouched
        #input

        // Submit an automated registration hook to execute at runtime boot phase
        ::gritshield::inventory::submit! {
            ::gritshield::core::ioc::AutoRegisterHook {
                name: std::stringify!(#self_ty),
                register_fn: |container| {
                    // Resolve all necessary constructor inputs out of CONTEXT
                    #(#dependency_resolutions)*

                    // Build the component instance via its native constructor
                    let instance = #self_ty::#constructor_ident(#(#constructor_args),*);

                    // Drop it safely directly into the registry container pool!
                    ::gritshield::core::ioc::CONTEXT.register(instance);
                }
            }
        }

        // Declare this type as available in the graph, and record what it needs,
        // so AutoWire::verify() can catch a missing dependency before boot completes.
        ::gritshield::inventory::submit! {
            ::gritshield::core::ioc::ProvidedComponent { name: std::stringify!(#self_ty) }
        }
        #(#edge_submissions)*
    };

    TokenStream::from(expanded)
}

/// Helper function to safely peek inside Arc<T> types
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

pub fn expand_grit_component(input: DeriveInput) -> TokenStream {
    let name = input.ident;

    // Extract struct fields
    let (fields_resolution, dependency_inner_types): (Vec<_>, Vec<Type>) = match input.data {
        Data::Struct(data) => match data.fields {
            Fields::Named(fields) => fields
                .named
                .into_iter()
                .map(|f| {
                    let field_name = f.ident.unwrap();
                    let field_type = f.ty;

                    // Extract inner type inside Arc<T> to find its registry lookup signature
                    let inner_type = extract_inner_arc_type(&field_type)
                        .cloned()
                        .unwrap_or_else(|| field_type.clone());

                    let resolution = quote! {
                        #field_name: container.resolve::<#inner_type>().expect(
                            std::concat!("Critical DI Fault: Dependency mapping initialization failed for component: '", std::stringify!(#inner_type), "'")
                        )
                    };

                    (resolution, inner_type)
                })
                .unzip(),
            _ => panic!("GritComponent derive macro only supports named fields on structs"),
        },
        _ => panic!("GritComponent derive macro can only be used on Struct definitions"),
    };

    let edge_submissions = dependency_inner_types.iter().map(|inner_type| {
        quote! {
            ::gritshield::inventory::submit! {
                ::gritshield::core::ioc::DependencyEdge {
                    component: std::stringify!(#name),
                    requires: std::stringify!(#inner_type),
                }
            }
        }
    });

    let expanded = quote! {
        // Injectable implementation uses the passed active runtime container context
        impl ::gritshield::core::ioc::Injectable for #name {
            fn resolve_new(container: &::gritshield::core::ioc::GritContainer) -> Self {
                Self {
                    #(#fields_resolution),*
                }
            }
        }

        // Register a factory closure instead of instantiating immediately
        ::gritshield::inventory::submit! {
            ::gritshield::core::ioc::AutoRegisterHook {
                name: std::stringify!(#name),
                register_fn: |container| {
                    container.register_factory::<#name>(|c| {
                        let instance = <#name as ::gritshield::core::ioc::Injectable>::resolve_new(c);
                        std::sync::Arc::new(instance)
                    });
                }
            }
        }

        // Declare this type as available in the graph, and record what its fields
        // need, so AutoWire::verify() can catch a missing dependency before boot.
        ::gritshield::inventory::submit! {
            ::gritshield::core::ioc::ProvidedComponent { name: std::stringify!(#name) }
        }
        #(#edge_submissions)*
    };

    TokenStream::from(expanded)
}