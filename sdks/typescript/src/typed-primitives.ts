/**
 * TypeScript bindings for the v0.7.0 typed primitives —
 * `DateField`, `Interval`, `SubstringRange`, `AggregateMemoryBudget`.
 *
 * These types live inside the query evaluator on the Rust side
 * (`crates/kimberlite-types/src/domain.rs`); the wire protocol's
 * `QueryValue` does not yet carry them as first-class kinds. Until
 * wire-level support lands, callers reach them through SQL — but they
 * shouldn't have to hand-concatenate strings. The helpers in this
 * module produce safe SQL fragments + literals so notebar (and any
 * other v0.7.0 consumer) can construct EXTRACT / DATE_TRUNC /
 * SUBSTRING / INTERVAL expressions ergonomically.
 *
 * Wire-level binding is tracked as a follow-up; once `QueryValue` and
 * `QueryParam` grow these variants, the helpers in this module will
 * gain typed-parameter overloads and stay backwards-compatible.
 */

// ============================================================================
// DateField — closed enum mirroring `DateField` at
// `crates/kimberlite-types/src/domain.rs:946`.
// ============================================================================

/**
 * `EXTRACT(field FROM ts)` field selector. Mirrors the Rust
 * `DateField` enum verbatim (TS string-literal form per v0.8.0
 * SDK shape decision). Postgres aliases `DOW` and `DOY` are
 * exposed as `dayOfWeek` and `dayOfYear` for consistency with
 * idiomatic TS naming.
 */
export type DateField =
  | 'year'
  | 'month'
  | 'day'
  | 'hour'
  | 'minute'
  | 'second'
  | 'millisecond'
  | 'microsecond'
  | 'dayOfWeek'
  | 'dayOfYear'
  | 'quarter'
  | 'week'
  | 'epoch';

/**
 * The subset of {@link DateField} accepted by `DATE_TRUNC`. The Rust
 * evaluator rejects the others (`dayOfWeek`, `dayOfYear`, `quarter`,
 * `epoch`, `millisecond`, `microsecond`) — this type catches the bug
 * at compile time instead of at query-execution time.
 */
export type TruncatableDateField =
  | 'year'
  | 'month'
  | 'day'
  | 'hour'
  | 'minute'
  | 'second';

/**
 * Map a {@link DateField} to its SQL keyword (uppercase, Postgres
 * aliases for `dayOfWeek` → `DOW`, `dayOfYear` → `DOY`).
 */
export function dateFieldKeyword(field: DateField): string {
  switch (field) {
    case 'year':
      return 'YEAR';
    case 'month':
      return 'MONTH';
    case 'day':
      return 'DAY';
    case 'hour':
      return 'HOUR';
    case 'minute':
      return 'MINUTE';
    case 'second':
      return 'SECOND';
    case 'millisecond':
      return 'MILLISECOND';
    case 'microsecond':
      return 'MICROSECOND';
    case 'dayOfWeek':
      return 'DOW';
    case 'dayOfYear':
      return 'DOY';
    case 'quarter':
      return 'QUARTER';
    case 'week':
      return 'WEEK';
    case 'epoch':
      return 'EPOCH';
  }
}

/**
 * Build an `EXTRACT(field FROM expr)` SQL fragment.
 *
 * `expr` is spliced verbatim — pass a column name, function call, or
 * other trusted expression. To extract from a parameter-bound value,
 * inline `$1` etc. as the `expr`.
 *
 * @example
 * ```ts
 * extractFromSql('year', 'created_at')
 * // → "EXTRACT(YEAR FROM created_at)"
 * ```
 */
export function extractFromSql(field: DateField, expr: string): string {
  return `EXTRACT(${dateFieldKeyword(field)} FROM ${expr})`;
}

/**
 * Build a `DATE_TRUNC('field', expr)` SQL fragment. The
 * {@link TruncatableDateField} type rules out fields that
 * `DATE_TRUNC` rejects (e.g. `dayOfWeek`).
 */
