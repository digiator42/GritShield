use crate::core_parser::unwrap_arc_type;
use proc_macro::TokenStream;
use quote::quote;
use syn::{Attribute, Data, Fields, Meta};
use syn::{DeriveInput, FnArg, Type};
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

                            let (is_arc, inner_type) = unwrap_arc_type(&arg_type);

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

        // Mark as runtime injectable for inventory/dynamic container management
        impl ::gritshield::core::ioc::RuntimeInjectable for #self_ty {}

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
            ::gritshield::core::ioc::ProvidedComponent {
                name: std::stringify!(#self_ty),
                kind: ::gritshield::core::ioc::ComponentKind::Transient,
            }
        }
        #(#edge_submissions)*
    };

    TokenStream::from(expanded)
}

/// Helper to check if a field contains #[grit(skip)] or #[component(skip)]
fn is_field_skipped(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let is_target_attr = attr.path().is_ident("grit") || attr.path().is_ident("component");
        if !is_target_attr {
            return false;
        }

        // Check if attribute is formatted as #[grit(skip)] or #[component(skip)]
        if let Meta::List(meta_list) = &attr.meta {
            if let Ok(nested_ident) = meta_list.parse_args::<syn::Ident>() {
                return nested_ident == "skip";
            }
        }
        false
    })
}

pub fn expand_grit_component(input: DeriveInput) -> TokenStream {
    let name = input.ident;

    let mut dynamic_resolutions = vec![];
    let mut dependency_inner_types = vec![];

    if let Data::Struct(data) = input.data {
        if let Fields::Named(fields) = data.fields {
            for f in fields.named {
                let field_name = f.ident.unwrap();
                let field_type = f.ty;

                // Check if field is marked #[grit(skip)] or #[component(skip)]
                if is_field_skipped(&f.attrs) {
                    dynamic_resolutions.push(quote! {
                        #field_name: unsafe { std::mem::zeroed() }
                    });
                    continue;
                }

                let (is_arc, inner_type) = unwrap_arc_type(&field_type);

                if is_arc {
                    dynamic_resolutions.push(quote! {
                        #field_name: container.resolve::<#inner_type>().expect(
                            std::concat!("Critical DI Fault: Dependency mapping initialization failed for component: '", std::stringify!(#inner_type), "'")
                        )
                    });
                } else {
                    dynamic_resolutions.push(quote! {
                        #field_name: (*container.resolve::<#inner_type>().expect(
                            std::concat!("Critical DI Fault: Dependency mapping initialization failed for component: '", std::stringify!(#inner_type), "'")
                        )).clone()
                    });
                }

                dependency_inner_types.push(inner_type);
            }
        } else {
            panic!("GritComponent derive macro only supports named fields on structs");
        }
    } else {
        panic!("GritComponent derive macro can only be used on Struct definitions");
    }

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

    let register_fn_name = syn::Ident::new(
        &format!("register_and_instantiate_{}", name),
        proc_macro2::Span::call_site(),
    );

    let expanded = quote! {
        impl ::gritshield::core::ioc::RuntimeInjectable for #name {}

        impl ::gritshield::core::ioc::Injectable for #name {
            fn resolve_new(container: &::gritshield::core::ioc::GritContainer) -> Self {
                Self {
                    #(#dynamic_resolutions),*
                }
            }
        }

        #[::gritshield::startup::ctor(unsafe)]
        fn #register_fn_name() {
            ::gritshield::inventory::submit! {
                ::gritshield::core::ioc::AutoRegisterHook {
                    name: std::stringify!(#name),
                    register_fn: |container| {
                        container.register_factory::<#name>(|c| {
                            std::sync::Arc::new(<#name as ::gritshield::core::ioc::Injectable>::resolve_new(c))
                        });
                    }
                }
            }

            let _ = ::gritshield::core::ioc::CONTEXT.resolve::<#name>();
        }

        ::gritshield::inventory::submit! {
            ::gritshield::core::ioc::ProvidedComponent {
                name: std::stringify!(#name),
                kind: ::gritshield::core::ioc::ComponentKind::Transient,
            }
        }
        #(#edge_submissions)*
    };

    TokenStream::from(expanded)
}

// ==============================================================================
// 2. THE NEW STRICT CONTAINER BOILERPLATE GENERATOR
// ==============================================================================
pub fn expand_wire_container(input: DeriveInput) -> TokenStream {
    let name = &input.ident;
    let mut trait_impls = vec![];

    if let Data::Struct(data) = input.data {
        if let Fields::Named(fields) = data.fields {
            for field in fields.named {
                let field_name = field.ident.unwrap();
                let field_type = field.ty;

                let (_is_arc, inner_type) = unwrap_arc_type(&field_type);

                trait_impls.push(quote! {
                    impl ::gritshield::core::ioc::HasComponent<#inner_type> for #name {
                        fn get_component(&self) -> std::sync::Arc<#inner_type> {
                            self.#field_name.clone()
                        }
                    }
                });
            }
        }
    }

    let expanded = quote! {
        impl ::gritshield::core::ioc::StrictContainer for #name {}
        #(#trait_impls)*
    };

    TokenStream::from(expanded)
}
