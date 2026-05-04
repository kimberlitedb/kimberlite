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
use kimberlite_types::{DateField, SubstringRange};

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

    // --- v0.7.0 scalar functions ----------------------------------------
    /// `MOD(a, b)` — remainder of integer division. `MOD(_, 0) → NULL`
    /// per Postgres semantics. AUDIT-2026-05 S3.7.
    Mod(Box<ScalarExpr>, Box<ScalarExpr>),
    /// `POWER(base, exp)` — `base^exp`. Returns `Real` for any real
    /// operand or non-integer exponent; integer-only inputs round-trip
    /// through `i64` and saturate on overflow. AUDIT-2026-05 S3.7.
    Power(Box<ScalarExpr>, Box<ScalarExpr>),
    /// `SQRT(x)` — square root. Negative input returns
    /// `QueryError::DomainError`. AUDIT-2026-05 S3.7.
    Sqrt(Box<ScalarExpr>),
    /// `SUBSTRING(s FROM start [FOR length])` with the
    /// [`SubstringRange`] domain primitive carrying the SQL
    /// 1-based / negative-start semantics. AUDIT-2026-05 S3.8.
    Substring(Box<ScalarExpr>, SubstringRange),
    /// `EXTRACT(field FROM ts)` — pull a calendar component from
    /// a `Date` or `Timestamp`. Field set is the closed
    /// [`DateField`] enum. AUDIT-2026-05 S3.7.
    Extract(DateField, Box<ScalarExpr>),
    /// `DATE_TRUNC('field', ts)` — truncate timestamp to the
    /// start of the field interval. Accepts only the truncatable
    /// subset of [`DateField`] (`Year/Month/Day/Hour/Minute/Second`).
    /// AUDIT-2026-05 S3.7.
    DateTrunc(DateField, Box<ScalarExpr>),
    /// `NOW()` — current statement-stable timestamp. The plan-time
    /// `fold_time_constants` pass replaces this variant with a
    /// `Literal(Timestamp)` before execution; if the evaluator
    /// ever sees a bare `Now`, that's a planner bug and we panic
    /// loudly. AUDIT-2026-05 S3.7.
    Now,
    /// `CURRENT_TIMESTAMP` — alias of `NOW()` per SQL standard;
    /// distinct variant only because some test fixtures want to
    /// pin the exact spelling. Same plan-time fold contract.
    CurrentTimestamp,
    /// `CURRENT_DATE` — current date, statement-stable. Plan-time
    /// folded to `Literal(Date(days_since_epoch))`.
    CurrentDate,
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

        // ---- v0.7.0: numeric ----
        ScalarExpr::Mod(a, b) => eval_mod(evaluate(a, ctx)?, evaluate(b, ctx)?),
        ScalarExpr::Power(base, exp) => eval_power(evaluate(base, ctx)?, evaluate(exp, ctx)?),
        ScalarExpr::Sqrt(inner) => eval_sqrt(evaluate(inner, ctx)?),

        // ---- v0.7.0: string ----
        ScalarExpr::Substring(inner, range) => eval_substring(evaluate(inner, ctx)?, *range),

        // ---- v0.7.0: date/time ----
        ScalarExpr::Extract(field, inner) => eval_extract(*field, evaluate(inner, ctx)?),
        ScalarExpr::DateTrunc(field, inner) => eval_date_trunc(*field, evaluate(inner, ctx)?),

        // ---- v0.7.0: time-now (plan-time fold sentinels) ----
        // The `fold_time_constants` planner pass MUST replace
        // these with `Literal(Timestamp/Date)` before execution.
        // Reaching the evaluator with a raw variant is a planner
        // bug — fail loudly per PRESSURECRAFT §1 (FCIS: the
        // evaluator stays pure, so it cannot read a clock here
        // even if it wanted to).
        ScalarExpr::Now | ScalarExpr::CurrentTimestamp | ScalarExpr::CurrentDate => {
            panic!(
                "scalar evaluator received raw NOW/CURRENT_TIMESTAMP/CURRENT_DATE \
                 — fold_time_constants planner pass must run first \
                 (AUDIT-2026-05 S3.7)"
            )
        }
    }
}

