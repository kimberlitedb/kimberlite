---
title: "JSON Operators"
section: "reference/sql"
slug: "json"
order: 6
---

# JSON Operators

Kimberlite supports three PostgreSQL-compatible JSON operators in `WHERE`
clauses: path extraction (`->` and `->>`) and containment (`@>`). The
JSON column type is recognised throughout DDL/DML; these operators add the
ability to query inside the JSON value without deserialising client-side.

## Operator reference

| Operator | Reads as | Result type |
|---|---|---|
| `data -> 'key'` | "extract value at key" | JSON value |
| `data ->> 'key'` | "extract value at key as text" | TEXT |
| `data @> json_value` | "data contains json_value" | BOOLEAN |

## Examples

### Healthcare: query inside FHIR resources

```sql
-- Find encounters whose FHIR Resource has status 'completed'
SELECT id FROM encounter
WHERE (resource->>'status') = $1;
-- params: [Value::Text("completed")]
```

### Legal: query inside case-document fragments

```sql
-- Find documents tagged 'urgent' in the metadata JSON
SELECT id, title FROM legal_documents
WHERE metadata @> $1;
-- params: [Value::Json({"tag": "urgent"})]
```

### Finance: filter on FIX message metadata

```sql
-- Trade messages with venue 'XNYS' in their FIX header
SELECT trade_id, executed_at FROM trade_messages
WHERE (header->>'venue') = $1
ORDER BY executed_at DESC
LIMIT 100;
```

## Containment semantics

`@>` follows PostgreSQL's containment rules:

- **Object containment** — every key in the right-hand side must exist in
  the left-hand side with a recursively-contained value.
- **Array containment** — every element of the right-hand array must appear
  somewhere in the left-hand array (multiset-subset).
- **Scalar containment** — equality.

Containment short-circuits when types don't match (e.g. an array can't
contain an object key).

## Operator precedence gotcha

Kimberlite's SQL dialect parses `->` and `->>` with **lower** precedence than
`=`, the opposite of PostgreSQL. Until that's fixed, parenthesise the JSON
extraction explicitly:

```sql
-- Right
SELECT id FROM t WHERE (data->>'status') = $1;

-- Wrong — parses as data->>('status' = $1) and errors
SELECT id FROM t WHERE data->>'status' = $1;
```

The parser unwraps one layer of parens on the LHS to make the workaround
ergonomic.

## What's not supported (yet)

- Multi-segment paths (`data->'a'->'b'`). Compose with explicit nested
  parens for now (`(data->'a')->'b'`).
- `#>` and `#>>` array-path operators.
- `@>` with non-JSON LHS — the column must be `JSON`.
- Range comparisons (`<`, `>`, etc.) on extracted values. Today only `=`
  works on the result of `->`/`->>`.
- Updates against extracted fields (`UPDATE t SET data->>'k' = ...`).
  Use full-document UPDATE for now.

## Related

- [Query Reference](queries.md) — `SELECT`, joins, aggregates
- [DML Reference](dml.md) — `INSERT`, `UPDATE`, `DELETE`
