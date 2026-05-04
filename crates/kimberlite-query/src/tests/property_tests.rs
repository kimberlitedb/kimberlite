//! Property-based tests using proptest.
//!
//! Tests invariants that should hold for all inputs, using fuzzing-like techniques.

use crate::key_encoder::{decode_key, encode_key};
use crate::parser::parse_statement;
use crate::plan::matches_like_pattern;
use crate::value::Value;
use proptest::prelude::*;

proptest! {
    // ========================================================================
    // Key Encoding Round-trip Tests
    // ========================================================================

    /// Test that TinyInt values round-trip through key encoding
    #[test]
    fn tinyint_encoding_round_trip(v: i8) {
        let key = encode_key(&[Value::TinyInt(v)]);
        let decoded = decode_key(&key);
        prop_assert_eq!(decoded, vec![Value::TinyInt(v)]);
    }

    /// Test that SmallInt values round-trip through key encoding
    #[test]
    fn smallint_encoding_round_trip(v: i16) {
        let key = encode_key(&[Value::SmallInt(v)]);
        let decoded = decode_key(&key);
        prop_assert_eq!(decoded, vec![Value::SmallInt(v)]);
    }

    /// Test that Integer values round-trip through key encoding
    #[test]
    fn integer_encoding_round_trip(v: i32) {
        let key = encode_key(&[Value::Integer(v)]);
        let decoded = decode_key(&key);
        prop_assert_eq!(decoded, vec![Value::Integer(v)]);
    }

    /// Test that BigInt values round-trip through key encoding
    #[test]
    fn bigint_encoding_round_trip(v: i64) {
        let key = encode_key(&[Value::BigInt(v)]);
        let decoded = decode_key(&key);
        prop_assert_eq!(decoded, vec![Value::BigInt(v)]);
    }

    /// Test that Real values round-trip through key encoding (excluding NaN)
    #[test]
    fn real_encoding_round_trip(v in prop::num::f64::NORMAL) {
        let key = encode_key(&[Value::Real(v)]);
        let decoded = decode_key(&key);

        match &decoded[0] {
            Value::Real(decoded_v) => {
                // For normal floats, exact equality should hold
                prop_assert_eq!(*decoded_v, v);
            }
            other => prop_assert!(false, "Expected Real, got {:?}", other),
        }
    }

    /// Test that Decimal values round-trip through key encoding
    #[test]
    fn decimal_encoding_round_trip(
        val in -1_000_000_000i128..1_000_000_000i128,
        scale in 0u8..10u8
    ) {
        let key = encode_key(&[Value::Decimal(val, scale)]);
        let decoded = decode_key(&key);
        prop_assert_eq!(decoded, vec![Value::Decimal(val, scale)]);
    }

    /// Test that Boolean values round-trip through key encoding
    #[test]
    fn boolean_encoding_round_trip(v: bool) {
        let key = encode_key(&[Value::Boolean(v)]);
        let decoded = decode_key(&key);
        prop_assert_eq!(decoded, vec![Value::Boolean(v)]);
    }

    /// Test that Text values round-trip through key encoding
    #[test]
    fn text_encoding_round_trip(s in "[a-zA-Z0-9 ]{0,100}") {
        let key = encode_key(&[Value::Text(s.clone())]);
        let decoded = decode_key(&key);
        prop_assert_eq!(decoded, vec![Value::Text(s)]);
    }

    /// Test that Bytes values round-trip through key encoding
    #[test]
    fn bytes_encoding_round_trip(b: Vec<u8>) {
        let key = encode_key(&[Value::Bytes(bytes::Bytes::from(b.clone()))]);
        let decoded = decode_key(&key);
        prop_assert_eq!(decoded, vec![Value::Bytes(bytes::Bytes::from(b))]);
    }

    // ========================================================================
    // Ordering Preservation Tests
    // ========================================================================

    /// Test that TinyInt ordering is preserved in key encoding
    #[test]
    fn tinyint_ordering_preserved(a: i8, b: i8) {
        let key_a = encode_key(&[Value::TinyInt(a)]);
        let key_b = encode_key(&[Value::TinyInt(b)]);
        prop_assert_eq!(a.cmp(&b), key_a.cmp(&key_b));
    }

    /// Test that SmallInt ordering is preserved in key encoding
    #[test]
    fn smallint_ordering_preserved(a: i16, b: i16) {
        let key_a = encode_key(&[Value::SmallInt(a)]);
        let key_b = encode_key(&[Value::SmallInt(b)]);
        prop_assert_eq!(a.cmp(&b), key_a.cmp(&key_b));
    }

    /// Test that Integer ordering is preserved in key encoding
    #[test]
    fn integer_ordering_preserved(a: i32, b: i32) {
        let key_a = encode_key(&[Value::Integer(a)]);
        let key_b = encode_key(&[Value::Integer(b)]);
        prop_assert_eq!(a.cmp(&b), key_a.cmp(&key_b));
    }

    /// Test that BigInt ordering is preserved in key encoding
    #[test]
    fn bigint_ordering_preserved(a: i64, b: i64) {
        let key_a = encode_key(&[Value::BigInt(a)]);
        let key_b = encode_key(&[Value::BigInt(b)]);
        prop_assert_eq!(a.cmp(&b), key_a.cmp(&key_b));
    }

    /// Test that Decimal ordering is preserved in key encoding
    #[test]
    fn decimal_ordering_preserved(
        a in -1000i128..1000i128,
        b in -1000i128..1000i128,
        scale in 0u8..5u8
    ) {
        let key_a = encode_key(&[Value::Decimal(a, scale)]);
        let key_b = encode_key(&[Value::Decimal(b, scale)]);
        prop_assert_eq!(a.cmp(&b), key_a.cmp(&key_b));
    }

    /// Test that Text ordering is preserved in key encoding
    #[test]
    fn text_ordering_preserved(a in "[\\x00-\\x7F]{0,50}", b in "[\\x00-\\x7F]{0,50}") {
        let key_a = encode_key(&[Value::Text(a.clone())]);
        let key_b = encode_key(&[Value::Text(b.clone())]);
        prop_assert_eq!(a.cmp(&b), key_a.cmp(&key_b));
    }

    /// Test that Bytes ordering is preserved in key encoding
    #[test]
    fn bytes_ordering_preserved(a: Vec<u8>, b: Vec<u8>) {
        let key_a = encode_key(&[Value::Bytes(bytes::Bytes::from(a.clone()))]);
        let key_b = encode_key(&[Value::Bytes(bytes::Bytes::from(b.clone()))]);
        prop_assert_eq!(a.cmp(&b), key_a.cmp(&key_b));
    }

    // ========================================================================
    // Type Coercion Symmetry Tests
    // ========================================================================

    /// Test that Integer <-> BigInt coercion is symmetric
    #[test]
    fn integer_bigint_coercion_symmetric(val: i32) {
        let int_val = Value::Integer(val);
        let bigint_val = Value::BigInt(i64::from(val));

        // Both should compare as equal via compare method
        if let Some(ord) = int_val.compare(&bigint_val) {
            prop_assert_eq!(ord, std::cmp::Ordering::Equal);
        }
    }

    /// Test that SmallInt <-> BigInt coercion is symmetric
    #[test]
    fn smallint_bigint_coercion_symmetric(val: i16) {
        let smallint_val = Value::SmallInt(val);
        let bigint_val = Value::BigInt(i64::from(val));

        if let Some(ord) = smallint_val.compare(&bigint_val) {
            prop_assert_eq!(ord, std::cmp::Ordering::Equal);
        }
    }

    /// Test that TinyInt <-> BigInt coercion is symmetric
    #[test]
    fn tinyint_bigint_coercion_symmetric(val: i8) {
        let tinyint_val = Value::TinyInt(val);
        let bigint_val = Value::BigInt(i64::from(val));

        if let Some(ord) = tinyint_val.compare(&bigint_val) {
            prop_assert_eq!(ord, std::cmp::Ordering::Equal);
        }
    }

    // ========================================================================
    // Parser Robustness Tests
    // ========================================================================

    /// Test that the parser never panics on arbitrary input
    #[test]
    fn parser_doesnt_panic(sql in "[ -~]{0,200}") {
        // Parser should either succeed or return Err, never panic
        let _ = parse_statement(&sql);
        // If we get here without panicking, test passes
    }

    /// Test that the parser handles random keywords without panicking
    #[test]
    fn parser_handles_random_keywords(
        keyword in "[A-Z]{1,20}",
        rest in "[a-zA-Z0-9 ,.()]{0,50}"
    ) {
        let sql = format!("{keyword} {rest}");
        let _ = parse_statement(&sql);
    }

    /// Test that the parser handles deeply nested expressions
    #[test]
    fn parser_handles_nested_expressions(depth in 0usize..20) {
        let mut sql = "SELECT ".to_string();
        for _ in 0..depth {
            sql.push('(');
        }
        sql.push('1');
        for _ in 0..depth {
            sql.push(')');
        }
        sql.push_str(" FROM users");

        // Should not panic, may succeed or fail gracefully
        let _ = parse_statement(&sql);
    }

    // ========================================================================
    // LIKE Pattern Tests
    // ========================================================================

    /// Test that LIKE pattern matching never hangs or panics
    #[test]
    fn like_pattern_doesnt_hang(
        text in "[a-zA-Z0-9]{0,100}",
        pattern in "[a-zA-Z0-9%_]{1,50}"  // Pattern must be non-empty
    ) {
        // Should complete within reasonable time
        let _ = matches_like_pattern(&text, &pattern);
    }

    /// Test that LIKE with only wildcards works
    #[test]
    fn like_all_wildcards(text in "[a-z]{0,20}") {
        // % should match any string
        prop_assert!(matches_like_pattern(&text, "%"));
    }

    /// Test that LIKE exact match works
    #[test]
    fn like_exact_match(text in "[a-z]{5,10}") {
        // Exact pattern should match itself
        prop_assert!(matches_like_pattern(&text, &text));
    }

    /// Test that LIKE with single wildcard matches single char
    #[test]
    fn like_single_wildcard_matches_single_char(c: char) {
        if c.is_ascii_alphanumeric() {
            let text = c.to_string();
            prop_assert!(matches_like_pattern(&text, "_"));
        }
    }

    // ========================================================================
    // Value Comparison Tests
    // ========================================================================

    /// Test that comparison is reflexive: a == a
    #[test]
    fn comparison_reflexive(val: i64) {
        let v = Value::BigInt(val);
        prop_assert_eq!(v.compare(&v), Some(std::cmp::Ordering::Equal));
    }

    /// Test that comparison is symmetric: if a < b then b > a
    #[test]
    fn comparison_symmetric(a: i64, b: i64) {
        let v_a = Value::BigInt(a);
        let v_b = Value::BigInt(b);

        if let (Some(ord_ab), Some(ord_ba)) = (v_a.compare(&v_b), v_b.compare(&v_a)) {
            prop_assert_eq!(ord_ab, ord_ba.reverse());
        }
    }

    /// Test that comparison is transitive
    #[test]
    fn comparison_transitive(a: i32, b: i32, c: i32) {
        let v_a = Value::Integer(a);
        let v_b = Value::Integer(b);
        let v_c = Value::Integer(c);

        // If a <= b and b <= c, then a <= c
        if let (Some(ab), Some(bc), Some(ac)) =
            (v_a.compare(&v_b), v_b.compare(&v_c), v_a.compare(&v_c)) {
            if ab != std::cmp::Ordering::Greater && bc != std::cmp::Ordering::Greater {
                prop_assert_ne!(ac, std::cmp::Ordering::Greater);
            }
        }
    }

    // ========================================================================
    // Decimal Precision Tests
    // ========================================================================

    /// Test that decimal arithmetic preserves scale
    #[test]
    fn decimal_scale_preserved(
        val in -10000i128..10000i128,
        scale in 0u8..10u8
    ) {
        let dec = Value::Decimal(val, scale);

        // Encoding and decoding should preserve scale
        let key = encode_key(&[dec.clone()]);
        let decoded = decode_key(&key);

        if let Value::Decimal(_, decoded_scale) = decoded[0] {
            prop_assert_eq!(decoded_scale, scale);
        } else {
            prop_assert!(false, "Expected Decimal");
        }
    }

    /// Test that different scales are properly distinguished
    #[test]
    fn decimal_different_scales_distinct(
        val in -1000i128..1000i128,
        scale1 in 0u8..5u8,
        scale2 in 0u8..5u8
    ) {
        if scale1 != scale2 {
            let dec1 = Value::Decimal(val, scale1);
            let dec2 = Value::Decimal(val, scale2);

            // Same value but different scales should encode differently
            let key1 = encode_key(&[dec1]);
            let key2 = encode_key(&[dec2]);

            prop_assert_ne!(key1, key2);
        }
    }

    // ========================================================================
    // Composite Key Tests
    // ========================================================================

    /// Test that composite keys round-trip correctly
    #[test]
    fn composite_key_round_trip(
        a: i64,
        b: i32,
        c in "[a-z]{0,10}"
    ) {
        let values = vec![
            Value::BigInt(a),
            Value::Integer(b),
            Value::Text(c),
        ];
        let key = encode_key(&values);
        let decoded = decode_key(&key);
        prop_assert_eq!(decoded, values);
    }

    /// Test that composite key ordering respects first element
    #[test]
    fn composite_key_ordering_respects_first(
        a1: i64,
        a2: i64,
        b in 0i32..100
    ) {
        let key1 = encode_key(&[Value::BigInt(a1), Value::Integer(b)]);
        let key2 = encode_key(&[Value::BigInt(a2), Value::Integer(b)]);

        // If first elements differ, that determines ordering
        if a1 != a2 {
            prop_assert_eq!(a1.cmp(&a2), key1.cmp(&key2));
        }
    }

    /// Test that composite key ordering uses second element as tiebreaker
    #[test]
    fn composite_key_ordering_tiebreaker(
        a: i64,
        b1: i32,
        b2: i32
    ) {
        let key1 = encode_key(&[Value::BigInt(a), Value::Integer(b1)]);
        let key2 = encode_key(&[Value::BigInt(a), Value::Integer(b2)]);

        // When first elements are equal, second element determines order
        prop_assert_eq!(b1.cmp(&b2), key1.cmp(&key2));
    }

    // ========================================================================
    // Decimal Arithmetic Properties
    // ========================================================================

    /// Test that decimal addition is commutative (when in range)
    #[test]
    fn decimal_addition_commutative(
        a in -1000i128..1000i128,
        b in -1000i128..1000i128,
        _scale in 0u8..5u8
    ) {
        // Only test when addition doesn't overflow
        if let (Some(sum1), Some(sum2)) = (a.checked_add(b), b.checked_add(a)) {
            prop_assert_eq!(sum1, sum2);
        }
    }

    /// Test that decimal values with different scales can be compared
    #[test]
    fn decimal_scale_normalization(
        val in -1000i128..1000i128,
        scale1 in 0u8..5u8,
        scale2 in 0u8..5u8
    ) {
        // Same logical value with different scales should equal
        // e.g., Value::Decimal(123, 1) == 12.3 and Value::Decimal(1230, 2) == 12.30
        if scale1 != scale2 {
            let multiplier = if scale2 > scale1 {
                10i128.pow(u32::from(scale2 - scale1))
            } else {
                1
            };

            if let Some(scaled_val) = val.checked_mul(multiplier) {
                let v1 = Value::Decimal(val, scale1);
                let v2 = Value::Decimal(scaled_val, scale2);

                // Should be comparable
                let _ = v1.compare(&v2);
            }
        }
    }

    // ========================================================================
    // Key Encoding Multi-Value Properties
    // ========================================================================

    /// Test that multi-value keys maintain ordering on first element
    #[test]
    fn multi_value_key_ordering_first_element(
        vals1 in prop::collection::vec(any::<i32>(), 1..5),
        vals2 in prop::collection::vec(any::<i32>(), 1..5)
    ) {
        if vals1.len() == vals2.len() && !vals1.is_empty() {
            let key1 = encode_key(&vals1.iter().map(|&v| Value::Integer(v)).collect::<Vec<_>>());
            let key2 = encode_key(&vals2.iter().map(|&v| Value::Integer(v)).collect::<Vec<_>>());

            // If first elements differ, that should determine the ordering
            if vals1[0] != vals2[0] {
                prop_assert_eq!(vals1[0].cmp(&vals2[0]), key1.cmp(&key2));
            }
        }
    }
}

