extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn, LitStr};

/// Internal declarative macro helper to generate the repetitive routing attribute pipelines.
macro_rules! generate_route_macro {
    ($macro_name:ident, $http_method:ident) => {
        #[proc_macro_attribute]
        pub fn $macro_name(attr: TokenStream, item: TokenStream) -> TokenStream {
            let path = parse_macro_input!(attr as LitStr);
            let input_fn = parse_macro_input!(item as ItemFn);
            let fn_name = &input_fn.sig.ident;
            let vis = &input_fn.vis;

            // Generate a unique wrapper identifier matching your 'Handler' type signature
            let wrapper_name = syn::Ident::new(&format!("{}_wrapper", fn_name), fn_name.span());

            let expanded = quote! {
                // Keep the original developer async function intact in the AST
                #input_fn

                // Define the pin wrapper that converts regular async fn into static BoxFutures
                #vis fn #wrapper_name(ctx: gritshield::routing::trie::RequestContext) -> gritshield::futures::future::BoxFuture<'static, gritshield::protocol::response::Response> {
                    Box::pin(#fn_name(ctx))
                }

                // Automatically submit the route metadata into the compile-time inventory registry
                gritshield::inventory::submit! {
                    gritshield::routing::trie::AutoRoute {
                        path: #path,
                        method: gritshield::protocol::request::HttpMethod::$http_method,
                        handler: #wrapper_name
                    }
                }
            };
            TokenStream::from(expanded)
        }
    };
}

generate_route_macro!(get, GET); //
generate_route_macro!(post, POST); //
generate_route_macro!(put, PUT); //
generate_route_macro!(patch, PATCH); //
generate_route_macro!(delete, DELETE); //
