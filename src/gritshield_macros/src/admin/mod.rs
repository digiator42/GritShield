use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, LitStr};
use crate::core_parser::parse_repository_attributes;

mod ctor_registry;

pub fn expand_admin(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;

    // Use our centralized parser to get layout attributes
    let repo_attrs = parse_repository_attributes(&input.attrs)?;
    
    let mut grid_columns = repo_attrs.grid_columns;
    let searchable_columns = repo_attrs.searchable_columns;
    let read_only_columns = repo_attrs.read_only_columns;

    // Resolve model module path
    let entity_module = match repo_attrs.entity_module_path {
        Some(path) => quote! { #path },
        None => {
            let repo_name_lower = name.to_string().replace("Repository", "").to_lowercase();
            let module_ident = syn::Ident::new(&repo_name_lower, name.span());
            quote! { crate::models::#module_ident }
        }
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
    let route_path_literal = syn::LitStr::new(&route_path_str, proc_macro2::Span::call_site());

    let searchable_literals: Vec<LitStr> = searchable_columns
        .iter()
        .map(|s| LitStr::new(s, proc_macro2::Span::call_site()))
        .collect();

    let ctor_registration_block = ctor_registry::generate_registration(
        name,
        &entity_module,
        &repo_name_lower,
        &route_path_literal,
        &searchable_literals,
    );

    Ok(quote! {
        const _: () = {
            use ::gritshield::deps::sea_orm;

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

                #[allow(unreachable_code)]
                async fn update_column_value(
                    &self,
                    id: Self::Id,
                    column_name: &str,
                    value: ::std::string::String
                ) -> ::std::result::Result<Self::Model, ::gritshield::deps::sea_orm::DbErr> {
                    use ::gritshield::deps::sea_orm::{ActiveModelTrait, EntityTrait};
                    if let ::std::option::Option::Some(record) = #entity_module::Entity::find_by_id(id).one(self.get_db()).await? {
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

            impl ::gritshield::database::repository::ConvertFromModel<#entity_module::Model> for #entity_module::ActiveModel {
                fn from_model(model: #entity_module::Model) -> Self {
                    use sea_orm::IntoActiveModel;
                    model.into_active_model()
                }
            }

            #ctor_registration_block
        };
    })
}