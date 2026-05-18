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

            let wrapper_name = syn::Ident::new(&format!("{}_wrapper", fn_name), fn_name.span());

            let expanded = quote! {
                #input_fn

                // 🎯 Pure Expression mapping using .map() to avoid 'let' namespace leaks
                #vis fn #wrapper_name(ctx: gritshield::routing::trie::RequestContext) -> gritshield::futures::future::BoxFuture<'static, gritshield::protocol::response::Response> {
                    use gritshield::routing::trie::IntoResponse;
                    use gritshield::futures::future::FutureExt;

                    // maps the result via trait, and boxes it instantly
                    #fn_name(ctx).map(|res| res.into_response()).boxed()
                }

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

// Now these will expand flawlessly without any redefinition errors!
generate_route_macro!(get, GET);
generate_route_macro!(post, POST);
generate_route_macro!(put, PUT);
generate_route_macro!(patch, PATCH);
generate_route_macro!(delete, DELETE);
