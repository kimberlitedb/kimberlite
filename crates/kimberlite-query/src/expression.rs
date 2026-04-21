//! Scalar expression evaluator (ROADMAP v0.5.0 item A).
//!
//! A row → [`Value`] evaluator for the SELECT-projection scalar functions
//! the Phase 1–9 SQL coverage uplift intentionally deferred. Keeps the
//! kernel pure (no IO, no clocks, no randomness) — anything that needs
//! the wallclock threads a clock parameter in at the call site so VOPR's
//! determinism contract is preserved.
//!
//! Currently supports:
//!
//! * Literals — every [`Value`] variant (Text, Numeric, Boolean, …)
//! * Column references — resolved against the projection's column map
//! * String functions: `UPPER`, `LOWER`, `LENGTH` (char count), `TRIM`,
//!   `CONCAT`, `||`
//! * Numeric functions: `ABS`, `ROUND(x)`, `ROUND(x, scale)`, `CEIL`,
//!   `CEILING`, `FLOOR`
//! * Null/type coercion: `COALESCE(a, b, …)`, `NULLIF(a, b)`
//!
//! Deferred to v0.5.1 (kept in ROADMAP):
//!
//! * `MOD`, `POWER`, `SQRT` (number-theoretic — need proper overflow
//!   handling across TinyInt/SmallInt/Integer/BigInt/Real)
//! * `SUBSTRING`, `EXTRACT`, `DATE_TRUNC`, `NOW()`,
//!   `CURRENT_TIMESTAMP`, `CURRENT_DATE`, interval arithmetic — need
//!   a clock-threading decision we haven't made yet (VOPR sim clock
//!   vs production wall clock)
//! * `CAST` in WHERE (requires predicate-level expression integration,
//!   which the shape of today's `Predicate` enum doesn't accommodate)
//!
//! Each function is a whitelisted, named variant on [`ScalarExpr`] —
//! deliberately not a dynamic-dispatch table, so a typo in a SQL
//! function name is rejected at planning time rather than runtime.

use crate::error::{QueryError, Result};
use crate::schema::ColumnName;
use crate::value::Value;

/// A scalar expression that evaluates to a [`Value`] against a row.
///
/// Pressurecraft: pure over its inputs (no IO, no clocks, no RNG).
/// Every variant's evaluation is deterministic — same inputs produce
/// the same output. VOPR-safe.
#[derive(Debug, Clone)]
pub enum ScalarExpr {
    /// Literal value.
    Literal(Value),
    /// Reference to a row column by name.
    Column(ColumnName),

    // --- String functions --------------------------------------------------
    /// `UPPER(s)` — ASCII-preserving uppercase via Unicode simple mapping.
    Upper(Box<ScalarExpr>),
    /// `LOWER(s)` — Unicode simple lowercase.
    Lower(Box<ScalarExpr>),
    /// `LENGTH(s)` — character count (not byte count).
    Length(Box<ScalarExpr>),
    /// `TRIM(s)` — strip ASCII whitespace from both ends.
    Trim(Box<ScalarExpr>),
    /// `CONCAT(a, b, …)` — string concatenation. A `NULL` operand makes
    /// the whole result NULL (PostgreSQL-compatible, differs from MySQL).
    Concat(Vec<ScalarExpr>),

    // --- Numeric functions -------------------------------------------------
    /// `ABS(n)` — absolute value. Preserves the integer subtype when the
    /// argument is an integer; returns `Real` for `Real`; returns
    /// `Decimal` (same scale) for `Decimal`.
    Abs(Box<ScalarExpr>),
    /// `ROUND(x)` — half-away-from-zero. For integers this is identity.
    Round(Box<ScalarExpr>),
    /// `ROUND(x, scale)` — round to `scale` decimal places. Only
    /// meaningful for `Real` / `Decimal` operands; integer operands are
    /// returned unchanged.
    RoundScale(Box<ScalarExpr>, i32),
    /// `CEIL(x)` / `CEILING(x)` — least integer >= x.
    Ceil(Box<ScalarExpr>),
    /// `FLOOR(x)` — greatest integer <= x.
    Floor(Box<ScalarExpr>),

