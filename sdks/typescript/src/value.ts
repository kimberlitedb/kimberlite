/**
 * SQL value types for Kimberlite queries.
 *
 * AUDIT-2026-04 S4.5 — the discriminant is `kind` (string literal)
 * to match the rest of the ecosystem (`neverthrow`, Effect, and the
 * SDK's own `DomainError.kind`) and to keep `switch` exhaustiveness
 * readable (`'bigint'` instead of the opaque `1`). The legacy
 * `type: ValueType` shape is aliased for one release so 0.4.x
 * consumers can upgrade without code changes and is scheduled for
 * removal in 0.6.0.
 */

/**
 * Legacy numeric enum. Retained for FFI-boundary conversions and for
 * backward compatibility with 0.4.x call sites. New code should use
 * the string-literal `kind` discriminant directly.
 *
 * @deprecated Prefer `Value.kind` string literals.
 */
export enum ValueType {
  Null = 0,
  BigInt = 1,
  Text = 2,
  Boolean = 3,
  Timestamp = 4,
}

/**
 * Canonical tagged union — `kind` is a string literal so
 * `switch (v.kind) { case 'bigint': ... }` reads cleanly and logs
 * sensibly.
 */
export type Value =
  | { readonly kind: 'null'; readonly type: ValueType.Null }
  | { readonly kind: 'bigint'; readonly type: ValueType.BigInt; readonly value: bigint }
  | { readonly kind: 'text'; readonly type: ValueType.Text; readonly value: string }
  | { readonly kind: 'boolean'; readonly type: ValueType.Boolean; readonly value: boolean }
  | { readonly kind: 'timestamp'; readonly type: ValueType.Timestamp; readonly value: bigint };

/**
 * Value builder with static factory methods.
 *
 * @example
 * ```typescript
 * import { ValueBuilder } from '@kimberlite/client';
 *
 * const nullValue = ValueBuilder.null();
 * const intValue = ValueBuilder.bigint(42);
 * const textValue = ValueBuilder.text("hello");
 * const boolValue = ValueBuilder.boolean(true);
 * const timestampValue = ValueBuilder.timestamp(1234567890n);
 * const dateValue = ValueBuilder.fromDate(new Date());
 * ```
 */
export class ValueBuilder {
  static null(): Value {
    return { kind: 'null', type: ValueType.Null };
  }

  static bigint(value: bigint | number): Value {
    return { kind: 'bigint', type: ValueType.BigInt, value: BigInt(value) };
  }

  static text(value: string): Value {
    if (typeof value !== 'string') {
      throw new TypeError(`Expected string, got ${typeof value}`);
    }
    return { kind: 'text', type: ValueType.Text, value };
  }

  static boolean(value: boolean): Value {
    if (typeof value !== 'boolean') {
      throw new TypeError(`Expected boolean, got ${typeof value}`);
    }
    return { kind: 'boolean', type: ValueType.Boolean, value };
  }

  static timestamp(nanos: bigint): Value {
    return { kind: 'timestamp', type: ValueType.Timestamp, value: nanos };
  }

  static fromDate(date: Date): Value {
    if (!(date instanceof Date)) {
      throw new TypeError(`Expected Date, got ${typeof date}`);
    }
    const nanos = BigInt(Math.floor(date.getTime())) * 1_000_000n;
    return { kind: 'timestamp', type: ValueType.Timestamp, value: nanos };
  }
}

/**
 * Convert a TIMESTAMP value to a JavaScript Date.
 *
 * @returns A Date object in UTC, or null if value is not a TIMESTAMP
 */
export function valueToDate(val: Value): Date | null {
  if (val.kind === 'timestamp') {
    const millis = Number(val.value / 1_000_000n);
    return new Date(millis);
  }
  return null;
}

export function isNull(val: Value): val is Extract<Value, { kind: 'null' }> {
  return val.kind === 'null';
}

export function isBigInt(val: Value): val is Extract<Value, { kind: 'bigint' }> {
  return val.kind === 'bigint';
}

export function isText(val: Value): val is Extract<Value, { kind: 'text' }> {
  return val.kind === 'text';
}

export function isBoolean(val: Value): val is Extract<Value, { kind: 'boolean' }> {
  return val.kind === 'boolean';
}

export function isTimestamp(val: Value): val is Extract<Value, { kind: 'timestamp' }> {
  return val.kind === 'timestamp';
}

/**
 * Get a string representation of a Value.
 */
export function valueToString(val: Value): string {
  switch (val.kind) {
    case 'null':
      return 'NULL';
    case 'bigint':
      return val.value.toString();
    case 'text':
      return val.value;
    case 'boolean':
      return val.value.toString();
    case 'timestamp':
      return val.value.toString();
  }
}

/**
 * Compare two Values for equality.
 */
export function valueEquals(a: Value, b: Value): boolean {
  if (a.kind !== b.kind) {
    return false;
  }
  if (a.kind === 'null') {
    return true;
  }
  // After the `kind` check, TS narrows both `a` and `b` to the same
  // branch — but not quite enough to merge their `value` types, so we
  // help with a targeted cast.
  const av = (a as Extract<Value, { value: unknown }>).value;
  const bv = (b as Extract<Value, { value: unknown }>).value;
  return av === bv;
}