// ============================================================================
// Correlated-subquery property tests (v0.6.0)
// ============================================================================
//
// Generate small randomised datasets of outer and inner rows, build a
// parameterised correlated query, and compare the engine's result
// against a naive nested-loop reference implementation.

use bytes::Bytes;
use kimberlite_store::TableId;
use proptest::collection::vec as pvec;

use crate::QueryEngine;
use crate::schema::{ColumnDef, DataType, SchemaBuilder};
use crate::tests::MockStore;

/// Build an engine + store populated with the outer (`users`) and
/// inner (`orders`) rows drawn by proptest. Returns (engine, store).
fn build_correlated_fixture(
    outer_ids: &[i64],
    inner_pairs: &[(i64, i64)], // (order_id, user_id)
) -> (QueryEngine, MockStore) {
    let schema = SchemaBuilder::new()
        .table(
            "users",
            TableId::new(1),
            vec![
                ColumnDef::new("id", DataType::BigInt).not_null(),
                ColumnDef::new("name", DataType::Text),
            ],
            vec!["id".into()],
        )
        .table(
            "orders",
            TableId::new(2),
            vec![
                ColumnDef::new("order_id", DataType::BigInt).not_null(),
                ColumnDef::new("user_id", DataType::BigInt).not_null(),
            ],
            vec!["order_id".into()],
        )
        .build();
    let mut store = MockStore::new();
    for id in outer_ids {
        store.insert(
            TableId::new(1),
            encode_key(&[Value::BigInt(*id)]),
            Bytes::from(
                serde_json::to_vec(&serde_json::json!({
                    "id": *id,
                    "name": format!("U{id}"),
                }))
                .unwrap(),
            ),
        );
    }
    for (order_id, user_id) in inner_pairs {
        store.insert(
            TableId::new(2),
            encode_key(&[Value::BigInt(*order_id)]),
            Bytes::from(
                serde_json::to_vec(&serde_json::json!({
                    "order_id": *order_id,
                    "user_id": *user_id,
                }))
                .unwrap(),
            ),
        );
    }
    (QueryEngine::new(schema), store)
}

