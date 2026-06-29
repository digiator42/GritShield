use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Result, LitStr, Token, Ident, ItemFn, ItemImpl, ImplItem};

pub struct RouteArgs {
    pub path: LitStr,
    pub required_role: Option<LitStr>,
}

impl Parse for RouteArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        let path: LitStr = input.parse()?;
        let mut required_role = None;

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;

            let key: Ident = input.parse()?;
            if key == "role" || key == "required_role" {
                input.parse::<Token![=]>()?;
                required_role = Some(input.parse::<LitStr>()?);
            }
        }

        Ok(RouteArgs { path, required_role })
    }
}

pub fn expand_http_method(method_name: &str, attr: TokenStream, item: TokenStream) -> Result<TokenStream> {
    let args: RouteArgs = syn::parse2(attr)?;
    let input_fn: ItemFn = syn::parse2(item)?;

    let path = args.path;
    let required_role_opt = match args.required_role {
        Some(lit) => quote! { Some(#lit) },
        None => quote! { None },
    };

    let fn_name = &input_fn.sig.ident;
    let vis = &input_fn.vis;
    let wrapper_name = Ident::new(&format!("{}_wrapper", fn_name), fn_name.span());
    let http_method_ident = Ident::new(method_name, fn_name.span());

    Ok(quote! {
        #input_fn

        #vis fn #wrapper_name(ctx: gritshield::routing::trie::RequestContext) -> gritshield::futures::future::BoxFuture<'static, gritshield::protocol::response::Response> {
            use gritshield::routing::trie::IntoResponse;
            use gritshield::futures::future::FutureExt;

            #fn_name(ctx).map(|res| res.into_response()).boxed()
        }

        gritshield::inventory::submit! {
            gritshield::routing::trie::AutoRoute {
                path: #path,
                method: gritshield::protocol::request::HttpMethod::#http_method_ident,
                handler: #wrapper_name,
                required_role: #required_role_opt
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

                let http_method_ident = Ident::new(&http_method, fn_name.span());
                let wrapper_name = Ident::new(&format!("{}_wrapper", fn_name), fn_name.span());

                inventory_submissions.push(quote! {
                    fn #wrapper_name(ctx: gritshield::routing::trie::RequestContext) -> gritshield::futures::future::BoxFuture<'static, gritshield::protocol::response::Response> {
                        use gritshield::routing::trie::IntoResponse;
                        use gritshield::futures::future::FutureExt;

                        #self_ty::#fn_name(ctx).map(|res| res.into_response()).boxed()
                    }

                    gritshield::inventory::submit! {
                        gritshield::routing::trie::AutoRoute {
                            path: #combined_path,
                            method: gritshield::protocol::request::HttpMethod::#http_method_ident,
                            handler: #wrapper_name,
                            required_role: #required_role_opt
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