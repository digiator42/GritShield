use crate::core_parser::parse_repository_attributes;
use proc_macro2::TokenStream;
use quote::quote;
use syn::DeriveInput;

mod jpa_dsl;
mod query_builders;

pub fn expand_repository(input: DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;

    // Use our centralized parser here too
    let repo_attrs = parse_repository_attributes(&input.attrs)?;

    let mut grid_columns = repo_attrs.grid_columns;
    let searchable_columns = repo_attrs.searchable_columns;
    let has_many_relations = repo_attrs.has_many_relations;
    let has_one_relations = repo_attrs.has_one_relations;

    // Resolve structural target module path
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
    let extended_ident =
        syn::Ident::new(&format!("{}Record", name), proc_macro2::Span::call_site());

    // Parse Has-Many Relation Configurations
    let mut parsed_has_many: Vec<(String, syn::Path)> = Vec::new();
    let mut iter_many = has_many_relations.iter().peekable();
    while let Some(current) = iter_many.next() {
        if current.contains(':') {
            let parts: Vec<&str> = current.split(':').collect();
            let field = parts[0].to_string();
            let target: syn::Path = syn::parse_str(parts[1]).expect("Invalid entity path mapping");
            parsed_has_many.push((field, target));
        } else if iter_many
            .peek()
            .map_or(false, |next| next.contains("::") || next.contains("Entity"))
        {
            let field = current.clone();
            let next_str = iter_many.next().unwrap();
            let target: syn::Path = syn::parse_str(next_str).expect("Invalid entity path mapping");
            parsed_has_many.push((field, target));
        } else {
            let field = current.clone();
            let singular = if field.ends_with('s') {
                &field[..field.len() - 1]
            } else {
                &field
            };
            let path_str = format!("crate::models::{}::Entity", singular);
            let target: syn::Path = syn::parse_str(&path_str).unwrap();
            parsed_has_many.push((field, target));
        }
    }

    // Parse Has-One Relation Configurations
    let mut parsed_has_one: Vec<(String, syn::Path)> = Vec::new();
    let mut iter_one = has_one_relations.iter().peekable();
    while let Some(current) = iter_one.next() {
        if current.contains(':') {
            let parts: Vec<&str> = current.split(':').collect();
            let field = parts[0].to_string();
            let target: syn::Path = syn::parse_str(parts[1]).expect("Invalid entity path mapping");
            parsed_has_one.push((field, target));
        } else if iter_one
            .peek()
            .map_or(false, |next| next.contains("::") || next.contains("Entity"))
        {
            let field = current.clone();
            let next_str = iter_one.next().unwrap();
            let target: syn::Path = syn::parse_str(next_str).expect("Invalid entity path mapping");
            parsed_has_one.push((field, target));
        } else {
            let field = current.clone();
            let path_str = format!("crate::models::{}::Entity", field);
            let target: syn::Path = syn::parse_str(&path_str).unwrap();
            parsed_has_one.push((field, target));
        }
    }

    let mut unique_fields = grid_columns.clone();
    for col in &searchable_columns {
        if !unique_fields.contains(col) {
            unique_fields.push(col.clone());
        }
    }

    let jpa_methods_block = jpa_dsl::generate_jpa_methods(
        &entity_module,
        &all_builder_name,
        &one_builder_name,
        &unique_fields,
    );

    let builders_block = query_builders::generate_builders(
        name,
        &entity_module,
        &extended_ident,
        &all_builder_name,
        &one_builder_name,
        &parsed_has_many,
        &parsed_has_one,
    );

    Ok(quote! {
        impl #name {
            #jpa_methods_block
        }

        #builders_block
    })
}
