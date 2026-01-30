/**
 * Comprehensive query examples for Kimberlite TypeScript SDK.
 *
 * This example demonstrates:
 * - Table creation and management
 * - Parameterized queries with all value types
 * - CRUD operations
 * - Point-in-time queries for compliance
 * - Type-safe value handling
 * - Error handling
 */

import {
  Client,
  ValueBuilder,
  ValueType,
  isBigInt,
  isText,
  isNull,
  valueToDate,
} from '../src';

async function main() {
  console.log('='.repeat(60));
  console.log('Kimberlite TypeScript SDK - Query Examples');
  console.log('='.repeat(60));

  // Connect to Kimberlite
  const client = await Client.connect({
    addresses: ['localhost:5432'],
    tenantId: 1n,
    authToken: 'demo-token',
  });

  console.log('✓ Connected to Kimberlite');

  try {
    // ========================================================================
    // Example 1: Create Table (DDL)
    // ========================================================================
    console.log('\n=== Example 1: Create Table ===');

    await client.execute(`
      CREATE TABLE IF NOT EXISTS employees (
        id BIGINT PRIMARY KEY,
        name TEXT NOT NULL,
        email TEXT,
        salary BIGINT,
        is_active BOOLEAN,
        hired_at TIMESTAMP
      )
    `);
    console.log('✓ Created employees table');

    // ========================================================================
    // Example 2: Insert Data (Parameterized Queries)
    // ========================================================================
    console.log('\n=== Example 2: Insert Data ===');

    // Insert with all value types
    await client.execute(
      `
      INSERT INTO employees (id, name, email, salary, is_active, hired_at)
      VALUES ($1, $2, $3, $4, $5, $6)
      `,
      [
        ValueBuilder.bigint(1),
        ValueBuilder.text('Alice Johnson'),
        ValueBuilder.text('alice@example.com'),
        ValueBuilder.bigint(95000),
        ValueBuilder.boolean(true),
        ValueBuilder.fromDate(new Date('2020-01-15T09:00:00Z')),
      ]
    );
    console.log('✓ Inserted Alice Johnson');

    // Insert with NULL value
    await client.execute(
      `
      INSERT INTO employees (id, name, email, salary, is_active, hired_at)
      VALUES ($1, $2, $3, $4, $5, $6)
      `,
      [
        ValueBuilder.bigint(2),
        ValueBuilder.text('Bob Smith'),
        ValueBuilder.null(), // No email
        ValueBuilder.bigint(87000),
        ValueBuilder.boolean(true),
        ValueBuilder.timestamp(1610712000_000_000_000n),
      ]
    );
    console.log('✓ Inserted Bob Smith (with NULL email)');

    // Insert more employees
    const employees = [
      {
        id: 3,
        name: 'Carol Davis',
        email: 'carol@example.com',
        salary: 92000,
        active: false,
        hired: new Date('2019-05-01'),
      },
      {
        id: 4,
        name: 'David Lee',
        email: 'david@example.com',
        salary: 103000,
        active: true,
        hired: new Date('2021-03-10'),
      },
      {
        id: 5,
        name: 'Eve Martinez',
        email: 'eve@example.com',
        salary: 88000,
        active: true,
        hired: new Date('2022-07-20'),
      },
    ];

    for (const emp of employees) {
      await client.execute(
        `
        INSERT INTO employees (id, name, email, salary, is_active, hired_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        `,
        [
          ValueBuilder.bigint(emp.id),
          ValueBuilder.text(emp.name),
          ValueBuilder.text(emp.email),
          ValueBuilder.bigint(emp.salary),
          ValueBuilder.boolean(emp.active),
          ValueBuilder.fromDate(emp.hired),
        ]
      );
    }
    console.log(`✓ Inserted ${employees.length} more employees`);

    // ========================================================================
    // Example 3: Select Queries
    // ========================================================================
    console.log('\n=== Example 3: Select Queries ===');

    // Select all
    let result = await client.query('SELECT * FROM employees');
    console.log(`\nAll employees (${result.rows.length} total):`);
    console.log(`Columns: ${result.columns.join(', ')}`);
    for (const row of result.rows) {
      const idIdx = result.columns.indexOf('id');
      const nameIdx = result.columns.indexOf('name');
      if (isBigInt(row[idIdx]) && isText(row[nameIdx])) {
        console.log(`  - ID ${row[idIdx].value}: ${row[nameIdx].value}`);
      }
    }

    // Select with WHERE clause
    result = await client.query(
      'SELECT name, salary FROM employees WHERE is_active = $1',
      [ValueBuilder.boolean(true)]
    );
    console.log(`\nActive employees (${result.rows.length} found):`);
    for (const row of result.rows) {
      if (isText(row[0]) && isBigInt(row[1])) {
        const name = row[0].value;
        const salary = Number(row[1].value);
        console.log(`  - ${name}: $${salary.toLocaleString()}`);
      }
    }

    // Aggregate query
    result = await client.query('SELECT COUNT(*) as total FROM employees');
    if (result.rows.length > 0 && isBigInt(result.rows[0][0])) {
      const total = result.rows[0][0].value;
      console.log(`\nTotal employees: ${total}`);
    }

    // ========================================================================
    // Example 4: Update Data
    // ========================================================================
    console.log('\n=== Example 4: Update Data ===');

    // Update single row
    await client.execute(
      'UPDATE employees SET salary = $1 WHERE id = $2',
      [ValueBuilder.bigint(98000), ValueBuilder.bigint(1)]
    );
    console.log("✓ Updated Alice's salary to $98,000");

    // Verify update
    result = await client.query(
      'SELECT name, salary FROM employees WHERE id = $1',
      [ValueBuilder.bigint(1)]
    );
    if (result.rows.length > 0) {
      if (isText(result.rows[0][0]) && isBigInt(result.rows[0][1])) {
        const name = result.rows[0][0].value;
        const salary = Number(result.rows[0][1].value);
        console.log(`  Verified: ${name} now earns $${salary.toLocaleString()}`);
      }
    }

    // Update multiple rows
    await client.execute(
      'UPDATE employees SET is_active = $1 WHERE salary < $2',
      [ValueBuilder.boolean(false), ValueBuilder.bigint(90000)]
    );
    console.log('✓ Deactivated employees earning < $90,000');

    // ========================================================================
    // Example 5: Point-in-Time Queries (Compliance)
    // ========================================================================
    console.log('\n=== Example 5: Point-in-Time Queries ===');

    console.log('\nDemonstration of point-in-time query capability:');
    console.log('(Requires server API to expose current log position)');

    // Make a change
    await client.execute(
      'UPDATE employees SET email = $1 WHERE id = $2',
      [ValueBuilder.text('alice.new@example.com'), ValueBuilder.bigint(1)]
    );
    console.log("✓ Changed Alice's email to alice.new@example.com");

    // Query current state
    result = await client.query(
      'SELECT email FROM employees WHERE id = $1',
      [ValueBuilder.bigint(1)]
    );
    if (result.rows.length > 0 && isText(result.rows[0][0])) {
      const currentEmail = result.rows[0][0].value;
      console.log(`  Current email: ${currentEmail}`);
    }

    // Point-in-time query
    try {
      const historicalOffset = 0n; // Query at beginning of log
      const resultAt = await client.queryAt(
        'SELECT email FROM employees WHERE id = $1',
        [ValueBuilder.bigint(1)],
        historicalOffset
      );
      if (resultAt.rows.length > 0 && isText(resultAt.rows[0][0])) {
        const historicalEmail = resultAt.rows[0][0].value;
        console.log(
          `  Historical email (at offset ${historicalOffset}): ${historicalEmail}`
        );
        console.log('  ✓ Point-in-time query demonstrates audit capability');
      }
    } catch (e) {
      console.log(`  Note: Point-in-time query requires proper offset: ${e}`);
    }

    // ========================================================================
    // Example 6: Delete Data
    // ========================================================================
    console.log('\n=== Example 6: Delete Data ===');

    // Delete specific row
    await client.execute('DELETE FROM employees WHERE id = $1', [
      ValueBuilder.bigint(3),
    ]);
    console.log('✓ Deleted employee ID 3');

    // Verify deletion
    result = await client.query(
      'SELECT COUNT(*) FROM employees WHERE id = $1',
      [ValueBuilder.bigint(3)]
    );
    if (result.rows.length > 0 && isBigInt(result.rows[0][0])) {
      const count = result.rows[0][0].value;
      console.log(`  Verified: ${count} employees with ID 3 found`);
    }

    // ========================================================================
    // Example 7: Working with NULL Values
    // ========================================================================
    console.log('\n=== Example 7: Working with NULL Values ===');

    result = await client.query(
      'SELECT id, name, email FROM employees WHERE email IS NULL'
    );
    console.log(`\nEmployees with NULL email (${result.rows.length} found):`);
    for (const row of result.rows) {
      if (isBigInt(row[0]) && isText(row[1])) {
        const empId = row[0].value;
        const name = row[1].value;
        const emailStatus = isNull(row[2]) ? 'NULL' : 'has email';
        console.log(`  - ID ${empId}: ${name} (email: ${emailStatus})`);
      }
    }

    // ========================================================================
    // Example 8: Error Handling
    // ========================================================================
    console.log('\n=== Example 8: Error Handling ===');

    // Syntax error
    try {
      await client.query('INVALID SQL SYNTAX');
    } catch (e) {
      console.log(`✓ Caught syntax error: ${(e as Error).constructor.name}`);
    }

    // Table not found
    try {
      await client.query('SELECT * FROM nonexistent_table');
    } catch (e) {
      console.log(`✓ Caught table not found: ${(e as Error).constructor.name}`);
    }

    // ========================================================================
    // Example 9: Working with Timestamps
    // ========================================================================
    console.log('\n=== Example 9: Working with Timestamps ===');

    result = await client.query(
      'SELECT name, hired_at FROM employees WHERE is_active = $1',
      [ValueBuilder.boolean(true)]
    );
    console.log('\nEmployee hire dates:');
    for (const row of result.rows) {
      if (isText(row[0])) {
        const name = row[0].value;
        const hiredVal = row[1];
        if (hiredVal.type === ValueType.Timestamp) {
          const hiredDate = valueToDate(hiredVal);
          if (hiredDate) {
            console.log(`  - ${name}: ${hiredDate.toISOString().split('T')[0]}`);
          }
        }
      }
    }

    // ========================================================================
    // Example 10: Batch Operations
    // ========================================================================
    console.log('\n=== Example 10: Batch Operations ===');

    // Create a temporary table
    await client.execute(`
      CREATE TABLE IF NOT EXISTS temp_data (
        id BIGINT PRIMARY KEY,
        value TEXT
      )
    `);

    // Insert batch
    const batchSize = 10;
    for (let i = 0; i < batchSize; i++) {
      await client.execute(
        'INSERT INTO temp_data (id, value) VALUES ($1, $2)',
        [ValueBuilder.bigint(i), ValueBuilder.text(`Value_${i}`)]
      );
    }
    console.log(`✓ Inserted ${batchSize} rows in batch`);

    // Query batch
    result = await client.query('SELECT COUNT(*) FROM temp_data');
    if (result.rows.length > 0 && isBigInt(result.rows[0][0])) {
      const count = result.rows[0][0].value;
      console.log(`  Verified: ${count} rows in temp_data`);
    }

    // Cleanup
    await client.execute('DROP TABLE IF EXISTS temp_data');
    console.log('✓ Cleaned up temporary table');

    console.log('\n' + '='.repeat(60));
    console.log('✓ All examples completed successfully!');
    console.log('='.repeat(60));
  } finally {
    await client.disconnect();
    console.log('\n✓ Disconnected from Kimberlite');
  }
}

// Run examples
main().catch((err) => {
  console.error('Error running examples:', err);
  process.exit(1);
});