// ============================================================================
// v0.7.0 scalar evaluators
// ============================================================================

fn eval_mod(a: Value, b: Value) -> Result<Value> {
    // NULL propagation. SQL three-valued logic: any NULL → NULL.
    if matches!(a, Value::Null) || matches!(b, Value::Null) {
        return Ok(Value::Null);
    }
    // Coerce both operands to i64 for the divisor check; we
    // already widen TinyInt/SmallInt/Integer up to BigInt for ABS,
    // so the same widening is the right shape here.
    let a64 = numeric_as_i64(&a, "MOD")?;
    let b64 = numeric_as_i64(&b, "MOD")?;
    // Postgres semantics: MOD(_, 0) → NULL rather than panic.
    if b64 == 0 {
        return Ok(Value::Null);
    }
    // Postcondition: |result| < |b|. Pinned via debug_assert so
    // any future fast-path that miscomputes wraps a paired test.
    let result = a64.wrapping_rem(b64);
    debug_assert!(
        result.wrapping_abs() < b64.wrapping_abs() || b64 == i64::MIN,
        "MOD postcondition violated: |{result}| >= |{b64}|"
    );
    // Promote to whichever subtype matches `a`'s width — keeps
    // the result type discoverable at planning time.
    Ok(match a {
        Value::TinyInt(_) => i8::try_from(result)
            .map(Value::TinyInt)
            .unwrap_or(Value::BigInt(result)),
        Value::SmallInt(_) => i16::try_from(result)
            .map(Value::SmallInt)
            .unwrap_or(Value::BigInt(result)),
        Value::Integer(_) => i32::try_from(result)
            .map(Value::Integer)
            .unwrap_or(Value::BigInt(result)),
        _ => Value::BigInt(result),
    })
}

fn eval_power(base: Value, exp: Value) -> Result<Value> {
    if matches!(base, Value::Null) || matches!(exp, Value::Null) {
        return Ok(Value::Null);
    }
    // Any non-integer operand → Real. Pure-integer base+exp also
    // returns Real for now (matches Postgres `power()`); a future
    // optimisation could detect small integer exponents and stay
    // in i64, but correctness first.
    let base_f = numeric_as_f64(&base, "POWER")?;
    let exp_f = numeric_as_f64(&exp, "POWER")?;
    let result = base_f.powf(exp_f);
    // Reject NaN — that's a domain error, not a representable
    // numeric result.
    if result.is_nan() {
        return Err(domain_error(
            "POWER",
            &format!("POWER({base_f}, {exp_f}) is NaN"),
        ));
    }
    Ok(Value::Real(result))
}

fn eval_sqrt(value: Value) -> Result<Value> {
    if matches!(value, Value::Null) {
        return Ok(Value::Null);
    }
    let x = numeric_as_f64(&value, "SQRT")?;
    if x < 0.0 {
        return Err(domain_error(
            "SQRT",
            &format!("SQRT of negative input ({x})"),
        ));
    }
    Ok(Value::Real(x.sqrt()))
}

fn eval_substring(value: Value, range: SubstringRange) -> Result<Value> {
    if matches!(value, Value::Null) {
        return Ok(Value::Null);
    }
    let Value::Text(s) = value else {
        return Err(type_error("SUBSTRING", "Text", &value));
    };
    // SQL semantics: `start` is 1-based. A negative `start` shifts
    // the implicit slice left of position 1; `length` (when set)
    // is character count from the effective `start`. Compute the
    // effective char-index window and slice.
    let chars: Vec<char> = s.chars().collect();
    let total = chars.len() as i64;

    // Effective begin index (0-based, inclusive). `start = 1`
    // maps to index 0; `start <= 0` clips to 0.
    let begin_inclusive_1based = range.start;
    let begin0 = if begin_inclusive_1based < 1 {
        0_i64
    } else {
        begin_inclusive_1based - 1
    };

    // Effective end index (0-based, exclusive).
    let end0 = match range.length {
        Some(len) => {
            // `length` is from the user's `start`, NOT from `begin0`.
            // A negative start "consumes" some of the length.
            let raw_end = begin_inclusive_1based.saturating_sub(1).saturating_add(len);
            raw_end.min(total).max(0)
        }
        None => total,
    };

    let begin_clamped = begin0.max(0).min(total) as usize;
    let end_clamped = end0.max(0).min(total) as usize;

    if begin_clamped >= end_clamped {
        return Ok(Value::Text(String::new()));
    }
    let out: String = chars[begin_clamped..end_clamped].iter().collect();
    Ok(Value::Text(out))
}

