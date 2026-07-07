use crate::core_parser::parse_repository_attributes;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, LitStr};

mod ctor_registry;
pub mod action;

pub fn expand_admin(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;

    // Use our centralized parser to get layout attributes
    let repo_attrs = parse_repository_attributes(&input.attrs)?;

    let mut grid_columns = repo_attrs.grid_columns;
    let searchable_columns = repo_attrs.searchable_columns;
    let read_only_columns = repo_attrs.read_only_columns;

    // Resolve model module path
    let entity_module = match &repo_attrs.entity_module_path {
        Some(path) => quote! { #path },
        None => {
            let repo_name_lower = name.to_string().replace("Repository", "").to_lowercase();
            let module_ident = syn::Ident::new(&repo_name_lower, name.span());
            quote! { crate::models::#module_ident }
        }
    };

    // Check if the entity path starts with "crate::gritadmin" (internal framework model)
    let is_internal = if let Some(path) = &repo_attrs.entity_module_path {
        let segments: Vec<_> = path.segments.iter().collect();
        segments.len() >= 2 && segments[0].ident == "crate" && segments[1].ident == "gritadmin"
    } else {
        // No entity path provided → external user model
        false
    };

    // Determine the crate root based on detection
    let crate_root = if is_internal {
        quote! { crate }
    } else {
        quote! { ::gritshield }
    };

    if grid_columns.is_empty() {
        grid_columns.push("id".to_string());
        for col in &searchable_columns {
            if col != "id" {
                grid_columns.push(col.clone());
            }
        }
    }

    let mut grid_column_tokens = Vec::new();
    let mut get_field_tokens = Vec::new();
    let mut update_field_tokens = Vec::new();

    let all_readonly = read_only_columns.len() == 1 && read_only_columns[0] == "all";

    for col in &grid_columns {
        let is_editable = if all_readonly {
            false // All columns are read-only
        } else {
            col != "id" && !read_only_columns.contains(col)
        };

        let label_str = if col.is_empty() {
            String::new()
        } else {
            let mut chars = col.chars();
            chars.next().unwrap().to_uppercase().collect::<String>() + chars.as_str()
        };

        grid_column_tokens.push(quote! {
            #crate_root::database::repository::GridColumn {
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
                    .map_err(|e| #crate_root::deps::sea_orm::DbErr::Custom(::std::format!("Failed to parse column '{}': {}", #col, e)))?;
                active_model.#field_ident = #crate_root::deps::sea_orm::Set(parsed_val);
            }
        });
        }
    }

    // Use repo_name_lower as the route slug (e.g., "user")
    let repo_name_lower = name.to_string().replace("Repository", "").to_lowercase();
    let route_slug = repo_name_lower.clone();

    let searchable_literals: Vec<LitStr> = searchable_columns
        .iter()
        .map(|s| LitStr::new(s, proc_macro2::Span::call_site()))
        .collect();

    let ctor_registration_block = ctor_registry::generate_registration(
        name,
        &entity_module,
        &route_slug,
        &searchable_literals,
        is_internal,
    );

    Ok(quote! {
        const _: () = {
            use #crate_root::deps::sea_orm;

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
            impl_admin_field!(#crate_root::deps::chrono::NaiveDate);
            impl_admin_field!(#crate_root::deps::uuid::Uuid);
            impl_admin_field!(#crate_root::deps::rust_decimal::Decimal);

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

            impl AdminFieldFormat for #crate_root::deps::chrono::NaiveDateTime {
                fn to_display_str(&self) -> ::std::string::String {
                    self.format("%Y-%m-%d %H:%M:%S").to_string()
                }
            }

            impl AdminFieldParse for #crate_root::deps::chrono::NaiveDateTime {
                fn parse_field(s: &str) -> ::std::result::Result<Self, ::std::string::String> {
                    let clean = s.trim();
                    #crate_root::deps::chrono::NaiveDateTime::parse_from_str(clean, "%Y-%m-%d %H:%M:%S")
                        .or_else(|_| #crate_root::deps::chrono::NaiveDateTime::parse_from_str(clean, "%Y-%m-%dT%H:%M:%S"))
                        .or_else(|_| {
                            // Fallback validation for short dates (e.g., "2026-07-05") without timestamps
                            #crate_root::deps::chrono::NaiveDate::parse_from_str(clean, "%Y-%m-%d")
                                .map(|d| d.and_hms_opt(0, 0, 0).unwrap())
                        })
                        .map_err(|e| ::std::format!("Invalid datetime format: {}. Expected formats: YYYY-MM-DD HH:MM:SS or YYYY-MM-DD", e))
                }
            }

            impl AdminFieldFormat for #crate_root::deps::serde_json::Value {
                fn to_display_str(&self) -> ::std::string::String {
                    self.to_string()
                }
            }
            impl AdminFieldParse for #crate_root::deps::serde_json::Value {
                fn parse_field(s: &str) -> ::std::result::Result<Self, ::std::string::String> {
                    #crate_root::deps::serde_json::from_str(s).map_err(|e| ::std::format!("Invalid JSON syntax: {}", e))
                }
            }

            #[#crate_root::deps::async_trait]
            impl #crate_root::database::repository::GritRepository for #name {
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

                fn grid_columns(&self) -> ::std::vec::Vec<#crate_root::database::repository::GridColumn> {
                    ::std::vec![ #(#grid_column_tokens),* ]
                }

                fn get_field_as_string(&self, model: &Self::Model, column_name: &str) -> ::std::string::String {
                    match column_name {
                        #(#get_field_tokens)*
                        _ => ::std::string::String::new(),
                    }
                }

                #[allow(unreachable_code)]
                async fn update_column_value(
                    &self,
                    id: Self::Id,
                    column_name: &str,
                    value: ::std::string::String,
                    user_id: Option<&str>,
                ) -> ::std::result::Result<Self::Model, #crate_root::deps::sea_orm::DbErr> {
                    use #crate_root::deps::sea_orm::{ActiveModelTrait, EntityTrait, EntityName};
                    use #crate_root::deps::serde_json;

                    if let ::std::option::Option::Some(record) = #entity_module::Entity::find_by_id(id).one(self.get_db()).await? {
                        let mut active_model = <Self::ActiveModel as #crate_root::database::repository::ConvertFromModel<Self::Model>>::from_model(record.clone());
                        match column_name {
                            #(#update_field_tokens)*
                            _ => return ::std::result::Result::Err(#crate_root::deps::sea_orm::DbErr::Custom(::std::format!("Column '{}' is not editable", column_name))),
                        };
                        let updated_model = active_model.update(self.get_db()).await?;

                        // Capture old and new values for audit
                        let old_json = serde_json::to_value(&record).ok();
                        let new_json = serde_json::to_value(&updated_model).ok();

                        // Get table name from the entity
                        let table_name = <#entity_module::Entity as EntityName>::table_name(&#entity_module::Entity);

                        // Log the change
                        self.audit_log(
                            table_name,
                            &format!("{}", id),
                            "update",
                            old_json,
                            new_json,
                            user_id,
                        ).await?;

                        ::std::result::Result::Ok(updated_model)
                    } else {
                        ::std::result::Result::Err(#crate_root::deps::sea_orm::DbErr::Custom("Target row record not found".to_string()))
                    }
                }

                async fn audit_log(
                    &self,
                    table_name: &str,
                    record_id: &str,
                    action: &str,
                    old_values: Option<serde_json::Value>,
                    new_values: Option<serde_json::Value>,
                    user_id: Option<&str>,
                ) -> Result<(), sea_orm::DbErr> {
                    use #crate_root::gritadmin::models::audit_log::{ActiveModel, Entity, Model};
                    use sea_orm::ActiveModelTrait;
                    use chrono::Utc;

                    let new_entry = ActiveModel {
                        table_name: sea_orm::Set(table_name.to_string()),
                        record_id: sea_orm::Set(record_id.to_string()),
                        action: sea_orm::Set(action.to_string()),
                        old_values: sea_orm::Set(old_values),
                        new_values: sea_orm::Set(new_values),
                        user_id: sea_orm::Set(user_id.map(|s| s.to_string())),
                        timestamp: sea_orm::Set(Utc::now().naive_utc()),
                        ..Default::default()
                    };
                    new_entry.insert(self.get_db()).await?;
                    Ok(())
                }

                async fn search_admin_fields(&self, text: &str) -> ::std::result::Result<::std::vec::Vec<Self::Model>, #crate_root::deps::sea_orm::DbErr> {
                    use #crate_root::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, Iterable, Iden};
                    use #crate_root::deps::sea_orm::sea_query::{Expr, ExprTrait, Alias};

                    let db = &self.db;
                    let mut query = #entity_module::Entity::find();

                    if text.trim().is_empty() {
                        return query.all(db).await;
                    }

                    let mut condition = #crate_root::deps::sea_orm::Condition::any();
                    let configured_search_strings = ::std::vec![ #(#searchable_columns),* ];

                    for col in <#entity_module::Column as Iterable>::iter() {
                        if configured_search_strings.contains(&col.to_string().as_str()) {
                            let expr = Expr::col(col.clone()).cast_as(Alias::new("text"));
                            condition = condition.add(expr.like(format!("%{}%", text)));
                        }
                    }

                    query.filter(condition).all(db).await
                }
                async fn delete_by_id(
                    &self,
                    id: Self::Id,
                    user_id: Option<&str>,
                ) -> ::std::result::Result<#crate_root::deps::sea_orm::DeleteResult, #crate_root::deps::sea_orm::DbErr> {
                    use #crate_root::deps::sea_orm::{EntityTrait, EntityName};
                    use #crate_root::deps::serde_json;

                    // Fetch record before deletion for audit
                    let record = match #entity_module::Entity::find_by_id(id).one(self.get_db()).await? {
                        Some(r) => r,
                        None => return ::std::result::Result::Err(#crate_root::deps::sea_orm::DbErr::Custom("Record not found".to_string())),
                    };

                    let old_json = serde_json::to_value(&record).ok();
                    let table_name = <#entity_module::Entity as EntityName>::table_name(&#entity_module::Entity);

                    // Delete the record
                    let res = #entity_module::Entity::delete_by_id(id).exec(self.get_db()).await?;

                    // Log the deletion
                    self.audit_log(
                        table_name,
                        &format!("{}", id),
                        "delete",
                        old_json,
                        None,
                        user_id,
                    ).await?;

                    ::std::result::Result::Ok(res)
                }
            }

            impl #crate_root::database::repository::ConvertFromModel<#entity_module::Model> for #entity_module::ActiveModel {
                fn from_model(model: #entity_module::Model) -> Self {
                    use sea_orm::IntoActiveModel;
                    model.into_active_model()
                }
            }

            #ctor_registration_block
        };
    })
}
