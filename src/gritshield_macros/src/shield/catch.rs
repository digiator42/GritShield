use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn, LitInt, Token};
use syn::parse::{Parse, ParseStream, Result};

struct CatchArgs {
    status: u16,
}

impl Parse for CatchArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut status = None;

        while !input.is_empty() {
            let ident = input.parse::<syn::Ident>()?;
            input.parse::<Token![=]>()?;

            if ident == "status" {
                let lit_int: LitInt = input.parse()?;
                status = Some(lit_int.base10_parse::<u16>()?);
            }

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        match status {
            Some(code) => Ok(CatchArgs { status: code }),
            None => Err(input.error("`#[catch]` requires a `status` parameter, e.g., `#[catch(status = 404)]`")),
        }
    }
}

pub fn expand_catch(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as CatchArgs);
    let input_fn = parse_macro_input!(item as ItemFn);
    
    let status = args.status;
    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;
    let fn_block = &input_fn.block;
    let fn_attrs = &input_fn.attrs;
    let fn_args = &input_fn.sig.inputs;
    let fn_return = &input_fn.sig.output;

    // ─── Validate function signature ───
    if input_fn.sig.asyncness.is_none() {
        return syn::Error::new_spanned(
            &input_fn.sig,
            "`#[catch]` handlers must be async functions",
        )
        .to_compile_error()
        .into();
    }

    let has_ctx = fn_args.iter().any(|arg| {
        if let syn::FnArg::Typed(pat_type) = arg {
            if let syn::Type::Path(type_path) = &*pat_type.ty {
                if let Some(segment) = type_path.path.segments.last() {
                    return segment.ident == "RequestContext";
                }
            }
        }
        false
    });

    if !has_ctx {
        return syn::Error::new_spanned(
            &input_fn.sig,
            "`#[catch]` handlers must take `ctx: RequestContext` as a parameter",
        )
        .to_compile_error()
        .into();
    }

    // ─── Check return type ───
    let is_response = match fn_return {
        syn::ReturnType::Type(_, ty) => {
            let ty_str = quote! { #ty }.to_string();
            ty_str.contains("Response")
        }
        _ => false,
    };

    if !is_response {
        return syn::Error::new_spanned(
            &input_fn.sig,
            "`#[catch]` handlers must return `Response`",
        )
        .to_compile_error()
        .into();
    }

    // ─── Generate a unique registration function name ───
    let register_fn_name = syn::Ident::new(
        &format!("register_catch_{}_{}", status, fn_name),
        proc_macro2::Span::call_site(),
    );

    let expanded = quote! {
        #(#fn_attrs)*
        #fn_vis fn #fn_name(ctx: ::gritshield::routing::engine::RequestContext) -> ::gritshield::futures::future::BoxFuture<'static, ::gritshield::http::response::Response> {
            use ::gritshield::futures::future::FutureExt;

            async move {
                #fn_block
            }.boxed()
        }

        #[::gritshield::startup::ctor(unsafe)]
        fn #register_fn_name() {
            ::gritshield::security::errors::register_catch(#status, #fn_name);
        }
    };

    TokenStream::from(expanded)
}