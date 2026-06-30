// src/repository/jpa_dsl.rs
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::Ident;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModelColumnType {
    String,
    Numeric,
    DateTime,
    Bool,
    Unknown,
}

pub fn type_to_column_type(ty: &syn::Type) -> ModelColumnType {
    if let syn::Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            let type_str = segment.ident.to_string();
            match type_str.as_str() {
                "String" | "str" => return ModelColumnType::String,
                "bool" => return ModelColumnType::Bool,
                "i8" | "i16" | "i32" | "i64" | "i128" | "isize" |
                "u8" | "u16" | "u32" | "u64" | "u128" | "usize" |
                "f32" | "f64" | "Decimal" => return ModelColumnType::Numeric,
                "NaiveDateTime" | "DateTime" | "NaiveDate" | "NaiveTime" => return ModelColumnType::DateTime,
                _ => {}
            }
        }
    }
    ModelColumnType::Unknown
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

fn generate_type_specific_methods(
    field_ident: &Ident,
    col_type: ModelColumnType,
    entity_path: &TokenStream,
    column_path: &TokenStream,
    all_builder_path: &TokenStream,
) -> TokenStream {
    let col_str = field_ident.to_string();
    let variant_ident = Ident::new(&to_pascal_case(&col_str), Span::call_site());
    let mut extra_methods = Vec::new();

    match col_type {
        ModelColumnType::String => {
            let like_ident = Ident::new(&format!("find_by_{}_like", col_str), Span::call_site());
            let contains_ident = Ident::new(&format!("find_by_{}_contains", col_str), Span::call_site());

            extra_methods.push(quote! {
                pub fn #like_ident<V>(&self, val: V) -> #all_builder_path<'_> 
                where V: ::std::convert::Into<::std::string::String> {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                    let query = #entity_path::find().filter(#column_path::#variant_ident.like(val));
                    #all_builder_path::new(&self.db, query)
                }

                pub fn #contains_ident(&self, val: &str) -> #all_builder_path<'_> {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                    let query = #entity_path::find().filter(#column_path::#variant_ident.contains(val));
                    #all_builder_path::new(&self.db, query)
                }
            });
        }
        ModelColumnType::Numeric => {
            let gt_ident = Ident::new(&format!("find_by_{}_gt", col_str), Span::call_site());
            let lt_ident = Ident::new(&format!("find_by_{}_lt", col_str), Span::call_site());
            let between_ident = Ident::new(&format!("find_by_{}_between", col_str), Span::call_site());

            extra_methods.push(quote! {
                pub fn #gt_ident<V>(&self, val: V) -> #all_builder_path<'_>
                where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                    let query = #entity_path::find().filter(#column_path::#variant_ident.gt(val));
                    #all_builder_path::new(&self.db, query)
                }

                pub fn #lt_ident<V>(&self, val: V) -> #all_builder_path<'_>
                where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                    let query = #entity_path::find().filter(#column_path::#variant_ident.lt(val));
                    #all_builder_path::new(&self.db, query)
                }

                pub fn #between_ident<V>(&self, low: V, high: V) -> #all_builder_path<'_>
                where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                    let query = #entity_path::find().filter(#column_path::#variant_ident.between(low, high));
                    #all_builder_path::new(&self.db, query)
                }
            });
        }
        ModelColumnType::DateTime => {
            let gt_ident = Ident::new(&format!("find_by_{}_gt", col_str), Span::call_site());
            let lt_ident = Ident::new(&format!("find_by_{}_lt", col_str), Span::call_site());

            extra_methods.push(quote! {
                pub fn #gt_ident<V>(&self, val: V) -> #all_builder_path<'_>
                where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                    let query = #entity_path::find().filter(#column_path::#variant_ident.gt(val));
                    #all_builder_path::new(&self.db, query)
                }

                pub fn #lt_ident<V>(&self, val: V) -> #all_builder_path<'_>
                where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                    let query = #entity_path::find().filter(#column_path::#variant_ident.lt(val));
                    #all_builder_path::new(&self.db, query)
                }
            });
        }
        ModelColumnType::Bool => {
            let true_ident = Ident::new(&format!("find_by_{}_true", col_str), Span::call_site());
            let false_ident = Ident::new(&format!("find_by_{}_false", col_str), Span::call_site());

            extra_methods.push(quote! {
                pub fn #true_ident(&self) -> #all_builder_path<'_> {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                    let query = #entity_path::find().filter(#column_path::#variant_ident.eq(true));
                    #all_builder_path::new(&self.db, query)
                }

                pub fn #false_ident(&self) -> #all_builder_path<'_> {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                    let query = #entity_path::find().filter(#column_path::#variant_ident.eq(false));
                    #all_builder_path::new(&self.db, query)
                }
            });
        }
        _ => {}
    }

    quote! { #(#extra_methods)* }
}

