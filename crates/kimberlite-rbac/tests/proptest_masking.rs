//! Property-based tests for RBAC field-masking invariants.
//!
//! AUDIT-2026-04 S1.4 / L-1 continuation — exercises `apply_mask`
//! over randomised inputs and pins the invariants downstream
//! consumers (SDK deserialisation, audit logging, UI rendering)
//! rely on:
//!
//! - **Determinism.** `apply_mask(v, mask, role)` produces the same
//!   bytes on every call; no hidden randomness can leak a secret
//!   via "sometimes unmasked" behaviour.
//!
//! - **Output shape by strategy.** `Hash` → 64-hex; `Tokenize` →
//!   `tok_` + 16 hex; `Null` → empty; `Redact` preserves last-N
//!   pattern-specific digits.
//!
//! - **No silent drops.** Non-null strategies produce non-empty
//!   output for non-empty input.
//!
//! - **Role exemption.** Exempt roles always see the original
//!   bytes; masked roles never see the original bytes for
//!   strategies with information loss.
//!
//! - **Truncate bound.** `Truncate { max_chars }` output never
//!   exceeds `max_chars + 3` (for the `"..."` suffix).

#![cfg(test)]

use kimberlite_rbac::masking::{
    FieldMask, MaskingStrategy, RedactPattern, apply_mask,
};
use kimberlite_rbac::roles::Role;
use proptest::prelude::*;

// -----------------------------------------------------------------------
// Strategies.
// -----------------------------------------------------------------------

fn non_empty_bytes() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 1..256)
}

fn ascii_printable_bytes() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(0x20u8..=0x7Eu8, 1..128)
}

// -----------------------------------------------------------------------
// Determinism — same input always produces the same output.
// -----------------------------------------------------------------------

proptest! {
    /// Hash masking is deterministic.
    #[test]
    fn prop_hash_deterministic(input in non_empty_bytes()) {
        let mask = FieldMask::new("x", MaskingStrategy::Hash);
        let a = apply_mask(&input, &mask, &Role::User).unwrap();
        let b = apply_mask(&input, &mask, &Role::User).unwrap();
        prop_assert_eq!(a, b);
    }

    /// Tokenize is deterministic — the audit log compares token
    /// values for identity, so flaky tokens would silently break
    /// re-identification prevention.
    #[test]
    fn prop_tokenize_deterministic(input in non_empty_bytes()) {
        let mask = FieldMask::new("x", MaskingStrategy::Tokenize);
        let a = apply_mask(&input, &mask, &Role::User).unwrap();
        let b = apply_mask(&input, &mask, &Role::User).unwrap();
        prop_assert_eq!(a, b);
    }

    /// Truncate is deterministic.
    #[test]
    fn prop_truncate_deterministic(
        input in ascii_printable_bytes(),
        max in 0usize..200,
    ) {
        let mask = FieldMask::new(
            "x",
            MaskingStrategy::Truncate { max_chars: max },
        );
        let a = apply_mask(&input, &mask, &Role::User).unwrap();
        let b = apply_mask(&input, &mask, &Role::User).unwrap();
        prop_assert_eq!(a, b);
    }
}

// -----------------------------------------------------------------------
// Output-shape invariants.
// -----------------------------------------------------------------------

