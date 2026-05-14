extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn, LitStr};

// --- GET MACRO ---
#[proc_macro_attribute]
pub fn get(attr: TokenStream, item: TokenStream) -> TokenStream {
    let path = parse_macro_input!(attr as LitStr);
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;
    let vis = &input_fn.vis;

    let wrapper_name = syn::Ident::new(&format!("{}_wrapper", fn_name), fn_name.span());

    let expanded = quote! {
        // The original async function from the developer
        #input_fn

        // The wrapper that matches the 'Handler' type signature
        #vis fn #wrapper_name(ctx: RequestContext) -> futures::future::BoxFuture<'static, Response> {
            Box::pin(#fn_name(ctx))
        }

        inventory::submit! {
            crate::routing::trie::AutoRoute {
                path: #path,
                method: crate::protocol::request::HttpMethod::GET,
                handler: #wrapper_name // Register the wrapper
            }
        }
    };
    TokenStream::from(expanded)
}

// --- POST MACRO ---
#[proc_macro_attribute]
pub fn post(attr: TokenStream, item: TokenStream) -> TokenStream {
    let path = parse_macro_input!(attr as LitStr);
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;

    let vis = &input_fn.vis;

    let wrapper_name = syn::Ident::new(&format!("{}_wrapper", fn_name), fn_name.span());

    let expanded = quote! {
        #input_fn

                // The wrapper that matches the 'Handler' type signature
        #vis fn #wrapper_name(ctx: RequestContext) -> futures::future::BoxFuture<'static, Response> {
            Box::pin(#fn_name(ctx))
        }

        inventory::submit! {
            crate::routing::trie::AutoRoute {
                path: #path,
                method: crate::protocol::request::HttpMethod::POST,
                handler: #wrapper_name // Register the wrapper
            }
        }
    };
    TokenStream::from(expanded)
}

// --- PUT MACRO ---
#[proc_macro_attribute]
pub fn put(attr: TokenStream, item: TokenStream) -> TokenStream {
    let path = parse_macro_input!(attr as LitStr);
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;

    let vis = &input_fn.vis;

    let wrapper_name = syn::Ident::new(&format!("{}_wrapper", fn_name), fn_name.span());

    let expanded = quote! {
        #input_fn

                // The wrapper that matches the 'Handler' type signature
        #vis fn #wrapper_name(ctx: RequestContext) -> futures::future::BoxFuture<'static, Response> {
            Box::pin(#fn_name(ctx))
        }

        inventory::submit! {
            crate::routing::trie::AutoRoute {
                path: #path,
                method: crate::protocol::request::HttpMethod::PUT,
                handler: #wrapper_name // Register the wrapper
            }
        }
    };
    TokenStream::from(expanded)
}

// --- PATCH MACRO ---
#[proc_macro_attribute]
pub fn patch(attr: TokenStream, item: TokenStream) -> TokenStream {
    let path = parse_macro_input!(attr as LitStr);
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;

        let vis = &input_fn.vis;

    let wrapper_name = syn::Ident::new(&format!("{}_wrapper", fn_name), fn_name.span());

    let expanded = quote! {
        #input_fn

                // The wrapper that matches the 'Handler' type signature
        #vis fn #wrapper_name(ctx: RequestContext) -> futures::future::BoxFuture<'static, Response> {
            Box::pin(#fn_name(ctx))
        }

        inventory::submit! {
            crate::routing::trie::AutoRoute {
                path: #path,
                method: crate::protocol::request::HttpMethod::PATCH,
                handler: #wrapper_name // Register the wrapper
            }
        }
    };
    TokenStream::from(expanded)
}

// --- DELETE MACRO ---
#[proc_macro_attribute]
pub fn delete(attr: TokenStream, item: TokenStream) -> TokenStream {
    let path = parse_macro_input!(attr as LitStr);
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;
    
        let vis = &input_fn.vis;

    let wrapper_name = syn::Ident::new(&format!("{}_wrapper", fn_name), fn_name.span());

    let expanded = quote! {
        #input_fn

                // The wrapper that matches the 'Handler' type signature
        #vis fn #wrapper_name(ctx: RequestContext) -> futures::future::BoxFuture<'static, Response> {
            Box::pin(#fn_name(ctx))
        }

        inventory::submit! {
            crate::routing::trie::AutoRoute {
                path: #path,
                method: crate::protocol::request::HttpMethod::DELETE,
                handler: #wrapper_name // Register the wrapper
            }
        }
    };
    TokenStream::from(expanded)
}
