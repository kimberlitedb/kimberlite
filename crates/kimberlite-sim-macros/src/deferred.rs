//! Deferred assertion macros.

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    Expr, LitInt, LitStr, Result, Token,
    parse::{Parse, ParseStream},
};

/// Input for the `assert_after!` macro.
///
/// Syntax: `assert_after!(trigger = "category:event", within_steps = N, key = "...", || check, "message")`
pub(crate) struct AssertAfterInput {
    pub trigger: String,
    pub within_steps: u64,
    pub key: String,
    pub check: Expr,
    pub message: String,
}

impl Parse for AssertAfterInput {
    fn parse(input: ParseStream) -> Result<Self> {
        // Parse: trigger = "category:event"
        input.parse::<syn::Ident>()?; // "trigger"
        input.parse::<Token![=]>()?;
        let trigger_lit: LitStr = input.parse()?;
        let trigger = trigger_lit.value();
        input.parse::<Token![,]>()?;

        // Parse: within_steps = N
        input.parse::<syn::Ident>()?; // "within_steps"
        input.parse::<Token![=]>()?;
        let steps_lit: LitInt = input.parse()?;
        let within_steps = steps_lit.base10_parse::<u64>()?;
        input.parse::<Token![,]>()?;

        // Parse: key = "..."
        input.parse::<syn::Ident>()?; // "key"
        input.parse::<Token![=]>()?;
        let key_lit: LitStr = input.parse()?;
        let key = key_lit.value();
        input.parse::<Token![,]>()?;

        // Parse: || check
        let check: Expr = input.parse()?;
        input.parse::<Token![,]>()?;

        // Parse: "message"
        let message_lit: LitStr = input.parse()?;
        let message = message_lit.value();

        Ok(AssertAfterInput {
            trigger,
            within_steps,
            key,
            check,
            message,
        })
    }
}

pub(crate) fn expand_assert_after(input: AssertAfterInput) -> TokenStream {
    let trigger = input.trigger;
    let within_steps = input.within_steps;
    let key = input.key;
    let _check = input.check; // TODO(v0.9.0): Store for later execution
    let message = input.message;

    let expanded = quote! {
        #[cfg(any(test, feature = "sim"))]
        {
            // Get current step and calculate fire step
            let current_step = kimberlite_sim::instrumentation::invariant_runtime::get_step();
            let fire_at_step = current_step + #within_steps;

            // Register the deferred assertion
            kimberlite_sim::instrumentation::deferred_assertions::register_deferred_assertion(
                fire_at_step,
                Some(#trigger.to_string()),
                #key.to_string(),
                format!("After {}: {}", #trigger, #message),
            );

            // Note: The actual check will be executed by the simulation runtime
            // when the trigger event fires or the step is reached
        }
    };

    TokenStream::from(expanded)
}

/// Input for the `assert_within_steps!` macro.
///
/// Syntax: `assert_within_steps!(steps = N, key = "...", || check, "message")`
pub(crate) struct AssertWithinStepsInput {
    pub steps: u64,
    pub key: String,
    pub check: Expr,
    pub message: String,
}

impl Parse for AssertWithinStepsInput {
    fn parse(input: ParseStream) -> Result<Self> {
        // Parse: steps = N
        input.parse::<syn::Ident>()?; // "steps"
        input.parse::<Token![=]>()?;
        let steps_lit: LitInt = input.parse()?;
        let steps = steps_lit.base10_parse::<u64>()?;
        input.parse::<Token![,]>()?;

        // Parse: key = "..."
        input.parse::<syn::Ident>()?; // "key"
        input.parse::<Token![=]>()?;
        let key_lit: LitStr = input.parse()?;
        let key = key_lit.value();
        input.parse::<Token![,]>()?;

        // Parse: || check
        let check: Expr = input.parse()?;
        input.parse::<Token![,]>()?;

        // Parse: "message"
        let message_lit: LitStr = input.parse()?;
        let message = message_lit.value();

        Ok(AssertWithinStepsInput {
            steps,
            key,
            check,
            message,
        })
    }
}

pub(crate) fn expand_assert_within_steps(input: AssertWithinStepsInput) -> TokenStream {
    let steps = input.steps;
    let key = input.key;
    let _check = input.check; // TODO(v0.9.0): Store for later execution
    let message = input.message;

    let expanded = quote! {
        #[cfg(any(test, feature = "sim"))]
        {
            // Get current step and calculate fire step
            let current_step = kimberlite_sim::instrumentation::invariant_runtime::get_step();
            let fire_at_step = current_step + #steps;

            // Register the deferred assertion
            kimberlite_sim::instrumentation::deferred_assertions::register_deferred_assertion(
                fire_at_step,
                None,
                #key.to_string(),
                format!("Within {} steps: {}", #steps, #message),
            );

            // Note: The actual check will be executed by the simulation runtime
            // when the step is reached
        }
    };

    TokenStream::from(expanded)
}
