#![no_main]
//! ROADMAP v0.5.1 — isolates the scalar-expression evaluator from
//! planner/executor noise.
//!
//! Given a byte string, build a small `ScalarExpr` tree from a fixed
//! set of shapes (CAST / UPPER / LOWER / COALESCE / NULLIF / `||` /
//! column refs / literals) and evaluate it twice against the same
//! context. Asserts:
//!
//!   * `evaluate(expr, ctx)` doesn't panic under any input tree we
//!     build. Errors (type mismatch, CAST overflow) are fine — panics
//!     are not.
//!   * Two consecutive evaluations with the same context return the
//!     same `Value`. This is the determinism contract every scalar
//!     function is supposed to honour (no IO, no clocks, no RNG).

use arbitrary::{Arbitrary, Unstructured};
use kimberlite_query::{
    ColumnName, DataType, EvalContext, ScalarExpr, Value, evaluate,
};
use libfuzzer_sys::fuzz_target;

/// Shape enum we build via `Arbitrary`. Translated to `ScalarExpr`
/// below so the fuzzer doesn't have to mutate Rust-private structs.
#[derive(Debug, Arbitrary)]
enum Shape {
    ColA,
    ColB,
    LitText(String),
    LitInt(i64),
    LitNull,
    Upper(Box<Shape>),
    Lower(Box<Shape>),
    Length(Box<Shape>),
    Trim(Box<Shape>),
    Concat(Vec<Shape>),
    Abs(Box<Shape>),
    Round(Box<Shape>),
    Ceil(Box<Shape>),
    Floor(Box<Shape>),
    Coalesce(Vec<Shape>),
    Nullif(Box<Shape>, Box<Shape>),
    CastInt(Box<Shape>),
    CastText(Box<Shape>),
    CastBool(Box<Shape>),
}

fn shape_to_expr(s: &Shape, depth: usize) -> ScalarExpr {
    // Depth cap — fuzz inputs can recurse heavily; bound so we don't
    // blow the stack before the evaluator even runs.
    if depth > 16 {
        return ScalarExpr::Literal(Value::Null);
    }
    match s {
        Shape::ColA => ScalarExpr::Column(ColumnName::new(String::from("a"))),
        Shape::ColB => ScalarExpr::Column(ColumnName::new(String::from("b"))),
        Shape::LitText(s) => ScalarExpr::Literal(Value::Text(s.clone())),
        Shape::LitInt(n) => ScalarExpr::Literal(Value::BigInt(*n)),
        Shape::LitNull => ScalarExpr::Literal(Value::Null),
        Shape::Upper(x) => ScalarExpr::Upper(Box::new(shape_to_expr(x, depth + 1))),
        Shape::Lower(x) => ScalarExpr::Lower(Box::new(shape_to_expr(x, depth + 1))),
        Shape::Length(x) => ScalarExpr::Length(Box::new(shape_to_expr(x, depth + 1))),
        Shape::Trim(x) => ScalarExpr::Trim(Box::new(shape_to_expr(x, depth + 1))),
        Shape::Concat(xs) => {
            let mut parts: Vec<ScalarExpr> =
                xs.iter().take(6).map(|x| shape_to_expr(x, depth + 1)).collect();
            if parts.is_empty() {
                parts.push(ScalarExpr::Literal(Value::Text(String::new())));
            }
            ScalarExpr::Concat(parts)
        }
        Shape::Abs(x) => ScalarExpr::Abs(Box::new(shape_to_expr(x, depth + 1))),
        Shape::Round(x) => ScalarExpr::Round(Box::new(shape_to_expr(x, depth + 1))),
        Shape::Ceil(x) => ScalarExpr::Ceil(Box::new(shape_to_expr(x, depth + 1))),
        Shape::Floor(x) => ScalarExpr::Floor(Box::new(shape_to_expr(x, depth + 1))),
        Shape::Coalesce(xs) => {
            let mut parts: Vec<ScalarExpr> =
                xs.iter().take(6).map(|x| shape_to_expr(x, depth + 1)).collect();
            if parts.is_empty() {
                parts.push(ScalarExpr::Literal(Value::Null));
            }
            ScalarExpr::Coalesce(parts)
        }
        Shape::Nullif(a, b) => ScalarExpr::Nullif(
            Box::new(shape_to_expr(a, depth + 1)),
            Box::new(shape_to_expr(b, depth + 1)),
        ),
        Shape::CastInt(x) => ScalarExpr::Cast(Box::new(shape_to_expr(x, depth + 1)), DataType::Integer),
        Shape::CastText(x) => ScalarExpr::Cast(Box::new(shape_to_expr(x, depth + 1)), DataType::Text),
        Shape::CastBool(x) => ScalarExpr::Cast(Box::new(shape_to_expr(x, depth + 1)), DataType::Boolean),
    }
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(shape) = Shape::arbitrary(&mut u) else {
        return;
    };
    let expr = shape_to_expr(&shape, 0);

    let cols = vec![
        ColumnName::new(String::from("a")),
        ColumnName::new(String::from("b")),
    ];
    let row = vec![Value::Text("hello".into()), Value::BigInt(42)];
    let ctx = EvalContext::new(&cols, &row);

    let r1 = evaluate(&expr, &ctx);
    let r2 = evaluate(&expr, &ctx);
    match (&r1, &r2) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a, b, "determinism: same expr + ctx must produce same Value");
        }
        (Err(_), Err(_)) => {
            // Both errored — also deterministic. Don't compare error
            // strings verbatim; underlying QueryError is good enough.
        }
        _ => panic!(
            "non-determinism: re-evaluation flipped Ok/Err. first={:?} second={:?}",
            r1, r2
        ),
    }
});
