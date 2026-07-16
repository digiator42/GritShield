use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{
    GenericArgument, Ident, ImplItem, ItemFn, ItemImpl, LitStr, PathArguments, Result, Token, Type,
};

pub struct RouteArgs {
    pub path: LitStr,
    pub required_role: Option<LitStr>,
    pub body: Option<syn::Path>,
}

impl Parse for RouteArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        let path: LitStr = input.parse()?;
        let mut required_role = None;
        let mut body = None;

        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;

            let key: Ident = input.parse()?;

            if key == "role" || key == "required_role" {
                input.parse::<Token![=]>()?;
                required_role = Some(input.parse::<LitStr>()?);
            } else if key == "body" {
                input.parse::<Token![=]>()?;
                body = Some(input.parse::<syn::Path>()?);
            }
        }

        Ok(RouteArgs {
            path,
            required_role,
            body,
        })
    }
}

/// Helper function to safely extract the internal type T out of Arc<T> signatures
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

pub fn expand_http_method(
    method_name: &str,
    attr: TokenStream,
    item: TokenStream,
) -> Result<TokenStream> {
    let args: RouteArgs = syn::parse2(attr)?;
    let input_fn: ItemFn = syn::parse2(item)?;

    let path = args.path;
    let required_role_opt = match args.required_role {
        Some(lit) => quote! { Some(#lit) },
        None => quote! { None },
    };

    let body_schema = match args.body {
        Some(schema_path) => {
            let schema_name = schema_path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();
            quote! { Some(#schema_name) }
        }
        None => quote! { None },
    };

    let fn_name = &input_fn.sig.ident;
    let vis = &input_fn.vis;
    let wrapper_name = Ident::new(&format!("{}_wrapper", fn_name), fn_name.span());
    let http_method_ident = Ident::new(method_name, fn_name.span());

    // Check if the function is async
    let is_async = input_fn.sig.asyncness.is_some();

    let mut dependency_resolutions = vec![];
    let mut invocation_args = vec![quote! { ctx }];
    let mut dependency_inner_types: Vec<&Type> = vec![];

    for (i, arg) in input_fn.sig.inputs.iter().enumerate() {
        if i == 0 {
            continue;
        }
        if let syn::FnArg::Typed(pat_type) = arg {
            if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                let arg_name = &pat_ident.ident;
                let arg_type = &pat_type.ty;
                let inner_type = extract_inner_arc_type(arg_type).unwrap_or(arg_type);

                dependency_resolutions.push(quote! {
                    let #arg_name = ::gritshield::core::ioc::CONTEXT.resolve::<#inner_type>().expect(
                        std::concat!("DI Error: Missing component '", std::stringify!(#inner_type), "' for standalone router handler context")
                    );
                });
                invocation_args.push(quote! { #arg_name });
                dependency_inner_types.push(inner_type);
            }
        }
    }

    // Record what this handler needs so AutoWire::verify() can catch a missing
    // dependency (e.g. an unregistered PaymentService) at boot instead of the
    // first time a request happens to hit this specific route.
    let edge_submissions = dependency_inner_types.iter().map(|inner_type| {
        quote! {
            gritshield::inventory::submit! {
                gritshield::core::ioc::DependencyEdge {
                    component: std::stringify!(#fn_name),
                    requires: std::stringify!(#inner_type),
                }
            }
        }
    });

    // Generate the handler call based on sync/async
    let handler_call = if is_async {
        quote! {
            #fn_name(#(#invocation_args),*).await.into_response()
        }
    } else {
        quote! {
            #fn_name(#(#invocation_args),*).into_response()
        }
    };

    Ok(quote! {
        #input_fn

        #vis fn #wrapper_name(ctx: gritshield::routing::trie::RequestContext) -> gritshield::futures::future::BoxFuture<'static, gritshield::http::response::Response> {
            use gritshield::routing::trie::IntoResponse;
            use gritshield::futures::future::FutureExt;

            #(#dependency_resolutions)*

            async move {
                #handler_call
            }.boxed()
        }

        gritshield::inventory::submit! {
            gritshield::routing::trie::AutoRoute {
                path: #path,
                method: gritshield::http::request::HttpMethod::#http_method_ident,
                handler: #wrapper_name,
                required_role: #required_role_opt,
                request_body_schema: #body_schema,
            }
        }

        #(#edge_submissions)*
    })
}

/// Consolidated engine driving both controller routing paradigms
pub fn expand_controller(attr: TokenStream, item: TokenStream) -> Result<TokenStream> {
    let base_path_lit: LitStr = syn::parse2(attr)?;
    let base_path = base_path_lit.value();

    let mut input_impl: ItemImpl = syn::parse2(item)?;
    let self_ty = &input_impl.self_ty;

    let mut inventory_submissions = vec![];

    for item in &mut input_impl.items {
        if let ImplItem::Fn(method) = item {
            let fn_name = &method.sig.ident;
            let mut matched_method = None;
            let mut route_args = None;

            method.attrs.retain(|attr| {
                let path = attr.path();
                if path.is_ident("get")
                    || path.is_ident("post")
                    || path.is_ident("put")
                    || path.is_ident("patch")
                    || path.is_ident("delete")
                {
                    matched_method = Some(path.get_ident().unwrap().to_string().to_uppercase());

                    if let Ok(args) = attr.parse_args::<RouteArgs>() {
                        route_args = Some(args);
                    }
                    false
                } else {
                    true
                }
            });

            if let (Some(http_method), Some(args)) = (matched_method, route_args) {
                let sub_path = args.path.value();
                let combined_path = format!("{}{}", base_path, sub_path);

                let required_role_opt = match args.required_role {
                    Some(lit) => quote! { Some(#lit) },
                    None => quote! { None },
                };

                let body_schema = match args.body {
                    Some(schema_path) => {
                        let schema_name = schema_path
                            .segments
                            .last()
                            .map(|s| s.ident.to_string())
                            .unwrap_or_default();
                        quote! { Some(#schema_name) }
                    }
                    None => quote! { None },
                };

                let http_method_ident = Ident::new(&http_method, fn_name.span());
                let wrapper_name = Ident::new(&format!("{}_wrapper", fn_name), fn_name.span());

                // Check if the method is async
                let is_async = method.sig.asyncness.is_some();

                // Build argument dispatch list dynamically based on handler signatures
                let mut dispatch_args = vec![];
                let mut dispatch_dependency_types: Vec<Type> = vec![];

                for arg in &method.sig.inputs {
                    if let syn::FnArg::Typed(pat_type) = arg {
                        let arg_type = &pat_type.ty;

                        if quote! { #arg_type }.to_string().contains("RequestContext") {
                            dispatch_args.push(quote! { ctx });
                        } else {
                            let inner_type = extract_inner_arc_type(arg_type).unwrap_or(arg_type);

                            dispatch_args.push(quote! {
                                ::gritshield::core::ioc::CONTEXT.resolve::<#inner_type>().expect(
                                    std::concat!(
                                        "GritShield Route Dispatch Error: Failed to satisfy parameter dependency '",
                                        std::stringify!(#inner_type),
                                        "' for route handler '",
                                        std::stringify!(#fn_name),
                                        "'."
                                    )
                                )
                            });
                            dispatch_dependency_types.push(inner_type.clone());
                        }
                    }
                }

                let has_self = method
                    .sig
                    .inputs
                    .iter()
                    .any(|arg| matches!(arg, syn::FnArg::Receiver(_)));

                // Maintain specific error strings to fit user-facing debugging preferences
                let missing_controller_msg = std::concat!(
                    "GritShield Routing Fault: Structural Controller component '",
                    std::stringify!(#self_ty),
                    "' was not instantiated in the DI container."
                );

                // Add controller itself as dependency if it has self
                // as a dependency of this handler so a forgotten #[component]/
                // #[derive(GritComponent)] on the controller shows up in verify().
                if has_self {
                    dispatch_dependency_types.push((**self_ty).clone());
                }

                let invocation = if has_self {
                    quote! {
                        let controller = ::gritshield::core::ioc::CONTEXT.resolve::<#self_ty>().expect(
                            #missing_controller_msg
                        );
                        controller.#fn_name(#(#dispatch_args),*)
                    }
                } else {
                    quote! {
                        #self_ty::#fn_name(#(#dispatch_args),*)
                    }
                };

                // Generate the handler call based on sync/async
                let handler_call = if is_async {
                    quote! {
                        #invocation.await.into_response()
                    }
                } else {
                    quote! {
                        #invocation.into_response()
                    }
                };

                let edge_submissions = dispatch_dependency_types.iter().map(|dep_ty| {
                    quote! {
                        gritshield::inventory::submit! {
                            gritshield::core::ioc::DependencyEdge {
                                component: std::stringify!(#fn_name),
                                requires: std::stringify!(#dep_ty),
                            }
                        }
                    }
                });

                inventory_submissions.push(quote! {
                    fn #wrapper_name(ctx: gritshield::routing::trie::RequestContext) -> gritshield::futures::future::BoxFuture<'static, gritshield::http::response::Response> {
                        use gritshield::routing::trie::IntoResponse;
                        use gritshield::futures::future::FutureExt;

                        async move {
                            #handler_call
                        }.boxed()
                    }

                    gritshield::inventory::submit! {
                        gritshield::routing::trie::AutoRoute {
                            path: #combined_path,
                            method: gritshield::http::request::HttpMethod::#http_method_ident,
                            handler: #wrapper_name,
                            required_role: #required_role_opt,
                            request_body_schema: #body_schema,
                        }
                    }

                    #(#edge_submissions)*
                });
            }
        }
    }

    Ok(quote! {
        #input_impl

        const _: () = {
            #(#inventory_submissions)*
        };
    })
}
