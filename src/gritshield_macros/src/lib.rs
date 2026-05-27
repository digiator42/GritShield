extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn, LitStr, Token, Ident};
use syn::parse::{Parse, ParseStream, Result};

/// Storage container for parsed macro metadata arguments
struct RouteArgs {
    path: LitStr,
    required_role: Option<LitStr>,
}

/// Implement parsing for structural argument formats: 
/// e.g. `"/api/route"` OR `"/api/route", role = "Operator"`
impl Parse for RouteArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        // 1. The first parameter is always the absolute string routing path literal
        let path: LitStr = input.parse()?;
        let mut required_role = None;

        // 2. Check if a trailing comma exists indicating secondary settings attributes
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            
            // Look for specific named parameter blocks, e.g., "role"
            let key: Ident = input.parse()?;
            if key == "role" {
                input.parse::<Token![=]>()?;
                required_role = Some(input.parse::<LitStr>()?);
            }
        }

        Ok(RouteArgs { path, required_role })
    }
}

macro_rules! generate_route_macro {
    ($macro_name:ident, $http_method:ident) => {
        #[proc_macro_attribute]
        pub fn $macro_name(attr: TokenStream, item: TokenStream) -> TokenStream {
            // Parse utilizing our explicit RouteArgs composite layout wrapper
            let args = parse_macro_input!(attr as RouteArgs);
            let input_fn = parse_macro_input!(item as ItemFn);
            
            let path = args.path;
            let required_role_opt = match args.required_role {
                Some(lit) => quote! { Some(#lit) },
                None => quote! { None },
            };

            let fn_name = &input_fn.sig.ident;
            let vis = &input_fn.vis;

            let wrapper_name = syn::Ident::new(&format!("{}_wrapper", fn_name), fn_name.span());

            let expanded = quote! {
                #input_fn

                #vis fn #wrapper_name(ctx: gritshield::routing::trie::RequestContext) -> gritshield::futures::future::BoxFuture<'static, gritshield::protocol::response::Response> {
                    use gritshield::routing::trie::IntoResponse;
                    use gritshield::futures::future::FutureExt;

                    #fn_name(ctx).map(|res| res.into_response()).boxed()
                }

                gritshield::inventory::submit! {
                    gritshield::routing::trie::AutoRoute {
                        path: #path,
                        method: gritshield::protocol::request::HttpMethod::$http_method,
                        handler: #wrapper_name,
                        required_role: #required_role_opt // Automatically compiled and linked
                    }
                }
            };
            TokenStream::from(expanded)
        }
    };
}

generate_route_macro!(get, GET);
generate_route_macro!(post, POST);
generate_route_macro!(put, PUT);
generate_route_macro!(patch, PATCH);
generate_route_macro!(delete, DELETE);