fn eval_extract(field: DateField, value: Value) -> Result<Value> {
    use chrono::{Datelike, Timelike};
    if matches!(value, Value::Null) {
        return Ok(Value::Null);
    }
    let timestamp_ns = match &value {
        Value::Date(days) => i64::from(*days) * 86_400_000_000_000,
        Value::Timestamp(ts) => ts.as_nanos() as i64,
        other => return Err(type_error("EXTRACT", "Date or Timestamp", other)),
    };

    // Convert ns-since-epoch to chrono::NaiveDateTime via
    // DateTime<Utc>. We do not use chrono::Local — VOPR
    // determinism requires UTC.
    let secs = timestamp_ns.div_euclid(1_000_000_000);
    let nsec_part = timestamp_ns.rem_euclid(1_000_000_000) as u32;
    let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nsec_part).ok_or_else(|| {
        domain_error(
            "EXTRACT",
            &format!("timestamp {timestamp_ns} ns out of chrono range"),
        )
    })?;

    let result = match field {
        DateField::Year => Value::Integer(dt.year()),
        DateField::Month => Value::Integer(dt.month() as i32),
        DateField::Day => Value::Integer(dt.day() as i32),
        DateField::Hour => Value::Integer(dt.hour() as i32),
        DateField::Minute => Value::Integer(dt.minute() as i32),
        DateField::Second => Value::Integer(dt.second() as i32),
        DateField::Millisecond => Value::Integer((dt.timestamp_subsec_millis()) as i32),
        DateField::Microsecond => Value::Integer((dt.timestamp_subsec_micros()) as i32),
        DateField::DayOfWeek => {
            // Postgres: 0 = Sunday … 6 = Saturday.
            let nfu = dt.weekday().num_days_from_sunday() as i32;
            Value::Integer(nfu)
        }
        DateField::DayOfYear => Value::Integer(dt.ordinal() as i32),
        DateField::Quarter => Value::Integer(((dt.month() - 1) / 3 + 1) as i32),
        DateField::Week => Value::Integer(dt.iso_week().week() as i32),
        DateField::Epoch => Value::BigInt(secs),
    };
    Ok(result)
}

