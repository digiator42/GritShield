use syn::punctuated::Punctuated;
use syn::{Expr, ExprArray, ExprLit, ExprPath, Lit, Meta, Result, Token};

#[derive(Default)]
pub struct RepositoryAttributes {
    pub entity_module_path: Option<syn::Path>,
    pub searchable_columns: Vec<String>,
    pub grid_columns: Vec<String>,
    pub read_only_columns: Vec<String>,
}

pub fn parse_repository_attributes(attrs: &[syn::Attribute]) -> Result<RepositoryAttributes> {
    let mut result = RepositoryAttributes::default();

    for attr in attrs {
        if attr.path().is_ident("repository") {
            if let Meta::List(meta_list) = &attr.meta {
                let nested_metas =
                    meta_list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
                for meta in nested_metas {
                    if let Meta::NameValue(nv) = meta {
                        if nv.path.is_ident("entity") {
                            match nv.value {
                                Expr::Lit(ExprLit {
                                    lit: Lit::Str(lit_str),
                                    ..
                                }) => {
                                    if let Ok(path) = lit_str.parse::<syn::Path>() {
                                        result.entity_module_path = Some(path);
                                    }
                                }
                                Expr::Path(expr_path) => {
                                    result.entity_module_path = Some(expr_path.path);
                                }
                                _ => {}
                            }
                        } else if nv.path.is_ident("searchable") {
                            extract_strings(&nv.value, &mut result.searchable_columns);
                        } else if nv.path.is_ident("grid_columns") {
                            extract_strings(&nv.value, &mut result.grid_columns);
                        } else if nv.path.is_ident("read_only") {
                            extract_strings(&nv.value, &mut result.read_only_columns);
                        }
                    }
                }
            }
        }
    }
    Ok(result)
}

fn extract_strings(expr: &Expr, target: &mut Vec<String>) {
    if let Expr::Array(ExprArray { elems, .. }) = expr {
        for elem in elems {
            if let Expr::Lit(ExprLit {
                lit: Lit::Str(lit_str),
                ..
            }) = elem
            {
                target.push(lit_str.value());
            }
        }
    }
}
