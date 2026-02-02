//! Phase marker macros for event-triggered assertions.

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    Expr, LitStr, Result, Token,
};

/// Input for the `phase!` macro.
///
/// Syntax: `phase!("category", "event_name", { context })`
pub(crate) struct PhaseInput {
    pub category: String,
    pub event: String,
    pub context: Option<Expr>,
}

impl Parse for PhaseInput {
    fn parse(input: ParseStream) -> Result<Self> {
        // Parse: "category"
        let category_lit: LitStr = input.parse()?;
        let category = category_lit.value();
        input.parse::<Token![,]>()?;

        // Parse: "event_name"
        let event_lit: LitStr = input.parse()?;
        let event = event_lit.value();

        // Optional context
        let context = if input.parse::<Token![,]>().is_ok() {
            Some(input.parse::<Expr>()?)
        } else {
            None
        };

        Ok(PhaseInput {
            category,
            event,
            context,
        })
    }
}

pub(crate) fn expand_phase(input: PhaseInput) -> TokenStream {
    let category = input.category;
    let event = input.event;

    let expanded = if let Some(context) = input.context {
        quote! {
            #[cfg(any(test, feature = "sim"))]
            {
                kimberlite_sim::instrumentation::phase_tracker::record_phase(
                    #category,
                    #event,
                    format!("{:?}", #context)
                );
            }
        }
    } else {
        quote! {
            #[cfg(any(test, feature = "sim"))]
            {
                kimberlite_sim::instrumentation::phase_tracker::record_phase(
                    #category,
                    #event,
                    String::new()
                );
            }
        }
    };

    TokenStream::from(expanded)
}
