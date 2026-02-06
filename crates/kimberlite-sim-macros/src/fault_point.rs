//! Fault injection point macros.

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    Expr, LitStr, Result, Token,
    parse::{Parse, ParseStream},
};

/// Input for the `fault!` macro.
///
/// Syntax: `fault!("key", { context }, || expr)`
pub(crate) struct FaultInput {
    pub key: LitStr,
    #[allow(dead_code)]
    pub context: Option<Expr>,
    pub operation: Expr,
}

impl Parse for FaultInput {
    fn parse(input: ParseStream) -> Result<Self> {
        let key: LitStr = input.parse()?;
        input.parse::<Token![,]>()?;

        // Optional context (currently ignored, but reserved for future use)
        let context = if input.peek(syn::token::Brace) {
            let _brace_content;
            syn::braced!(_brace_content in input);
            input.parse::<Token![,]>()?;
            Some(input.parse::<Expr>()?)
        } else {
            Some(input.parse::<Expr>()?)
        };

        Ok(FaultInput {
            key,
            context: None, // We parse it but don't use it yet
            operation: context.unwrap(),
        })
    }
}

pub(crate) fn expand_fault(input: FaultInput) -> TokenStream {
    let key = input.key;
    let operation = input.operation;

    let expanded = quote! {
        {
            #[cfg(any(test, feature = "sim"))]
            {
                kimberlite_sim::instrumentation::fault_registry::record_fault_point(#key);

                // Check if SimFaultInjector wants to inject a fault here
                if kimberlite_sim::instrumentation::fault_registry::should_inject_fault(#key) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Simulated fault: {}", #key)
                    ).into());
                }
            }

            // Execute the actual operation
            #operation
        }
    };

    TokenStream::from(expanded)
}
