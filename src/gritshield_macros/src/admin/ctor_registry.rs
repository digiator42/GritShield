// src/proc-macro code (e.g., ctor_registry.rs)
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, LitStr};

pub fn generate_registration(
    name: &Ident,
    entity_module: &TokenStream,
    route_slug: &str,
    route_path_lit: &LitStr,
    searchable_literals: &[LitStr],
    is_internal: bool,
) -> TokenStream {
    let initializer_name = Ident::new(
        &format!("init_meta_{}", route_slug),
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
            use #crate_root::deps::sea_orm::EntityName;

            let table_name_str = <#entity_module::Entity as EntityName>::table_name(&#entity_module::Entity);
            let table_name: &'static str = Box::leak(table_name_str.to_string().replace("_", "-").into_boxed_str());  // kebab route

            let table_slug: &'static str = #route_slug;

            // Supplied by the caller, which derives it from the repository name
            // -- the same singular slug every handler uses to build its own
            // links. Deriving it here from `table_name` instead registers the
            // route at `/admin/products` while the grid points its search,
            // export, update-cell and delete links at `/admin/product`, and the
            // difference shows up as a 404 on the first cell edit.
            let route_path: &'static str = #route_path_lit;

            let searchable_columns: Vec<&'static str> = vec![
                #(
                    Box::leak(::std::string::String::from(#searchable_literals).into_boxed_str())
                ),*
            ];

            let list_handler: #crate_root::database::repository::registry::AdminHandlerFn =
                ::std::sync::Arc::new(move |ctx| {
                    let table_name = table_name;
                    Box::pin(async move {
                        let db = ctx.db.clone().expect("DB connection missing");
                        let repo = #name {
                            db: (*db).clone(),
                        };

                        #crate_root::gritadmin::dashboard::handle_list(
                            ctx,
                            repo,
                            table_name,
                        )
                        .await
                    })
                });

            let search_handler: #crate_root::database::repository::registry::AdminHandlerFn =
                ::std::sync::Arc::new(move |ctx| {
                    let table_name = table_name;
                    Box::pin(async move {
                        let db = ctx.db.clone().expect("DB connection missing");
                        let repo = #name {
                            db: (*db).clone(),
                        };

                        #crate_root::gritadmin::dashboard::handle_search(
                            ctx,
                            repo,
                            table_name,
                        )
                        .await
                    })
                });

            let delete_handler: #crate_root::database::repository::registry::AdminHandlerFn =
                ::std::sync::Arc::new(move |ctx| {
                    let table_name = table_name;
                    Box::pin(async move {
                        let db = ctx.db.clone().expect("DB connection missing");
                        let repo = #name {
                            db: (*db).clone(),
                        };

                        #crate_root::gritadmin::dashboard::handle_delete(
                            ctx,
                            repo,
                            table_name,
                        )
                        .await
                    })
                });

            let patch_handler: #crate_root::database::repository::registry::AdminHandlerFn =
                ::std::sync::Arc::new(move |ctx| {
                    let table_name = table_name;
                    Box::pin(async move {
                        let db = ctx.db.clone().expect("DB connection missing");
                        let repo = #name {
                            db: (*db).clone(),
                        };

                        #crate_root::gritadmin::dashboard::handle_patch(
                            ctx,
                            repo,
                            table_name,
                        )
                        .await
                    })
                });

            let advanced_search_handler: #crate_root::database::repository::registry::AdminHandlerFn =
                ::std::sync::Arc::new(move |ctx| {
                    let table_name = table_name;
                    Box::pin(async move {
                        let db = ctx.db.clone().expect("DB connection missing");
                        let repo = #name {
                            db: (*db).clone(),
                        };
                        #crate_root::gritadmin::dashboard::handle_custom_search_viewer(
                            ctx,
                            repo,
                            table_name,
                        )
                        .await
                    })
                });

            let detail_handler: #crate_root::database::repository::registry::AdminHandlerFn =
                ::std::sync::Arc::new(move |ctx| {
                    use #crate_root::routing::engine::IntoResponse;
                    let table_name = table_name;
                    Box::pin(async move {
                        let db = ctx.db.clone().expect("DB connection missing");
                        let repo = #name { db: (*db).clone() };
                        #crate_root::gritadmin::dashboard::handle_detail(
                            ctx,
                            repo,
                            table_name,
                        )
                        .await
                        .into_response()
                    })
                });

            let bulk_delete_handler: #crate_root::database::repository::registry::AdminHandlerFn =
                ::std::sync::Arc::new(move |ctx| {
                    use #crate_root::routing::engine::IntoResponse;
                    let table_name = table_name;
                    Box::pin(async move {
                        let db = ctx.db.clone().expect("DB connection missing");
                        let repo = #name { db: (*db).clone() };
                        #crate_root::gritadmin::dashboard::handle_bulk_delete(
                            ctx,
                            repo,
                            table_name,
                        )
                        .await
                        .into_response()
                    })
                });

            let bulk_create_records_handler: #crate_root::database::repository::registry::AdminHandlerFn =
                ::std::sync::Arc::new(move |ctx| {
                    use #crate_root::routing::engine::IntoResponse;
                    let table_name = table_name;
                    Box::pin(async move {
                        let db = ctx.db.clone().expect("DB connection missing");
                        let repo = #name { db: (*db).clone() };
                        #crate_root::gritadmin::dashboard::handle_bulk_create(
                            ctx,
                            repo,
                            table_name,
                        )
                        .await
                        .into_response()
                    })
                });

            let bulk_create_modal_handler: #crate_root::database::repository::registry::AdminHandlerFn =
                ::std::sync::Arc::new(move |ctx| {
                    use #crate_root::routing::engine::IntoResponse;
                    let table_name = table_name;
                    Box::pin(async move {
                        let db = ctx.db.clone().expect("DB connection missing");
                        let repo = #name { db: (*db).clone() };
                        #crate_root::gritadmin::dashboard::handle_bulk_create_modal(
                            ctx,
                            repo,
                            table_name,
                        )
                        .await
                        .into_response()
                    })
                });

            let export_handler: #crate_root::database::repository::registry::AdminHandlerFn =
                ::std::sync::Arc::new(move |ctx| {
                    let table_name = table_name;
                    Box::pin(async move {
                        let db = ctx.db.clone().expect("DB connection missing");
                        let repo = #name { db: (*db).clone() };
                        #crate_root::gritadmin::dashboard::handle_export(
                            ctx,
                            repo,
                            table_name,
                        )
                        .await
                    })
                });

            let meta = #crate_root::database::repository::registry::ModelMetadata {
                table_name,
                table_slug,
                route_path,
                searchable_columns,
                list_handler,
                search_handler,
                delete_handler,
                patch_handler,
                advanced_search_handler,
                detail_handler,
                bulk_delete_handler,
                bulk_create_records_handler,
                bulk_create_modal_handler,
                export_handler,
            };


            #crate_root::database::repository::registry::register_model(table_name, meta);
        }
    }
}
