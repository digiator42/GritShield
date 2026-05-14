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

    let expanded = quote! {
        #input_fn
        inventory::submit! {
            crate::routing::trie::AutoRoute {
                path: #path,
                method: crate::protocol::request::HttpMethod::GET,
                handler: #fn_name
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

    let expanded = quote! {
        #input_fn
        inventory::submit! {
            crate::routing::trie::AutoRoute {
                path: #path,
                method: crate::protocol::request::HttpMethod::POST,
                handler: #fn_name
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

    let expanded = quote! {
        #input_fn
        inventory::submit! {
            crate::routing::trie::AutoRoute {
                path: #path,
                method: crate::protocol::request::HttpMethod::PUT,
                handler: #fn_name
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

    let expanded = quote! {
        #input_fn
        inventory::submit! {
            crate::routing::trie::AutoRoute {
                path: #path,
                method: crate::protocol::request::HttpMethod::PATCH,
                handler: #fn_name
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

    let expanded = quote! {
        #input_fn
        inventory::submit! {
            crate::routing::trie::AutoRoute {
                path: #path,
                method: crate::protocol::request::HttpMethod::DELETE,
                handler: #fn_name
            }
        }
    };
    TokenStream::from(expanded)
}
