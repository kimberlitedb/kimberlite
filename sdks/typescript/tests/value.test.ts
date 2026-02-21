/**
 * Tests for value types.
 */

import { describe, expect, test } from '@jest/globals';
import {
  ValueType,
  ValueBuilder,
  valueToDate,
  valueToString,
  valueEquals,
  isNull,
  isBigInt,
  isText,
  isBoolean,
  isTimestamp,
} from '../src/value';

describe('ValueBuilder', () => {
  describe('null', () => {
    test('creates null value', () => {
      const val = ValueBuilder.null();
      expect(val.type).toBe(ValueType.Null);
      expect(isNull(val)).toBe(true);
    });
  });

  describe('bigint', () => {
    test('creates bigint from number', () => {
      const val = ValueBuilder.bigint(42);
      expect(val.type).toBe(ValueType.BigInt);
      if (isBigInt(val)) {
        expect(val.value).toBe(42n);
      }
    });

    test('creates bigint from bigint', () => {
      const val = ValueBuilder.bigint(9007199254740991n);
      expect(val.type).toBe(ValueType.BigInt);
      if (isBigInt(val)) {
        expect(val.value).toBe(9007199254740991n);
      }
    });

    test('handles negative numbers', () => {
      const val = ValueBuilder.bigint(-100);
      if (isBigInt(val)) {
        expect(val.value).toBe(-100n);
      }
    });

    test('handles zero', () => {
      const val = ValueBuilder.bigint(0);
      if (isBigInt(val)) {
        expect(val.value).toBe(0n);
      }
    });
  });

  describe('text', () => {
    test('creates text value', () => {
      const val = ValueBuilder.text('hello');
      expect(val.type).toBe(ValueType.Text);
      if (isText(val)) {
        expect(val.value).toBe('hello');
      }
    });

    test('handles empty string', () => {
      const val = ValueBuilder.text('');
      if (isText(val)) {
        expect(val.value).toBe('');
      }
    });

    test('handles unicode', () => {
      const val = ValueBuilder.text('Hello, 世界! 🌍');
      if (isText(val)) {
        expect(val.value).toBe('Hello, 世界! 🌍');
      }
    });

    test('throws on non-string', () => {
      expect(() => ValueBuilder.text(42 as any)).toThrow(TypeError);
    });
  });

  describe('boolean', () => {
    test('creates boolean true', () => {
      const val = ValueBuilder.boolean(true);
      expect(val.type).toBe(ValueType.Boolean);
      if (isBoolean(val)) {
        expect(val.value).toBe(true);
      }
    });

    test('creates boolean false', () => {
      const val = ValueBuilder.boolean(false);
      if (isBoolean(val)) {
        expect(val.value).toBe(false);
      }
    });

    test('throws on non-boolean', () => {
      expect(() => ValueBuilder.boolean(1 as any)).toThrow(TypeError);
    });
  });

  describe('timestamp', () => {
    test('creates timestamp', () => {
      const val = ValueBuilder.timestamp(1234567890n);
      expect(val.type).toBe(ValueType.Timestamp);
      if (isTimestamp(val)) {
        expect(val.value).toBe(1234567890n);
      }
    });

    test('handles zero timestamp', () => {
      const val = ValueBuilder.timestamp(0n);
      if (isTimestamp(val)) {
        expect(val.value).toBe(0n);
      }
    });

    test('handles negative timestamp', () => {
      const val = ValueBuilder.timestamp(-1000n);
      if (isTimestamp(val)) {
        expect(val.value).toBe(-1000n);
      }
    });
  });

  describe('fromDate', () => {
    test('creates timestamp from date', () => {
      const date = new Date('2024-01-01T12:00:00Z');
      const val = ValueBuilder.fromDate(date);
      expect(val.type).toBe(ValueType.Timestamp);
      expect(isTimestamp(val)).toBe(true);
    });

    test('throws on non-date', () => {
      expect(() => ValueBuilder.fromDate('2024-01-01' as any)).toThrow(TypeError);
    });
  });
});

