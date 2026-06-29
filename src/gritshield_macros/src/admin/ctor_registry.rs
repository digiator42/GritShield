use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, LitStr};

pub fn generate_registration(
    name: &Ident,
    entity_module: &TokenStream,
    repo_name_lower: &str,
    route_path_literal: &LitStr,
    searchable_literals: &[LitStr],
) -> TokenStream {
    let initializer_name = Ident::new(
        &format!("init_meta_{}", repo_name_lower),
        proc_macro2::Span::call_site(),
    );

    quote! {
        #[::gritshield::startup::ctor(unsafe)]
        fn #initializer_name() {
            use ::gritshield::deps::sea_orm::EntityName;

            let table_name: &'static str =
                Box::leak(<#entity_module::Entity as EntityName>::table_name(&#entity_module::Entity)
                    .to_owned()
                    .into_boxed_str());

            let route_path: &'static str =
                Box::leak(::std::string::String::from(#route_path_literal).into_boxed_str());

            let searchable_columns: Vec<&'static str> = vec![
                #(
                    Box::leak(::std::string::String::from(#searchable_literals).into_boxed_str())
                ),*
            ];

            let list_handler: ::gritshield::database::repository::AdminHandlerFn =
                ::std::sync::Arc::new(move |ctx| {
                    let table_name = table_name;
                    Box::pin(async move {
                        let db = ctx.db.clone().expect("DB connection missing");
                        let repo = #name {
                            db: (*db).clone(),
                        };

                        ::gritshield::gritadmin::main_handler::handle_list(
                            ctx,
                            repo,
                            table_name,
                        )
                        .await
                    })
                });

            let search_handler: ::gritshield::database::repository::AdminHandlerFn =
                ::std::sync::Arc::new(move |ctx| {
                    let table_name = table_name;
                    Box::pin(async move {
                        let db = ctx.db.clone().expect("DB connection missing");
                        let repo = #name {
                            db: (*db).clone(),
                        };

                        ::gritshield::gritadmin::main_handler::handle_search(
                            ctx,
                            repo,
                            table_name,
                        )
                        .await
                    })
                });

            let patch_handler: ::gritshield::database::repository::AdminHandlerFn =
                ::std::sync::Arc::new(move |ctx| {
                    let table_name = table_name;
                    Box::pin(async move {
                        let db = ctx.db.clone().expect("DB connection missing");
                        let repo = #name {
                            db: (*db).clone(),
                        };

                        ::gritshield::gritadmin::main_handler::handle_patch(
                            ctx,
                            repo,
                            table_name,
                        )
                        .await
                    })
                });

            let meta = ::gritshield::database::repository::ModelMetadata {
                table_name,
                route_path,
                searchable_columns,
                list_handler,
                search_handler,
                patch_handler,
            };

            println!("REGISTERING ADMIN MODEL: {}", table_name);

            ::gritshield::database::repository::register_model(table_name, meta);
        }
    }
}
