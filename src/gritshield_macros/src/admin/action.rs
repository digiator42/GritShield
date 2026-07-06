use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn, LitStr, Token, Ident};
use syn::parse::{Parse, ParseStream, Result};

struct ActionArgs {
    table: LitStr,
    label: LitStr,
    icon: Option<LitStr>,
    color: LitStr,
}

impl Parse for ActionArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut table = None;
        let mut label = None;
        let mut icon = None;
        let mut color = None;

        while !input.is_empty() {
            let ident = input.parse::<syn::Ident>()?;
            input.parse::<Token![=]>()?;
            
            if ident == "table" {
                table = Some(input.parse::<LitStr>()?);
            } else if ident == "label" {
                label = Some(input.parse::<LitStr>()?);
            } else if ident == "icon" {
                icon = Some(input.parse::<LitStr>()?);
            } else if ident == "color" {
                color = Some(input.parse::<LitStr>()?);
            }
            
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(Self {
            table: table.unwrap_or_else(|| LitStr::new("", proc_macro2::Span::call_site())),
            label: label.unwrap_or_else(|| LitStr::new("Action", proc_macro2::Span::call_site())),
            icon,
            color: color.unwrap_or_else(|| LitStr::new("text-blue-400", proc_macro2::Span::call_site())),
        })
    }
}

pub fn expand_action(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as ActionArgs);
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;
    let table = args.table;
    let label = args.label;
    let icon = args.icon;
    let color = args.color;

    // Determine crate root (internal vs external)
    let is_internal = std::env::var("CARGO_PKG_NAME")
        .map(|name| name == "gritshield")
        .unwrap_or(false);
    
    let crate_root = if is_internal {
        quote! { crate }
    } else {
        quote! { ::gritshield }
    };

    // Create a new identifier for the registration function
    let register_fn_name = Ident::new(
        &format!("{}_register_action", fn_name),
        proc_macro2::Span::call_site(),
    );

    let icon_expr = match icon {
        Some(ic) => quote! { Some(#ic) },
        None => quote! { None },
    };

    let expanded = quote! {
        #input_fn

        #[#crate_root::startup::ctor(unsafe)]
        fn #register_fn_name() {
            let action = #crate_root::database::repository::CustomAction {
                label: #label,
                icon: #icon_expr,
                color: #color,
                action: ::std::sync::Arc::new(|ctx| {
                    Box::pin(async move {
                        #fn_name(ctx).await
                    })
                }),
            };

            #crate_root::database::repository::register_action(#table, action);
        }
    };

    TokenStream::from(expanded)
}