describe('valueToDate', () => {
  test('converts timestamp to date', () => {
    // 2021-01-01 00:00:00 UTC in nanoseconds
    const nanos = 1609459200_000_000_000n;
    const val = ValueBuilder.timestamp(nanos);
    const date = valueToDate(val);

    expect(date).toBeInstanceOf(Date);
    expect(date?.getUTCFullYear()).toBe(2021);
    expect(date?.getUTCMonth()).toBe(0); // January (0-indexed)
    expect(date?.getUTCDate()).toBe(1);
  });

  test('returns null for non-timestamp', () => {
    expect(valueToDate(ValueBuilder.bigint(42))).toBeNull();
    expect(valueToDate(ValueBuilder.null())).toBeNull();
    expect(valueToDate(ValueBuilder.text('hello'))).toBeNull();
  });

  test('roundtrip conversion', () => {
    const originalDate = new Date('2024-06-15T14:30:45Z');
    const val = ValueBuilder.fromDate(originalDate);
    const reconstructedDate = valueToDate(val);

    expect(reconstructedDate).toBeTruthy();
    // Allow small difference due to precision
    const diff = Math.abs(
      originalDate.getTime() - (reconstructedDate?.getTime() ?? 0)
    );
    expect(diff).toBeLessThan(1); // Less than 1ms difference
  });
});

describe('Type guards', () => {
  test('isNull', () => {
    expect(isNull(ValueBuilder.null())).toBe(true);
    expect(isNull(ValueBuilder.bigint(42))).toBe(false);
  });

  test('isBigInt', () => {
    expect(isBigInt(ValueBuilder.bigint(42))).toBe(true);
    expect(isBigInt(ValueBuilder.null())).toBe(false);
  });

  test('isText', () => {
    expect(isText(ValueBuilder.text('hello'))).toBe(true);
    expect(isText(ValueBuilder.null())).toBe(false);
  });

  test('isBoolean', () => {
    expect(isBoolean(ValueBuilder.boolean(true))).toBe(true);
    expect(isBoolean(ValueBuilder.null())).toBe(false);
  });

  test('isTimestamp', () => {
    expect(isTimestamp(ValueBuilder.timestamp(1234n))).toBe(true);
    expect(isTimestamp(ValueBuilder.null())).toBe(false);
  });
});

describe('valueToString', () => {
  test('null to string', () => {
    expect(valueToString(ValueBuilder.null())).toBe('NULL');
  });

  test('bigint to string', () => {
    expect(valueToString(ValueBuilder.bigint(42))).toBe('42');
  });

  test('text to string', () => {
    expect(valueToString(ValueBuilder.text('hello'))).toBe('hello');
  });

  test('boolean to string', () => {
    expect(valueToString(ValueBuilder.boolean(true))).toBe('true');
    expect(valueToString(ValueBuilder.boolean(false))).toBe('false');
  });

  test('timestamp to string', () => {
    expect(valueToString(ValueBuilder.timestamp(1234567890n))).toBe(
      '1234567890'
    );
  });
});

describe('valueEquals', () => {
  test('null equality', () => {
    expect(valueEquals(ValueBuilder.null(), ValueBuilder.null())).toBe(true);
  });

  test('bigint equality', () => {
    expect(valueEquals(ValueBuilder.bigint(42), ValueBuilder.bigint(42))).toBe(
      true
    );
    expect(valueEquals(ValueBuilder.bigint(42), ValueBuilder.bigint(43))).toBe(
      false
    );
  });

  test('text equality', () => {
    expect(
      valueEquals(ValueBuilder.text('hello'), ValueBuilder.text('hello'))
    ).toBe(true);
    expect(
      valueEquals(ValueBuilder.text('hello'), ValueBuilder.text('world'))
    ).toBe(false);
  });

  test('boolean equality', () => {
    expect(
      valueEquals(ValueBuilder.boolean(true), ValueBuilder.boolean(true))
    ).toBe(true);
    expect(
      valueEquals(ValueBuilder.boolean(true), ValueBuilder.boolean(false))
    ).toBe(false);
  });

  test('timestamp equality', () => {
    expect(
      valueEquals(ValueBuilder.timestamp(1234n), ValueBuilder.timestamp(1234n))
    ).toBe(true);
    expect(
      valueEquals(ValueBuilder.timestamp(1234n), ValueBuilder.timestamp(5678n))
    ).toBe(false);
  });

  test('different types not equal', () => {
    expect(valueEquals(ValueBuilder.bigint(42), ValueBuilder.text('42'))).toBe(
      false
    );
    expect(valueEquals(ValueBuilder.null(), ValueBuilder.bigint(0))).toBe(
      false
    );
  });
});
