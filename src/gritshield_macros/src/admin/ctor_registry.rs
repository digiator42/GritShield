// src/proc-macro code (e.g., ctor_registry.rs)
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, LitStr};

pub fn generate_registration(
    name: &Ident,
    entity_module: &TokenStream,
    repo_name_lower: &str,
    route_path_literal: &LitStr,
    searchable_literals: &[LitStr],
    is_internal: bool,
) -> TokenStream {
    let initializer_name = Ident::new(
        &format!("init_meta_{}", repo_name_lower),
        proc_macro2::Span::call_site(),
    );

        // Determine the crate root based on detection
    let crate_root = if is_internal {
        quote! { crate }
    } else {
        quote! { ::gritshield }
    };

    quote! {
            #[#crate_root::startup::ctor(unsafe)]
            fn #initializer_name() {
                let table_name: &'static str = #repo_name_lower;

                let route_path: &'static str =
                    Box::leak(::std::string::String::from(#route_path_literal).into_boxed_str());

                let searchable_columns: Vec<&'static str> = vec![
                    #(
                        Box::leak(::std::string::String::from(#searchable_literals).into_boxed_str())
                    ),*
                ];

                let list_handler: #crate_root::database::repository::AdminHandlerFn =
                    ::std::sync::Arc::new(move |ctx| {
                        let table_name = table_name;
                        Box::pin(async move {
                            let db = ctx.db.clone().expect("DB connection missing");
                            let repo = #name {
                                db: (*db).clone(),
                            };

                            #crate_root::gritadmin::main_handler::handle_list(
                                ctx,
                                repo,
                                table_name,
                            )
                            .await
                        })
                    });

                let search_handler: #crate_root::database::repository::AdminHandlerFn =
                    ::std::sync::Arc::new(move |ctx| {
                        let table_name = table_name;
                        Box::pin(async move {
                            let db = ctx.db.clone().expect("DB connection missing");
                            let repo = #name {
                                db: (*db).clone(),
                            };

                            #crate_root::gritadmin::main_handler::handle_search(
                                ctx,
                                repo,
                                table_name,
                            )
                            .await
                        })
                    });

                let delete_handler: #crate_root::database::repository::AdminHandlerFn =
                    ::std::sync::Arc::new(move |ctx| {
                        let table_name = table_name;
                        Box::pin(async move {
                            let db = ctx.db.clone().expect("DB connection missing");
                            let repo = #name {
                                db: (*db).clone(),
                            };

                            #crate_root::gritadmin::main_handler::handle_delete(
                                ctx,
                                repo,
                                table_name,
                            )
                            .await
                        })
                    });

                let patch_handler: #crate_root::database::repository::AdminHandlerFn =
                    ::std::sync::Arc::new(move |ctx| {
                        let table_name = table_name;
                        Box::pin(async move {
                            let db = ctx.db.clone().expect("DB connection missing");
                            let repo = #name {
                                db: (*db).clone(),
                            };

                            #crate_root::gritadmin::main_handler::handle_patch(
                                ctx,
                                repo,
                                table_name,
                            )
                            .await
                        })
                    });

                let advanced_search_handler: #crate_root::database::repository::AdminHandlerFn =
                    ::std::sync::Arc::new(move |ctx| {
                        let table_name = table_name;
                        Box::pin(async move {

                            #crate_root::gritadmin::main_handler::handle_custom_search_viewer(
                                ctx,
                                // table_name,
                            )
                            .await
                        })
                    });

                let detail_handler: #crate_root::database::repository::AdminHandlerFn =
                    ::std::sync::Arc::new(move |ctx| {
                        use #crate_root::routing::trie::IntoResponse;
                        let table_name = table_name;
                        Box::pin(async move {
                            let db = ctx.db.clone().expect("DB connection missing");
                            let repo = #name { db: (*db).clone() };
                            #crate_root::gritadmin::main_handler::handle_detail(
                                ctx,
                                repo,
                                table_name,
                            )
                            .await
                            .into_response()
                        })
                    });

                let bulk_delete_handler: #crate_root::database::repository::AdminHandlerFn =
                    ::std::sync::Arc::new(move |ctx| {
                        use #crate_root::routing::trie::IntoResponse;
                        let table_name = table_name;
                        Box::pin(async move {
                            let db = ctx.db.clone().expect("DB connection missing");
                            let repo = #name { db: (*db).clone() };
                            #crate_root::gritadmin::main_handler::handle_bulk_delete(
                                ctx,
                                repo,
                                table_name,
                            )
                            .await
                            .into_response()
                        })
                    });

                let export_handler: #crate_root::database::repository::AdminHandlerFn =
                    ::std::sync::Arc::new(move |ctx| {
                        let table_name = table_name;
                        Box::pin(async move {
                            let db = ctx.db.clone().expect("DB connection missing");
                            let repo = #name { db: (*db).clone() };
                            #crate_root::gritadmin::main_handler::handle_export(
                                ctx,
                                repo,
                                table_name,
                            )
                            .await
                        })
                    });

                let meta = #crate_root::database::repository::ModelMetadata {
                    table_name,
                    table_slug: #repo_name_lower,
                    route_path,
                    searchable_columns,
                    list_handler,
                    search_handler,
                    delete_handler,
                    patch_handler,
                    advanced_search_handler,
                    detail_handler,
                    bulk_delete_handler,
                    export_handler,
                };

                println!("REGISTERING ADMIN MODEL: {}", table_name);

                #crate_root::database::repository::register_model(table_name, meta);
            }
        }
}
