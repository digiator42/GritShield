use proc_macro2::{Span, TokenStream};
use quote::quote;
use quote::ToTokens;
use syn::{Data, DeriveInput, Ident, LitStr, Meta, Path, Result};

pub fn expand_relation(input: DeriveInput) -> Result<TokenStream> {
    let variants = match &input.data {
        Data::Enum(e) => &e.variants,
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "GritRelation can only be derived on Enums",
            ))
        }
    };

    // ---- Extract table name from the enum's attributes ----
    let mut table_name: Option<String> = None;

    for attr in &input.attrs {
        if attr.path().is_ident("grit") {
            if let Meta::List(meta_list) = &attr.meta {
                let _ = meta_list.parse_nested_meta(|meta| {
                    if meta.path.is_ident("table") {
                        let value = meta.value()?;
                        let lit_str: LitStr = value.parse()?;
                        table_name = Some(lit_str.value());
                    }
                    Ok(())
                });
            }
        }
        if attr.path().is_ident("sea_orm") {
            if let Meta::List(meta_list) = &attr.meta {
                let _ = meta_list.parse_nested_meta(|meta| {
                    if meta.path.is_ident("table_name") {
                        let value = meta.value()?;
                        let lit_str: LitStr = value.parse()?;
                        table_name = Some(lit_str.value());
                    }
                    Ok(())
                });
            }
        }
    }

    let table_name_str = table_name.ok_or_else(|| {
        syn::Error::new_spanned(
            &input,
            "GritRelation requires a table name. Add #[grit(table = \"your_table\")] or #[sea_orm(table_name = \"your_table\")] to the enum.",
        )
    })?;

    // ---- Create a unique registration function name ----
    let register_fn_name = Ident::new(
        &format!("register_relations_{}", table_name_str),
        proc_macro2::Span::call_site(),
    );

    // Standardized static names
    let all_builder_name = Ident::new("GritAllQueryBuilder", Span::call_site());
    let one_builder_name = Ident::new("GritOneQueryBuilder", Span::call_site());
    let extended_ident = Ident::new("GritRepositoryRecord", Span::call_site());

    let entity_module = quote! { self };

    let mut parsed_has_many: Vec<(String, Path)> = Vec::new();
    let mut parsed_has_one: Vec<(String, Path)> = Vec::new();
    let mut parsed_belongs_to: Vec<(String, Path, Option<String>)> = Vec::new(); // (field, target_path, foreign_key)

    // Fix: Proper parsing loop for flat sibling key-value pairs per variant
    for variant in variants {
        let variant_field_name = variant.ident.to_string().to_lowercase();

        let mut belongs_to_path: Option<Path> = None;
        let mut foreign_key: Option<String> = None;
        let mut is_belongs_to = false;

        for attr in &variant.attrs {
            if attr.path().is_ident("sea_orm") {
                if let Meta::List(meta_list) = &attr.meta {
                    let _ = meta_list.parse_nested_meta(|meta| {
                        if meta.path.is_ident("has_many") {
                            let value = meta.value()?;
                            let lit_str: LitStr = value.parse()?;
                            if let Ok(target_path) = lit_str.parse::<Path>() {
                                let field = format!("{}s", variant_field_name);
                                parsed_has_many.push((field, target_path));
                            }
                        } else if meta.path.is_ident("has_one") {
                            let value = meta.value()?;
                            let lit_str: LitStr = value.parse()?;
                            if let Ok(target_path) = lit_str.parse::<Path>() {
                                parsed_has_one.push((variant_field_name.clone(), target_path));
                            }
                        } else if meta.path.is_ident("belongs_to") {
                            is_belongs_to = true;
                            let value = meta.value()?;
                            let lit_str: LitStr = value.parse()?;
                            if let Ok(target_path) = lit_str.parse::<Path>() {
                                belongs_to_path = Some(target_path);
                            }
                        } else if meta.path.is_ident("from") {
                            let value = meta.value()?;
                            let lit_str: LitStr = value.parse()?;
                            if let Ok(path) = lit_str.parse::<Path>() {
                                if let Some(segment) = path.segments.last() {
                                    foreign_key = Some(segment.ident.to_string());
                                }
                            } else {
                                let val_str = lit_str.value();
                                let clean_fk = val_str.split("::").last().unwrap_or(&val_str).to_string();
                                foreign_key = Some(clean_fk);
                            }
                        } else if meta.path.is_ident("to") {
                            let value = meta.value()?;
                            let _: LitStr = value.parse()?;
                        }
                        Ok(())
                    });
                }
            }
        }

        if is_belongs_to {
            if let Some(path) = belongs_to_path {
                parsed_belongs_to.push((
                    variant_field_name.clone(),
                    path,
                    foreign_key,
                ));
            }
        }
    }

    // ---- Helper function to extract table name from path ----
    let extract_table_name = |path: &Path| -> String {
        if let Some(module_segment) = path.segments.iter().nth_back(1) {
            return module_segment.ident.to_string();
        }

        if let Some(last) = path.segments.last() {
            let name = last.ident.to_string();
            let mut snake = String::new();
            for (i, c) in name.chars().enumerate() {
                if i > 0 && c.is_uppercase() {
                    snake.push('_');
                }
                snake.push(c.to_ascii_lowercase());
            }
            return snake;
        }

        "unknown".to_string()
    };

    // ---- Build RelationSchema for each parsed relation ----
    let table_name_lit = LitStr::new(&table_name_str, proc_macro2::Span::call_site());
    let mut relations = Vec::new();

    // HasMany relations
    for (field, target_path) in &parsed_has_many {
        let target_table = extract_table_name(target_path);
        let target_table_lit = LitStr::new(&target_table, proc_macro2::Span::call_site());
        relations.push(quote! {
            ::gritshield::core::schema::RelationSchema {
                kind: ::gritshield::core::schema::RelationKind::HasMany,
                target_table: #target_table_lit.to_string(),
                foreign_key: ::std::option::Option::None,
            }
        });
    }

    // HasOne relations
    for (_field, target_path) in &parsed_has_one {
        let target_table = extract_table_name(target_path);
        let target_table_lit = LitStr::new(&target_table, proc_macro2::Span::call_site());
        relations.push(quote! {
            ::gritshield::core::schema::RelationSchema {
                kind: ::gritshield::core::schema::RelationKind::HasOne,
                target_table: #target_table_lit.to_string(),
                foreign_key: ::std::option::Option::None,
            }
        });
    }

    // BelongsTo relations
    for (_field, target_path, foreign_key) in &parsed_belongs_to {
        let target_table = extract_table_name(target_path);
        let target_table_lit = LitStr::new(&target_table, proc_macro2::Span::call_site());
        
        // Fix: Evaluate Option mapping at macro compile-time instead of output code runtime
        let fk_value = match foreign_key {
            Some(fk) => {
                let fk_lit = LitStr::new(fk, proc_macro2::Span::call_site());
                quote! { ::std::option::Option::Some(#fk_lit.to_string()) }
            }
            None => quote! { ::std::option::Option::None },
        };

        relations.push(quote! {
            ::gritshield::core::schema::RelationSchema {
                kind: ::gritshield::core::schema::RelationKind::BelongsTo,
                target_table: #target_table_lit.to_string(),
                foreign_key: #fk_value,
            }
        });
    }

    // ---- Registration function that adds relations to the schema registry ----
    let registration = quote! {
        #[::gritshield::startup::ctor(unsafe)]
        fn #register_fn_name() {
            let relations = vec![ #(#relations),* ];
            ::gritshield::core::schema::add_relations(#table_name_lit, relations);
        }
    };

    // ---- Build the query builders ----
    let builders_block = crate::repository::query_builders::generate_builders(
        &entity_module,
        &extended_ident,
        &all_builder_name,
        &one_builder_name,
        &parsed_has_many,
        &parsed_has_one,
        &parsed_belongs_to,
    );

    Ok(quote! {
        #builders_block
        #registration
    })
}