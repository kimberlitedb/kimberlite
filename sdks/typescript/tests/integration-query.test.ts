/**
 * Integration tests for query functionality against live kmb-server.
 *
 * These tests require a running kmb-server instance.
 * Run with: npm test -- integration-query.test.ts
 */

import { describe, expect, test, beforeEach, afterEach } from '@jest/globals';
import { Client, ValueBuilder, ValueType } from '../src';

describe('Query Integration Tests (require running server)', () => {
  let client: Client | null = null;

  beforeEach(async () => {
    try {
      client = await Client.connect({
        addresses: ['localhost:5432'],
        tenantId: 1n,
        authToken: 'test-token',
      });
    } catch (e) {
      // Skip tests if server not available
      console.log('Skipping: Server not available');
      client = null;
    }
  });

  afterEach(async () => {
    if (client) {
      try {
        await client.disconnect();
      } catch (e) {
        // Ignore cleanup errors
      }
      client = null;
    }
  });

  describe('Basic Queries', () => {
    test('should create table', async () => {
      if (!client) return;

      await client.execute(`
        CREATE TABLE IF NOT EXISTS test_users (
          id BIGINT PRIMARY KEY,
          name TEXT,
          active BOOLEAN,
          created_at TIMESTAMP
        )
      `);

      // Should not throw
      expect(true).toBe(true);
    });

    test('should insert single row', async () => {
      if (!client) return;

      await client.execute(`
        CREATE TABLE IF NOT EXISTS users_insert (
          id BIGINT PRIMARY KEY,
          name TEXT
        )
      `);

      const rowsAffected = await client.execute(
        'INSERT INTO users_insert (id, name) VALUES ($1, $2)',
        [ValueBuilder.bigint(1), ValueBuilder.text('Alice')]
      );

      expect(rowsAffected).toBeGreaterThanOrEqual(0);
    });

    test('should select all rows', async () => {
      if (!client) return;

      await client.execute(`
        CREATE TABLE IF NOT EXISTS users_select (
          id BIGINT PRIMARY KEY,
          name TEXT
        )
      `);

      await client.execute(
        'INSERT INTO users_select (id, name) VALUES ($1, $2)',
        [ValueBuilder.bigint(1), ValueBuilder.text('Alice')]
      );

      const result = await client.query('SELECT * FROM users_select');

      expect(result.columns.length).toBeGreaterThan(0);
      expect(result.columns).toContain('id');
      expect(result.columns).toContain('name');
      expect(result.rows.length).toBeGreaterThanOrEqual(1);
    });

    test('should select with WHERE clause', async () => {
      if (!client) return;

      await client.execute(`
        CREATE TABLE IF NOT EXISTS users_where (
          id BIGINT PRIMARY KEY,
          name TEXT
        )
      `);

      await client.execute(
        'INSERT INTO users_where (id, name) VALUES ($1, $2)',
        [ValueBuilder.bigint(1), ValueBuilder.text('Alice')]
      );
      await client.execute(
        'INSERT INTO users_where (id, name) VALUES ($1, $2)',
        [ValueBuilder.bigint(2), ValueBuilder.text('Bob')]
      );

      const result = await client.query(
        'SELECT * FROM users_where WHERE id = $1',
        [ValueBuilder.bigint(1)]
      );

      expect(result.rows.length).toBe(1);
      const nameIdx = result.columns.indexOf('name');
      expect(result.rows[0][nameIdx].type).toBe(ValueType.Text);
      if (result.rows[0][nameIdx].type === ValueType.Text) {
        expect((result.rows[0][nameIdx] as any).value).toBe('Alice');
      }
    });
  });

  describe('Value Types', () => {
    beforeEach(async () => {
      if (!client) return;

      await client.execute(`
        CREATE TABLE IF NOT EXISTS test_values (
          id BIGINT PRIMARY KEY,
          name TEXT,
          active BOOLEAN,
          created_at TIMESTAMP
        )
      `);
    });

    test('should handle NULL values', async () => {
      if (!client) return;

      await client.execute(
        'INSERT INTO test_values (id, name, active, created_at) VALUES ($1, $2, $3, $4)',
        [
          ValueBuilder.bigint(1),
          ValueBuilder.null(),
          ValueBuilder.boolean(true),
          ValueBuilder.timestamp(1000n),
        ]
      );

      const result = await client.query(
        'SELECT name FROM test_values WHERE id = $1',
        [ValueBuilder.bigint(1)]
      );

      expect(result.rows.length).toBe(1);
      expect(result.rows[0][0].type).toBe(ValueType.Null);
    });

    test('should handle BIGINT values', async () => {
      if (!client) return;

      const largeNum = 9007199254740991n; // Max safe integer
      await client.execute(
        'INSERT INTO test_values (id, name, active, created_at) VALUES ($1, $2, $3, $4)',
        [
          ValueBuilder.bigint(largeNum),
          ValueBuilder.text('Test'),
          ValueBuilder.boolean(true),
          ValueBuilder.timestamp(1000n),
        ]
      );

      const result = await client.query(
        'SELECT id FROM test_values WHERE id = $1',
        [ValueBuilder.bigint(largeNum)]
      );

      expect(result.rows.length).toBe(1);
      expect(result.rows[0][0].type).toBe(ValueType.BigInt);
      if (result.rows[0][0].type === ValueType.BigInt) {
        expect((result.rows[0][0] as any).value).toBe(largeNum);
      }
    });

    test('should handle TEXT values with unicode', async () => {
      if (!client) return;

      const unicodeText = 'Hello, 世界! 🌍';
      await client.execute(
        'INSERT INTO test_values (id, name, active, created_at) VALUES ($1, $2, $3, $4)',
        [
          ValueBuilder.bigint(1),
          ValueBuilder.text(unicodeText),
          ValueBuilder.boolean(true),
          ValueBuilder.timestamp(1000n),
        ]
      );

      const result = await client.query(
        'SELECT name FROM test_values WHERE id = $1',
        [ValueBuilder.bigint(1)]
      );

      expect(result.rows[0][0].type).toBe(ValueType.Text);
      if (result.rows[0][0].type === ValueType.Text) {
        expect((result.rows[0][0] as any).value).toBe(unicodeText);
      }
    });

    test('should handle BOOLEAN values', async () => {
      if (!client) return;

      await client.execute(
        'INSERT INTO test_values (id, name, active, created_at) VALUES ($1, $2, $3, $4)',
        [
          ValueBuilder.bigint(1),
          ValueBuilder.text('Test'),
          ValueBuilder.boolean(true),
          ValueBuilder.timestamp(1000n),
        ]
      );

      const result = await client.query(
        'SELECT active FROM test_values WHERE id = $1',
        [ValueBuilder.bigint(1)]
      );

      expect(result.rows[0][0].type).toBe(ValueType.Boolean);
      if (result.rows[0][0].type === ValueType.Boolean) {
        expect((result.rows[0][0] as any).value).toBe(true);
      }
    });

    test('should handle TIMESTAMP values', async () => {
      if (!client) return;

      const timestampNanos = 1609459200_000_000_000n; // 2021-01-01 00:00:00 UTC
      await client.execute(
        'INSERT INTO test_values (id, name, active, created_at) VALUES ($1, $2, $3, $4)',
        [
          ValueBuilder.bigint(1),
          ValueBuilder.text('Test'),
          ValueBuilder.boolean(true),
          ValueBuilder.timestamp(timestampNanos),
        ]
      );

      const result = await client.query(
        'SELECT created_at FROM test_values WHERE id = $1',
        [ValueBuilder.bigint(1)]
      );

      expect(result.rows[0][0].type).toBe(ValueType.Timestamp);
    });

    test('should handle timestamp from Date', async () => {
      if (!client) return;

      const date = new Date('2024-01-01T12:00:00Z');
      await client.execute(
        'INSERT INTO test_values (id, name, active, created_at) VALUES ($1, $2, $3, $4)',
        [
          ValueBuilder.bigint(1),
          ValueBuilder.text('Test'),
          ValueBuilder.boolean(true),
          ValueBuilder.fromDate(date),
        ]
      );

      const result = await client.query(
        'SELECT created_at FROM test_values WHERE id = $1',
        [ValueBuilder.bigint(1)]
      );

      expect(result.rows[0][0].type).toBe(ValueType.Timestamp);
    });
  });

  describe('Parameterized Queries', () => {
    beforeEach(async () => {
      if (!client) return;

      await client.execute(`
        CREATE TABLE IF NOT EXISTS test_params (
          id BIGINT PRIMARY KEY,
          name TEXT,
          active BOOLEAN
        )
      `);
    });

    test('should handle multiple parameters', async () => {
      if (!client) return;

      await client.execute(
        'INSERT INTO test_params (id, name, active) VALUES ($1, $2, $3)',
        [ValueBuilder.bigint(1), ValueBuilder.text('Alice'), ValueBuilder.boolean(true)]
      );
      await client.execute(
        'INSERT INTO test_params (id, name, active) VALUES ($1, $2, $3)',
        [ValueBuilder.bigint(2), ValueBuilder.text('Bob'), ValueBuilder.boolean(true)]
      );

      const result = await client.query(
        'SELECT * FROM test_params WHERE active = $1 AND id > $2',
        [ValueBuilder.boolean(true), ValueBuilder.bigint(0)]
      );

      expect(result.rows.length).toBeGreaterThanOrEqual(2);
    });

    test('should handle query with no parameters', async () => {
      if (!client) return;

      const result = await client.query('SELECT * FROM test_params');

      expect(result).toBeDefined();
      expect(Array.isArray(result.rows)).toBe(true);
    });
  });

  describe('DML Operations', () => {
    beforeEach(async () => {
      if (!client) return;

      await client.execute(`
        CREATE TABLE IF NOT EXISTS test_dml (
          id BIGINT PRIMARY KEY,
          name TEXT
        )
      `);
    });

    test('should UPDATE records', async () => {
      if (!client) return;

      await client.execute(
        'INSERT INTO test_dml (id, name) VALUES ($1, $2)',
        [ValueBuilder.bigint(1), ValueBuilder.text('Alice')]
      );

      await client.execute('UPDATE test_dml SET name = $1 WHERE id = $2', [
        ValueBuilder.text('Alice Updated'),
        ValueBuilder.bigint(1),
      ]);

      const result = await client.query(
        'SELECT name FROM test_dml WHERE id = $1',
        [ValueBuilder.bigint(1)]
      );

      if (result.rows[0][0].type === ValueType.Text) {
        expect((result.rows[0][0] as any).value).toBe('Alice Updated');
      }
    });

    test('should DELETE records', async () => {
      if (!client) return;

      await client.execute(
        'INSERT INTO test_dml (id, name) VALUES ($1, $2)',
        [ValueBuilder.bigint(1), ValueBuilder.text('Alice')]
      );

      await client.execute('DELETE FROM test_dml WHERE id = $1', [
        ValueBuilder.bigint(1),
      ]);

      const result = await client.query(
        'SELECT * FROM test_dml WHERE id = $1',
        [ValueBuilder.bigint(1)]
      );

      expect(result.rows.length).toBe(0);
    });
  });

  describe('Error Handling', () => {
    test('should handle SQL syntax errors', async () => {
      if (!client) return;

      await expect(
        client.query('INVALID SQL SYNTAX')
      ).rejects.toThrow();
    });

    test('should handle non-existent table', async () => {
      if (!client) return;

      await expect(
        client.query('SELECT * FROM nonexistent_table_xyz')
      ).rejects.toThrow();
    });
  });

  describe('Empty Results', () => {
    test('should handle empty result set', async () => {
      if (!client) return;

      await client.execute(`
        CREATE TABLE IF NOT EXISTS test_empty (
          id BIGINT PRIMARY KEY
        )
      `);

      const result = await client.query(
        'SELECT * FROM test_empty WHERE id = $1',
        [ValueBuilder.bigint(99999)]
      );

      expect(result.rows.length).toBe(0);
      expect(result.columns.length).toBeGreaterThan(0);
    });
  });

  describe('Large Result Sets', () => {
    test('should handle multiple rows', async () => {
      if (!client) return;

      await client.execute(`
        CREATE TABLE IF NOT EXISTS test_large (
          id BIGINT PRIMARY KEY,
          name TEXT
        )
      `);

      // Insert multiple rows
      for (let i = 0; i < 10; i++) {
        await client.execute(
          'INSERT INTO test_large (id, name) VALUES ($1, $2)',
          [ValueBuilder.bigint(i), ValueBuilder.text(`User${i}`)]
        );
      }

      const result = await client.query('SELECT * FROM test_large');
      expect(result.rows.length).toBeGreaterThanOrEqual(10);
    });
  });

  describe('Point-in-Time Queries', () => {
    test('should execute queryAt without errors', async () => {
      if (!client) return;

      await client.execute(`
        CREATE TABLE IF NOT EXISTS test_pit (
          id BIGINT PRIMARY KEY,
          name TEXT
        )
      `);

      await client.execute(
        'INSERT INTO test_pit (id, name) VALUES ($1, $2)',
        [ValueBuilder.bigint(1), ValueBuilder.text('Alice')]
      );

      // Query at position 0 (beginning)
      const result = await client.queryAt('SELECT * FROM test_pit', [], 0n);

      expect(result).toBeDefined();
      expect(Array.isArray(result.rows)).toBe(true);
    });
  });
});
