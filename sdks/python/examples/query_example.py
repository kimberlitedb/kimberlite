"""Comprehensive query examples for Kimberlite Python SDK.

This example demonstrates:
- Table creation and management
- Parameterized queries with all value types
- CRUD operations
- Point-in-time queries for compliance
- Error handling
"""

from datetime import datetime
from kimberlite import Client, Value, DataClass
from kimberlite.types import Offset


def main():
    """Run comprehensive query examples."""

    # Connect to Kimberlite
    with Client.connect(
        addresses=["localhost:5432"],
        tenant_id=1,
        auth_token="demo-token"
    ) as client:
        print("✓ Connected to Kimberlite")

        # ============================================================================
        # Example 1: Create Table (DDL)
        # ============================================================================
        print("\n=== Example 1: Create Table ===")

        client.execute("""
            CREATE TABLE IF NOT EXISTS employees (
                id BIGINT PRIMARY KEY,
                name TEXT NOT NULL,
                email TEXT,
                salary BIGINT,
                is_active BOOLEAN,
                hired_at TIMESTAMP
            )
        """)
        print("✓ Created employees table")

        # ============================================================================
        # Example 2: Insert Data (Parameterized Queries)
        # ============================================================================
        print("\n=== Example 2: Insert Data ===")

        # Insert with all value types
        client.execute(
            """
            INSERT INTO employees (id, name, email, salary, is_active, hired_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            """,
            [
                Value.bigint(1),
                Value.text("Alice Johnson"),
                Value.text("alice@example.com"),
                Value.bigint(95000),
                Value.boolean(True),
                Value.from_datetime(datetime(2020, 1, 15, 9, 0, 0))
            ]
        )
        print("✓ Inserted Alice Johnson")

        # Insert with NULL value
        client.execute(
            """
            INSERT INTO employees (id, name, email, salary, is_active, hired_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            """,
            [
                Value.bigint(2),
                Value.text("Bob Smith"),
                Value.null(),  # No email
                Value.bigint(87000),
                Value.boolean(True),
                Value.timestamp(1610712000_000_000_000)  # Manual timestamp
            ]
        )
        print("✓ Inserted Bob Smith (with NULL email)")

        # Insert more employees
        employees = [
            (3, "Carol Davis", "carol@example.com", 92000, False, datetime(2019, 5, 1)),
            (4, "David Lee", "david@example.com", 103000, True, datetime(2021, 3, 10)),
            (5, "Eve Martinez", "eve@example.com", 88000, True, datetime(2022, 7, 20)),
        ]

        for emp_id, name, email, salary, is_active, hired in employees:
            client.execute(
                """
                INSERT INTO employees (id, name, email, salary, is_active, hired_at)
                VALUES ($1, $2, $3, $4, $5, $6)
                """,
                [
                    Value.bigint(emp_id),
                    Value.text(name),
                    Value.text(email),
                    Value.bigint(salary),
                    Value.boolean(is_active),
                    Value.from_datetime(hired)
                ]
            )
        print(f"✓ Inserted {len(employees)} more employees")

        # ============================================================================
        # Example 3: Select Queries
        # ============================================================================
        print("\n=== Example 3: Select Queries ===")

        # Select all
        result = client.query("SELECT * FROM employees")
        print(f"\nAll employees ({len(result.rows)} total):")
        print(f"Columns: {result.columns}")
        for row in result.rows:
            id_idx = result.columns.index('id')
            name_idx = result.columns.index('name')
            print(f"  - ID {row[id_idx].data}: {row[name_idx].data}")

        # Select with WHERE clause
        result = client.query(
            "SELECT name, salary FROM employees WHERE is_active = $1",
            [Value.boolean(True)]
        )
        print(f"\nActive employees ({len(result.rows)} found):")
        for row in result.rows:
            name = row[0].data
            salary = row[1].data
            print(f"  - {name}: ${salary:,}")

        # Select with ORDER BY
        result = client.query(
            "SELECT name, salary FROM employees WHERE is_active = $1 ORDER BY salary DESC",
            [Value.boolean(True)]
        )
        print(f"\nActive employees (ordered by salary):")
        for row in result.rows:
            name = row[0].data
            salary = row[1].data
            print(f"  - {name}: ${salary:,}")

        # Aggregate query
        result = client.query("SELECT COUNT(*) as total FROM employees")
        if result.rows:
            total = result.rows[0][0].data
            print(f"\nTotal employees: {total}")

        # ============================================================================
        # Example 4: Update Data
        # ============================================================================
        print("\n=== Example 4: Update Data ===")

        # Update single row
        client.execute(
            "UPDATE employees SET salary = $1 WHERE id = $2",
            [Value.bigint(98000), Value.bigint(1)]
        )
        print("✓ Updated Alice's salary to $98,000")

        # Verify update
        result = client.query(
            "SELECT name, salary FROM employees WHERE id = $1",
            [Value.bigint(1)]
        )
        if result.rows:
            name = result.rows[0][0].data
            salary = result.rows[0][1].data
            print(f"  Verified: {name} now earns ${salary:,}")

        # Update multiple rows
        client.execute(
            "UPDATE employees SET is_active = $1 WHERE salary < $2",
            [Value.boolean(False), Value.bigint(90000)]
        )
        print("✓ Deactivated employees earning < $90,000")

        # ============================================================================
        # Example 5: Point-in-Time Queries (Compliance)
        # ============================================================================
        print("\n=== Example 5: Point-in-Time Queries ===")

        # Note: This requires server support for exposing log positions
        # For demonstration, we'll use a hypothetical offset
        print("\nDemonstration of point-in-time query capability:")
        print("(Requires server API to expose current log position)")

        # Hypothetical: Get current position
        # current_offset = client.log_position()  # Not yet implemented

        # Make a change
        client.execute(
            "UPDATE employees SET email = $1 WHERE id = $2",
            [Value.text("alice.new@example.com"), Value.bigint(1)]
        )
        print("✓ Changed Alice's email to alice.new@example.com")

        # Query current state
        result = client.query(
            "SELECT email FROM employees WHERE id = $1",
            [Value.bigint(1)]
        )
        if result.rows:
            current_email = result.rows[0][0].data
            print(f"  Current email: {current_email}")

        # Point-in-time query (query state before the change)
        try:
            historical_offset = Offset(0)  # Query at beginning of log
            result_at = client.query_at(
                "SELECT email FROM employees WHERE id = $1",
                [Value.bigint(1)],
                historical_offset
            )
            if result_at.rows:
                historical_email = result_at.rows[0][0].data
                print(f"  Historical email (at offset {historical_offset}): {historical_email}")
                print("  ✓ Point-in-time query demonstrates audit capability")
        except Exception as e:
            print(f"  Note: Point-in-time query requires proper offset: {e}")

        # ============================================================================
        # Example 6: Delete Data
        # ============================================================================
        print("\n=== Example 6: Delete Data ===")

        # Delete specific row
        client.execute(
            "DELETE FROM employees WHERE id = $1",
            [Value.bigint(3)]
        )
        print("✓ Deleted employee ID 3")

        # Verify deletion
        result = client.query(
            "SELECT COUNT(*) FROM employees WHERE id = $1",
            [Value.bigint(3)]
        )
        if result.rows:
            count = result.rows[0][0].data
            print(f"  Verified: {count} employees with ID 3 found")

        # ============================================================================
        # Example 7: Working with NULL Values
        # ============================================================================
        print("\n=== Example 7: Working with NULL Values ===")

        result = client.query(
            "SELECT id, name, email FROM employees WHERE email IS NULL"
        )
        print(f"\nEmployees with NULL email ({len(result.rows)} found):")
        for row in result.rows:
            emp_id = row[0].data
            name = row[1].data
            email_val = row[2]
            email_status = "NULL" if email_val.is_null() else email_val.data
            print(f"  - ID {emp_id}: {name} (email: {email_status})")

        # ============================================================================
        # Example 8: Error Handling
        # ============================================================================
        print("\n=== Example 8: Error Handling ===")

        # Syntax error
        try:
            client.query("INVALID SQL SYNTAX")
        except Exception as e:
            print(f"✓ Caught syntax error: {type(e).__name__}")

        # Table not found
        try:
            client.query("SELECT * FROM nonexistent_table")
        except Exception as e:
            print(f"✓ Caught table not found: {type(e).__name__}")

        # Parameter mismatch
        try:
            client.query("SELECT * FROM employees WHERE id = $1")  # Missing parameter
        except Exception as e:
            print(f"✓ Caught parameter error: {type(e).__name__}")

        # ============================================================================
        # Example 9: Working with Timestamps
        # ============================================================================
        print("\n=== Example 9: Working with Timestamps ===")

        result = client.query("SELECT name, hired_at FROM employees WHERE is_active = $1", [Value.boolean(True)])
        print("\nEmployee hire dates:")
        for row in result.rows:
            name = row[0].data
            hired_val = row[1]
            if hired_val.type == Value.timestamp(0).type:
                hired_dt = hired_val.to_datetime()
                if hired_dt:
                    print(f"  - {name}: {hired_dt.strftime('%Y-%m-%d')}")

        # ============================================================================
        # Example 10: Batch Operations
        # ============================================================================
        print("\n=== Example 10: Batch Operations ===")

        # Create a temporary table
        client.execute("""
            CREATE TABLE IF NOT EXISTS temp_data (
                id BIGINT PRIMARY KEY,
                value TEXT
            )
        """)

        # Insert batch
        batch_size = 10
        for i in range(batch_size):
            client.execute(
                "INSERT INTO temp_data (id, value) VALUES ($1, $2)",
                [Value.bigint(i), Value.text(f"Value_{i}")]
            )
        print(f"✓ Inserted {batch_size} rows in batch")

        # Query batch
        result = client.query("SELECT COUNT(*) FROM temp_data")
        if result.rows:
            count = result.rows[0][0].data
            print(f"  Verified: {count} rows in temp_data")

        # Cleanup
        client.execute("DROP TABLE IF EXISTS temp_data")
        print("✓ Cleaned up temporary table")

        print("\n" + "=" * 60)
        print("✓ All examples completed successfully!")
        print("=" * 60)


if __name__ == "__main__":
    main()