export function dateTruncSql(field: TruncatableDateField, expr: string): string {
  return `DATE_TRUNC('${field}', ${expr})`;
}

// ============================================================================
// Interval — mirrors `Interval` at
// `crates/kimberlite-types/src/domain.rs:1119`.
// ============================================================================

/** One day in nanoseconds. Mirrors Rust `NANOS_PER_DAY`. */
export const NANOS_PER_DAY = 86_400_000_000_000n;

/**
 * SQL `INTERVAL` value with three independent components.
 *
 * Mirrors the Rust struct: month, day, and sub-day (nanos)
 * components are kept separate because they have different
 * semantics under arithmetic. `months` are calendar-relative,
 * `days` are wall-clock, `nanos` is the sub-day remainder.
 *
 * Construction is unchecked at the type level; use
 * {@link intervalFromComponents} to enforce the
 * `|nanos| < NANOS_PER_DAY` invariant and normalise overflow into
 * `days`.
 */
export interface Interval {
  /** Calendar months. Independent of days/nanos under arithmetic. */
  months: number;
  /** Wall-clock days. */
  days: number;
  /** Sub-day remainder in nanoseconds. */
  nanos: bigint;
}

/** Thrown when {@link intervalFromComponents} would overflow `i32`. */
export class IntervalOverflowError extends Error {
  constructor(reason: 'days' | 'months', value: bigint | number) {
    super(`Interval ${reason} overflow: ${value} exceeds i32 range`);
    this.name = 'IntervalOverflowError';
    Object.setPrototypeOf(this, IntervalOverflowError.prototype);
  }
}

const I32_MAX = 2_147_483_647n;
const I32_MIN = -2_147_483_648n;

/**
 * Constructs an {@link Interval} from raw components, normalising
 * `nanos` overflow into `days` so the in-memory invariant
 * `|nanos| < NANOS_PER_DAY` holds afterward. Mirrors the Rust
 * `Interval::try_from_components` path.
 *
 * @throws {IntervalOverflowError} if normalisation pushes `days`
 *   past `i32::MAX`.
 */
export function intervalFromComponents(
  months: number,
  days: number,
  nanos: bigint,
): Interval {
  if (!Number.isInteger(months) || BigInt(months) > I32_MAX || BigInt(months) < I32_MIN) {
    throw new IntervalOverflowError('months', months);
  }
  if (!Number.isInteger(days)) {
    throw new IntervalOverflowError('days', days);
  }

  const extraDays = nanos / NANOS_PER_DAY;
  const normalisedNanos = nanos % NANOS_PER_DAY;

  if (extraDays > I32_MAX || extraDays < I32_MIN) {
    throw new IntervalOverflowError('days', extraDays);
  }
  const newDays = BigInt(days) + extraDays;
  if (newDays > I32_MAX || newDays < I32_MIN) {
    throw new IntervalOverflowError('days', newDays);
  }

  return {
    months,
    days: Number(newDays),
    nanos: normalisedNanos,
  };
}

/** Construct an interval entirely in months. */
export function intervalFromMonths(months: number): Interval {
  return intervalFromComponents(months, 0, 0n);
}

/** Construct an interval entirely in days. */
export function intervalFromDays(days: number): Interval {
  return intervalFromComponents(0, days, 0n);
}

/** Construct an interval entirely in nanoseconds (auto-normalises). */
export function intervalFromNanos(nanos: bigint): Interval {
  return intervalFromComponents(0, 0, nanos);
}

/**
 * Build an `INTERVAL` SQL literal — three components emitted
 * independently to match Postgres semantics. Suitable for splicing
 * into a query string or into a `Query` builder fragment.
 *
 * @example
 * ```ts
 * intervalLiteral(intervalFromComponents(1, 2, 3_000_000_000n))
 * // → "INTERVAL '1 months 2 days 3000000000 nanoseconds'"
 * ```
 */