proptest! {
    /// SHA-256 hex is exactly 64 lowercase hex characters.
    #[test]
    fn prop_hash_is_64_hex_chars(input in non_empty_bytes()) {
        let mask = FieldMask::new("x", MaskingStrategy::Hash);
        let out = apply_mask(&input, &mask, &Role::User).unwrap();
        prop_assert_eq!(out.len(), 64);
        for byte in &out {
            let is_hex_lower = byte.is_ascii_digit()
                || (*byte >= b'a' && *byte <= b'f');
            prop_assert!(
                is_hex_lower,
                "hash output must be lowercase hex, saw byte {byte:#04x}"
            );
        }
    }

    /// Tokenize always produces `tok_` prefix + 16 hex chars = 20 bytes.
    #[test]
    fn prop_tokenize_20_char_tok_prefix(input in non_empty_bytes()) {
        let mask = FieldMask::new("x", MaskingStrategy::Tokenize);
        let out = apply_mask(&input, &mask, &Role::User).unwrap();
        prop_assert_eq!(out.len(), 20);
        prop_assert_eq!(&out[..4], b"tok_");
    }

    /// Null strategy always produces empty bytes regardless of input.
    #[test]
    fn prop_null_always_empty(input in non_empty_bytes()) {
        let mask = FieldMask::new("x", MaskingStrategy::Null);
        let out = apply_mask(&input, &mask, &Role::User).unwrap();
        prop_assert!(out.is_empty());
    }

    /// Truncate output never exceeds `max_chars + 3` (the `"..."`).
    /// If input is shorter than `max_chars` the bytes pass through
    /// unchanged.
    #[test]
    fn prop_truncate_bounded(
        input in ascii_printable_bytes(),
        max in 1usize..100,
    ) {
        let mask = FieldMask::new(
            "x",
            MaskingStrategy::Truncate { max_chars: max },
        );
        let out = apply_mask(&input, &mask, &Role::User).unwrap();
        prop_assert!(
            out.len() <= max + 3,
            "truncate({max}) produced {} bytes: {:?}",
            out.len(),
            String::from_utf8_lossy(&out),
        );
    }

    /// Non-null strategies produce non-empty output for non-empty
    /// input. A silent drop would collapse an attribute to an
    /// empty string without any caller signal.
    #[test]
    fn prop_non_null_non_empty_on_non_empty_input(
        input in non_empty_bytes(),
    ) {
        // Truncate(0) is explicitly "truncate to nothing", a valid
        // non-null shape that can legitimately produce "..." —
        // skip it for this property.
        let strategies = vec![
            MaskingStrategy::Hash,
            MaskingStrategy::Tokenize,
            MaskingStrategy::Truncate { max_chars: 5 },
        ];
        for strategy in strategies {
            let mask = FieldMask::new("x", strategy.clone());
            let out = apply_mask(&input, &mask, &Role::User).unwrap();
            prop_assert!(
                !out.is_empty(),
                "non-null strategy {strategy:?} produced empty output for non-empty input"
            );
        }
    }
}

// -----------------------------------------------------------------------
// Role exemption — an exempt role must see the exact original bytes.
// -----------------------------------------------------------------------

proptest! {
    /// Exempt role sees bytes verbatim regardless of strategy.
    #[test]
    fn prop_exempt_role_sees_original_bytes(
        input in non_empty_bytes(),
    ) {
        let strategies = vec![
            MaskingStrategy::Hash,
            MaskingStrategy::Tokenize,
            MaskingStrategy::Null,
            MaskingStrategy::Truncate { max_chars: 4 },
        ];
        for strategy in strategies {
            // Build a mask that exempts Admin.
            let mask = FieldMask::new("x", strategy.clone())
                .exempt(Role::Admin);
            let out = apply_mask(&input, &mask, &Role::Admin).unwrap();
            prop_assert_eq!(
                &out,
                &input,
                "exempt role must see verbatim bytes for {:?}",
                strategy
            );
        }
    }

    /// Masked (non-exempt) role never sees the full original bytes
    /// for information-destroying strategies. The Hash output and
    /// Tokenize output are of fixed form, so their equality with
    /// the input is only possible in astronomically-rare collisions.
    #[test]
    fn prop_masked_role_does_not_see_original_hash(
        input in non_empty_bytes()
            .prop_filter("input is short enough that collision is impossible",
                         |v| v.len() != 64 || v.iter().any(|b| !b.is_ascii_hexdigit())),
    ) {
        let mask = FieldMask::new("x", MaskingStrategy::Hash)
            .applies_to(Role::User);
        let out = apply_mask(&input, &mask, &Role::User).unwrap();
        prop_assert_ne!(&out, &input);
    }
}

// -----------------------------------------------------------------------
// Pattern-specific redaction — SSN redaction always masks first 5 digits.
// -----------------------------------------------------------------------

proptest! {
    /// SSN redaction always preserves exactly the last 4 digits
    /// and masks the first 5. A regression that off-by-one'd the
    /// split would either leak digits (bad) or drop the last-4
    /// (breaking business logic that needs the suffix for lookup).
    #[test]
    fn prop_ssn_redact_preserves_last_four_only(
        area in 100u32..999,
        group in 10u32..99,
        serial in 1000u32..9999,
    ) {
        let ssn = format!("{area:03}-{group:02}-{serial:04}");
        let mask = FieldMask::new(
            "ssn",
            MaskingStrategy::Redact(RedactPattern::Ssn),
        )
        .applies_to(Role::User);
        let out = apply_mask(ssn.as_bytes(), &mask, &Role::User).unwrap();
        let out_str = String::from_utf8(out).unwrap();
        prop_assert_eq!(&out_str, &format!("***-**-{serial:04}"));
    }
}
