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
//! Added in v0.5.1:
//!
//! * `CAST(x AS T)` — numeric subtype conversions, numeric ↔ Text
//!   parsing/formatting, Boolean ↔ Text, and NULL preservation. The
//!   predicate-level integration landed with the `ScalarCmp`
//!   `Predicate` variant in the parser.
//!
//! Still deferred:
//!
//! * `MOD`, `POWER`, `SQRT` (number-theoretic — need proper overflow
//!   handling across TinyInt/SmallInt/Integer/BigInt/Real)
//! * `SUBSTRING`, `EXTRACT`, `DATE_TRUNC`, `NOW()`,
//!   `CURRENT_TIMESTAMP`, `CURRENT_DATE`, interval arithmetic — need
//!   a clock-threading decision we haven't made yet (VOPR sim clock
//!   vs production wall clock)
//!
//! Each function is a whitelisted, named variant on [`ScalarExpr`] —
//! deliberately not a dynamic-dispatch table, so a typo in a SQL
//! function name is rejected at planning time rather than runtime.

use crate::error::{QueryError, Result};
use crate::schema::{ColumnName, DataType};
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

    // --- Type coercion -----------------------------------------------------
    /// `CAST(x AS T)` — convert `x` to the target [`DataType`].
    ///
    /// NULL in → NULL out for every target. Overflow on narrowing
    /// integer casts and unparseable strings surface as
    /// [`QueryError::TypeMismatch`] rather than silent truncation.
    Cast(Box<ScalarExpr>, DataType),
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

        // ---- Type coercion ----
        ScalarExpr::Cast(inner, target) => cast_value(evaluate(inner, ctx)?, *target),
    }
}

