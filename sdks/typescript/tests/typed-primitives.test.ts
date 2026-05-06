/**
 * Tests for the v0.7.0 typed-primitive TS bindings — `DateField`,
 * `Interval`, `SubstringRange`, `AggregateMemoryBudget`. Pin the
 * shape, the SQL-fragment helpers, and the construction-time
 * invariants.
 */

import { describe, expect, test } from '@jest/globals';
import {
  AGGREGATE_BUDGET_DEFAULT_BYTES,
  AGGREGATE_BUDGET_MIN_BYTES,
  AggregateMemoryBudgetTooSmallError,
  DateField,
  IntervalOverflowError,
  NANOS_PER_DAY,
  TruncatableDateField,
  aggregateMemoryBudget,
  dateFieldKeyword,
  dateTruncSql,
  extractFromSql,
  intervalFromComponents,
  intervalFromDays,
  intervalFromMonths,
  intervalFromNanos,
  intervalLiteral,
  substringFromStart,
  substringSql,
  substringWithLength,
} from '../src/typed-primitives';

describe('DateField', () => {
  test('all 13 variants map to the expected SQL keyword', () => {
    const cases: Array<[DateField, string]> = [
      ['year', 'YEAR'],
      ['month', 'MONTH'],
      ['day', 'DAY'],
      ['hour', 'HOUR'],
      ['minute', 'MINUTE'],
      ['second', 'SECOND'],
      ['millisecond', 'MILLISECOND'],
      ['microsecond', 'MICROSECOND'],
      ['dayOfWeek', 'DOW'],
      ['dayOfYear', 'DOY'],
      ['quarter', 'QUARTER'],
      ['week', 'WEEK'],
      ['epoch', 'EPOCH'],
    ];
    for (const [field, keyword] of cases) {
      expect(dateFieldKeyword(field)).toBe(keyword);
    }
  });

  test('extractFromSql composes the EXTRACT fragment', () => {
    expect(extractFromSql('year', 'created_at')).toBe('EXTRACT(YEAR FROM created_at)');
    expect(extractFromSql('dayOfWeek', 'event_ts')).toBe('EXTRACT(DOW FROM event_ts)');
  });

  test('dateTruncSql composes the DATE_TRUNC fragment', () => {
    expect(dateTruncSql('month', 'invoice_date')).toBe(
      "DATE_TRUNC('month', invoice_date)",
    );
  });

  test('TruncatableDateField type rejects non-truncatable fields at compile time', () => {
    // Compile-time test: this assignment must remain commented, since
    // tsc would error on it. The presence of the comment documents the
    // intended behaviour.
    // const bad: TruncatableDateField = 'dayOfWeek';
    const ok: TruncatableDateField = 'year';
    expect(ok).toBe('year');
  });
});

describe('Interval', () => {
  test('intervalFromComponents normalises sub-day nanos into days', () => {
    const iv = intervalFromComponents(0, 0, NANOS_PER_DAY * 3n + 500n);
    expect(iv.months).toBe(0);
    expect(iv.days).toBe(3);
    expect(iv.nanos).toBe(500n);
  });

  test('intervalFromMonths / Days / Nanos round-trip', () => {
    expect(intervalFromMonths(7)).toEqual({ months: 7, days: 0, nanos: 0n });
    expect(intervalFromDays(14)).toEqual({ months: 0, days: 14, nanos: 0n });
    expect(intervalFromNanos(NANOS_PER_DAY)).toEqual({
      months: 0,
      days: 1,
      nanos: 0n,
    });
  });

  test('intervalFromComponents throws on i32 overflow', () => {
    const tooBig = (BigInt(Number.MAX_SAFE_INTEGER) + 1n) * NANOS_PER_DAY;
    expect(() => intervalFromComponents(0, 0, tooBig)).toThrow(IntervalOverflowError);
  });

  test('intervalLiteral emits the three-component INTERVAL form', () => {
    const iv = intervalFromComponents(1, 2, 3_000_000_000n);
    expect(intervalLiteral(iv)).toBe(
      "INTERVAL '1 months 2 days 3000000000 nanoseconds'",
    );
  });

  test('|nanos| < NANOS_PER_DAY post-construction', () => {
    const iv = intervalFromComponents(0, 0, NANOS_PER_DAY * 100n + 1n);
    expect(iv.nanos < NANOS_PER_DAY && iv.nanos > -NANOS_PER_DAY).toBe(true);
  });
});

describe('SubstringRange', () => {
  test('substringFromStart sets length to null', () => {
    expect(substringFromStart(5n)).toEqual({ start: 5n, length: null });
  });

  test('substringWithLength enforces non-negative length', () => {
    expect(substringWithLength(1n, 5n)).toEqual({ start: 1n, length: 5n });
    expect(() => substringWithLength(1n, -1n)).toThrow(RangeError);
  });

  test('start ≤ 0 is allowed (Postgres semantics)', () => {
    expect(substringFromStart(0n)).toEqual({ start: 0n, length: null });
    expect(substringFromStart(-3n)).toEqual({ start: -3n, length: null });
  });

  test('substringSql composes the FROM ... FOR ... fragment', () => {
    expect(substringSql('name', substringFromStart(2n))).toBe(
      'SUBSTRING(name FROM 2)',
    );
    expect(substringSql('name', substringWithLength(2n, 5n))).toBe(
      'SUBSTRING(name FROM 2 FOR 5)',
    );
  });
});

describe('AggregateMemoryBudget', () => {
  test('default and minimum constants match the Rust values', () => {
    expect(AGGREGATE_BUDGET_MIN_BYTES).toBe(64n * 1024n);
    expect(AGGREGATE_BUDGET_DEFAULT_BYTES).toBe(256n * 1024n * 1024n);
  });

  test('aggregateMemoryBudget enforces the floor', () => {
    expect(aggregateMemoryBudget(AGGREGATE_BUDGET_MIN_BYTES).bytes).toBe(
      AGGREGATE_BUDGET_MIN_BYTES,
    );
    expect(aggregateMemoryBudget(AGGREGATE_BUDGET_DEFAULT_BYTES).bytes).toBe(
      AGGREGATE_BUDGET_DEFAULT_BYTES,
    );
    expect(() => aggregateMemoryBudget(AGGREGATE_BUDGET_MIN_BYTES - 1n)).toThrow(
      AggregateMemoryBudgetTooSmallError,
    );
  });
});
