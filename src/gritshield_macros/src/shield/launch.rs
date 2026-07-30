use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn, ReturnType};

pub fn expand_launch(_args: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let fn_name = &input_fn.sig.ident;
    let fn_block = &input_fn.block;
    let fn_attrs = &input_fn.attrs;
    let fn_vis = &input_fn.vis;
    let fn_args = &input_fn.sig.inputs;
    let fn_return = &input_fn.sig.output;

    // Validate that the function is async
    if input_fn.sig.asyncness.is_none() {
        return syn::Error::new_spanned(
            &input_fn.sig,
            "`#[launch]` can only be applied to async functions",
        )
        .to_compile_error()
        .into();
    }

    // Validate no arguments
    if !fn_args.is_empty() {
        return syn::Error::new_spanned(
            &input_fn.sig,
            "`#[launch]` function must not take any arguments",
        )
        .to_compile_error()
        .into();
    }

    // Check if it returns Result
    let is_result = match fn_return {
        ReturnType::Type(_, ty) => {
            let ty_str = quote! { #ty }.to_string();
            ty_str.contains("Result")
        }
        _ => false,
    };

    let expanded = if is_result {
        quote! {
            #(#fn_attrs)*
            #fn_vis fn #fn_name() #fn_return {
                let rt = ::gritshield::deps::tokio::runtime::Runtime::new()
                    .expect("Failed to create Tokio runtime");
                rt.block_on(async {
                    #fn_block
                })
            }
        }
    } else {
        quote! {
            #(#fn_attrs)*
            #fn_vis fn #fn_name() {
                let rt = ::gritshield::deps::tokio::runtime::Runtime::new()
                    .expect("Failed to create Tokio runtime");
                rt.block_on(async {
                    #fn_block
                })
            }
        }
    };

    // If the function returns Result, we don't need to handle errors separately
    // If it returns (), we just call it
    if is_result {
        // Return the function with Result handling
        TokenStream::from(quote! {
            #expanded
        })
    } else {
        // Wrap in a proper main function
        TokenStream::from(quote! {
            #expanded
        })
    }
}
