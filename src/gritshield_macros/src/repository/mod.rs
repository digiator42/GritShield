// src/repository/mod.rs
use crate::core_parser::parse_repository_attributes;
use proc_macro2::TokenStream;
use quote::quote;
use syn::DeriveInput;

pub mod query_dsl;
pub mod model;
pub mod query_builders;
pub mod relation;

pub fn expand_repository(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;

    let repo_attrs = parse_repository_attributes(&input.attrs)?;
    let mut grid_columns = repo_attrs.grid_columns;
    let searchable_columns = repo_attrs.searchable_columns;

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

    let mut unique_fields = grid_columns.clone();
    for col in &searchable_columns {
        if !unique_fields.contains(col) {
            unique_fields.push(col.clone());
        }
    }

    let converted_fields: Vec<(syn::Ident, crate::repository::query_dsl::ModelColumnType)> =
        unique_fields
            .iter()
            .map(|field_str| {
                (
                    syn::Ident::new(field_str, proc_macro2::Span::call_site()),
                    crate::repository::query_dsl::ModelColumnType::Unknown,
                )
            })
            .collect();

    // Point explicitly to the namespaced static builder targets inside the target module!
    let entity_path = quote! { #entity_module::Entity };
    let column_path = quote! { #entity_module::Column };
    let all_builder_path = quote! { #entity_module::GritAllQueryBuilder };
    let one_builder_path = quote! { #entity_module::GritOneQueryBuilder };

    let query_methods_block = crate::repository::query_dsl::generate_query_methods(
        &entity_path,
        &column_path,
        &all_builder_path,
        &one_builder_path,
        &converted_fields,
    );

    Ok(quote! {
        impl #name {
            #query_methods_block
        }
    })
}