export function intervalLiteral(iv: Interval): string {
  return `INTERVAL '${iv.months} months ${iv.days} days ${iv.nanos.toString()} nanoseconds'`;
}

// ============================================================================
// SubstringRange — mirrors `SubstringRange` at
// `crates/kimberlite-types/src/domain.rs:1033`.
// ============================================================================

/**
 * `SUBSTRING(s FROM start [FOR length])` operand range.
 *
 * `start` is 1-based; values ≤ 0 shift the slice left (Postgres
 * semantics). `length` MUST be non-negative when set; `null` means
 * "to end of string". Construction via {@link substringFromStart}
 * or {@link substringWithLength} enforces the non-negative invariant.
 */
export interface SubstringRange {
  /** 1-based starting position. May be ≤ 0. */
  start: bigint;
  /** Inclusive length, or `null` for "to end". Always non-negative. */
  length: bigint | null;
}

/** Two-argument form: `SUBSTRING(s FROM start)`. */
export function substringFromStart(start: bigint): SubstringRange {
  return { start, length: null };
}

/**
 * Three-argument form: `SUBSTRING(s FROM start FOR length)`.
 *
 * @throws {RangeError} if `length` is negative — Postgres rejects
 *   negative lengths at parse time, mirrored here as a typed
 *   precondition (Parse, Don't Validate).
 */
export function substringWithLength(start: bigint, length: bigint): SubstringRange {
  if (length < 0n) {
    throw new RangeError(`SUBSTRING length must be non-negative, got ${length}`);
  }
  return { start, length };
}

/**
 * Build a `SUBSTRING(expr FROM start [FOR length])` SQL fragment.
 *
 * `expr` is spliced verbatim — pass a column name or other trusted
 * expression. The `start` and `length` are emitted as SQL integer
 * literals (safe by construction since they're typed `bigint`).
 */
export function substringSql(expr: string, range: SubstringRange): string {
  if (range.length === null) {
    return `SUBSTRING(${expr} FROM ${range.start.toString()})`;
  }
  return `SUBSTRING(${expr} FROM ${range.start.toString()} FOR ${range.length.toString()})`;
}

// ============================================================================
// AggregateMemoryBudget — mirrors `AggregateMemoryBudget` at
// `crates/kimberlite-types/src/domain.rs:846`.
// ============================================================================

/** Minimum aggregate budget. Below this the per-group overhead dominates. */
export const AGGREGATE_BUDGET_MIN_BYTES = 65_536n;

/** Default aggregate budget — 256 MiB, ≈ 1M groups. */
export const AGGREGATE_BUDGET_DEFAULT_BYTES = 256n * 1024n * 1024n;

/**
 * Aggregate-executor memory budget. Exposed as a typed shape so
 * callers can pass it through forthcoming server-config plumbing
 * without re-implementing the floor check. Server-side enforcement
 * lives at `crates/kimberlite-query/src/executor.rs`.
 */
export interface AggregateMemoryBudget {
  /** Memory budget in bytes. Must be ≥ {@link AGGREGATE_BUDGET_MIN_BYTES}. */
  bytes: bigint;
}

/** Thrown by {@link aggregateMemoryBudget} when below the floor. */
export class AggregateMemoryBudgetTooSmallError extends Error {
  constructor(bytes: bigint) {
    super(
      `AggregateMemoryBudget too small: ${bytes} bytes < ${AGGREGATE_BUDGET_MIN_BYTES} (minimum)`,
    );
    this.name = 'AggregateMemoryBudgetTooSmallError';
    Object.setPrototypeOf(this, AggregateMemoryBudgetTooSmallError.prototype);
  }
}

/** Construct an {@link AggregateMemoryBudget}, enforcing the floor. */
export function aggregateMemoryBudget(bytes: bigint): AggregateMemoryBudget {
  if (bytes < AGGREGATE_BUDGET_MIN_BYTES) {
    throw new AggregateMemoryBudgetTooSmallError(bytes);
  }
  return { bytes };
}
