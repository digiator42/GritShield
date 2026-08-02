use proc_macro::TokenStream;
use syn::{ItemFn, Type};
use quote::quote;

pub fn expand_intercept(interceptor_type: Type, input_fn: ItemFn) -> TokenStream {
    let vis = &input_fn.vis;
    let sig = &input_fn.sig;
    let body = &input_fn.block;
    let attrs = &input_fn.attrs;
    let fn_name = &sig.ident;

    if sig.asyncness.is_none() {
        return syn::Error::new_spanned(
            sig.fn_token,
            "#[intercept] can only be applied to async functions",
        )
        .to_compile_error()
        .into();
    }

    let expanded = quote! {
        #(#attrs)*
        #vis #sig {
            use ::gritshield::core::aop::{Interceptor, InvocationContext};

            let interceptor = #interceptor_type;

            let ctx = InvocationContext {
                target_name: std::any::type_name::<Self>(),
                method_name: stringify!(#fn_name),
                db: &self.db,
            };

            // Execute the original body as the 'next' closure
            interceptor.intercept(ctx, Box::new(|| {
                Box::pin(async move {
                    #body
                })
            })).await
        }
    };

    TokenStream::from(expanded)
}
