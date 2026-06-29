use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

pub fn generate_builders(
    name: &Ident,
    entity_module: &TokenStream,
    extended_ident: &Ident,
    all_builder_name: &Ident,
    one_builder_name: &Ident,
    parsed_has_many: &[(String, Path)],
    parsed_has_one: &[(String, Path)],
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
    let relation_one_variants: Vec<Path> = parsed_has_one.iter().map(|(_, t)| t.clone()).collect();

    let none_initializers = quote! {
        #( #has_many_fields: ::std::option::Option::None, )*
        #( #has_one_fields: ::std::option::Option::None, )*
    };

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
        }

        impl ::std::ops::Deref for #extended_ident {
            type Target = #entity_module::Model;
            fn deref(&self) -> &Self::Target { &self.core }
        }

        impl ::std::ops::DerefMut for #extended_ident {
            fn deref_mut(&mut self) -> &mut Self::Target { &mut self.core }
        }

        // RAQB
        pub struct #all_builder_name<'a> {
            repo: &'a #name,
            query: ::gritshield::deps::sea_orm::Select<#entity_module::Entity>,
            #( #has_many_idents: bool, )*
            #( #has_one_idents: bool, )*
        }

        impl<'a> #all_builder_name<'a> {
            pub fn new(repo: &'a #name, query: ::gritshield::deps::sea_orm::Select<#entity_module::Entity>) -> Self {
                Self {
                    repo,
                    query,
                    #( #has_many_idents: false, )*
                    #( #has_one_idents: false, )*
                }
            }

            #( pub fn #with_many_idents(mut self) -> Self { self.#has_many_idents = true; self } )*
            #( pub fn #with_one_idents(mut self) -> Self { self.#has_one_idents = true; self } )*

            pub async fn execute(self) -> ::std::result::Result<::std::vec::Vec<#extended_ident>, ::gritshield::deps::sea_orm::DbErr> {
                use ::gritshield::deps::sea_orm::{EntityTrait, QuerySelect};
                let core_models = self.query.clone().all(&self.repo.db).await?;
                let mut ids = ::std::vec::Vec::new();
                let mut records_map = ::std::collections::HashMap::new();

                for m in core_models {
                    let id = m.id;
                    ids.push(id);
                    records_map.insert(id, #extended_ident { core: m, #none_initializers });
                }

                #(
                    if self.#has_many_idents {
                        let pairs = self.query.clone().find_with_related(<#relation_many_variants>::default()).all(&self.repo.db).await?;
                        for (core_model, related) in pairs {
                            if let Some(rec) = records_map.get_mut(&core_model.id) { rec.#has_many_fields = ::std::option::Option::Some(related); }
                        }
                    }
                )*

                #(
                    if self.#has_one_idents {
                        let pairs = self.query.clone().find_also_related(<#relation_one_variants>::default()).all(&self.repo.db).await?;
                        for (core_model, opt_related) in pairs {
                            if let Some(rec) = records_map.get_mut(&core_model.id) { rec.#has_one_fields = opt_related; }
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

        // ROQB
        pub struct #one_builder_name<'a> {
            repo: &'a #name,
            query: ::gritshield::deps::sea_orm::Select<#entity_module::Entity>,
            #( #has_many_idents: bool, )*
            #( #has_one_idents: bool, )*
        }

        impl<'a> #one_builder_name<'a> {
            pub fn new(repo: &'a #name, query: ::gritshield::deps::sea_orm::Select<#entity_module::Entity>) -> Self {
                Self {
                    repo,
                    query,
                    #( #has_many_idents: false, )*
                    #( #has_one_idents: false, )*
                }
            }

            #( pub fn #with_many_idents(mut self) -> Self { self.#has_many_idents = true; self } )*
            #( pub fn #with_one_idents(mut self) -> Self { self.#has_one_idents = true; self } )*

            pub async fn execute(self) -> ::std::result::Result<::std::option::Option<#extended_ident>, ::gritshield::deps::sea_orm::DbErr> {
                use ::gritshield::deps::sea_orm::{EntityTrait, QuerySelect};
                let opt_model = self.query.clone().one(&self.repo.db).await?;
                let core_model = match opt_model {
                    ::std::option::Option::Some(m) => m,
                    ::std::option::Option::None => return ::std::result::Result::Ok(::std::option::Option::None),
                };

                let mut rec = #extended_ident { core: core_model, #none_initializers };

                #(
                    if self.#has_many_idents {
                        let pairs = self.query.clone().find_with_related(<#relation_many_variants>::default()).all(&self.repo.db).await?;
                        if let ::std::option::Option::Some((_, related)) = pairs.into_iter().next() { rec.#has_many_fields = ::std::option::Option::Some(related); }
                    }
                )*

                #(
                    if self.#has_one_idents {
                        let pairs = self.query.clone().find_also_related(<#relation_one_variants>::default()).all(&self.repo.db).await?;
                        if let ::std::option::Option::Some((_, opt_related)) = pairs.into_iter().next() { rec.#has_one_fields = opt_related; }
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
