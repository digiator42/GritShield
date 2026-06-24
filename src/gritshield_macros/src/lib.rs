extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream, Result};
use syn::{parse_macro_input, DeriveInput, Ident, ImplItem, ItemFn, ItemImpl, LitStr, Token};
use syn::{Expr, ExprArray, ExprLit, Lit};

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
            if key == "role" || key == "required_role" {
                input.parse::<Token![=]>()?;
                required_role = Some(input.parse::<LitStr>()?);
            }
        }

        Ok(RouteArgs {
            path,
            required_role,
        })
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

#[proc_macro_attribute]
pub fn controller(attr: TokenStream, item: TokenStream) -> TokenStream {
    // 1. Parse the base prefix route string: #[request("/auth")]
    let base_path_lit = parse_macro_input!(attr as LitStr);
    let base_path = base_path_lit.value();

    // 2. Parse the impl block target
    let mut input_impl = parse_macro_input!(item as ItemImpl);
    let self_ty = &input_impl.self_ty;

    let mut inventory_submissions = vec![];

    // 3. Look through all items inside the impl block
    for item in &mut input_impl.items {
        if let ImplItem::Fn(method) = item {
            let fn_name = &method.sig.ident;
            let mut matched_method = None;
            let mut route_args = None;

            // Check if this method has one of your routing attributes
            method.attrs.retain(|attr| {
                let path = attr.path();
                if path.is_ident("get")
                    || path.is_ident("post")
                    || path.is_ident("put")
                    || path.is_ident("patch")
                    || path.is_ident("delete")
                {
                    matched_method = Some(path.get_ident().unwrap().to_string().to_uppercase());

                    // Parse arguments using your exact pre-existing `RouteArgs` structure!
                    if let Ok(args) = attr.parse_args::<RouteArgs>() {
                        route_args = Some(args);
                    }
                    false // Remove this attribute so standard compiler doesn't throw errors
                } else {
                    true // Keep other attributes (e.g., #[doc])
                }
            });

            // 4. If a routing attribute was intercepted, build an explicit AutoRoute link
            if let (Some(http_method), Some(args)) = (matched_method, route_args) {
                let sub_path = args.path.value();
                // Combine prefix cleanly: e.g., "/auth" + "/login" -> "/auth/login"
                let combined_path = format!("{}{}", base_path, sub_path);

                let required_role_opt = match args.required_role {
                    Some(lit) => quote! { Some(#lit) },
                    None => quote! { None },
                };

                let http_method_ident = syn::Ident::new(&http_method, fn_name.span());
                let wrapper_name = syn::Ident::new(&format!("{}_wrapper", fn_name), fn_name.span());

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

    // 5. Output the rewritten struct block alongside the generated inventory wrappers
    let expanded = quote! {
        #input_impl

        const _: () = {
            #(#inventory_submissions)*
        };
    };

    TokenStream::from(expanded)
}

#[proc_macro_derive(GritRepository, attributes(repository))]
pub fn derive_grit_repository(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let entity_module = quote! { crate::models::user };
    let mut searchable_columns = Vec::new();

    for attr in &input.attrs {
        if attr.path().is_ident("repository") {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("admin_searchable") {
                    let value = meta.value()?;
                    if let Ok(Expr::Array(ExprArray { elems, .. })) = value.parse::<Expr>() {
                        for elem in elems {
                            if let Expr::Lit(ExprLit {
                                lit: Lit::Str(lit_str),
                                ..
                            }) = elem
                            {
                                searchable_columns.push(lit_str.value());
                            }
                        }
                    }
                }
                Ok(())
            });
        }
    }

    let repo_name_lower = name.to_string().replace("Repository", "").to_lowercase();
    let route_path_str = format!("/admin/{}", repo_name_lower);

    let initializer_name = syn::Ident::new(
        &format!("init_meta_{}", repo_name_lower),
        proc_macro2::Span::call_site(),
    );

    let expanded = quote! {
        const _: () = {
            use ::gritshield::deps::sea_orm;

            #[::gritshield::deps::async_trait]
            impl ::gritshield::database::repository::GritRepository for #name {
                type Entity = #entity_module::Entity;
                type Model = #entity_module::Model;
                type Column = #entity_module::Column;
                type ActiveModel = #entity_module::ActiveModel;
                type Id = <<#entity_module::Entity as sea_orm::EntityTrait>::PrimaryKey as sea_orm::PrimaryKeyTrait>::ValueType;

                fn get_db(&self) -> &sea_orm::DatabaseConnection {
                    &self.db
                }

                fn id_column() -> Self::Column {
                    #entity_module::Column::Id
                }

                fn email_column() -> std::option::Option<Self::Column> {
                    std::option::Option::Some(#entity_module::Column::Email)
                }
            }

            impl ::gritshield::database::repository::ConvertFromModel<#entity_module::Model> for #entity_module::ActiveModel {
                fn from_model(model: #entity_module::Model) -> Self {
                    use sea_orm::IntoActiveModel;
                    model.into_active_model()
                }
            }

            // FIXED: Using the local crate's direct ctor dependency instead of going through gritshield!
            #[::ctor::ctor]
            fn #initializer_name() {
                use sea_orm::EntityName;

                let table_name_str = <#entity_module::Entity as sea_orm::EntityName>::table_name(&#entity_module::Entity);

                ::gritshield::database::repository::register_model(
                    table_name_str,
                    ::gritshield::database::repository::ModelMetadata {
                        table_name: table_name_str,
                        route_path: #route_path_str,
                        searchable_columns: vec![ #(#searchable_columns),* ],
                    }
                );
            }
        };
    };

    TokenStream::from(expanded)
}