/// Coerce `value` to `target`. NULL is preserved verbatim for every
/// target. Integer subtype widening is lossless; narrowing checks
/// for overflow. Numeric ↔ Text goes through `str::parse` / `Display`.
/// Boolean ↔ Text accepts the literals `"true"` / `"false"` (case-
/// insensitive). Returns `QueryError::TypeMismatch` for unsupported
/// source/target pairs rather than panicking or silently truncating.
fn cast_value(value: Value, target: DataType) -> Result<Value> {
    if matches!(value, Value::Null) {
        return Ok(Value::Null);
    }
    match (value, target) {
        // Identity casts — short-circuit even across subtle subtype
        // boundaries (Decimal keeps its scale).
        (v @ Value::TinyInt(_), DataType::TinyInt)
        | (v @ Value::SmallInt(_), DataType::SmallInt)
        | (v @ Value::Integer(_), DataType::Integer)
        | (v @ Value::BigInt(_), DataType::BigInt)
        | (v @ Value::Real(_), DataType::Real)
        | (v @ Value::Text(_), DataType::Text)
        | (v @ Value::Bytes(_), DataType::Bytes)
        | (v @ Value::Boolean(_), DataType::Boolean)
        | (v @ Value::Date(_), DataType::Date)
        | (v @ Value::Time(_), DataType::Time)
        | (v @ Value::Timestamp(_), DataType::Timestamp)
        | (v @ Value::Uuid(_), DataType::Uuid)
        | (v @ Value::Json(_), DataType::Json) => Ok(v),

        // Integer widening — always lossless.
        (Value::TinyInt(n), DataType::SmallInt) => Ok(Value::SmallInt(i16::from(n))),
        (Value::TinyInt(n), DataType::Integer) => Ok(Value::Integer(i32::from(n))),
        (Value::TinyInt(n), DataType::BigInt) => Ok(Value::BigInt(i64::from(n))),
        (Value::SmallInt(n), DataType::Integer) => Ok(Value::Integer(i32::from(n))),
        (Value::SmallInt(n), DataType::BigInt) => Ok(Value::BigInt(i64::from(n))),
        (Value::Integer(n), DataType::BigInt) => Ok(Value::BigInt(i64::from(n))),

        // Integer narrowing — checked.
        (Value::SmallInt(n), DataType::TinyInt) => i8::try_from(n)
            .map(Value::TinyInt)
            .map_err(|_| cast_error("SmallInt", "TinyInt", "overflow")),
        (Value::Integer(n), DataType::TinyInt) => i8::try_from(n)
            .map(Value::TinyInt)
            .map_err(|_| cast_error("Integer", "TinyInt", "overflow")),
        (Value::Integer(n), DataType::SmallInt) => i16::try_from(n)
            .map(Value::SmallInt)
            .map_err(|_| cast_error("Integer", "SmallInt", "overflow")),
        (Value::BigInt(n), DataType::TinyInt) => i8::try_from(n)
            .map(Value::TinyInt)
            .map_err(|_| cast_error("BigInt", "TinyInt", "overflow")),
        (Value::BigInt(n), DataType::SmallInt) => i16::try_from(n)
            .map(Value::SmallInt)
            .map_err(|_| cast_error("BigInt", "SmallInt", "overflow")),
        (Value::BigInt(n), DataType::Integer) => i32::try_from(n)
            .map(Value::Integer)
            .map_err(|_| cast_error("BigInt", "Integer", "overflow")),

        // Integer → Real — lossless for i32 and below; possible rounding
        // for i64 past 2^53 but no information loss vs the user's intent.
        (Value::TinyInt(n), DataType::Real) => Ok(Value::Real(f64::from(n))),
        (Value::SmallInt(n), DataType::Real) => Ok(Value::Real(f64::from(n))),
        (Value::Integer(n), DataType::Real) => Ok(Value::Real(f64::from(n))),
        #[allow(clippy::cast_precision_loss)]
        (Value::BigInt(n), DataType::Real) => Ok(Value::Real(n as f64)),

        // Real → Integer — truncate toward zero (standard SQL).
        (Value::Real(x), DataType::TinyInt) => f64_to_int::<i8>(x, "TinyInt").map(Value::TinyInt),
        (Value::Real(x), DataType::SmallInt) => {
            f64_to_int::<i16>(x, "SmallInt").map(Value::SmallInt)
        }
        (Value::Real(x), DataType::Integer) => f64_to_int::<i32>(x, "Integer").map(Value::Integer),
        (Value::Real(x), DataType::BigInt) => f64_to_int::<i64>(x, "BigInt").map(Value::BigInt),

        // Text → numerics — parse, error on bad input.
        (Value::Text(s), DataType::TinyInt) => s
            .trim()
            .parse::<i8>()
            .map(Value::TinyInt)
            .map_err(|_| cast_error("Text", "TinyInt", &s)),
        (Value::Text(s), DataType::SmallInt) => s
            .trim()
            .parse::<i16>()
            .map(Value::SmallInt)
            .map_err(|_| cast_error("Text", "SmallInt", &s)),
        (Value::Text(s), DataType::Integer) => s
            .trim()
            .parse::<i32>()
            .map(Value::Integer)
            .map_err(|_| cast_error("Text", "Integer", &s)),
        (Value::Text(s), DataType::BigInt) => s
            .trim()
            .parse::<i64>()
            .map(Value::BigInt)
            .map_err(|_| cast_error("Text", "BigInt", &s)),
        (Value::Text(s), DataType::Real) => s
            .trim()
            .parse::<f64>()
            .map(Value::Real)
            .map_err(|_| cast_error("Text", "Real", &s)),
        (Value::Text(s), DataType::Boolean) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "t" | "1" => Ok(Value::Boolean(true)),
            "false" | "f" | "0" => Ok(Value::Boolean(false)),
            _ => Err(cast_error("Text", "Boolean", &s)),
        },

        // Numerics → Text (canonical Display).
        (Value::TinyInt(n), DataType::Text) => Ok(Value::Text(n.to_string())),
        (Value::SmallInt(n), DataType::Text) => Ok(Value::Text(n.to_string())),
        (Value::Integer(n), DataType::Text) => Ok(Value::Text(n.to_string())),
        (Value::BigInt(n), DataType::Text) => Ok(Value::Text(n.to_string())),
        (Value::Real(n), DataType::Text) => Ok(Value::Text(n.to_string())),
        (Value::Boolean(b), DataType::Text) => {
            Ok(Value::Text(if b { "true" } else { "false" }.to_string()))
        }

        // Explicit unsupported pair — surface a clear error.
        (v, t) => Err(QueryError::TypeMismatch {
            expected: format!("CAST to {t:?}"),
            actual: format!("{v:?}"),
        }),
    }
}

fn cast_error(from: &str, to: &str, detail: &str) -> QueryError {
    QueryError::TypeMismatch {
        expected: format!("CAST from {from} to {to}"),
        actual: detail.to_string(),
    }
}