pub fn generate_model_specific_methods(
    entity_path: &TokenStream,
    column_path: &TokenStream,
    all_builder_path: &TokenStream,
    fields: &[(syn::Ident, ModelColumnType)],
) -> TokenStream {
    let mut advanced_methods = Vec::new();
    for (field_ident, col_type) in fields {
        let block = generate_type_specific_methods(
            field_ident,
            *col_type,
            entity_path,
            column_path,
            all_builder_path,
        );
        advanced_methods.push(block);
    }
    quote! { #(#advanced_methods)* }
}

pub fn generate_jpa_methods(
    entity_path: &TokenStream,
    column_path: &TokenStream,
    all_builder_path: &TokenStream,
    one_builder_path: &TokenStream,
    fields: &[(syn::Ident, ModelColumnType)],
) -> TokenStream {
    let mut jpa_dsl_methods = Vec::new();

    // 1. Single Column Invocations
    for (field_ident, col_type) in fields {
        let col_str = field_ident.to_string();
        let variant_ident = Ident::new(&to_pascal_case(&col_str), Span::call_site());

        let find_by_ident = Ident::new(&format!("find_by_{}", col_str), Span::call_site());
        let find_one_by_ident = Ident::new(&format!("find_one_by_{}", col_str), Span::call_site());
        let exists_by_ident = Ident::new(&format!("exists_by_{}", col_str), Span::call_site());
        let delete_by_ident = Ident::new(&format!("delete_by_{}", col_str), Span::call_site());
        let count_by_ident = Ident::new(&format!("count_by_{}", col_str), Span::call_site());

        jpa_dsl_methods.push(quote! {
            pub fn #find_by_ident<V>(&self, val: V) -> #all_builder_path<'_>
            where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                let query = #entity_path::find().filter(#column_path::#variant_ident.eq(val));
                #all_builder_path::new(&self.db, query)
            }

            pub fn #find_one_by_ident<V>(&self, val: V) -> #one_builder_path<'_>
            where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                let query = #entity_path::find().filter(#column_path::#variant_ident.eq(val));
                #one_builder_path::new(&self.db, query)
            }

            pub async fn #exists_by_ident<V>(&self, val: V) -> ::std::result::Result<bool, ::gritshield::deps::sea_orm::DbErr>
            where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, PaginatorTrait};
                let count = #entity_path::find()
                    .filter(#column_path::#variant_ident.eq(val))
                    .count(&self.db)
                    .await?;
                ::std::result::Result::Ok(count > 0)
            }

            pub async fn #delete_by_ident<V>(&self, val: V) -> ::std::result::Result<u64, ::gritshield::deps::sea_orm::DbErr>
            where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                let res = #entity_path::delete_many()
                    .filter(#column_path::#variant_ident.eq(val))
                    .exec(&self.db)
                    .await?;
                ::std::result::Result::Ok(res.rows_affected)
            }

            pub async fn #count_by_ident<V>(&self, val: V) -> ::std::result::Result<u64, ::gritshield::deps::sea_orm::DbErr>
            where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, PaginatorTrait};
                #entity_path::find()
                    .filter(#column_path::#variant_ident.eq(val))
                    .count(&self.db)
                    .await
            }
        });

        let type_specific_block = generate_type_specific_methods(
            field_ident,
            *col_type,
            entity_path,
            column_path,
            all_builder_path,
        );
        jpa_dsl_methods.push(type_specific_block);
    }

    // 2. Composite Multi-Column Invocations
    for i in 0..fields.len() {
        for j in (i + 1)..fields.len() {
            let col1 = fields[i].0.to_string();
            let col2 = fields[j].0.to_string();

            let var1_ident = Ident::new(&to_pascal_case(&col1), Span::call_site());
            let var2_ident = Ident::new(&to_pascal_case(&col2), Span::call_site());

            let find_and_ident = Ident::new(&format!("find_by_{}_and_{}", col1, col2), Span::call_site());
            let find_one_and_ident = Ident::new(&format!("find_one_by_{}_and_{}", col1, col2), Span::call_site());
            let find_or_ident = Ident::new(&format!("find_by_{}_or_{}", col1, col2), proc_macro2::Span::call_site());
            let exists_and_ident = Ident::new(&format!("exists_by_{}_and_{}", col1, col2), proc_macro2::Span::call_site());
            let delete_and_ident = Ident::new(&format!("delete_by_{}_and_{}", col1, col2), proc_macro2::Span::call_site());

            jpa_dsl_methods.push(quote! {
                pub fn #find_and_ident<V1, V2>(&self, val1: V1, val2: V2) -> #all_builder_path<'_>
                where 
                    V1: ::std::convert::Into<::gritshield::deps::sea_orm::Value>,
                    V2: ::std::convert::Into<::gritshield::deps::sea_orm::Value>
                {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, Condition};
                    let condition = Condition::all().add(#column_path::#var1_ident.eq(val1)).add(#column_path::#var2_ident.eq(val2));
                    let query = #entity_path::find().filter(condition);
                    #all_builder_path::new(&self.db, query)
                }

                pub fn #find_one_and_ident<V1, V2>(&self, val1: V1, val2: V2) -> #one_builder_path<'_>
                where 
                    V1: ::std::convert::Into<::gritshield::deps::sea_orm::Value>,
                    V2: ::std::convert::Into<::gritshield::deps::sea_orm::Value>
                {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, Condition};
                    let condition = Condition::all().add(#column_path::#var1_ident.eq(val1)).add(#column_path::#var2_ident.eq(val2));
                    let query = #entity_path::find().filter(condition);
                    #one_builder_path::new(&self.db, query)
                }

                pub fn #find_or_ident<V1, V2>(&self, val1: V1, val2: V2) -> #all_builder_path<'_>
                where 
                    V1: ::std::convert::Into<::gritshield::deps::sea_orm::Value>,
                    V2: ::std::convert::Into<::gritshield::deps::sea_orm::Value>
                {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, Condition};
                    let condition = Condition::any().add(#column_path::#var1_ident.eq(val1)).add(#column_path::#var2_ident.eq(val2));
                    let query = #entity_path::find().filter(condition);
                    #all_builder_path::new(&self.db, query)
                }

                pub async fn #exists_and_ident<V1, V2>(&self, val1: V1, val2: V2) -> ::std::result::Result<bool, ::gritshield::deps::sea_orm::DbErr>
                where 
                    V1: ::std::convert::Into<::gritshield::deps::sea_orm::Value>,
                    V2: ::std::convert::Into<::gritshield::deps::sea_orm::Value>
                {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, Condition, PaginatorTrait};
                    let condition = Condition::all().add(#column_path::#var1_ident.eq(val1)).add(#column_path::#var2_ident.eq(val2));
                    let count = #entity_path::find().filter(condition).count(&self.db).await?;
                    ::std::result::Result::Ok(count > 0)
                }

                pub async fn #delete_and_ident<V1, V2>(&self, val1: V1, val2: V2) -> ::std::result::Result<u64, ::gritshield::deps::sea_orm::DbErr>
                where 
                    V1: ::std::convert::Into<::gritshield::deps::sea_orm::Value>,
                    V2: ::std::convert::Into<::gritshield::deps::sea_orm::Value>
                {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, Condition};
                    let condition = Condition::all().add(#column_path::#var1_ident.eq(val1)).add(#column_path::#var2_ident.eq(val2));
                    let res = #entity_path::delete_many().filter(condition).exec(&self.db).await?;
                    ::std::result::Result::Ok(res.rows_affected)
                }
            });
        }
    }

    quote! { #(#jpa_dsl_methods)* }
}