"""Lightweight SQL query builder — mirrors the Rust `Query` type.

Not an ORM. Emits a ``(sql, params)`` tuple you pass to ``client.query``.

Example:

    >>> from kimberlite import Query, Value
    >>> sql, params = (
    ...     Query.from_table("patients")
    ...         .select(["id", "name", "dob"])
    ...         .where_eq("tenant_id", Value.bigint(42))
    ...         .where_eq("active", Value.boolean(True))
    ...         .order_by("name")
    ...         .limit(100)
    ...         .build()
    ... )
    >>> rows = client.query(sql, params)
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import List, Optional, Tuple

from .value import Value


@dataclass
class _Predicate:
    column: str
    cmp: str
    value: Value


@dataclass
class Query:
    """Fluent SQL SELECT builder."""

    _from_table: str
    _columns: List[str] = field(default_factory=list)
    _wheres: List[_Predicate] = field(default_factory=list)
    _order_col: Optional[str] = None
    _order_desc: bool = False
    _limit: Optional[int] = None

    @classmethod
    def from_table(cls, table: str) -> "Query":
        """Start a query over ``table``. Mirrors the Rust `Query::from()`."""
        return cls(_from_table=table)

    def select(self, columns: List[str]) -> "Query":
        self._columns = list(columns)
        return self

    def where_eq(self, column: str, value: Value) -> "Query":
        self._wheres.append(_Predicate(column, "=", value))
        return self

    def where_lt(self, column: str, value: Value) -> "Query":
        self._wheres.append(_Predicate(column, "<", value))
        return self

    def where_gt(self, column: str, value: Value) -> "Query":
        self._wheres.append(_Predicate(column, ">", value))
        return self

    def where_le(self, column: str, value: Value) -> "Query":
        self._wheres.append(_Predicate(column, "<=", value))
        return self

    def where_ge(self, column: str, value: Value) -> "Query":
        self._wheres.append(_Predicate(column, ">=", value))
        return self

    def where_ne(self, column: str, value: Value) -> "Query":
        self._wheres.append(_Predicate(column, "!=", value))
        return self

    def order_by(self, column: str) -> "Query":
        self._order_col = column
        self._order_desc = False
        return self

    def order_by_desc(self, column: str) -> "Query":
        self._order_col = column
        self._order_desc = True
        return self

    def limit(self, n: int) -> "Query":
        self._limit = n
        return self

    def build(self) -> Tuple[str, List[Value]]:
        cols = "*" if not self._columns else ", ".join(self._columns)
        sql = f"SELECT {cols} FROM {self._from_table}"
        params: List[Value] = []

        for i, p in enumerate(self._wheres):
            sql += " WHERE " if i == 0 else " AND "
            sql += f"{p.column} {p.cmp} ${i + 1}"
            params.append(p.value)

        if self._order_col is not None:
            sql += f" ORDER BY {self._order_col}"
            if self._order_desc:
                sql += " DESC"
        if self._limit is not None:
            sql += f" LIMIT {self._limit}"

        return sql, params
