use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, ImplItem, ItemFn, ItemImpl, LitStr, Result, Token, Type, PathArguments, GenericArgument};

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

    // Build the body schema reference
    let body_schema = match args.body {
        Some(schema_path) => {
            // schema name from the path (e.g., "SwaggerTestData")
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

    // Dynamic Argument Dependency Extractor for Standalone Handlers
    let mut dependency_resolutions = vec![];
    let mut invocation_args = vec![quote! { ctx }];

    for (i, arg) in input_fn.sig.inputs.iter().enumerate() {
        if i == 0 { continue; } // Skip the base RequestContext positional item
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
            }
        }
    }

    Ok(quote! {
        #input_fn

        #vis fn #wrapper_name(ctx: gritshield::routing::trie::RequestContext) -> gritshield::futures::future::BoxFuture<'static, gritshield::protocol::response::Response> {
            use gritshield::routing::trie::IntoResponse;
            use gritshield::futures::future::FutureExt;

            #(#dependency_resolutions)*

            #fn_name(#(#invocation_args),*).map(|res| res.into_response()).boxed()
        }

        gritshield::inventory::submit! {
            gritshield::routing::trie::AutoRoute {
                path: #path,
                method: gritshield::protocol::request::HttpMethod::#http_method_ident,
                handler: #wrapper_name,
                required_role: #required_role_opt,
                request_body_schema: #body_schema,
            }
        }
    })
}
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
                        let schema_name = schema_path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default();
                        quote! { Some(#schema_name) }
                    }
                    None => quote! { None },
                };

                let http_method_ident = Ident::new(&http_method, fn_name.span());
                let wrapper_name = Ident::new(&format!("{}_wrapper", fn_name), fn_name.span());

                // Build argument dispatch list dynamically based on handler parameters
                let mut dispatch_args = vec![];

                for arg in &method.sig.inputs {
                    if let syn::FnArg::Typed(pat_type) = arg {
                        let arg_type = &pat_type.ty;
                        
                        // Check if the argument is the RequestContext
                        if quote! { #arg_type }.to_string().contains("RequestContext") {
                            dispatch_args.push(quote! { ctx });
                        } else {
                            // Automatically extract the underlying dependency type inside Arc<T> if present
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
                        }
                    }
                }

                let has_self = method.sig.inputs.iter().any(|arg| {
                    if let syn::FnArg::Receiver(_) = arg { true } else { false }
                });

                let invocation = if has_self {
                    quote! {
                        let controller = ::gritshield::core::ioc::CONTEXT.resolve::<#self_ty>().expect(
                            std::concat!("Routing Failure: Controller component '", std::stringify!(#self_ty), "' was not found in the DI pool.")
                        );
                        controller.#fn_name(#(#dispatch_args),*)
                    }
                } else {
                    quote! {
                        #self_ty::#fn_name(#(#dispatch_args),*)
                    }
                };

                inventory_submissions.push(quote! {
                    fn #wrapper_name(ctx: gritshield::routing::trie::RequestContext) -> gritshield::futures::future::BoxFuture<'static, gritshield::protocol::response::Response> {
                        use gritshield::routing::trie::IntoResponse;
                        use gritshield::futures::future::FutureExt;

                        async move {
                            #invocation.await.into_response()
                        }.boxed()
                    }

                    gritshield::inventory::submit! {
                        gritshield::routing::trie::AutoRoute {
                            path: #combined_path,
                            method: gritshield::protocol::request::HttpMethod::#http_method_ident,
                            handler: #wrapper_name,
                            required_role: #required_role_opt,
                            request_body_schema: #body_schema,
                        }
                    }
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

pub fn expand_structural_controller(attr: TokenStream, item: TokenStream) -> Result<TokenStream> {
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

            // Retain only non-routing attributes, extracting the target HTTP method
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
                        let schema_name = schema_path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default();
                        quote! { Some(#schema_name) }
                    }
                    None => quote! { None },
                };

                let http_method_ident = Ident::new(&http_method, fn_name.span());
                let wrapper_name = Ident::new(&format!("{}_wrapper", fn_name), fn_name.span());

                inventory_submissions.push(quote! {
                    fn #wrapper_name(ctx: gritshield::routing::trie::RequestContext) -> gritshield::futures::future::BoxFuture<'static, gritshield::protocol::response::Response> {
                        use gritshield::routing::trie::IntoResponse;
                        use gritshield::futures::future::FutureExt;

                        async move {
                            // Lombok/Spring Style: Resolve the instantiated controller out of DI CONTEXT
                            let controller = ::gritshield::core::ioc::CONTEXT.resolve::<#self_ty>().expect(
                                std::concat!("GritShield Routing Fault: Structural Controller component '", std::stringify!(#self_ty), "' was not instantiated in the DI container.")
                            );
                            
                            // Invoke the handler method on the resolved instance reference
                            controller.#fn_name(ctx).await.into_response()
                        }.boxed()
                    }

                    gritshield::inventory::submit! {
                        gritshield::routing::trie::AutoRoute {
                            path: #combined_path,
                            method: gritshield::protocol::request::HttpMethod::#http_method_ident,
                            handler: #wrapper_name,
                            required_role: #required_role_opt,
                            request_body_schema: #body_schema,
                        }
                    }
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