---
title: "Scalar functions"
section: "reference"
slug: "sql/scalar-functions"
order: 5
---

# SQL scalar functions

Kimberlite's SQL surface ships a closed, named-variant set of scalar
functions. Each function is a whitelisted variant on
[`ScalarExpr`](https://docs.rs/kimberlite-query/0.7.0/kimberlite_query/expression/enum.ScalarExpr.html)
— typos in function names are rejected at planning time, not at
runtime. Every function preserves PRESSURECRAFT §1
(Functional Core / Imperative Shell): pure over its operands, no IO,
no clock reads from inside the evaluator.

## String

| Function | Description | Returns | Null in ⇒ null out |
|---|---|---|---|
| `UPPER(s)` | Unicode simple uppercase | `Text` | ✓ |
| `LOWER(s)` | Unicode simple lowercase | `Text` | ✓ |
| `LENGTH(s)` | Char count (NOT byte count) | `BigInt` | ✓ |
| `TRIM(s)` | Strip ASCII whitespace from both ends | `Text` | ✓ |
| `CONCAT(a, b, ...)` | Concatenation; one NULL ⇒ NULL out (Postgres-compat) | `Text` | ✓ |
| `SUBSTRING(s FROM start [FOR length])` | 1-based start; negative start clips left of position 1 (Postgres-compat); negative length rejected at parse time. Char-correct on Unicode (operates on `chars`, not bytes). | `Text` | ✓ |

## Numeric

| Function | Description | Returns | Null / domain |
|---|---|---|---|
| `ABS(n)` | Absolute value, preserves int subtype | matches input | NULL ⇒ NULL |
| `ROUND(x)` | Half-away-from-zero | matches input | NULL ⇒ NULL |
| `ROUND(x, scale)` | Round to `scale` decimal places | matches input | NULL ⇒ NULL |
| `CEIL(x)` / `CEILING(x)` | Least integer ≥ x | matches input | NULL ⇒ NULL |
| `FLOOR(x)` | Greatest integer ≤ x | matches input | NULL ⇒ NULL |
| `MOD(a, b)` | Remainder of integer division | matches `a` subtype | NULL ⇒ NULL; `MOD(_, 0) → NULL` (Postgres-compat) |
| `POWER(base, exp)` / `POW(base, exp)` | `base^exp` | `Real` | NULL ⇒ NULL; NaN result raises domain error |
| `SQRT(x)` | Square root | `Real` | NULL ⇒ NULL; negative input raises domain error |

## Date / time

| Function | Description | Returns |
|---|---|---|
| `EXTRACT(field FROM ts)` | Extract calendar component | `Integer` for sub-day fields, `BigInt` for `Epoch` |
| `DATE_TRUNC(field, ts)` | Truncate to start of field interval | `Date` if input is `Date` and field is `Year`/`Month`/`Day`; `Timestamp` otherwise |

`field` for both is the closed
[`DateField`](https://docs.rs/kimberlite-types/0.7.0/kimberlite_types/enum.DateField.html)
enum. Recognised SQL keywords (case-insensitive): `YEAR`, `MONTH`,
`DAY`, `HOUR`, `MINUTE`, `SECOND`, `MILLISECOND` (alias `MILLISECONDS`),
`MICROSECOND` (alias `MICROSECONDS`), `DOW` / `DAYOFWEEK`, `DOY` /
`DAYOFYEAR`, `QUARTER`, `WEEK`, `EPOCH`. `DATE_TRUNC` accepts only
the truncatable subset (`Year`/`Month`/`Day`/`Hour`/`Minute`/
`Second`); other fields surface as a parse error.

`DOW` returns 0 = Sunday … 6 = Saturday (Postgres-compat).

## Time-now

| Function | Statement-stable | Returns |
|---|---|---|
| `NOW()` | Yes | `Timestamp` |
| `CURRENT_TIMESTAMP` | Yes | `Timestamp` |
| `CURRENT_DATE` | Yes | `Date` |

**Statement-stable** semantics: every reference to `NOW()` (or
its aliases) inside a single statement evaluates to the same
timestamp — the wall-clock value at planning time. This matches
PostgreSQL's `now()` (NOT `clock_timestamp()`). The planner's
`fold_time_constants` pass replaces these sentinel variants with
literal values before execution; calling `NOW()` outside a
planner-invoked path panics loudly to surface the contract.

`clock_timestamp()` (per-call wall-clock) is intentionally out of
scope — it would impair VOPR determinism.

## Null / conditional

| Function | Description | Returns | Null behaviour |
|---|---|---|---|
| `COALESCE(e1, e2, ...)` | First non-NULL argument | matches first non-NULL | NULL only if every operand is NULL |
| `NULLIF(a, b)` | NULL if `a == b`, else `a` | matches `a` | — |
| `CAST(x AS T)` | Type coercion | `T` | NULL ⇒ NULL |

## Discipline

Every function in the table above:

- Has 2+ assertions (precondition on operand types, postcondition on result)
- Has a paired `#[should_panic]` test in `crates/kimberlite-query/src/expression.rs`
- Has property tests in `crates/kimberlite-query/src/tests/property_tests.rs`
  for null-propagation + determinism (AUDIT-2026-05 S3.7)
- Is covered by the [`ScalarPurity.tla`](../../../specs/tla/ScalarPurity.tla)
  formal-verification spec
