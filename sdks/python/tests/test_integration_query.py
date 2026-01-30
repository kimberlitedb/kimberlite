"""Integration tests for query functionality against live kmb-server.

These tests require a running kmb-server instance.
Run with: pytest tests/test_integration_query.py
"""

import pytest
from datetime import datetime
from kimberlite import Client, DataClass, Value, ValueType


@pytest.fixture
def client():
    """Connect to test server."""
    try:
        client = Client.connect(
            addresses=["localhost:5432"],
            tenant_id=1,
            auth_token="test-token",
        )
        yield client
        client.disconnect()
    except Exception as e:
        pytest.skip(f"Server not available: {e}")


@pytest.fixture
def setup_test_table(client):
    """Create and populate a test table."""
    # Create table
    client.execute("""
        CREATE TABLE IF NOT EXISTS test_users (
            id BIGINT PRIMARY KEY,
            name TEXT,
            active BOOLEAN,
            created_at TIMESTAMP
        )
    """)

    # Clean up any existing data
    try:
        client.execute("DELETE FROM test_users")
    except:
        pass  # Table might not exist yet

    yield client

    # Cleanup after tests
    try:
        client.execute("DROP TABLE IF EXISTS test_users")
    except:
        pass


class TestBasicQueries:
    """Test basic SQL queries."""

    def test_create_table(self, client):
        """Test CREATE TABLE statement."""
        result = client.execute("""
            CREATE TABLE IF NOT EXISTS users (
                id BIGINT PRIMARY KEY,
                name TEXT
            )
        """)
        # DDL returns 0 rows affected
        assert result == 0

    def test_insert_single_row(self, setup_test_table):
        """Test INSERT with parameterized values."""
        client = setup_test_table

        rows_affected = client.execute(
            "INSERT INTO test_users (id, name, active, created_at) VALUES ($1, $2, $3, $4)",
            [
                Value.bigint(1),
                Value.text("Alice"),
                Value.boolean(True),
                Value.timestamp(1609459200_000_000_000),  # 2021-01-01
            ]
        )

        # INSERT typically returns 0 for non-RETURNING
        assert rows_affected >= 0

    def test_select_all(self, setup_test_table):
        """Test SELECT * query."""
        client = setup_test_table

        # Insert test data
        client.execute(
            "INSERT INTO test_users (id, name, active, created_at) VALUES ($1, $2, $3, $4)",
            [Value.bigint(1), Value.text("Alice"), Value.boolean(True), Value.timestamp(1000)]
        )

        # Query
        result = client.query("SELECT * FROM test_users")

        assert len(result.columns) == 4
        assert "id" in result.columns
        assert "name" in result.columns
        assert len(result.rows) >= 1

    def test_select_with_where(self, setup_test_table):
        """Test SELECT with WHERE clause and parameters."""
        client = setup_test_table

        # Insert multiple rows
        client.execute(
            "INSERT INTO test_users (id, name, active, created_at) VALUES ($1, $2, $3, $4)",
            [Value.bigint(1), Value.text("Alice"), Value.boolean(True), Value.timestamp(1000)]
        )
        client.execute(
            "INSERT INTO test_users (id, name, active, created_at) VALUES ($1, $2, $3, $4)",
            [Value.bigint(2), Value.text("Bob"), Value.boolean(False), Value.timestamp(2000)]
        )

        # Query with WHERE
        result = client.query(
            "SELECT * FROM test_users WHERE id = $1",
            [Value.bigint(1)]
        )

        assert len(result.rows) == 1
        row = result.rows[0]

        # Find name column index
        name_idx = result.columns.index("name")
        assert row[name_idx].type == ValueType.TEXT
        assert row[name_idx].data == "Alice"