fn eval_date_trunc(field: DateField, value: Value) -> Result<Value> {
    use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
    if matches!(value, Value::Null) {
        return Ok(Value::Null);
    }
    if !field.is_truncatable() {
        return Err(QueryError::ParseError(format!(
            "DATE_TRUNC field {field:?} is not truncatable (use one of YEAR, MONTH, DAY, HOUR, MINUTE, SECOND)"
        )));
    }
    let timestamp_ns = match &value {
        Value::Date(days) => i64::from(*days) * 86_400_000_000_000,
        Value::Timestamp(ts) => ts.as_nanos() as i64,
        other => return Err(type_error("DATE_TRUNC", "Date or Timestamp", other)),
    };

    let secs = timestamp_ns.div_euclid(1_000_000_000);
    let nsec_part = timestamp_ns.rem_euclid(1_000_000_000) as u32;
    let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nsec_part)
        .ok_or_else(|| domain_error("DATE_TRUNC", "timestamp out of range"))?;
    let nv = dt.naive_utc();

    let truncated: NaiveDateTime = match field {
        DateField::Year => NaiveDate::from_ymd_opt(nv.year(), 1, 1)
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .ok_or_else(|| domain_error("DATE_TRUNC", "year truncation"))?,
        DateField::Month => NaiveDate::from_ymd_opt(nv.year(), nv.month(), 1)
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .ok_or_else(|| domain_error("DATE_TRUNC", "month truncation"))?,
        DateField::Day => NaiveDate::from_ymd_opt(nv.year(), nv.month(), nv.day())
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .ok_or_else(|| domain_error("DATE_TRUNC", "day truncation"))?,
        DateField::Hour => nv
            .date()
            .and_hms_opt(nv.hour(), 0, 0)
            .ok_or_else(|| domain_error("DATE_TRUNC", "hour truncation"))?,
        DateField::Minute => nv
            .date()
            .and_hms_opt(nv.hour(), nv.minute(), 0)
            .ok_or_else(|| domain_error("DATE_TRUNC", "minute truncation"))?,
        DateField::Second => nv
            .date()
            .and_hms_opt(nv.hour(), nv.minute(), nv.second())
            .ok_or_else(|| domain_error("DATE_TRUNC", "second truncation"))?,
        // Non-truncatable fields are rejected above; this is dead code that
        // exists only to keep the match exhaustive over the closed enum.
        _ => unreachable!("non-truncatable field passed `is_truncatable` check"),
    };

    let truncated_ns = truncated
        .and_utc()
        .timestamp_nanos_opt()
        .ok_or_else(|| domain_error("DATE_TRUNC", "truncated timestamp out of nanos range"))?;

    // Match the input shape: Date in → Date out (only when the
    // field is Year/Month/Day); everything else returns Timestamp.
    match (&value, field) {
        (Value::Date(_), DateField::Year | DateField::Month | DateField::Day) => Ok(Value::Date(
            i32::try_from(truncated_ns / 86_400_000_000_000).unwrap_or(0),
        )),
        _ => Ok(Value::Timestamp(kimberlite_types::Timestamp::from_nanos(
            truncated_ns.max(0) as u64,
        ))),
    }
}

/// Coerces any numeric `Value` into i64 for integer-style ops.
fn numeric_as_i64(v: &Value, fn_name: &str) -> Result<i64> {
    match v {
        Value::TinyInt(n) => Ok(i64::from(*n)),
        Value::SmallInt(n) => Ok(i64::from(*n)),
        Value::Integer(n) => Ok(i64::from(*n)),
        Value::BigInt(n) => Ok(*n),
        other => Err(type_error(fn_name, "Integer", other)),
    }
}

/// Coerces any numeric `Value` into f64 for float-style ops.
fn numeric_as_f64(v: &Value, fn_name: &str) -> Result<f64> {
    match v {
        Value::TinyInt(n) => Ok(f64::from(*n)),
        Value::SmallInt(n) => Ok(f64::from(*n)),
        Value::Integer(n) => Ok(f64::from(*n)),
        #[allow(clippy::cast_precision_loss)]
        Value::BigInt(n) => Ok(*n as f64),
        Value::Real(n) => Ok(*n),
        Value::Decimal(val, scale) => {
            #[allow(clippy::cast_precision_loss)]
            let f = (*val as f64) / 10f64.powi(i32::from(*scale));
            Ok(f)
        }
        other => Err(type_error(fn_name, "Numeric", other)),
    }
}