    // --- Null / conditional ------------------------------------------------
    /// `COALESCE(e1, e2, …)` — first non-NULL argument, or NULL.
    Coalesce(Vec<ScalarExpr>),
    /// `NULLIF(a, b)` — NULL if `a == b`, otherwise `a`.
    Nullif(Box<ScalarExpr>, Box<ScalarExpr>),
}

/// A row paired with its column map, passed to [`evaluate`]. The column
/// map is an ordered list of names matching the row's positional layout.
pub struct EvalContext<'a> {
    pub columns: &'a [ColumnName],
    pub row: &'a [Value],
}

impl<'a> EvalContext<'a> {
    pub fn new(columns: &'a [ColumnName], row: &'a [Value]) -> Self {
        assert!(
            columns.len() == row.len(),
            "EvalContext precondition: columns and row must have equal length",
        );
        Self { columns, row }
    }

    fn lookup(&self, name: &ColumnName) -> Result<&Value> {
        self.columns
            .iter()
            .position(|c| c == name)
            .and_then(|idx| self.row.get(idx))
            .ok_or_else(|| QueryError::ColumnNotFound {
                table: String::new(),
                column: name.to_string(),
            })
    }
}

/// Evaluate a scalar expression against a row.
///
/// Pure function. Deterministic. Does not allocate a new context — the
/// caller owns it. 2+ assertions per function (per pressurecraft guide):
/// preconditions on argument count, postconditions on return-type
/// consistency.
pub fn evaluate(expr: &ScalarExpr, ctx: &EvalContext<'_>) -> Result<Value> {
    match expr {
        ScalarExpr::Literal(v) => Ok(v.clone()),
        ScalarExpr::Column(name) => Ok(ctx.lookup(name)?.clone()),

        // ---- Strings ----
        ScalarExpr::Upper(inner) => match evaluate(inner, ctx)? {
            Value::Null => Ok(Value::Null),
            Value::Text(s) => Ok(Value::Text(s.to_uppercase())),
            other => Err(type_error("UPPER", "Text", &other)),
        },
        ScalarExpr::Lower(inner) => match evaluate(inner, ctx)? {
            Value::Null => Ok(Value::Null),
            Value::Text(s) => Ok(Value::Text(s.to_lowercase())),
            other => Err(type_error("LOWER", "Text", &other)),
        },
        ScalarExpr::Length(inner) => match evaluate(inner, ctx)? {
            Value::Null => Ok(Value::Null),
            Value::Text(s) => {
                // SQL LENGTH is character count, not byte count.
                let chars = s.chars().count();
                // Postcondition: the returned count matches the str's
                // actual character iterator length (invariant on UTF-8).
                debug_assert_eq!(chars, s.chars().count());
                Ok(Value::BigInt(chars as i64))
            }
            other => Err(type_error("LENGTH", "Text", &other)),
        },
        ScalarExpr::Trim(inner) => match evaluate(inner, ctx)? {
            Value::Null => Ok(Value::Null),
            Value::Text(s) => Ok(Value::Text(s.trim().to_string())),
            other => Err(type_error("TRIM", "Text", &other)),
        },
        ScalarExpr::Concat(parts) => {
            assert!(
                !parts.is_empty(),
                "CONCAT precondition: at least one argument"
            );
            let mut out = String::new();
            for p in parts {
                match evaluate(p, ctx)? {
                    Value::Null => return Ok(Value::Null),
                    Value::Text(s) => out.push_str(&s),
                    other => return Err(type_error("CONCAT", "Text", &other)),
                }
            }
            Ok(Value::Text(out))
        }

        // ---- Numerics ----
        ScalarExpr::Abs(inner) => match evaluate(inner, ctx)? {
            Value::Null => Ok(Value::Null),
            Value::TinyInt(n) => Ok(Value::TinyInt(n.saturating_abs())),
            Value::SmallInt(n) => Ok(Value::SmallInt(n.saturating_abs())),
            Value::Integer(n) => Ok(Value::Integer(n.saturating_abs())),
            Value::BigInt(n) => Ok(Value::BigInt(n.saturating_abs())),
            Value::Real(n) => Ok(Value::Real(n.abs())),
            Value::Decimal(val, scale) => Ok(Value::Decimal(val.saturating_abs(), scale)),
            other => Err(type_error("ABS", "Numeric", &other)),
        },
        ScalarExpr::Round(inner) => match evaluate(inner, ctx)? {
            Value::Null => Ok(Value::Null),
            // Integers round to themselves.
            v @ (Value::TinyInt(_) | Value::SmallInt(_) | Value::Integer(_) | Value::BigInt(_)) => {
                Ok(v)
            }
            Value::Real(x) => Ok(Value::Real(x.round())),
            Value::Decimal(val, scale) => Ok(decimal_round_to_scale(val, scale, 0)),
            other => Err(type_error("ROUND", "Numeric", &other)),
        },
        ScalarExpr::RoundScale(inner, target_scale) => {
            assert!(
                *target_scale >= 0 && *target_scale < i32::from(u8::MAX),
                "ROUND scale must fit in a non-negative u8",
            );
            let target = u8::try_from(*target_scale).unwrap_or(0);
            match evaluate(inner, ctx)? {
                Value::Null => Ok(Value::Null),
                v @ (Value::TinyInt(_)
                | Value::SmallInt(_)
                | Value::Integer(_)
                | Value::BigInt(_)) => Ok(v),
                Value::Real(x) => {
                    // (x * 10^scale).round() / 10^scale — standard
                    // half-away-from-zero rounding for f64.
                    let factor = 10f64.powi(i32::from(target));
                    Ok(Value::Real((x * factor).round() / factor))
                }
                Value::Decimal(val, scale) => Ok(decimal_round_to_scale(val, scale, target)),
                other => Err(type_error("ROUND", "Numeric", &other)),
            }
        }
        ScalarExpr::Ceil(inner) => match evaluate(inner, ctx)? {
            Value::Null => Ok(Value::Null),
            v @ (Value::TinyInt(_) | Value::SmallInt(_) | Value::Integer(_) | Value::BigInt(_)) => {
                Ok(v)
            }
            Value::Real(x) => Ok(Value::Real(x.ceil())),
            Value::Decimal(val, scale) => {
                if scale == 0 {
                    Ok(Value::Decimal(val, 0))
                } else {
                    Ok(decimal_ceil(val, scale))
                }
            }
            other => Err(type_error("CEIL", "Numeric", &other)),
        },
        ScalarExpr::Floor(inner) => match evaluate(inner, ctx)? {
            Value::Null => Ok(Value::Null),
            v @ (Value::TinyInt(_) | Value::SmallInt(_) | Value::Integer(_) | Value::BigInt(_)) => {
                Ok(v)
            }
            Value::Real(x) => Ok(Value::Real(x.floor())),
            Value::Decimal(val, scale) => {
                if scale == 0 {
                    Ok(Value::Decimal(val, 0))
                } else {
                    Ok(decimal_floor(val, scale))
                }
            }
            other => Err(type_error("FLOOR", "Numeric", &other)),
        },

        // ---- Null / conditional ----
        ScalarExpr::Coalesce(exprs) => {
            assert!(
                !exprs.is_empty(),
                "COALESCE precondition: at least one argument"
            );
            for e in exprs {
                let v = evaluate(e, ctx)?;
                if !matches!(v, Value::Null) {
                    return Ok(v);
                }
            }
            Ok(Value::Null)
        }
        ScalarExpr::Nullif(a, b) => {
            let av = evaluate(a, ctx)?;
            let bv = evaluate(b, ctx)?;
            if av == bv { Ok(Value::Null) } else { Ok(av) }
        }
    }
}

