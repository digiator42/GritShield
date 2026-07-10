use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

pub fn generate_builders(
    entity_module: &TokenStream,
    extended_ident: &Ident,
    all_builder_name: &Ident,
    one_builder_name: &Ident,
    parsed_has_many: &[(String, Path)],
    parsed_has_one: &[(String, Path)],
    parsed_belongs_to: &[(String, Path, Option<String>)],
) -> TokenStream {
    let has_many_idents: Vec<Ident> = parsed_has_many
        .iter()
        .map(|(f, _)| Ident::new(&format!("load_{}", f), proc_macro2::Span::call_site()))
        .collect();
    let with_many_idents: Vec<Ident> = parsed_has_many
        .iter()
        .map(|(f, _)| Ident::new(&format!("with_{}", f), proc_macro2::Span::call_site()))
        .collect();
    let has_many_fields: Vec<Ident> = parsed_has_many
        .iter()
        .map(|(f, _)| Ident::new(f, proc_macro2::Span::call_site()))
        .collect();
    let relation_many_variants: Vec<Path> =
        parsed_has_many.iter().map(|(_, t)| t.clone()).collect();

    let has_one_idents: Vec<Ident> = parsed_has_one
        .iter()
        .map(|(f, _)| Ident::new(&format!("load_{}", f), proc_macro2::Span::call_site()))
        .collect();
    let with_one_idents: Vec<Ident> = parsed_has_one
        .iter()
        .map(|(f, _)| Ident::new(&format!("with_{}", f), proc_macro2::Span::call_site()))
        .collect();
    let has_one_fields: Vec<Ident> = parsed_has_one
        .iter()
        .map(|(f, _)| Ident::new(f, proc_macro2::Span::call_site()))
        .collect();
    let belongs_to_idents: Vec<Ident> = parsed_belongs_to
        .iter()
        .map(|(f, _, _)| Ident::new(&format!("load_{}", f), proc_macro2::Span::call_site()))
        .collect();
    let with_belongs_idents: Vec<Ident> = parsed_belongs_to
        .iter()
        .map(|(f, _, _)| Ident::new(&format!("with_{}", f), proc_macro2::Span::call_site()))
        .collect();
    let belongs_to_fields: Vec<Ident> = parsed_belongs_to
        .iter()
        .map(|(f, _, _)| Ident::new(f, proc_macro2::Span::call_site()))
        .collect();
    let relation_belongs_variants: Vec<Path> = parsed_belongs_to
        .iter()
        .map(|(_, t, _)| t.clone())
        .collect();

    let relation_one_variants: Vec<Path> = parsed_has_one.iter().map(|(_, t)| t.clone()).collect();

    let none_initializers = quote! {
        #( #has_many_fields: ::std::option::Option::None, )*
        #( #has_one_fields: ::std::option::Option::None, )*
        #( #belongs_to_fields: ::std::option::Option::None, )*
    };

    // Luxury Navigation Properties Vectors
    let with_nested_many_idents: Vec<Ident> = parsed_has_many
        .iter()
        .map(|(f, _)| {
            Ident::new(
                &format!("with_{}_nested", f),
                proc_macro2::Span::call_site(),
            )
        })
        .collect();

    let nested_many_closure_fields: Vec<Ident> = parsed_has_many
        .iter()
        .map(|(f, _)| Ident::new(&format!("closure_{}", f), proc_macro2::Span::call_site()))
        .collect();

    // Dynamically derive downstream builders and records by replacing standard naming endings
    let target_builders: Vec<Path> = parsed_has_many
        .iter()
        .map(|(_, path)| {
            let mut p = path.clone();
            if let Some(last) = p.segments.last_mut() {
                last.ident = Ident::new("GritAllQueryBuilder", proc_macro2::Span::call_site());
            }
            p
        })
        .collect();

    quote! {
        #[derive(::std::clone::Clone, ::std::fmt::Debug, ::serde::Serialize, ::serde::Deserialize)]
        pub struct #extended_ident {
            #[serde(flatten)]
            pub core: #entity_module::Model,
            #(
                #[serde(skip_serializing_if = "::std::option::Option::is_none")]
                pub #has_many_fields: ::std::option::Option<::std::vec::Vec<<#relation_many_variants as ::gritshield::deps::sea_orm::EntityTrait>::Model>>,
            )*
            #(
                #[serde(skip_serializing_if = "::std::option::Option::is_none")]
                pub #has_one_fields: ::std::option::Option<<#relation_one_variants as ::gritshield::deps::sea_orm::EntityTrait>::Model>,
            )*
            #(
                #[serde(skip_serializing_if = "::std::option::Option::is_none")]
                pub #belongs_to_fields: ::std::option::Option<<#relation_belongs_variants as ::gritshield::deps::sea_orm::EntityTrait>::Model>,
            )*

            #[serde(skip_serializing_if = "::std::collections::HashMap::is_empty", default)]
            pub nested_relations: ::std::collections::HashMap<::std::string::String, ::gritshield::deps::serde_json::Value>,
        }

        impl ::std::ops::Deref for #extended_ident {
            type Target = #entity_module::Model;
            fn deref(&self) -> &Self::Target { &self.core }
        }

        impl ::std::ops::DerefMut for #extended_ident {
            fn deref_mut(&mut self) -> &mut Self::Target { &mut self.core }
        }

        // RAQB (Relation All Query Builder)
        pub struct #all_builder_name<'a> {
            db: &'a ::gritshield::deps::sea_orm::DatabaseConnection,
            query: ::gritshield::deps::sea_orm::Select<#entity_module::Entity>,
            #( #has_many_idents: bool, )*
            #( #has_one_idents: bool, )*
            #( #belongs_to_idents: bool, )*

            // Allocation storage for luxury chaining functions
            #( #nested_many_closure_fields: ::std::option::Option<::std::boxed::Box<dyn Fn(#target_builders<'a>) -> #target_builders<'a> + ::std::marker::Send + ::std::marker::Sync + 'a>>, )*
        }

        impl<'a> #all_builder_name<'a> {
            pub fn new(db: &'a ::gritshield::deps::sea_orm::DatabaseConnection, query: ::gritshield::deps::sea_orm::Select<#entity_module::Entity>) -> Self {
                Self {
                    db,
                    query,
                    #( #has_many_idents: false, )*
                    #( #has_one_idents: false, )*
                    #( #belongs_to_idents: false, )*
                    #( #nested_many_closure_fields: ::std::option::Option::None, )*
                }
            }

            #( pub fn #with_many_idents(mut self) -> Self { self.#has_many_idents = true; self } )*
            #( pub fn #with_one_idents(mut self) -> Self { self.#has_one_idents = true; self } )*
            #( pub fn #with_belongs_idents(mut self) -> Self { self.#belongs_to_idents = true; self } )*

            // Zero-boilerplate nesting injection function
            #(
                pub fn #with_nested_many_idents<F>(mut self, loader_logic: F) -> Self
                where
                    F: Fn(#target_builders<'a>) -> #target_builders<'a> + ::std::marker::Send + ::std::marker::Sync + 'a
                {
                    self.#has_many_idents = true;
                    self.#nested_many_closure_fields = ::std::option::Option::Some(::std::boxed::Box::new(loader_logic));
                    self
                }
            )*

            pub async fn execute(mut self) -> ::std::result::Result<::std::vec::Vec<#extended_ident>, ::gritshield::deps::sea_orm::DbErr> {
                use ::gritshield::deps::sea_orm::{EntityTrait, QuerySelect};
                let core_models = self.query.clone().all(self.db).await?;
                let mut ids = ::std::vec::Vec::new();
                let mut records_map = ::std::collections::HashMap::new();

                for m in core_models {
                    let id = m.id;
                    ids.push(id);
                    records_map.insert(id, #extended_ident { core: m, nested_relations: ::std::collections::HashMap::new(), #none_initializers });
                }

                #(
                    if self.#has_many_idents {
                        let pairs = self.query.clone().find_with_related(<#relation_many_variants>::default()).all(self.db).await?;
                        for (core_model, related) in pairs {
                            if let Some(rec) = records_map.get_mut(&core_model.id) { rec.#has_many_fields = ::std::option::Option::Some(related); }
                        }
                    }
                )*

                #(
                    if self.#has_one_idents {
                        let pairs = self.query.clone().find_also_related(<#relation_one_variants>::default()).all(self.db).await?;
                        for (core_model, opt_related) in pairs {
                            if let Some(rec) = records_map.get_mut(&core_model.id) { rec.#has_one_fields = opt_related; }
                        }
                    }
                )*
                #(
                    if self.#belongs_to_idents {
                        let pairs = self.query.clone().find_also_related(<#relation_belongs_variants>::default()).all(self.db).await?;
                        for (core_model, opt_related) in pairs {
                            if let Some(rec) = records_map.get_mut(&core_model.id) { rec.#belongs_to_fields = opt_related; }
                        }
                    }
                )*

                // Automated Sub-tree Batch Loading Processing Strategy
                #(
                    if let ::std::option::Option::Some(closure) = self.#nested_many_closure_fields {
                        let mut all_child_ids = ::std::vec::Vec::new();
                        for rec in records_map.values() {
                            if let ::std::option::Option::Some(children) = &rec.#has_many_fields {
                                for child in children {
                                    all_child_ids.push(child.id);
                                }
                            }
                        }

                        if !all_child_ids.is_empty() {
                            use ::gritshield::deps::sea_orm::{QueryFilter, ColumnTrait};
                            let target_query = <#relation_many_variants as EntityTrait>::find()
                                .filter(<#relation_many_variants as EntityTrait>::Column::Id.is_in(all_child_ids));

                            let target_builder = #target_builders::new(self.db, target_query);
                            let configured_builder = (closure)(target_builder);
                            let extended_children = configured_builder.execute().await?;

                            let mut children_map = ::std::collections::HashMap::new();
                            for ext_child in extended_children {
                                children_map.insert(ext_child.core.id, ext_child);
                            }

                            for rec in records_map.values_mut() {
                                if let ::std::option::Option::Some(children) = &rec.#has_many_fields {
                                    let mut back_mapped = ::std::vec::Vec::new();
                                    for child in children {
                                        if let Some(ext_child) = children_map.get(&child.id) {
                                            back_mapped.push(ext_child.clone());
                                        }
                                    }
                                    let json_blob = ::gritshield::deps::serde_json::to_value(back_mapped).unwrap();
                                    rec.nested_relations.insert(::std::string::String::from(stringify!(#has_many_fields)), json_blob);
                                }
                            }
                        }
                    }
                )*

                let mut results = ::std::vec::Vec::new();
                for id in ids {
                    if let Some(rec) = records_map.remove(&id) { results.push(rec); }
                }
                ::std::result::Result::Ok(results)
            }
        }

        impl<'a> ::std::future::IntoFuture for #all_builder_name<'a> {
            type Output = ::std::result::Result<::std::vec::Vec<#extended_ident>, ::gritshield::deps::sea_orm::DbErr>;
            type IntoFuture = ::gritshield::futures::future::BoxFuture<'a, Self::Output>;
            fn into_future(self) -> Self::IntoFuture { ::std::boxed::Box::pin(self.execute()) }
        }

        // ROQB (Relation One Query Builder)
        pub struct #one_builder_name<'a> {
            db: &'a ::gritshield::deps::sea_orm::DatabaseConnection,
            query: ::gritshield::deps::sea_orm::Select<#entity_module::Entity>,
            #( #has_many_idents: bool, )*
            #( #has_one_idents: bool, )*
            #( #belongs_to_idents: bool, )*
            #( #nested_many_closure_fields: ::std::option::Option<::std::boxed::Box<dyn Fn(#target_builders<'a>) -> #target_builders<'a> + ::std::marker::Send + ::std::marker::Sync + 'a>>, )*
        }

        impl<'a> #one_builder_name<'a> {
            pub fn new(db: &'a ::gritshield::deps::sea_orm::DatabaseConnection, query: ::gritshield::deps::sea_orm::Select<#entity_module::Entity>) -> Self {
                Self {
                    db,
                    query,
                    #( #has_many_idents: false, )*
                    #( #has_one_idents: false, )*
                    #( #belongs_to_idents: false, )*
                    #( #nested_many_closure_fields: ::std::option::Option::None, )*
                }
            }

            #( pub fn #with_many_idents(mut self) -> Self { self.#has_many_idents = true; self } )*
            #( pub fn #with_one_idents(mut self) -> Self { self.#has_one_idents = true; self } )*
            #( pub fn #with_belongs_idents(mut self) -> Self { self.#belongs_to_idents = true; self } )*

            #(
                pub fn #with_nested_many_idents<F>(mut self, loader_logic: F) -> Self
                where
                    F: Fn(#target_builders<'a>) -> #target_builders<'a> + ::std::marker::Send + ::std::marker::Sync + 'a
                {
                    self.#has_many_idents = true;
                    self.#nested_many_closure_fields = ::std::option::Option::Some(::std::boxed::Box::new(loader_logic));
                    self
                }
            )*

            pub async fn execute(self) -> ::std::result::Result<::std::option::Option<#extended_ident>, ::gritshield::deps::sea_orm::DbErr> {
                use ::gritshield::deps::sea_orm::{EntityTrait, QuerySelect};
                let opt_model = self.query.clone().one(self.db).await?;
                let core_model = match opt_model {
                    ::std::option::Option::Some(m) => m,
                    ::std::option::Option::None => return ::std::result::Result::Ok(::std::option::Option::None),
                };

                let mut rec = #extended_ident { core: core_model, nested_relations: ::std::collections::HashMap::new(), #none_initializers };

                #(
                    if self.#has_many_idents {
                        let pairs = self.query.clone().find_with_related(<#relation_many_variants>::default()).all(self.db).await?;
                        if let ::std::option::Option::Some((_, related)) = pairs.into_iter().next() { rec.#has_many_fields = ::std::option::Option::Some(related); }
                    }
                )*

                #(
                    if self.#has_one_idents {
                        let pairs = self.query.clone().find_also_related(<#relation_one_variants>::default()).all(self.db).await?;
                        if let ::std::option::Option::Some((_, opt_related)) = pairs.into_iter().next() { rec.#has_one_fields = opt_related; }
                    }
                )*

                #(
                    if let ::std::option::Option::Some(closure) = self.#nested_many_closure_fields {
                        if let ::std::option::Option::Some(children) = &rec.#has_many_fields {
                            let all_child_ids: ::std::vec::Vec<_> = children.iter().map(|c| c.id).collect();
                            if !all_child_ids.is_empty() {
                                use ::gritshield::deps::sea_orm::{QueryFilter, ColumnTrait};
                                let target_query = <#relation_many_variants as EntityTrait>::find()
                                    .filter(<#relation_many_variants as EntityTrait>::Column::Id.is_in(all_child_ids));
                                let target_builder = #target_builders::new(self.db, target_query);
                                let configured_builder = (closure)(target_builder);
                                let extended_children = configured_builder.execute().await?;

                                let mut children_map = ::std::collections::HashMap::new();
                                for ext_child in extended_children {
                                    children_map.insert(ext_child.core.id, ext_child);
                                }

                                let mut back_mapped = ::std::vec::Vec::new();
                                for child in children {
                                    if let Some(ext_child) = children_map.get(&child.id) {
                                        back_mapped.push(ext_child.clone());
                                    }
                                }
                                let json_blob = ::gritshield::deps::serde_json::to_value(back_mapped).unwrap();
                                rec.nested_relations.insert(::std::string::String::from(stringify!(#has_many_fields)), json_blob);
                            }
                        }
                    }
                )*

                ::std::result::Result::Ok(::std::option::Option::Some(rec))
            }
        }

        impl<'a> ::std::future::IntoFuture for #one_builder_name<'a> {
            type Output = ::std::result::Result<::std::option::Option<#extended_ident>, ::gritshield::deps::sea_orm::DbErr>;
            type IntoFuture = ::gritshield::futures::future::BoxFuture<'a, Self::Output>;
            fn into_future(self) -> Self::IntoFuture { ::std::boxed::Box::pin(self.execute()) }
        }
    }
}
