# SQL Parser Adversarial Corpus

**Created for:** AUDIT-2026-03 M-1 (SQL Parser Fuzzing Enhancement)

This corpus contains 27 adversarial SQL test cases designed to stress-test the SQL parser's correctness, security, and edge-case handling.

## Categories

### SQL Injection Patterns (5 cases)
- `sql_injection_01_union.txt` - UNION-based injection
- `sql_injection_02_comment.txt` - Comment-based injection
- `sql_injection_03_or.txt` - OR-based authentication bypass
- `sql_injection_04_stacked.txt` - Stacked queries (semicolon)
- `sql_injection_05_nested.txt` - Nested subquery injection

### Deep Nesting (2 cases)
- `deep_nesting_01.txt` - 5-level subquery nesting
- `deep_nesting_02_cte.txt` - Recursive CTE with 4 levels

### Unicode Handling (3 cases)
- `unicode_01_emoji.txt` - Emoji in strings
- `unicode_02_rtl.txt` - Right-to-left (RTL) characters (Arabic)
- `unicode_03_zalgo.txt` - Combining diacritics (Zalgo text)

### Whitespace Variations (3 cases)
- `whitespace_01_tabs.txt` - Tab-separated SQL
- `whitespace_02_newlines.txt` - Newline-separated SQL
- `whitespace_03_mixed.txt` - Mixed tabs/spaces/newlines

### Case Sensitivity (2 cases)
- `case_01_lower.txt` - All lowercase keywords
- `case_02_mixed.txt` - Mixed case keywords

### Quote Handling (3 cases)
- `quotes_01_single.txt` - Escaped single quotes
- `quotes_02_double.txt` - Double quotes for identifiers
- `quotes_03_backticks.txt` - MySQL-style backticks

### Numeric Edge Cases (3 cases)
- `numeric_01_overflow.txt` - Integer overflow
- `numeric_02_scientific.txt` - Scientific notation
- `numeric_03_hex.txt` - Hexadecimal literals

### Complex Queries (3 cases)
- `complex_01_join_chain.txt` - 5-table JOIN chain
- `complex_02_union_chain.txt` - 4-query UNION chain
- `complex_03_nested_joins.txt` - Nested JOIN subqueries

### Edge Cases (3 cases)
- `edge_01_empty_string.txt` - Empty string literal
- `edge_02_null.txt` - NULL handling
- `edge_03_boolean.txt` - Boolean literals

## Usage

These cases are automatically included in fuzzing campaigns via cargo-fuzz:

```bash
# Local fuzzing (requires nightly)
cargo +nightly fuzz run fuzz_sql_parser corpus/fuzz_sql_parser_adversarial/

# AWS continuous fuzzing
# Automatically included in 48-hour cycles (40 min/target)
```

## Security Context

- **AUDIT-2026-03 M-1:** Enhanced SQL parser fuzzing
- **CWE-707:** Improper Neutralization
- **OWASP:** A03:2021 – Injection
- **Compliance:** SOC 2 CC7.2, PCI-DSS Req 6.5.1

## Expected Outcomes

All test cases should either:
1. Parse successfully and pass AST validation
2. Return a proper error (never panic)

The enhanced fuzzer validates AST structure including:
- Non-empty column lists
- Valid JOIN conditions
- Column/value count matching in INSERT
- Proper CTE structure
