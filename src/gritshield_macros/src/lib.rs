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

    let mut entity_module_path: Option<syn::Path> = None;
    let mut searchable_columns = Vec::new();
    let mut grid_columns = Vec::new();
    let mut read_only_columns = Vec::new();

    for attr in &input.attrs {
        if attr.path().is_ident("repository") {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("entity") {
                    let value = meta.value()?;
                    entity_module_path = Some(value.parse::<syn::Path>()?);
                } else if meta.path.is_ident("searchable") {
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
                } else if meta.path.is_ident("grid_columns") {
                    let value = meta.value()?;
                    if let Ok(Expr::Array(ExprArray { elems, .. })) = value.parse::<Expr>() {
                        for elem in elems {
                            if let Expr::Lit(ExprLit {
                                lit: Lit::Str(lit_str),
                                ..
                            }) = elem
                            {
                                grid_columns.push(lit_str.value());
                            }
                        }
                    }
                } else if meta.path.is_ident("read_only") {
                    let value = meta.value()?;
                    if let Ok(Expr::Array(ExprArray { elems, .. })) = value.parse::<Expr>() {
                        for elem in elems {
                            if let Expr::Lit(ExprLit {
                                lit: Lit::Str(lit_str),
                                ..
                            }) = elem
                            {
                                read_only_columns.push(lit_str.value());
                            }
                        }
                    }
                }
                Ok(())
            });
        }
    }

    // Fallback entity module inference
    let entity_module = match entity_module_path {
        Some(path) => quote! { #path },
        None => {
            let repo_name_lower = name.to_string().replace("Repository", "").to_lowercase();
            let module_ident = syn::Ident::new(&repo_name_lower, name.span());
            quote! { crate::models::#module_ident }
        }
    };

    // Default grid columns: id + searchable columns (excluding duplicates)
    if grid_columns.is_empty() {
        grid_columns.push("id".to_string());
        for col in &searchable_columns {
            if col != "id" {
                grid_columns.push(col.clone());
            }
        }
    }

    // Build tokens for grid columns, get_field, and update_field
    let mut grid_column_tokens = Vec::new();
    let mut get_field_tokens = Vec::new();
    let mut update_field_tokens = Vec::new();

    for col in &grid_columns {
        let is_editable = col != "id" && !read_only_columns.contains(col);
        let label_str = if col.is_empty() {
            String::new()
        } else {
            let mut chars = col.chars();
            chars.next().unwrap().to_uppercase().collect::<String>() + chars.as_str()
        };

        grid_column_tokens.push(quote! {
            ::gritshield::database::repository::GridColumn {
                name: #col,
                label: #label_str,
                is_editable: #is_editable,
            }
        });

        let field_ident = syn::Ident::new(col, proc_macro2::Span::call_site());

        get_field_tokens.push(quote! {
            #col => AdminFieldFormat::to_display_str(&model.#field_ident),
        });

        if is_editable {
            update_field_tokens.push(quote! {
                #col => {
                    let parsed_val = AdminFieldParse::parse_field(&value)
                        .map_err(|e| ::gritshield::deps::sea_orm::DbErr::Custom(::std::format!("Failed to parse column '{}': {}", #col, e)))?;
                    active_model.#field_ident = ::gritshield::deps::sea_orm::Set(parsed_val);
                }
            });
        }
    }

    let repo_name_lower = name.to_string().replace("Repository", "").to_lowercase();
    let route_path_str = format!("/admin/{}", repo_name_lower);

    let route_path_literal = syn::LitStr::new(
        &route_path_str,
        proc_macro2::Span::call_site(),
    );

    // Convert searchable column names to compile‑time string literals
    let searchable_literals: Vec<LitStr> = searchable_columns
        .iter()
        .map(|s| LitStr::new(s, proc_macro2::Span::call_site()))
        .collect();

    let initializer_name = syn::Ident::new(
        &format!("init_meta_{}", repo_name_lower),
        proc_macro2::Span::call_site(),
    );

    let expanded = quote! {
        const _: () = {
            use ::gritshield::deps::sea_orm;

            // ────────── Admin field formatting traits ──────────
            trait AdminFieldFormat {
                fn to_display_str(&self) -> ::std::string::String;
            }
            trait AdminFieldParse {
                fn parse_field(s: &str) -> ::std::result::Result<Self, ::std::string::String>
                where
                    Self: ::std::marker::Sized;
            }

            macro_rules! impl_admin_field {
                ($t:ty) => {
                    impl AdminFieldFormat for $t {
                        fn to_display_str(&self) -> ::std::string::String {
                            ::std::format!("{}", self)
                        }
                    }
                    impl AdminFieldParse for $t {
                        fn parse_field(s: &str) -> ::std::result::Result<Self, ::std::string::String> {
                            s.parse().map_err(|e| ::std::format!("{}", e))
                        }
                    }
                };
            }

            impl_admin_field!(::std::string::String);
            impl_admin_field!(i16);
            impl_admin_field!(i32);
            impl_admin_field!(i64);
            impl_admin_field!(u16);
            impl_admin_field!(u32);
            impl_admin_field!(u64);
            impl_admin_field!(f32);
            impl_admin_field!(f64);
            impl_admin_field!(bool);
            impl_admin_field!(::gritshield::deps::chrono::NaiveDate);
            impl_admin_field!(::gritshield::deps::uuid::Uuid);
            impl_admin_field!(::gritshield::deps::rust_decimal::Decimal);

            impl<T> AdminFieldFormat for ::std::option::Option<T>
            where
                T: AdminFieldFormat,
            {
                fn to_display_str(&self) -> ::std::string::String {
                    match self {
                        ::std::option::Option::Some(val) => val.to_display_str(),
                        ::std::option::Option::None => ::std::string::String::new(),
                    }
                }
            }
            impl<T> AdminFieldParse for ::std::option::Option<T>
            where
                T: AdminFieldParse,
            {
                fn parse_field(s: &str) -> ::std::result::Result<Self, ::std::string::String> {
                    if s.trim().is_empty() {
                        ::std::result::Result::Ok(::std::option::Option::None)
                    } else {
                        let parsed = T::parse_field(s)?;
                        ::std::result::Result::Ok(::std::option::Option::Some(parsed))
                    }
                }
            }

            impl AdminFieldFormat for ::gritshield::deps::chrono::NaiveDateTime {
                fn to_display_str(&self) -> ::std::string::String {
                    self.format("%Y-%m-%d %H:%M:%S").to_string()
                }
            }
            impl AdminFieldParse for ::gritshield::deps::chrono::NaiveDateTime {
                fn parse_field(s: &str) -> ::std::result::Result<Self, ::std::string::String> {
                    ::gritshield::deps::chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S")
                        .or_else(|_| ::gritshield::deps::chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%dT%H:%M:%S"))
                        .map_err(|e| ::std::format!("Invalid datetime format: {}", e))
                }
            }

            impl AdminFieldFormat for ::gritshield::deps::serde_json::Value {
                fn to_display_str(&self) -> ::std::string::String {
                    self.to_string()
                }
            }
            impl AdminFieldParse for ::gritshield::deps::serde_json::Value {
                fn parse_field(s: &str) -> ::std::result::Result<Self, ::std::string::String> {
                    ::gritshield::deps::serde_json::from_str(s).map_err(|e| ::std::format!("Invalid JSON syntax: {}", e))
                }
            }

            // ────────── Trait implementation ──────────
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

                fn grid_columns(&self) -> ::std::vec::Vec<::gritshield::database::repository::GridColumn> {
                    ::std::vec![ #(#grid_column_tokens),* ]
                }

                fn get_field_as_string(&self, model: &Self::Model, column_name: &str) -> ::std::string::String {
                    match column_name {
                        #(#get_field_tokens)*
                        _ => ::std::string::String::new(),
                    }
                }

                async fn update_column_value(
                    &self,
                    id: Self::Id,
                    column_name: &str,
                    value: ::std::string::String
                ) -> ::std::result::Result<Self::Model, ::gritshield::deps::sea_orm::DbErr> {
                    use ::gritshield::deps::sea_orm::ActiveModelTrait;
                    if let ::std::option::Option::Some(record) = self.find_by_id(id).await? {
                        let mut active_model = <Self::ActiveModel as ::gritshield::database::repository::ConvertFromModel<Self::Model>>::from_model(record);
                        match column_name {
                            #(#update_field_tokens)*
                            _ => return ::std::result::Result::Err(::gritshield::deps::sea_orm::DbErr::Custom(::std::format!("Column '{}' is not editable", column_name))),
                        };
                        let updated_model = active_model.update(self.get_db()).await?;
                        ::std::result::Result::Ok(updated_model)
                    } else {
                        ::std::result::Result::Err(::gritshield::deps::sea_orm::DbErr::Custom("Target row record not found".to_string()))
                    }
                }

                async fn search_admin_fields(&self, text: &str) -> ::std::result::Result<::std::vec::Vec<Self::Model>, ::gritshield::deps::sea_orm::DbErr> {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, Iterable, Iden};
                    use ::gritshield::deps::sea_orm::sea_query::{Expr, ExprTrait, Alias};

                    let db = &self.db;
                    let mut query = #entity_module::Entity::find();

                    if text.trim().is_empty() {
                        return query.all(db).await;
                    }

                    let mut condition = ::gritshield::deps::sea_orm::Condition::any();
                    let configured_search_strings = ::std::vec![ #(#searchable_columns),* ];

                    for col in <#entity_module::Column as Iterable>::iter() {
                        if configured_search_strings.contains(&col.to_string().as_str()) {
                            let expr = Expr::col(col.clone()).cast_as(Alias::new("text"));
                            condition = condition.add(expr.like(format!("%{}%", text)));
                        }
                    }

                    query.filter(condition).all(db).await
                }
            }

            // ────────── Inherent methods ──────────
            impl #name {
                pub fn find() -> ::gritshield::deps::sea_orm::Select<#entity_module::Entity> {
                    use ::gritshield::deps::sea_orm::EntityTrait;
                    #entity_module::Entity::find()
                }

                pub fn id_col() -> #entity_module::Column {
                    #entity_module::Column::Id
                }

                pub fn column_names() -> ::std::vec::Vec<::std::string::String> {
                    use ::gritshield::deps::sea_orm::{Iterable, Iden};
                    <#entity_module::Column as Iterable>::iter()
                        .map(|col| col.to_string())
                        .collect()
                }

                pub fn column_from_str(name: &str) -> ::std::option::Option<#entity_module::Column> {
                    use ::gritshield::deps::sea_orm::{Iterable, Iden};
                    for col in <#entity_module::Column as Iterable>::iter() {
                        if col.to_string() == name {
                            return ::std::option::Option::Some(col);
                        }
                    }
                    ::std::option::Option::None
                }

                pub async fn find_by_id(
                    &self,
                    id: <#name as ::gritshield::database::repository::GritRepository>::Id
                ) -> ::std::result::Result<::std::option::Option<#entity_module::Model>, ::gritshield::deps::sea_orm::DbErr> {
                    use ::gritshield::deps::sea_orm::EntityTrait;
                    let db = <Self as ::gritshield::database::repository::GritRepository>::get_db(self);
                    #entity_module::Entity::find_by_id(id).one(db).await
                }

                pub async fn total_count(&self) -> ::std::result::Result<u64, ::gritshield::deps::sea_orm::DbErr> {
                    use ::gritshield::deps::sea_orm::PaginatorTrait;
                    let db = <Self as ::gritshield::database::repository::GritRepository>::get_db(self);
                    Self::find().count(db).await
                }

                pub async fn delete_by_id(
                    &self,
                    id: <#name as ::gritshield::database::repository::GritRepository>::Id
                ) -> ::std::result::Result<::gritshield::deps::sea_orm::DeleteResult, ::gritshield::deps::sea_orm::DbErr> {
                    use ::gritshield::deps::sea_orm::EntityTrait;
                    let db = <Self as ::gritshield::database::repository::GritRepository>::get_db(self);
                    #entity_module::Entity::delete_by_id(id).exec(db).await
                }

                pub async fn fetch_page_slice(
                    &self,
                    page: u64,
                    page_size: u64
                ) -> ::std::result::Result<::std::vec::Vec<#entity_module::Model>, ::gritshield::deps::sea_orm::DbErr> {
                    use ::gritshield::deps::sea_orm::{QueryOrder, PaginatorTrait};
                    let db = <Self as ::gritshield::database::repository::GritRepository>::get_db(self);
                    Self::find()
                        .order_by_desc(Self::id_col())
                        .paginate(db, page_size)
                        .fetch_page(page)
                        .await
                }
            }

            // ────────── ConvertFromModel ──────────
            impl ::gritshield::database::repository::ConvertFromModel<#entity_module::Model> for #entity_module::ActiveModel {
                fn from_model(model: #entity_module::Model) -> Self {
                    use sea_orm::IntoActiveModel;
                    model.into_active_model()
                }
            }

            // ────────── Admin registration ──────────
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
        };
    };

    TokenStream::from(expanded)
}