class TestValueTypes:
    """Test all value types in queries."""

    def test_null_value(self, setup_test_table):
        """Test NULL value handling."""
        client = setup_test_table

        # Insert with NULL
        client.execute(
            "INSERT INTO test_users (id, name, active, created_at) VALUES ($1, $2, $3, $4)",
            [Value.bigint(1), Value.null(), Value.boolean(True), Value.timestamp(1000)]
        )

        result = client.query("SELECT name FROM test_users WHERE id = $1", [Value.bigint(1)])
        assert len(result.rows) == 1
        assert result.rows[0][0].is_null()

    def test_bigint_value(self, setup_test_table):
        """Test BIGINT value handling."""
        client = setup_test_table

        large_num = 9007199254740991  # Max safe integer
        client.execute(
            "INSERT INTO test_users (id, name, active, created_at) VALUES ($1, $2, $3, $4)",
            [Value.bigint(large_num), Value.text("Test"), Value.boolean(True), Value.timestamp(1000)]
        )

        result = client.query("SELECT id FROM test_users WHERE id = $1", [Value.bigint(large_num)])
        assert len(result.rows) == 1
        assert result.rows[0][0].data == large_num

    def test_text_value(self, setup_test_table):
        """Test TEXT value with unicode."""
        client = setup_test_table

        unicode_text = "Hello, 世界! 🌍"
        client.execute(
            "INSERT INTO test_users (id, name, active, created_at) VALUES ($1, $2, $3, $4)",
            [Value.bigint(1), Value.text(unicode_text), Value.boolean(True), Value.timestamp(1000)]
        )

        result = client.query("SELECT name FROM test_users WHERE id = $1", [Value.bigint(1)])
        name_idx = 0
        assert result.rows[0][name_idx].data == unicode_text

    def test_boolean_value(self, setup_test_table):
        """Test BOOLEAN value handling."""
        client = setup_test_table

        client.execute(
            "INSERT INTO test_users (id, name, active, created_at) VALUES ($1, $2, $3, $4)",
            [Value.bigint(1), Value.text("Test"), Value.boolean(True), Value.timestamp(1000)]
        )

        result = client.query("SELECT active FROM test_users WHERE id = $1", [Value.bigint(1)])
        assert result.rows[0][0].type == ValueType.BOOLEAN
        assert result.rows[0][0].data is True

    def test_timestamp_value(self, setup_test_table):
        """Test TIMESTAMP value handling."""
        client = setup_test_table

        timestamp_nanos = 1609459200_000_000_000  # 2021-01-01 00:00:00 UTC
        client.execute(
            "INSERT INTO test_users (id, name, active, created_at) VALUES ($1, $2, $3, $4)",
            [Value.bigint(1), Value.text("Test"), Value.boolean(True), Value.timestamp(timestamp_nanos)]
        )

        result = client.query("SELECT created_at FROM test_users WHERE id = $1", [Value.bigint(1)])
        created_idx = 0
        assert result.rows[0][created_idx].type == ValueType.TIMESTAMP

        # Convert to datetime and verify
        dt = result.rows[0][created_idx].to_datetime()
        assert dt is not None
        assert dt.year == 2021


class TestParameterizedQueries:
    """Test parameterized query support."""

    def test_multiple_parameters(self, setup_test_table):
        """Test query with multiple parameters."""
        client = setup_test_table

        # Insert test data
        client.execute(
            "INSERT INTO test_users (id, name, active, created_at) VALUES ($1, $2, $3, $4)",
            [Value.bigint(1), Value.text("Alice"), Value.boolean(True), Value.timestamp(1000)]
        )
        client.execute(
            "INSERT INTO test_users (id, name, active, created_at) VALUES ($1, $2, $3, $4)",
            [Value.bigint(2), Value.text("Bob"), Value.boolean(True), Value.timestamp(2000)]
        )

        # Query with multiple params
        result = client.query(
            "SELECT * FROM test_users WHERE active = $1 AND id > $2",
            [Value.boolean(True), Value.bigint(0)]
        )

        assert len(result.rows) >= 2

    def test_no_parameters(self, setup_test_table):
        """Test query with no parameters."""
        client = setup_test_table

        result = client.query("SELECT * FROM test_users")
        assert result is not None
        assert isinstance(result.rows, list)


