"""Tests for query functionality (unit tests without server)."""

import pytest
from kimberlite.value import Value, ValueType
from kimberlite.client import QueryResult


class TestQueryResult:
    """Tests for QueryResult class."""

    def test_create_empty_result(self):
        result = QueryResult([], [])
        assert result.columns == []
        assert result.rows == []
        assert len(result) == 0

    def test_create_result_with_data(self):
        columns = ["id", "name", "active"]
        rows = [
            [Value.bigint(1), Value.text("Alice"), Value.boolean(True)],
            [Value.bigint(2), Value.text("Bob"), Value.boolean(False)],
        ]
        result = QueryResult(columns, rows)

        assert result.columns == ["id", "name", "active"]
        assert len(result) == 2
        assert len(result.rows) == 2

    def test_result_repr(self):
        result = QueryResult(["col1"], [[Value.bigint(1)]])
        repr_str = repr(result)
        assert "QueryResult" in repr_str
        assert "col1" in repr_str
        assert "1 rows" in repr_str

    def test_result_length(self):
        result = QueryResult(["a"], [])
        assert len(result) == 0

        result = QueryResult(["a"], [[Value.bigint(1)], [Value.bigint(2)]])
        assert len(result) == 2


class TestValueConversion:
    """Tests for value conversion helpers (simulated)."""

    def test_value_types_match_ffi_constants(self):
        # Ensure our ValueType enum matches FFI constants
        assert ValueType.NULL == 0
        assert ValueType.BIGINT == 1
        assert ValueType.TEXT == 2
        assert ValueType.BOOLEAN == 3
        assert ValueType.TIMESTAMP == 4


# Integration tests that require a running server would go in test_integration_query.py
