// src/repository/model.rs
use crate::repository::jpa_dsl::{generate_model_specific_methods, type_to_column_type};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Ident, Meta, Path, Result};

pub fn expand_model(input: DeriveInput) -> Result<TokenStream> {
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            syn::Fields::Named(f) => &f.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "GritModel only supports named field structures",
                ))
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "GritModel can only be derived on Structs",
            ))
        }
    };

    let mut repo_path: Option<Path> = None;
    for attr in &input.attrs {
        if attr.path().is_ident("grit") {
            if let Meta::List(meta_list) = &attr.meta {
                let _ = meta_list.parse_nested_meta(|meta| {
                    if meta.path.is_ident("repository") {
                        let value = meta.value()?;
                        let path_str: syn::LitStr = value.parse()?;
                        repo_path = Some(path_str.parse::<Path>()?);
                    }
                    Ok(())
                });
            }
        }
    }

    let repo_path = repo_path.ok_or_else(|| {
        syn::Error::new_spanned(
            &input,
            "Missing required link attribute notation: #[grit(repository = \"...\")]",
        )
    })?;

    let repo_ident = &repo_path.segments.last().unwrap().ident;
    let all_builder_name = Ident::new(&format!("{}RAQB", repo_ident), Span::call_site());

    let mut parsed_fields = Vec::new();
    for field in fields {
        let ident = field.ident.clone().unwrap();
        let col_type = type_to_column_type(&field.ty);
        parsed_fields.push((ident, col_type));
    }

    // Inside the model file: Entity, Column, and QueryBuilders are all local to this module scope!
    let entity_path = quote! { Entity };
    let column_path = quote! { Column };
    let all_builder_path = quote! { #all_builder_name };

    let jpa_methods_block = generate_model_specific_methods(
        &entity_path,
        &column_path,
        &all_builder_path,
        &parsed_fields,
    );

    Ok(quote! {
        impl #repo_path {
            #jpa_methods_block
        }
    })
}