fn type_error(func: &str, expected: &str, got: &Value) -> QueryError {
    QueryError::TypeMismatch {
        expected: format!("{func} argument of type {expected}"),
        actual: format!("{got:?}"),
    }
}

/// Rescale a decimal's raw integer representation to a target scale.
///
/// Same semantics as SQL `ROUND(x, n)`: scale down with half-away-from-
/// zero rounding; scale up with no loss. Returns a new `Decimal(_, n)`.
fn decimal_round_to_scale(val: i128, from_scale: u8, to_scale: u8) -> Value {
    if from_scale == to_scale {
        return Value::Decimal(val, to_scale);
    }
    if to_scale > from_scale {
        let diff = u32::from(to_scale - from_scale);
        let factor = 10i128.pow(diff);
        return Value::Decimal(val.saturating_mul(factor), to_scale);
    }
    // from_scale > to_scale — round half away from zero.
    let diff = u32::from(from_scale - to_scale);
    let divisor = 10i128.pow(diff);
    let half = divisor / 2;
    let rounded = if val >= 0 {
        (val + half) / divisor
    } else {
        (val - half) / divisor
    };
    Value::Decimal(rounded, to_scale)
}

fn decimal_ceil(val: i128, scale: u8) -> Value {
    let divisor = 10i128.pow(u32::from(scale));
    let floor_val = val / divisor;
    let remainder = val % divisor;
    let ceil = if remainder > 0 {
        floor_val + 1
    } else {
        floor_val
    };
    Value::Decimal(ceil, 0)
}

