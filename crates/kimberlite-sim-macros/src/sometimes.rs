//! "Sometimes assertions" - deterministically sampled expensive checks.

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    Expr, LitInt, LitStr, Result, Token,
};

/// Input for the `sometimes_assert!` macro.
///
/// Syntax: `sometimes_assert!(rate = N, key = "...", || expr, "message")`
pub(crate) struct SometimesInput {
    pub rate: u64,
    pub key: String,
    pub check: Expr,
    pub message: String,
}

impl Parse for SometimesInput {
    fn parse(input: ParseStream) -> Result<Self> {
        // Parse: rate = N
        input.parse::<syn::Ident>()?; // "rate"
        input.parse::<Token![=]>()?;
        let rate_lit: LitInt = input.parse()?;
        let rate = rate_lit.base10_parse::<u64>()?;
        input.parse::<Token![,]>()?;

        // Parse: key = "..."
        input.parse::<syn::Ident>()?; // "key"
        input.parse::<Token![=]>()?;
        let key_lit: LitStr = input.parse()?;
        let key = key_lit.value();
        input.parse::<Token![,]>()?;

        // Parse: || expr
        let check: Expr = input.parse()?;
        input.parse::<Token![,]>()?;

        // Parse: "message"
        let message_lit: LitStr = input.parse()?;
        let message = message_lit.value();

        Ok(SometimesInput {
            rate,
            key,
            check,
            message,
        })
    }
}

pub(crate) fn expand_sometimes_assert(input: SometimesInput) -> TokenStream {
    let rate = input.rate;
    let key = input.key;
    let check = input.check;
    let message = input.message;

    let expanded = quote! {
        #[cfg(any(test, feature = "sim"))]
        {
            if kimberlite_sim::instrumentation::invariant_runtime::should_check_invariant(
                #key,
                #rate
            ) {
                let check_fn = #check;
                assert!(check_fn, #message);
            }
        }
    };

    TokenStream::from(expanded)
}
