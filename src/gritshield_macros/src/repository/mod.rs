// src/repository/mod.rs
use crate::core_parser::parse_repository_attributes;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::DeriveInput;

pub mod jpa_dsl;
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

    let all_builder_name = syn::Ident::new(&format!("{}RAQB", name), name.span());
    let one_builder_name = syn::Ident::new(&format!("{}ROQB", name), name.span());

    let mut unique_fields = grid_columns.clone();
    for col in &searchable_columns {
        if !unique_fields.contains(col) {
            unique_fields.push(col.clone());
        }
    }

    let converted_fields: Vec<(syn::Ident, crate::repository::jpa_dsl::ModelColumnType)> =
        unique_fields
            .iter()
            .map(|field_str| {
                (
                    syn::Ident::new(field_str, proc_macro2::Span::call_site()),
                    crate::repository::jpa_dsl::ModelColumnType::Unknown,
                )
            })
            .collect();

    // Construct precise cross-module tokens pointing directly to the entity module context where builders live
    let entity_path = quote! { #entity_module::Entity };
    let column_path = quote! { #entity_module::Column };
    let all_builder_path = quote! { #entity_module::#all_builder_name };
    let one_builder_path = quote! { #entity_module::#one_builder_name };

    let jpa_methods_block = crate::repository::jpa_dsl::generate_jpa_methods(
        &entity_path,
        &column_path,
        &all_builder_path,
        &one_builder_path,
        &converted_fields,
    );

    Ok(quote! {
        impl #name {
            #jpa_methods_block
        }
    })
}
