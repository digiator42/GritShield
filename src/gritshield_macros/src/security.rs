use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::Parser;
use syn::{Attribute, ImplItem, Item, ItemFn, ItemImpl, LitInt, LitStr, ReturnType, Type};

struct ChainAttr {
    name: Option<String>,
    order: i32,
}

impl ChainAttr {
    fn parse(attr: TokenStream) -> syn::Result<Self> {
        let mut name = None;
        let mut order = 0i32;

        if !attr.is_empty() {
            let parser = syn::meta::parser(|meta| {
                if meta.path.is_ident("name") {
                    name = Some(meta.value()?.parse::<LitStr>()?.value());
                    Ok(())
                } else if meta.path.is_ident("order") {
                    order = meta.value()?.parse::<LitInt>()?.base10_parse()?;
                    Ok(())
                } else {
                    Err(meta.error("unsupported option; use `name = \"...\"` and/or `order = N`"))
                }
            });

            parser.parse(attr)?;
        }

        Ok(Self { name, order })
    }
}

pub fn expand_security_chain(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr = match ChainAttr::parse(attr) {
        Ok(attr) => attr,
        Err(err) => return err.to_compile_error().into(),
    };

    match syn::parse::<Item>(item) {
        Ok(Item::Fn(function)) => expand_function(attr, function),
        Ok(Item::Impl(implementation)) => expand_impl(attr, implementation),
        Ok(other) => syn::Error::new_spanned(
            other,
            "#[security_chain] must be applied to a function returning SecurityFilterChain or an inherent impl block with one such associated function",
        )
        .to_compile_error()
        .into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand_function(attr: ChainAttr, function: ItemFn) -> TokenStream {
    if function.sig.asyncness.is_some() {
        return syn::Error::new_spanned(
            &function.sig,
            "#[security_chain] functions must be synchronous; SecurityFilterChain construction cannot await",
        )
        .to_compile_error()
        .into();
    }

    if !function.sig.inputs.is_empty() {
        return syn::Error::new_spanned(
            &function.sig.inputs,
            "#[security_chain] functions must take no arguments; read configuration from the environment or a component",
        )
        .to_compile_error()
        .into();
    }

    let name = attr.name.unwrap_or_else(|| function.sig.ident.to_string());
    let order = attr.order;
    let builder = &function.sig.ident;
    let wrapper = format_ident!("__gritshield_security_chain_{}", builder);
    let cfg_attrs = cfg_attrs(&function.attrs);

    let expanded = quote! {
        #function

        #(#cfg_attrs)*
        #[doc(hidden)]
        #[allow(dead_code, non_snake_case)]
        fn #wrapper() -> ::gritshield::security::filter_chain::SecurityFilterChain {
            let mut chain = #builder();
            chain.set_name(#name);
            chain
        }

        #(#cfg_attrs)*
        const _: () = {
            ::gritshield::inventory::submit! {
                ::gritshield::security::filter_chain::SecurityChainRegistration {
                    name: #name,
                    order: #order,
                    build: #wrapper,
                }
            }
        };
    };

    expanded.into()
}

fn expand_impl(attr: ChainAttr, implementation: ItemImpl) -> TokenStream {
    if implementation.trait_.is_some() {
        return syn::Error::new_spanned(
            &implementation,
            "#[security_chain] supports inherent impl blocks; define the associated function directly on the type",
        )
        .to_compile_error()
        .into();
    }

    if !implementation.generics.params.is_empty() {
        return syn::Error::new_spanned(
            &implementation.generics,
            "#[security_chain] does not support generic impl blocks",
        )
        .to_compile_error()
        .into();
    }

    let mut candidates = implementation.items.iter().filter_map(|item| {
        let ImplItem::Fn(method) = item else {
            return None;
        };

        if method.sig.receiver().is_none()
            && method.sig.inputs.is_empty()
            && method.sig.asyncness.is_none()
            && returns_security_chain(&method.sig.output)
        {
            Some(&method.sig.ident)
        } else {
            None
        }
    });

    let Some(builder) = candidates.next() else {
        return syn::Error::new_spanned(
            &implementation,
            "#[security_chain] needs one associated function such as `fn build() -> SecurityFilterChain` (no self, no arguments, not async)",
        )
        .to_compile_error()
        .into();
    };

    if let Some(extra) = candidates.next() {
        return syn::Error::new_spanned(
            extra,
            "#[security_chain] impl blocks may contain only one associated function that returns SecurityFilterChain",
        )
        .to_compile_error()
        .into();
    }

    let name = attr
        .name
        .unwrap_or_else(|| type_name(&implementation.self_ty));
    let order = attr.order;
    let self_ty = &implementation.self_ty;
    let wrapper = format_ident!(
        "__gritshield_security_chain_{}_{}",
        type_name(&implementation.self_ty),
        builder
    );
    let cfg_attrs = cfg_attrs(&implementation.attrs);

    let expanded = quote! {
        #implementation

        #(#cfg_attrs)*
        #[doc(hidden)]
        #[allow(dead_code, non_snake_case)]
        fn #wrapper() -> ::gritshield::security::filter_chain::SecurityFilterChain {
            let mut chain = <#self_ty>::#builder();
            chain.set_name(#name);
            chain
        }

        #(#cfg_attrs)*
        const _: () = {
            ::gritshield::inventory::submit! {
                ::gritshield::security::filter_chain::SecurityChainRegistration {
                    name: #name,
                    order: #order,
                    build: #wrapper,
                }
            }
        };
    };

    expanded.into()
}

fn cfg_attrs(attrs: &[Attribute]) -> Vec<Attribute> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
        .cloned()
        .collect()
}

fn returns_security_chain(output: &ReturnType) -> bool {
    let ReturnType::Type(_, ty) = output else {
        return false;
    };

    let Type::Path(path) = &**ty else {
        return false;
    };

    path.path
        .segments
        .last()
        .map(|segment| segment.ident == "SecurityFilterChain")
        .unwrap_or(false)
}

fn type_name(ty: &Type) -> String {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_else(|| "security_chain".to_string()),
        _ => "security_chain".to_string(),
    }
}