/// Truncate an `f64` toward zero into an integer, checking range.
/// NaN / ±∞ surface as `TypeMismatch` rather than silent conversion.
fn f64_to_int<T>(x: f64, target: &str) -> Result<T>
where
    T: TryFrom<i64>,
{
    if !x.is_finite() {
        return Err(cast_error("Real", target, &format!("{x}")));
    }
    // Truncate toward zero (i.e. SQL `CAST(Real AS Integer)` semantics).
    let truncated = x.trunc();
    // Range check against i64 first to get into `TryFrom` territory.
    #[allow(clippy::cast_possible_truncation)]
    let as_i64 = if (i64::MIN as f64) <= truncated && truncated <= (i64::MAX as f64) {
        truncated as i64
    } else {
        return Err(cast_error("Real", target, &format!("{x}")));
    };
    T::try_from(as_i64).map_err(|_| cast_error("Real", target, &format!("{x}")))
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
            ScalarExpr::Cast(Box::new(lit(Value::Null)), DataType::Integer),
        ] {
            assert_eq!(eval_standalone(&expr).unwrap(), Value::Null);
        }
    }

    #[test]
    fn cast_integer_widening_and_narrowing() {
        // Widening: always ok.
        let w = eval_standalone(&ScalarExpr::Cast(
            Box::new(lit(Value::TinyInt(42))),
            DataType::BigInt,
        ))
        .unwrap();
        assert_eq!(w, Value::BigInt(42));

        // Narrowing ok when in range.
        let ok = eval_standalone(&ScalarExpr::Cast(
            Box::new(lit(Value::BigInt(127))),
            DataType::TinyInt,
        ))
        .unwrap();
        assert_eq!(ok, Value::TinyInt(127));

        // Narrowing overflow errors out rather than silently truncating.
        let err = eval_standalone(&ScalarExpr::Cast(
            Box::new(lit(Value::BigInt(i64::from(i16::MAX) + 1))),
            DataType::SmallInt,
        ));
        assert!(err.is_err(), "narrowing overflow must be an error");
    }

    #[test]
    fn cast_text_to_numeric_parses() {
        assert_eq!(
            eval_standalone(&ScalarExpr::Cast(
                Box::new(lit(Value::Text("42".into()))),
                DataType::Integer,
            ))
            .unwrap(),
            Value::Integer(42),
        );
        assert_eq!(
            eval_standalone(&ScalarExpr::Cast(
                Box::new(lit(Value::Text("1.5".into()))),
                DataType::Real,
            ))
            .unwrap(),
            Value::Real(1.5),
        );
        assert!(
            eval_standalone(&ScalarExpr::Cast(
                Box::new(lit(Value::Text("nope".into()))),
                DataType::Integer,
            ))
            .is_err(),
            "unparseable text must error rather than coerce to 0"
        );
    }

    #[test]
    fn cast_numeric_to_text_formats_canonically() {
        assert_eq!(
            eval_standalone(&ScalarExpr::Cast(
                Box::new(lit(Value::BigInt(99))),
                DataType::Text,
            ))
            .unwrap(),
            Value::Text("99".into()),
        );
        assert_eq!(
            eval_standalone(&ScalarExpr::Cast(
                Box::new(lit(Value::Boolean(true))),
                DataType::Text,
            ))
            .unwrap(),
            Value::Text("true".into()),
        );
    }

    #[test]
    fn cast_real_to_int_truncates_toward_zero() {
        assert_eq!(
            eval_standalone(&ScalarExpr::Cast(
                Box::new(lit(Value::Real(1.9))),
                DataType::Integer,
            ))
            .unwrap(),
            Value::Integer(1),
        );
        assert_eq!(
            eval_standalone(&ScalarExpr::Cast(
                Box::new(lit(Value::Real(-1.9))),
                DataType::Integer,
            ))
            .unwrap(),
            Value::Integer(-1),
        );
        assert!(
            eval_standalone(&ScalarExpr::Cast(
                Box::new(lit(Value::Real(f64::NAN))),
                DataType::Integer,
            ))
            .is_err(),
            "NaN cast must error"
        );
    }

    #[test]
    fn cast_text_to_boolean_accepts_common_literals() {
        for (s, want) in [
            ("true", true),
            ("TRUE", true),
            ("t", true),
            ("1", true),
            ("false", false),
            ("F", false),
            ("0", false),
        ] {
            assert_eq!(
                eval_standalone(&ScalarExpr::Cast(
                    Box::new(lit(Value::Text(s.into()))),
                    DataType::Boolean,
                ))
                .unwrap(),
                Value::Boolean(want),
                "cast('{s}' as boolean)",
            );
        }
    }
}
