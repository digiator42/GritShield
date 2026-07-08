use crate::repository::query_dsl::{generate_query_methods, type_to_column_type, ModelColumnType};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Ident, Meta, Path, Result, Type};

pub fn expand_model(input: DeriveInput) -> Result<TokenStream> {
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            syn::Fields::Named(f) => &f.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "GritModel only supports named fields",
                ))
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "GritModel must be a Struct",
            ))
        }
    };

    let mut repo_path: Option<Path> = None;
    let mut table_name: Option<String> = None;

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
        } else if attr.path().is_ident("sea_orm") {
            if let Meta::List(meta_list) = &attr.meta {
                let _ = meta_list.parse_nested_meta(|meta| {
                    if meta.path.is_ident("table_name") {
                        let value = meta.value()?;
                        let lit_str: syn::LitStr = value.parse()?;
                        table_name = Some(lit_str.value());
                    }
                    Ok(())
                });
            }
        }
    }

    let table_str = table_name.clone().ok_or_else(|| {
        syn::Error::new_spanned(
            &input,
            "GritModel requires a table_name to derive repository path",
        )
    })?;

    let module_name = if table_str.ends_with('s') && table_str.len() > 1 {
        &table_str[..table_str.len() - 1]
    } else {
        &table_str
    };

    let repo_path = match repo_path {
        Some(path) => path,
        None => {
            let mut chars = module_name.chars();
            let pascal_repo = chars.next().unwrap().to_uppercase().collect::<String>()
                + chars.as_str()
                + "Repository";
            let derived_path_str = format!("crate::repositories::{}::{}", module_name, pascal_repo);
            syn::parse_str::<Path>(&derived_path_str)?
        }
    };

    // ---- Parse fields with enhanced metadata ----
    let mut parsed_fields = Vec::new();
    let mut field_schemas = Vec::new();

    for field in fields {
        let ident = field.ident.clone().unwrap();
        let (col_type, nullable) = type_to_column_type(&field.ty);

        // Parse primary key attribute
        let mut primary_key = false;
        for attr in &field.attrs {
            if attr.path().is_ident("sea_orm") {
                if let Meta::List(meta_list) = &attr.meta {
                    let _ = meta_list.parse_nested_meta(|meta| {
                        if meta.path.is_ident("primary_key") {
                            primary_key = true;
                        }
                        Ok(())
                    });
                }
            }
        }

        parsed_fields.push((ident.clone(), col_type));

        // Build schema for registration
        let name = ident.to_string();
        let type_str = match col_type {
            ModelColumnType::String => "String",
            ModelColumnType::Numeric => "i64",
            ModelColumnType::DateTime => "NaiveDateTime",
            ModelColumnType::Bool => "bool",
            ModelColumnType::Unknown => "unknown",
        };

        let primary_key_val = primary_key;
        let nullable_val = nullable;

        field_schemas.push(quote! {
            ::gritshield::core::schema::FieldSchema {
                name: #name.to_string(),
                type_: #type_str.to_string(),
                nullable: #nullable_val,
                primary_key: #primary_key_val,
            }
        });
    }

    // ---- Create the registration function ----
    let table_name_str = table_name.clone().unwrap();
    let register_fn_name = Ident::new(
        &format!("register_model_schema_{}", module_name),
        proc_macro2::Span::call_site(),
    );

    let registration = quote! {
        #[::gritshield::startup::ctor(unsafe)]
        fn #register_fn_name() {
            let fields = vec![ #(#field_schemas),* ];
            let relations = vec![]; // relations will be added by GritRelation
            ::gritshield::core::schema::register_model_schema(#table_name_str, fields, relations);
        }
    };

    // ---- Build query DSL ----
    let module_ident = Ident::new(module_name, Span::call_site());
    let entity_module = quote! { crate::models::#module_ident };

    let entity_path = quote! { #entity_module::Entity };
    let column_path = quote! { #entity_module::Column };
    let all_builder_path = quote! { #entity_module::GritAllQueryBuilder };
    let one_builder_path = quote! { #entity_module::GritOneQueryBuilder };

    let query_methods_block = generate_query_methods(
        &entity_path,
        &column_path,
        &all_builder_path,
        &one_builder_path,
        &parsed_fields,
    );

    Ok(quote! {
        impl #repo_path {
            #query_methods_block
        }
        #registration
    })
}