class TestDMLOperations:
    """Test INSERT, UPDATE, DELETE operations."""

    def test_update_statement(self, setup_test_table):
        """Test UPDATE statement."""
        client = setup_test_table

        # Insert
        client.execute(
            "INSERT INTO test_users (id, name, active, created_at) VALUES ($1, $2, $3, $4)",
            [Value.bigint(1), Value.text("Alice"), Value.boolean(True), Value.timestamp(1000)]
        )

        # Update
        client.execute(
            "UPDATE test_users SET name = $1 WHERE id = $2",
            [Value.text("Alice Updated"), Value.bigint(1)]
        )

        # Verify
        result = client.query("SELECT name FROM test_users WHERE id = $1", [Value.bigint(1)])
        assert result.rows[0][0].data == "Alice Updated"

    def test_delete_statement(self, setup_test_table):
        """Test DELETE statement."""
        client = setup_test_table

        # Insert
        client.execute(
            "INSERT INTO test_users (id, name, active, created_at) VALUES ($1, $2, $3, $4)",
            [Value.bigint(1), Value.text("Alice"), Value.boolean(True), Value.timestamp(1000)]
        )

        # Delete
        client.execute("DELETE FROM test_users WHERE id = $1", [Value.bigint(1)])

        # Verify deletion
        result = client.query("SELECT * FROM test_users WHERE id = $1", [Value.bigint(1)])
        assert len(result.rows) == 0


class TestPointInTimeQueries:
    """Test point-in-time query functionality."""

    def test_query_at_specific_position(self, setup_test_table):
        """Test querying state at a specific log position."""
        client = setup_test_table

        # Insert initial data
        client.execute(
            "INSERT INTO test_users (id, name, active, created_at) VALUES ($1, $2, $3, $4)",
            [Value.bigint(1), Value.text("Alice"), Value.boolean(True), Value.timestamp(1000)]
        )

        # Get current state
        result1 = client.query("SELECT COUNT(*) FROM test_users", [])
        initial_count = result1.rows[0][0].data if len(result1.rows) > 0 else 0

        # Note: To properly test query_at, we would need to:
        # 1. Get the current log position from the server
        # 2. Make modifications
        # 3. Query at the previous position
        # This requires additional server API support for getting log positions

        # For now, just verify query_at doesn't crash
        from kimberlite.types import Offset
        result = client.query_at(
            "SELECT * FROM test_users",
            [],
            Offset(0)  # Query at beginning
        )
        assert result is not None


class TestErrorHandling:
    """Test error handling for queries."""

    def test_syntax_error(self, client):
        """Test handling of SQL syntax errors."""
        with pytest.raises(Exception):  # Should raise QuerySyntaxError
            client.query("INVALID SQL SYNTAX", [])

    def test_table_not_found(self, client):
        """Test handling of non-existent table."""
        with pytest.raises(Exception):  # Should raise QueryExecutionError
            client.query("SELECT * FROM nonexistent_table_xyz", [])

    def test_wrong_parameter_count(self, setup_test_table):
        """Test parameter count mismatch."""
        client = setup_test_table

        # Query expects 1 parameter but we provide 0
        with pytest.raises(Exception):
            client.query("SELECT * FROM test_users WHERE id = $1", [])


class TestEmptyResults:
    """Test handling of empty result sets."""

    def test_empty_result_set(self, setup_test_table):
        """Test query returning no rows."""
        client = setup_test_table

        result = client.query("SELECT * FROM test_users WHERE id = $1", [Value.bigint(99999)])
        assert len(result.rows) == 0
        assert len(result.columns) > 0  # Should still have column names


class TestLargeResultSets:
    """Test handling of large result sets."""

    def test_multiple_rows(self, setup_test_table):
        """Test query returning multiple rows."""
        client = setup_test_table

        # Insert multiple rows
        for i in range(10):
            client.execute(
                "INSERT INTO test_users (id, name, active, created_at) VALUES ($1, $2, $3, $4)",
                [
                    Value.bigint(i),
                    Value.text(f"User{i}"),
                    Value.boolean(i % 2 == 0),
                    Value.timestamp(1000 * i)
                ]
            )

        # Query all
        result = client.query("SELECT * FROM test_users", [])
        assert len(result.rows) >= 10
