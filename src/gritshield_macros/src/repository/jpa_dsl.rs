use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

enum MacroColumnType {
    String,
    Numeric,
    DateTime,
    Bool,
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

fn infer_macro_column_type(name: &str) -> MacroColumnType {
    let name_lower = name.to_lowercase();
    if name_lower == "active" || name_lower.starts_with("is_") || name_lower.starts_with("has_") {
        MacroColumnType::Bool
    } else if name_lower.ends_with("_at") || name_lower.contains("date") || name_lower.contains("time") {
        MacroColumnType::DateTime
    } else if name_lower == "id" || name_lower.ends_with("_id") || name_lower == "count" || name_lower == "amount" || name_lower == "price" {
        MacroColumnType::Numeric
    } else {
        MacroColumnType::String
    }
}

pub fn generate_jpa_methods(
    entity_module: &TokenStream,
    all_builder_name: &Ident,
    one_builder_name: &Ident,
    unique_fields: &[String],
) -> TokenStream {
    let mut jpa_dsl_methods = Vec::new();

    // 1. Single Column Invocations
    for col in unique_fields {
        let variant_name = to_pascal_case(col);
        let variant_ident = Ident::new(&variant_name, proc_macro2::Span::call_site());

        let find_by_ident = Ident::new(&format!("find_by_{}", col), proc_macro2::Span::call_site());
        let find_one_by_ident = Ident::new(&format!("find_one_by_{}", col), proc_macro2::Span::call_site());
        let exists_by_ident = Ident::new(&format!("exists_by_{}", col), proc_macro2::Span::call_site());
        let delete_by_ident = Ident::new(&format!("delete_by_{}", col), proc_macro2::Span::call_site());
        let count_by_ident = Ident::new(&format!("count_by_{}", col), proc_macro2::Span::call_site());

        jpa_dsl_methods.push(quote! {
            pub fn #find_by_ident<V>(&self, val: V) -> #all_builder_name
            where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                let query = #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.eq(val));
                #all_builder_name::new(self, query)
            }

            pub fn #find_one_by_ident<V>(&self, val: V) -> #one_builder_name
            where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                let query = #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.eq(val));
                #one_builder_name::new(self, query)
            }

            pub async fn #exists_by_ident<V>(&self, val: V) -> ::std::result::Result<bool, ::gritshield::deps::sea_orm::DbErr>
            where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, PaginatorTrait};
                let count = #entity_module::Entity::find()
                    .filter(#entity_module::Column::#variant_ident.eq(val))
                    .count(&self.db)
                    .await?;
                ::std::result::Result::Ok(count > 0)
            }

            pub async fn #delete_by_ident<V>(&self, val: V) -> ::std::result::Result<u64, ::gritshield::deps::sea_orm::DbErr>
            where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                let res = #entity_module::Entity::delete_many()
                    .filter(#entity_module::Column::#variant_ident.eq(val))
                    .exec(&self.db)
                    .await?;
                ::std::result::Result::Ok(res.rows_affected)
            }

            pub async fn #count_by_ident<V>(&self, val: V) -> ::std::result::Result<u64, ::gritshield::deps::sea_orm::DbErr>
            where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, PaginatorTrait};
                #entity_module::Entity::find()
                    .filter(#entity_module::Column::#variant_ident.eq(val))
                    .count(&self.db)
                    .await
            }
        });

        match infer_macro_column_type(col) {
            MacroColumnType::String => {
                let like_ident = Ident::new(&format!("find_by_{}_like", col), proc_macro2::Span::call_site());
                let contains_ident = Ident::new(&format!("find_by_{}_contains", col), proc_macro2::Span::call_site());
                let starts_with_ident = Ident::new(&format!("find_by_{}_starts_with", col), proc_macro2::Span::call_site());
                let ends_with_ident = Ident::new(&format!("find_by_{}_ends_with", col), proc_macro2::Span::call_site());

                jpa_dsl_methods.push(quote! {
                    pub fn #like_ident<V>(&self, val: V) -> #all_builder_name 
                    where V: ::std::convert::Into<::std::string::String> 
                    {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        let query = #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.like(val));
                        #all_builder_name::new(self, query)
                    }

                    pub fn #contains_ident(&self, val: &str) -> #all_builder_name {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        let query = #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.contains(val));
                        #all_builder_name::new(self, query)
                    }

                    pub fn #starts_with_ident(&self, val: &str) -> #all_builder_name {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        let query = #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.starts_with(val));
                        #all_builder_name::new(self, query)                    
                    }

                    pub fn #ends_with_ident(&self, val: &str) -> #all_builder_name {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        let query = #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.ends_with(val));
                        #all_builder_name::new(self, query)
                    }
                });
            }
            MacroColumnType::Numeric => {
                let gt_ident = Ident::new(&format!("find_by_{}_gt", col), proc_macro2::Span::call_site());
                let lt_ident = Ident::new(&format!("find_by_{}_lt", col), proc_macro2::Span::call_site());
                let ge_ident = Ident::new(&format!("find_by_{}_ge", col), proc_macro2::Span::call_site());
                let le_ident = Ident::new(&format!("find_by_{}_le", col), proc_macro2::Span::call_site());
                let between_ident = Ident::new(&format!("find_by_{}_between", col), proc_macro2::Span::call_site());

                jpa_dsl_methods.push(quote! {
                    pub fn #gt_ident<V>(&self, val: V) -> #all_builder_name
                    where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        let query = #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.gt(val));
                        #all_builder_name::new(self, query)
                    }

                    pub fn #lt_ident<V>(&self, val: V) -> #all_builder_name
                    where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        let query = #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.lt(val));
                        #all_builder_name::new(self, query)
                    }

                    pub fn #ge_ident<V>(&self, val: V) -> #all_builder_name
                    where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        let query = #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.gte(val));
                        #all_builder_name::new(self, query)
                    }

                    pub fn #le_ident<V>(&self, val: V) -> #all_builder_name
                    where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        let query = #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.lte(val));
                        #all_builder_name::new(self, query)
                    }

                    pub fn #between_ident<V>(&self, low: V, high: V) -> #all_builder_name
                    where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        let query = #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.between(low, high));
                        #all_builder_name::new(self, query)
                    }
                });
            }
            MacroColumnType::DateTime => {
                let after_ident = Ident::new(&format!("find_by_{}_after", col), proc_macro2::Span::call_site());
                let before_ident = Ident::new(&format!("find_by_{}_before", col), proc_macro2::Span::call_site());
                let between_ident = Ident::new(&format!("find_by_{}_between", col), proc_macro2::Span::call_site());
                let gt_ident = Ident::new(&format!("find_by_{}_gt", col), proc_macro2::Span::call_site());
                let lt_ident = Ident::new(&format!("find_by_{}_lt", col), proc_macro2::Span::call_site());
                let ge_ident = Ident::new(&format!("find_by_{}_ge", col), proc_macro2::Span::call_site());
                let le_ident = Ident::new(&format!("find_by_{}_le", col), proc_macro2::Span::call_site());

                jpa_dsl_methods.push(quote! {
                    pub async fn #after_ident<V>(&self, val: V) -> ::std::result::Result<::std::vec::Vec<#entity_module::Model>, ::gritshield::deps::sea_orm::DbErr>
                    where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.gt(val)).all(&self.db).await
                    }

                    pub async fn #before_ident<V>(&self, val: V) -> ::std::result::Result<::std::vec::Vec<#entity_module::Model>, ::gritshield::deps::sea_orm::DbErr>
                    where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.lt(val)).all(&self.db).await
                    }

                    pub fn #gt_ident<V>(&self, val: V) -> #all_builder_name
                    where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        let query = #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.gt(val));
                        #all_builder_name::new(self, query)
                    }

                    pub fn #lt_ident<V>(&self, val: V) -> #all_builder_name
                    where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        let query = #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.lt(val));
                        #all_builder_name::new(self, query)
                    }

                    pub async fn #ge_ident<V>(&self, val: V) -> ::std::result::Result<::std::vec::Vec<#entity_module::Model>, ::gritshield::deps::sea_orm::DbErr>
                    where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.gte(val)).all(&self.db).await
                    }

                    pub async fn #le_ident<V>(&self, val: V) -> ::std::result::Result<::std::vec::Vec<#entity_module::Model>, ::gritshield::deps::sea_orm::DbErr>
                    where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.lte(val)).all(&self.db).await
                    }

                    pub async fn #between_ident<V>(&self, low: V, high: V) -> ::std::result::Result<::std::vec::Vec<#entity_module::Model>, ::gritshield::deps::sea_orm::DbErr>
                    where V: ::std::convert::Into<::gritshield::deps::sea_orm::Value> {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.between(low, high)).all(&self.db).await
                    }
                });
            }
            MacroColumnType::Bool => {
                let true_ident = Ident::new(&format!("find_by_{}_true", col), proc_macro2::Span::call_site());
                let false_ident = Ident::new(&format!("find_by_{}_false", col), proc_macro2::Span::call_site());

                jpa_dsl_methods.push(quote! {
                    pub fn #true_ident(&self) -> #all_builder_name {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        let query = #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.eq(true));
                        #all_builder_name::new(self, query)
                    }

                    pub fn #false_ident(&self) -> #all_builder_name {
                        use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait};
                        let query = #entity_module::Entity::find().filter(#entity_module::Column::#variant_ident.eq(false));
                        #all_builder_name::new(self, query)
                    }
                });
            }
        }
    }

    // 2. Multi-Column Composite Invocations
    for i in 0..unique_fields.len() {
        for j in 0..unique_fields.len() {
            if i == j { continue; }
            let col1 = &unique_fields[i];
            let col2 = &unique_fields[j];

            let var1_ident = Ident::new(&to_pascal_case(col1), proc_macro2::Span::call_site());
            let var2_ident = Ident::new(&to_pascal_case(col2), proc_macro2::Span::call_site());

            let find_and_ident = Ident::new(&format!("find_by_{}_and_{}", col1, col2), proc_macro2::Span::call_site());
            let find_one_and_ident = Ident::new(&format!("find_one_by_{}_and_{}", col1, col2), proc_macro2::Span::call_site());
            let find_or_ident = Ident::new(&format!("find_by_{}_or_{}", col1, col2), proc_macro2::Span::call_site());
            let exists_and_ident = Ident::new(&format!("exists_by_{}_and_{}", col1, col2), proc_macro2::Span::call_site());
            let delete_and_ident = Ident::new(&format!("delete_by_{}_and_{}", col1, col2), proc_macro2::Span::call_site());

            jpa_dsl_methods.push(quote! {
                pub fn #find_and_ident<V1, V2>(&self, val1: V1, val2: V2) -> #all_builder_name
                where 
                    V1: ::std::convert::Into<::gritshield::deps::sea_orm::Value>,
                    V2: ::std::convert::Into<::gritshield::deps::sea_orm::Value>
                {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, Condition};
                    let condition = Condition::all().add(#entity_module::Column::#var1_ident.eq(val1)).add(#entity_module::Column::#var2_ident.eq(val2));
                    let query = #entity_module::Entity::find().filter(condition);
                    #all_builder_name::new(self, query)
                }

                pub fn #find_one_and_ident<V1, V2>(&self, val1: V1, val2: V2) -> #one_builder_name
                where 
                    V1: ::std::convert::Into<::gritshield::deps::sea_orm::Value>,
                    V2: ::std::convert::Into<::gritshield::deps::sea_orm::Value>
                {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, Condition};
                    let condition = Condition::all().add(#entity_module::Column::#var1_ident.eq(val1)).add(#entity_module::Column::#var2_ident.eq(val2));
                    let query = #entity_module::Entity::find().filter(condition);
                    #one_builder_name::new(self, query)
                }

                pub fn #find_or_ident<V1, V2>(&self, val1: V1, val2: V2) -> #all_builder_name
                where 
                    V1: ::std::convert::Into<::gritshield::deps::sea_orm::Value>,
                    V2: ::std::convert::Into<::gritshield::deps::sea_orm::Value>
                {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, Condition};
                    let condition = Condition::any().add(#entity_module::Column::#var1_ident.eq(val1)).add(#entity_module::Column::#var2_ident.eq(val2));
                    let query = #entity_module::Entity::find().filter(condition);
                    #all_builder_name::new(self, query)
                }

                pub async fn #exists_and_ident<V1, V2>(&self, val1: V1, val2: V2) -> ::std::result::Result<bool, ::gritshield::deps::sea_orm::DbErr>
                where 
                    V1: ::std::convert::Into<::gritshield::deps::sea_orm::Value>,
                    V2: ::std::convert::Into<::gritshield::deps::sea_orm::Value>
                {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, Condition, PaginatorTrait};
                    let condition = Condition::all().add(#entity_module::Column::#var1_ident.eq(val1)).add(#entity_module::Column::#var2_ident.eq(val2));
                    let count = #entity_module::Entity::find().filter(condition).count(&self.db).await?;
                    ::std::result::Result::Ok(count > 0)
                }

                pub async fn #delete_and_ident<V1, V2>(&self, val1: V1, val2: V2) -> ::std::result::Result<u64, ::gritshield::deps::sea_orm::DbErr>
                where 
                    V1: ::std::convert::Into<::gritshield::deps::sea_orm::Value>,
                    V2: ::std::convert::Into<::gritshield::deps::sea_orm::Value>
                {
                    use ::gritshield::deps::sea_orm::{EntityTrait, QueryFilter, ColumnTrait, Condition};
                    let condition = Condition::all().add(#entity_module::Column::#var1_ident.eq(val1)).add(#entity_module::Column::#var2_ident.eq(val2));
                    let res = #entity_module::Entity::delete_many().filter(condition).exec(&self.db).await?;
                    ::std::result::Result::Ok(res.rows_affected)
                }
            });
        }
    }

    quote! { #(#jpa_dsl_methods)* }
}