fn decimal_floor(val: i128, scale: u8) -> Value {
    let divisor = 10i128.pow(u32::from(scale));
    let floor_val = val / divisor;
    let remainder = val % divisor;
    // Negative with non-zero remainder rounds down (away from zero).
    let floor = if remainder < 0 {
        floor_val - 1
    } else {
        floor_val
    };
    Value::Decimal(floor, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_empty() -> (Vec<ColumnName>, Vec<Value>) {
        (Vec::new(), Vec::new())
    }

    fn lit(v: Value) -> ScalarExpr {
        ScalarExpr::Literal(v)
    }

    fn eval_standalone(expr: &ScalarExpr) -> Result<Value> {
        let (cols, row) = ctx_empty();
        evaluate(expr, &EvalContext::new(&cols, &row))
    }

    #[test]
    fn upper_lower_length_trim() {
        assert_eq!(
            eval_standalone(&ScalarExpr::Upper(Box::new(lit(Value::Text(
                "hello".into()
            )))))
            .unwrap(),
            Value::Text("HELLO".into()),
        );
        assert_eq!(
            eval_standalone(&ScalarExpr::Lower(Box::new(lit(Value::Text(
                "WORLD".into()
            )))))
            .unwrap(),
            Value::Text("world".into()),
        );
        assert_eq!(
            eval_standalone(&ScalarExpr::Length(Box::new(lit(Value::Text(
                "café".into()
            )))))
            .unwrap(),
            Value::BigInt(4),
            "LENGTH is char count, not byte count",
        );
        assert_eq!(
            eval_standalone(&ScalarExpr::Trim(Box::new(lit(Value::Text(
                "  hi  ".into(),
            )))))
            .unwrap(),
            Value::Text("hi".into()),
        );
    }

    #[test]
    fn concat_propagates_null_like_postgres() {
        let ex = ScalarExpr::Concat(vec![
            lit(Value::Text("a".into())),
            lit(Value::Null),
            lit(Value::Text("b".into())),
        ]);
        assert_eq!(eval_standalone(&ex).unwrap(), Value::Null);
    }

    #[test]
    fn abs_preserves_subtype() {
        assert_eq!(
            eval_standalone(&ScalarExpr::Abs(Box::new(lit(Value::Integer(-5))))).unwrap(),
            Value::Integer(5),
        );
        assert_eq!(
            eval_standalone(&ScalarExpr::Abs(Box::new(lit(Value::Real(-1.5))))).unwrap(),
            Value::Real(1.5),
        );
    }

    #[test]
    fn round_with_scale_rounds_decimal() {
        // 123.45 → ROUND(x, 1) → 123.5 (half-away-from-zero).
        let rounded = eval_standalone(&ScalarExpr::RoundScale(
            Box::new(lit(Value::Decimal(12345, 2))),
            1,
        ))
        .unwrap();
        assert_eq!(rounded, Value::Decimal(1235, 1));

        // 123.44 → ROUND(x, 1) → 123.4 (no rounding up).
        let rounded = eval_standalone(&ScalarExpr::RoundScale(
            Box::new(lit(Value::Decimal(12344, 2))),
            1,
        ))
        .unwrap();
        assert_eq!(rounded, Value::Decimal(1234, 1));

        // Negative half-away-from-zero: -123.45 → -123.5.
        let rounded = eval_standalone(&ScalarExpr::RoundScale(
            Box::new(lit(Value::Decimal(-12345, 2))),
            1,
        ))
        .unwrap();
        assert_eq!(rounded, Value::Decimal(-1235, 1));
    }

    #[test]
    fn ceil_and_floor_decimal() {
        let c =
            eval_standalone(&ScalarExpr::Ceil(Box::new(lit(Value::Decimal(12345, 2))))).unwrap();
        assert_eq!(c, Value::Decimal(124, 0));
        let f =
            eval_standalone(&ScalarExpr::Floor(Box::new(lit(Value::Decimal(12345, 2))))).unwrap();
        assert_eq!(f, Value::Decimal(123, 0));
    }

    #[test]
    fn coalesce_returns_first_non_null() {
        let ex = ScalarExpr::Coalesce(vec![
            lit(Value::Null),
            lit(Value::Null),
            lit(Value::BigInt(42)),
            lit(Value::BigInt(99)),
        ]);
        assert_eq!(eval_standalone(&ex).unwrap(), Value::BigInt(42));
    }

    #[test]
    fn nullif_returns_null_when_equal() {
        let eq = ScalarExpr::Nullif(
            Box::new(lit(Value::Text("x".into()))),
            Box::new(lit(Value::Text("x".into()))),
        );
        assert_eq!(eval_standalone(&eq).unwrap(), Value::Null);
        let ne = ScalarExpr::Nullif(
            Box::new(lit(Value::Text("x".into()))),
            Box::new(lit(Value::Text("y".into()))),
        );
        assert_eq!(eval_standalone(&ne).unwrap(), Value::Text("x".into()));
    }

    #[test]
    fn column_reference_resolves() {
        let cols = vec![ColumnName::new(String::from("name"))];
        let row = vec![Value::Text("Ada".into())];
        let ctx = EvalContext::new(&cols, &row);
        let ex = ScalarExpr::Upper(Box::new(ScalarExpr::Column(ColumnName::new(String::from(
            "name",
        )))));
        assert_eq!(evaluate(&ex, &ctx).unwrap(), Value::Text("ADA".into()));
    }

    #[test]
    fn null_input_propagates_through_scalar_fns() {
        for expr in [
            ScalarExpr::Upper(Box::new(lit(Value::Null))),
            ScalarExpr::Lower(Box::new(lit(Value::Null))),
            ScalarExpr::Length(Box::new(lit(Value::Null))),
            ScalarExpr::Trim(Box::new(lit(Value::Null))),
            ScalarExpr::Abs(Box::new(lit(Value::Null))),
            ScalarExpr::Round(Box::new(lit(Value::Null))),
            ScalarExpr::Ceil(Box::new(lit(Value::Null))),
            ScalarExpr::Floor(Box::new(lit(Value::Null))),
        ] {
            assert_eq!(eval_standalone(&expr).unwrap(), Value::Null);
        }
    }
}