fn domain_error(fn_name: &str, detail: &str) -> QueryError {
    QueryError::TypeMismatch {
        expected: format!("{fn_name} domain"),
        actual: detail.to_string(),
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
    use kimberlite_types::{DateField, SubstringRange};

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

    // ========================================================================
    // v0.7.0 scalar functions — AUDIT-2026-05 S3.7 / S3.8
    // ========================================================================

    #[test]
    fn mod_basic() {
        assert_eq!(
            eval_standalone(&ScalarExpr::Mod(
                Box::new(lit(Value::BigInt(10))),
                Box::new(lit(Value::BigInt(3)))
            ))
            .unwrap(),
            Value::BigInt(1),
        );
    }

    #[test]
    fn mod_by_zero_returns_null_not_panic() {
        // Postgres semantics — diverges from Rust's `i64 % 0` panic.
        assert_eq!(
            eval_standalone(&ScalarExpr::Mod(
                Box::new(lit(Value::BigInt(7))),
                Box::new(lit(Value::BigInt(0)))
            ))
            .unwrap(),
            Value::Null,
        );
    }

    #[test]
    fn mod_propagates_null() {
        assert_eq!(
            eval_standalone(&ScalarExpr::Mod(
                Box::new(lit(Value::Null)),
                Box::new(lit(Value::BigInt(3)))
            ))
            .unwrap(),
            Value::Null,
        );
    }

    #[test]
    fn power_returns_real() {
        let r = eval_standalone(&ScalarExpr::Power(
            Box::new(lit(Value::BigInt(2))),
            Box::new(lit(Value::BigInt(10))),
        ))
        .unwrap();
        match r {
            Value::Real(x) => assert!((x - 1024.0).abs() < 1e-9),
            other => panic!("expected Real, got {other:?}"),
        }
    }

    #[test]
    fn sqrt_basic() {
        let r = eval_standalone(&ScalarExpr::Sqrt(Box::new(lit(Value::BigInt(16))))).unwrap();
        match r {
            Value::Real(x) => assert!((x - 4.0).abs() < 1e-9),
            other => panic!("expected Real, got {other:?}"),
        }
    }

    #[test]
    fn sqrt_negative_is_domain_error() {
        let err = eval_standalone(&ScalarExpr::Sqrt(Box::new(lit(Value::BigInt(-1)))))
            .expect_err("sqrt(-1) is a domain error");
        let msg = format!("{err:?}");
        assert!(msg.contains("SQRT") || msg.to_lowercase().contains("domain"));
    }

    #[test]
    fn substring_basic() {
        let r = eval_standalone(&ScalarExpr::Substring(
            Box::new(lit(Value::Text("kimberlite".into()))),
            SubstringRange::try_new(1, 5).unwrap(),
        ))
        .unwrap();
        assert_eq!(r, Value::Text("kimbe".into()));
    }

    #[test]
    fn substring_two_arg_form() {
        // 1-based start: position 5 → 0-based index 4 → 'e'.
        let r = eval_standalone(&ScalarExpr::Substring(
            Box::new(lit(Value::Text("kimberlite".into()))),
            SubstringRange::from_start(5),
        ))
        .unwrap();
        assert_eq!(r, Value::Text("erlite".into()));
    }

    #[test]
    fn substring_unicode_char_correct() {
        // 4 chars total, char-based slicing not byte-based.
        let r = eval_standalone(&ScalarExpr::Substring(
            Box::new(lit(Value::Text("café".into()))),
            SubstringRange::try_new(1, 3).unwrap(),
        ))
        .unwrap();
        assert_eq!(r, Value::Text("caf".into()));
    }

    #[test]
    fn substring_negative_start_clips_left() {
        // start = -1, length = 5 → effective end at index 3 (chars 0..3).
        let r = eval_standalone(&ScalarExpr::Substring(
            Box::new(lit(Value::Text("hello".into()))),
            SubstringRange::try_new(-1, 5).unwrap(),
        ))
        .unwrap();
        assert_eq!(r, Value::Text("hel".into()));
    }

    #[test]
    fn substring_propagates_null() {
        let r = eval_standalone(&ScalarExpr::Substring(
            Box::new(lit(Value::Null)),
            SubstringRange::from_start(1),
        ))
        .unwrap();
        assert_eq!(r, Value::Null);
    }

    #[test]
    fn extract_year_from_timestamp() {
        // 2025-05-04T00:00:00Z → 1746316800 epoch seconds.
        let ts = kimberlite_types::Timestamp::from_nanos(1_746_316_800 * 1_000_000_000);
        let r = eval_standalone(&ScalarExpr::Extract(
            DateField::Year,
            Box::new(lit(Value::Timestamp(ts))),
        ))
        .unwrap();
        assert_eq!(r, Value::Integer(2025));
    }

    #[test]
    fn extract_month_day_from_date() {
        // 1746316800 epoch sec / 86400 sec-per-day = 20212 days
        // = 2025-05-04 — verify month/day extraction.
        let days_since_epoch = 20_212_i32;
        let r_month = eval_standalone(&ScalarExpr::Extract(
            DateField::Month,
            Box::new(lit(Value::Date(days_since_epoch))),
        ))
        .unwrap();
        let r_day = eval_standalone(&ScalarExpr::Extract(
            DateField::Day,
            Box::new(lit(Value::Date(days_since_epoch))),
        ))
        .unwrap();
        assert_eq!(r_month, Value::Integer(5));
        assert_eq!(r_day, Value::Integer(4));
    }

    #[test]
    fn extract_epoch_from_timestamp() {
        // EPOCH returns Unix-epoch seconds round-trip.
        let ts = kimberlite_types::Timestamp::from_nanos(1_746_316_800 * 1_000_000_000);
        let r = eval_standalone(&ScalarExpr::Extract(
            DateField::Epoch,
            Box::new(lit(Value::Timestamp(ts))),
        ))
        .unwrap();
        assert_eq!(r, Value::BigInt(1_746_316_800));
    }

    #[test]
    fn extract_propagates_null() {
        let r = eval_standalone(&ScalarExpr::Extract(
            DateField::Year,
            Box::new(lit(Value::Null)),
        ))
        .unwrap();
        assert_eq!(r, Value::Null);
    }

    #[test]
    fn extract_rejects_non_temporal_input() {
        let err = eval_standalone(&ScalarExpr::Extract(
            DateField::Year,
            Box::new(lit(Value::Text("not a date".into()))),
        ))
        .expect_err("EXTRACT requires Date or Timestamp");
        assert!(format!("{err:?}").contains("EXTRACT"));
    }

    #[test]
    fn date_trunc_year_collapses_to_january_first() {
        // 2025-05-04T12:34:56Z = 1746362096 epoch sec.
        // Truncated to year → 2025-01-01T00:00:00Z = 1735689600.
        let ts = kimberlite_types::Timestamp::from_nanos(1_746_362_096 * 1_000_000_000);
        let r = eval_standalone(&ScalarExpr::DateTrunc(
            DateField::Year,
            Box::new(lit(Value::Timestamp(ts))),
        ))
        .unwrap();
        match r {
            Value::Timestamp(out) => {
                assert_eq!(out.as_nanos() as i64, 1_735_689_600 * 1_000_000_000_i64);
            }
            other => panic!("expected Timestamp, got {other:?}"),
        }
    }

    #[test]
    fn date_trunc_rejects_non_truncatable_field() {
        let ts = kimberlite_types::Timestamp::from_nanos(1_746_316_800 * 1_000_000_000);
        let err = eval_standalone(&ScalarExpr::DateTrunc(
            DateField::Quarter,
            Box::new(lit(Value::Timestamp(ts))),
        ))
        .expect_err("DATE_TRUNC rejects non-truncatable field");
        assert!(format!("{err:?}").to_lowercase().contains("trunc"));
    }

    #[test]
    fn date_trunc_propagates_null() {
        let r = eval_standalone(&ScalarExpr::DateTrunc(
            DateField::Year,
            Box::new(lit(Value::Null)),
        ))
        .unwrap();
        assert_eq!(r, Value::Null);
    }

    #[test]
    #[should_panic(expected = "fold_time_constants")]
    fn now_panics_at_evaluator_when_unfolded() {
        // PRESSURECRAFT §1 — evaluator stays pure. NOW must be
        // folded by the planner before reaching here. This pinned
        // panic is the canary that catches a regression where a
        // future planner refactor forgets the fold pass.
        let _ = eval_standalone(&ScalarExpr::Now);
    }

    #[test]
    #[should_panic(expected = "fold_time_constants")]
    fn current_timestamp_panics_at_evaluator_when_unfolded() {
        let _ = eval_standalone(&ScalarExpr::CurrentTimestamp);
    }

    #[test]
    #[should_panic(expected = "fold_time_constants")]
    fn current_date_panics_at_evaluator_when_unfolded() {
        let _ = eval_standalone(&ScalarExpr::CurrentDate);
    }
}
