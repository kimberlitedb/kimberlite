/**
 * SQL value types for Kimberlite queries.
 *
 * This module provides type-safe representations of SQL values that can be used
 * as query parameters and returned from query results.
 */

/**
 * Type tag for SQL values (matches FFI enum).
 */
export enum ValueType {
  Null = 0,
  BigInt = 1,
  Text = 2,
  Boolean = 3,
  Timestamp = 4,
}

/**
 * SQL value discriminated union.
 *
 * A Value represents a typed SQL value that can be used as a query parameter
 * or returned from a query result. The type system ensures type safety at compile time.
 */
export type Value =
  | { type: ValueType.Null }
  | { type: ValueType.BigInt; value: bigint }
  | { type: ValueType.Text; value: string }
  | { type: ValueType.Boolean; value: boolean }
  | { type: ValueType.Timestamp; value: bigint };

/**
 * Value builder with static factory methods.
 *
 * Provides convenient constructors for creating Value objects.
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
  /**
   * Create a NULL value.
   *
   * @returns A Value representing SQL NULL
   */
  static null(): Value {
    return { type: ValueType.Null };
  }

  /**
   * Create a BIGINT value from a number or bigint.
   *
   * @param value - Integer value
   * @returns A Value containing the integer
   *
   * @example
   * ```typescript
   * ValueBuilder.bigint(42)
   * ValueBuilder.bigint(9007199254740991n) // Use bigint for large values
   * ```
   */
  static bigint(value: bigint | number): Value {
    return { type: ValueType.BigInt, value: BigInt(value) };
  }

  /**
   * Create a TEXT value from a string.
   *
   * @param value - UTF-8 string value
   * @returns A Value containing the string
   *
   * @example
   * ```typescript
   * ValueBuilder.text("hello")
   * ValueBuilder.text("Hello, 世界! 🌍")
   * ```
   */
  static text(value: string): Value {
    if (typeof value !== 'string') {
      throw new TypeError(`Expected string, got ${typeof value}`);
    }
    return { type: ValueType.Text, value };
  }

  /**
   * Create a BOOLEAN value from a boolean.
   *
   * @param value - Boolean value
   * @returns A Value containing the boolean
   *
   * @example
   * ```typescript
   * ValueBuilder.boolean(true)
   * ValueBuilder.boolean(false)
   * ```
   */
  static boolean(value: boolean): Value {
    if (typeof value !== 'boolean') {
      throw new TypeError(`Expected boolean, got ${typeof value}`);
    }
    return { type: ValueType.Boolean, value };
  }

  /**
   * Create a TIMESTAMP value from nanoseconds since Unix epoch.
   *
   * @param nanos - Nanoseconds since Unix epoch (1970-01-01 00:00:00 UTC)
   * @returns A Value containing the timestamp
   *
   * @example
   * ```typescript
   * ValueBuilder.timestamp(1609459200_000_000_000n) // 2021-01-01 00:00:00 UTC
   * ```
   */
  static timestamp(nanos: bigint): Value {
    return { type: ValueType.Timestamp, value: nanos };
  }

  /**
   * Create a TIMESTAMP value from a JavaScript Date.
   *
   * @param date - JavaScript Date object
   * @returns A Value containing the timestamp
   *
   * @example
   * ```typescript
   * ValueBuilder.fromDate(new Date('2024-01-01T12:00:00Z'))
   * ValueBuilder.fromDate(new Date())
   * ```
   */
  static fromDate(date: Date): Value {
    if (!(date instanceof Date)) {
      throw new TypeError(`Expected Date, got ${typeof date}`);
    }
    // Convert milliseconds to nanoseconds
    const nanos = BigInt(Math.floor(date.getTime())) * 1_000_000n;
    return { type: ValueType.Timestamp, value: nanos };
  }
}

/**
 * Convert a TIMESTAMP value to a JavaScript Date.
 *
 * @param val - Value to convert
 * @returns A Date object in UTC, or null if value is not a TIMESTAMP
 *
 * @example
 * ```typescript
 * const val = ValueBuilder.timestamp(1609459200_000_000_000n);
 * const date = valueToDate(val);
 * console.log(date?.toISOString()); // "2021-01-01T00:00:00.000Z"
 * ```
 */
export function valueToDate(val: Value): Date | null {
  if (val.type === ValueType.Timestamp) {
    // Convert nanoseconds to milliseconds
    const millis = Number(val.value / 1_000_000n);
    return new Date(millis);
  }
  return null;
}

/**
 * Check if a value is NULL.
 *
 * @param val - Value to check
 * @returns True if the value is NULL
 *
 * @example
 * ```typescript
 * isNull(ValueBuilder.null()) // true
 * isNull(ValueBuilder.bigint(42)) // false
 * ```
 */
export function isNull(val: Value): val is { type: ValueType.Null } {
  return val.type === ValueType.Null;
}

/**
 * Type guard to check if a value is a BIGINT.
 *
 * @param val - Value to check
 * @returns True if the value is a BIGINT
 */
export function isBigInt(val: Value): val is { type: ValueType.BigInt; value: bigint } {
  return val.type === ValueType.BigInt;
}

/**
 * Type guard to check if a value is TEXT.
 *
 * @param val - Value to check
 * @returns True if the value is TEXT
 */
export function isText(val: Value): val is { type: ValueType.Text; value: string } {
  return val.type === ValueType.Text;
}

/**
 * Type guard to check if a value is BOOLEAN.
 *
 * @param val - Value to check
 * @returns True if the value is BOOLEAN
 */
export function isBoolean(val: Value): val is { type: ValueType.Boolean; value: boolean } {
  return val.type === ValueType.Boolean;
}

/**
 * Type guard to check if a value is TIMESTAMP.
 *
 * @param val - Value to check
 * @returns True if the value is TIMESTAMP
 */
export function isTimestamp(val: Value): val is { type: ValueType.Timestamp; value: bigint } {
  return val.type === ValueType.Timestamp;
}

/**
 * Get a string representation of a Value.
 *
 * @param val - Value to convert
 * @returns String representation
 *
 * @example
 * ```typescript
 * valueToString(ValueBuilder.null()) // "NULL"
 * valueToString(ValueBuilder.bigint(42)) // "42"
 * valueToString(ValueBuilder.text("hello")) // "hello"
 * ```
 */
export function valueToString(val: Value): string {
  switch (val.type) {
    case ValueType.Null:
      return 'NULL';
    case ValueType.BigInt:
      return val.value.toString();
    case ValueType.Text:
      return val.value;
    case ValueType.Boolean:
      return val.value.toString();
    case ValueType.Timestamp:
      return val.value.toString();
  }
}

/**
 * Compare two Values for equality.
 *
 * @param a - First value
 * @param b - Second value
 * @returns True if values are equal
 *
 * @example
 * ```typescript
 * valueEquals(ValueBuilder.bigint(42), ValueBuilder.bigint(42)) // true
 * valueEquals(ValueBuilder.null(), ValueBuilder.null()) // true
 * valueEquals(ValueBuilder.bigint(42), ValueBuilder.text("42")) // false
 * ```
 */
export function valueEquals(a: Value, b: Value): boolean {
  if (a.type !== b.type) {
    return false;
  }

  if (a.type === ValueType.Null) {
    return true; // All NULLs are equal
  }

  // TypeScript knows these must have .value now
  return (a as any).value === (b as any).value;
}