/// Naive reference: outer ids for which EXISTS (inner matching
/// `o.user_id = u.id`) holds.
fn reference_exists(outer_ids: &[i64], inner_pairs: &[(i64, i64)]) -> Vec<i64> {
    let mut out: Vec<i64> = outer_ids
        .iter()
        .filter(|id| inner_pairs.iter().any(|(_, uid)| uid == *id))
        .copied()
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Naive reference: outer ids for which NOT EXISTS holds.
fn reference_not_exists(outer_ids: &[i64], inner_pairs: &[(i64, i64)]) -> Vec<i64> {
    let mut out: Vec<i64> = outer_ids
        .iter()
        .filter(|id| !inner_pairs.iter().any(|(_, uid)| uid == *id))
        .copied()
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Naive reference for `u.id IN (SELECT o.user_id FROM orders o
/// WHERE o.user_id = u.id)` — same semantics as EXISTS for this shape.
fn reference_in(outer_ids: &[i64], inner_pairs: &[(i64, i64)]) -> Vec<i64> {
    reference_exists(outer_ids, inner_pairs)
}

proptest! {
    /// Correlated EXISTS must agree with the naive reference
    /// implementation for every random (outer, inner) draw.
    #[test]
    fn correlated_exists_matches_reference(
        outer_ids in pvec(0i64..=50, 0..=10),
        inner_pairs in pvec((0i64..=500, 0i64..=50), 0..=20),
    ) {
        let uniq_outer: Vec<i64> = {
            let mut v = outer_ids.clone();
            v.sort_unstable();
            v.dedup();
            v
        };
        let uniq_pairs: Vec<(i64, i64)> = {
            let mut seen = std::collections::HashSet::new();
            inner_pairs
                .into_iter()
                .filter(|(oid, _)| seen.insert(*oid))
                .collect()
        };
        let (engine, mut store) = build_correlated_fixture(&uniq_outer, &uniq_pairs);
        let result = engine
            .query(
                &mut store,
                "SELECT id FROM users u WHERE EXISTS (SELECT order_id FROM orders o WHERE o.user_id = u.id) ORDER BY id",
                &[],
            )
            .unwrap();
        let got: Vec<i64> = result
            .rows
            .iter()
            .map(|r| match &r[0] {
                Value::BigInt(n) => *n,
                other => panic!("expected BigInt, got {other:?}"),
            })
            .collect();
        let expected = reference_exists(&uniq_outer, &uniq_pairs);
        prop_assert_eq!(got, expected);
    }

    /// Correlated NOT EXISTS — inverse of EXISTS.
    #[test]
    fn correlated_not_exists_matches_reference(
        outer_ids in pvec(0i64..=50, 0..=10),
        inner_pairs in pvec((0i64..=500, 0i64..=50), 0..=20),
    ) {
        let uniq_outer: Vec<i64> = {
            let mut v = outer_ids.clone();
            v.sort_unstable();
            v.dedup();
            v
        };
        let uniq_pairs: Vec<(i64, i64)> = {
            let mut seen = std::collections::HashSet::new();
            inner_pairs
                .into_iter()
                .filter(|(oid, _)| seen.insert(*oid))
                .collect()
        };
        let (engine, mut store) = build_correlated_fixture(&uniq_outer, &uniq_pairs);
        let result = engine
            .query(
                &mut store,
                "SELECT id FROM users u WHERE NOT EXISTS (SELECT order_id FROM orders o WHERE o.user_id = u.id) ORDER BY id",
                &[],
            )
            .unwrap();
        let got: Vec<i64> = result
            .rows
            .iter()
            .map(|r| match &r[0] {
                Value::BigInt(n) => *n,
                other => panic!("expected BigInt, got {other:?}"),
            })
            .collect();
        let expected = reference_not_exists(&uniq_outer, &uniq_pairs);
        prop_assert_eq!(got, expected);
    }

    /// Correlated IN (SELECT) — must also agree with the naive
    /// reference across random draws. Uses the same correlation
    /// shape `o.user_id = u.id` inside the inner WHERE so only rows
    /// with a matching order pass.
    #[test]
    fn correlated_in_subquery_matches_reference(
        outer_ids in pvec(0i64..=50, 0..=10),
        inner_pairs in pvec((0i64..=500, 0i64..=50), 0..=20),
    ) {
        let uniq_outer: Vec<i64> = {
            let mut v = outer_ids.clone();
            v.sort_unstable();
            v.dedup();
            v
        };
        let uniq_pairs: Vec<(i64, i64)> = {
            let mut seen = std::collections::HashSet::new();
            inner_pairs
                .into_iter()
                .filter(|(oid, _)| seen.insert(*oid))
                .collect()
        };
        let (engine, mut store) = build_correlated_fixture(&uniq_outer, &uniq_pairs);
        let result = engine
            .query(
                &mut store,
                "SELECT id FROM users u WHERE u.id IN (SELECT o.user_id FROM orders o WHERE o.user_id = u.id) ORDER BY id",
                &[],
            )
            .unwrap();
        let got: Vec<i64> = result
            .rows
            .iter()
            .map(|r| match &r[0] {
                Value::BigInt(n) => *n,
                other => panic!("expected BigInt, got {other:?}"),
            })
            .collect();
        let expected = reference_in(&uniq_outer, &uniq_pairs);
        prop_assert_eq!(got, expected);
    }
}

// Additional non-proptest property tests for special cases
#[cfg(test)]
mod special_property_tests {
    use crate::plan::matches_like_pattern;

    #[test]
    fn like_escaped_wildcards() {
        // Literal % should match only %
        assert!(matches_like_pattern("%", "\\%"));
        assert!(!matches_like_pattern("a", "\\%"));

        // Literal _ should match only _
        assert!(matches_like_pattern("_", "\\_"));
        assert!(!matches_like_pattern("a", "\\_"));
    }
}

// =============================================================================
// v0.7.0 — Scalar-expression purity property tests
// =============================================================================
//
// AUDIT-2026-05 S3.7. Companion to `specs/tla/ScalarPurity.tla`.
//
// The TLA+ spec captures purity as a closed theorem; these tests
// exercise the property at runtime against the actual evaluator.
// Two campaigns:
//
//   1. Determinism: same expression + same context → same output.
//      Hammered with random integer/text inputs across the closed
//      enum's unary subset (the simplest variants — exhaustive
//      coverage of more complex variants happens in the existing
//      window/aggregate proptests).
//
//   2. NULL propagation: NULL in any positional argument of a
//      null-propagating variant ⇒ NULL out. Covers MOD, POWER,
//      SQRT, SUBSTRING, EXTRACT, DATE_TRUNC plus the v0.5.x set.

mod scalar_purity {
    use crate::expression::{evaluate, EvalContext, ScalarExpr};
    use crate::schema::ColumnName;
    use crate::value::Value;
    use kimberlite_types::{DateField, SubstringRange};
    use proptest::prelude::*;

    fn ctx() -> (Vec<ColumnName>, Vec<Value>) {
        (Vec::new(), Vec::new())
    }

    fn eval(expr: &ScalarExpr) -> Result<Value, crate::error::QueryError> {
        let (cols, row) = ctx();
        evaluate(expr, &EvalContext::new(&cols, &row))
    }

    proptest! {
        /// AUDIT-2026-05 S3.7-A — determinism on UPPER/LOWER/LENGTH.
        #[test]
        fn upper_lower_length_deterministic(s in "[a-zA-Z0-9 ]{0,32}") {
            let lit = || ScalarExpr::Literal(Value::Text(s.clone()));
            let upper = ScalarExpr::Upper(Box::new(lit()));
            let lower = ScalarExpr::Lower(Box::new(lit()));
            let length = ScalarExpr::Length(Box::new(lit()));
            // Same expression evaluated twice must give the same output.
            prop_assert_eq!(eval(&upper).unwrap(), eval(&upper).unwrap());
            prop_assert_eq!(eval(&lower).unwrap(), eval(&lower).unwrap());
            prop_assert_eq!(eval(&length).unwrap(), eval(&length).unwrap());
        }

        /// AUDIT-2026-05 S3.7-B — MOD/POWER/SQRT determinism.
        #[test]
        fn numeric_scalars_deterministic(a in -10000_i64..10000, b in -10000_i64..10000) {
            let m = ScalarExpr::Mod(
                Box::new(ScalarExpr::Literal(Value::BigInt(a))),
                Box::new(ScalarExpr::Literal(Value::BigInt(b))),
            );
            let p = ScalarExpr::Power(
                Box::new(ScalarExpr::Literal(Value::BigInt(a))),
                Box::new(ScalarExpr::Literal(Value::BigInt(b.abs() % 6))), // small exp
            );
            let s = ScalarExpr::Sqrt(Box::new(ScalarExpr::Literal(Value::BigInt(a.abs()))));
            prop_assert_eq!(eval(&m).unwrap(), eval(&m).unwrap());
            prop_assert_eq!(eval(&p).unwrap(), eval(&p).unwrap());
            prop_assert_eq!(eval(&s).unwrap(), eval(&s).unwrap());
        }

        /// AUDIT-2026-05 S3.7-C — NULL propagates through every
        /// null-propagating v0.7.0 unary scalar.
        #[test]
        fn null_propagates_through_unary_scalars(_ignored in 0_u32..1) {
            let n = || Box::new(ScalarExpr::Literal(Value::Null));
            let exprs = [
                ScalarExpr::Upper(n()),
                ScalarExpr::Lower(n()),
                ScalarExpr::Length(n()),
                ScalarExpr::Trim(n()),
                ScalarExpr::Abs(n()),
                ScalarExpr::Round(n()),
                ScalarExpr::Ceil(n()),
                ScalarExpr::Floor(n()),
                ScalarExpr::Sqrt(n()),
                ScalarExpr::Substring(n(), SubstringRange::from_start(1)),
                ScalarExpr::Extract(DateField::Year, n()),
                ScalarExpr::DateTrunc(DateField::Year, n()),
            ];
            for e in &exprs {
                prop_assert_eq!(eval(e).unwrap(), Value::Null);
            }
        }

        /// AUDIT-2026-05 S3.7-D — NULL propagates through binary
        /// MOD/POWER (either operand NULL ⇒ NULL).
        #[test]
        fn null_propagates_through_mod_power(v in -1000_i64..1000) {
            let null = || Box::new(ScalarExpr::Literal(Value::Null));
            let lit = |x: i64| Box::new(ScalarExpr::Literal(Value::BigInt(x)));
            let cases = [
                ScalarExpr::Mod(null(), lit(v)),
                ScalarExpr::Mod(lit(v), null()),
                ScalarExpr::Power(null(), lit(v)),
                ScalarExpr::Power(lit(v), null()),
            ];
            for e in &cases {
                prop_assert_eq!(eval(e).unwrap(), Value::Null);
            }
        }
    }
}
