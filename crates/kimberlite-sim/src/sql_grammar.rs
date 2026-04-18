//! Weighted SQL grammar for coverage-guided fuzzing.
//!
//! Byte-level fuzzing of the SQL parser (`fuzz_sql_parser`) rejects
//! ~99% of mutations at the tokenizer, so coverage rarely reaches the
//! planner or executor. This module generates structurally valid SQL
//! from a small deterministic seed, which lets the fuzzer mutate the
//! *seed* (a `u64`) and have the grammar expand that mutation into a
//! new-shaped query every iteration.
//!
//! The grammar covers the subset of SQL that `parse_statement` in
//! `kimberlite-query` actually accepts (SELECT, INSERT, UPDATE,
//! DELETE, CREATE TABLE, with joins, GROUP BY/HAVING, UNION, and
//! predicates using AND/OR/NOT/IS NULL/BETWEEN). Everything is drawn
//! from a fixed identifier + value pool so the parser's identifier
//! rules don't become a rejection bottleneck.
//!
//! Two entry points:
//! - [`generate`] — full top-level statement.
//! - [`generate_predicate`] — schema-aware predicate string, used by
//!   the NoREC oracle to compose equivalent query pairs.
//!
//! Recursive productions are bounded by [`MAX_DEPTH`] to prevent a
//! pathological seed from producing deeply nested predicates that
//! would blow the stack during parsing.

use crate::SimRng;

/// Maximum nesting depth for recursive productions (predicates,
/// sub-expressions). A predicate tree deeper than this folds back to
/// leaf comparisons — the grammar never generates an unbounded tree.
pub const MAX_DEPTH: usize = 8;

/// A schema description used by predicate generators that must
/// reference columns that actually exist.
#[derive(Debug, Clone)]
pub struct SeedSchema {
    /// Table name (must match the `CREATE TABLE` statement used to
    /// seed the database before the generated predicate runs).
    pub table: String,
    /// Columns present in the table, in declaration order.
    pub columns: Vec<SeedColumn>,
}

/// A single column in a [`SeedSchema`].
#[derive(Debug, Clone)]
pub struct SeedColumn {
    /// Column name.
    pub name: String,
    /// Column type — steers the generator away from type-mismatch
    /// predicates that the planner would reject uninterestingly.
    pub ty: ColumnType,
}

/// Column data type supported by the grammar generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    BigInt,
    Integer,
    Text,
    Boolean,
}

impl SeedSchema {
    /// A three-column schema `(id BIGINT PRIMARY KEY, v BIGINT NULL,
    /// w BIGINT NULL)`. Used by the NoREC and PQS fuzz targets as
    /// the shared ground truth for generated predicates.
    pub fn numeric_trio(table: &str) -> Self {
        Self {
            table: table.to_string(),
            columns: vec![
                SeedColumn {
                    name: "id".into(),
                    ty: ColumnType::BigInt,
                },
                SeedColumn {
                    name: "v".into(),
                    ty: ColumnType::BigInt,
                },
                SeedColumn {
                    name: "w".into(),
                    ty: ColumnType::BigInt,
                },
            ],
        }
    }
}

// ---------------------------------------------------------------------
// Fixed pools — identifiers, operators, literal menus
// ---------------------------------------------------------------------

const TABLE_POOL: &[&str] = &["t", "t2", "events", "users", "records"];
const COLUMN_POOL: &[&str] = &["id", "a", "b", "v", "w", "name"];
const INT_TYPES: &[&str] = &["BIGINT", "INTEGER"];
const BOOL_OPS: &[&str] = &["AND", "OR"];
// `<>` is intentionally omitted — the current planner rejects NotEq in
// WHERE expressions. Adding it back once the planner lands inequality
// support is a one-line change.
const CMP_OPS: &[&str] = &["=", "<", "<=", ">", ">="];
const AGG_FNS: &[&str] = &["COUNT", "SUM", "MIN", "MAX"];

// ---------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------

/// Generates a top-level SQL statement from `seed`. Deterministic:
/// the same `seed` always produces the same string.
pub fn generate(seed: u64) -> String {
    let mut g = Gen::new(seed);
    g.statement()
}

/// Generates a boolean predicate suitable for a `WHERE` clause over
/// the columns of `schema`. Used by the NoREC oracle, which needs a
/// predicate the database can evaluate without grammar-induced
/// column-not-found errors.
pub fn generate_predicate(seed: u64, schema: &SeedSchema) -> String {
    debug_assert!(
        !schema.columns.is_empty(),
        "SeedSchema must have at least one column"
    );
    let mut g = Gen::new(seed);
    g.predicate_against(schema)
}

// ---------------------------------------------------------------------
// Internal generator
// ---------------------------------------------------------------------

struct Gen {
    rng: SimRng,
    depth: usize,
}

impl Gen {
    fn new(seed: u64) -> Self {
        Self {
            rng: SimRng::new(seed),
            depth: 0,
        }
    }

    fn pick<'a, T>(&mut self, pool: &'a [T]) -> &'a T {
        debug_assert!(!pool.is_empty(), "cannot pick from empty pool");
        self.rng.choose(pool)
    }

    /// Pick a `&str` from a slice of `&str`. Prefer this over `pick`
    /// when the caller will immediately convert to `String` — avoids
    /// `ToString` on `&&str`, which clippy flags as inefficient.
    ///
    /// The `*` is not removable despite what `clippy::explicit_auto_deref`
    /// suggests: `pick<T>` requires `T: Sized`, so the generic deduction
    /// picks `T = &str` and the call returns `&&str`; we must deref
    /// explicitly to get `&str`.
    #[allow(clippy::explicit_auto_deref)]
    fn pick_str<'a>(&mut self, pool: &[&'a str]) -> &'a str {
        *self.pick(pool)
    }

    fn int_lit(&mut self) -> String {
        // Small, readable literals — wide enough to hit sign-bit,
        // overflow-adjacent, and zero cases.
        const LITERALS: &[i64] = &[0, 1, -1, 2, 10, 100, -100, 255, i32::MAX as i64];
        self.pick(LITERALS).to_string()
    }

    fn text_lit(&mut self) -> String {
        const LITERALS: &[&str] = &["", "a", "abc", "alice", "bob"];
        format!("'{}'", self.pick_str(LITERALS))
    }

    fn bool_lit(&mut self) -> String {
        if self.rng.next_bool() {
            "TRUE".into()
        } else {
            "FALSE".into()
        }
    }

    /// Literal typed to match `ty`. Returns `NULL` with small
    /// probability so the NULL-handling partitions light up.
    fn typed_literal(&mut self, ty: ColumnType) -> String {
        if self.rng.next_bool_with_probability(0.08) {
            return "NULL".into();
        }
        match ty {
            ColumnType::BigInt | ColumnType::Integer => self.int_lit(),
            ColumnType::Text => self.text_lit(),
            ColumnType::Boolean => self.bool_lit(),
        }
    }

    fn value_any(&mut self) -> String {
        match self.rng.next_u64() % 5 {
            0 => "NULL".into(),
            1 => self.int_lit(),
            2 => self.text_lit(),
            3 => self.bool_lit(),
            _ => self.int_lit(),
        }
    }

    // -----------------------------------------------------------------
    // Top-level statement (weighted)
    // -----------------------------------------------------------------
    fn statement(&mut self) -> String {
        // Weights roughly reflect the interestingness of each kind to
        // the planner/executor. SELECT dominates because that's where
        // the metamorphic bugs live.
        match self.rng.next_u64() % 20 {
            0..=8 => self.select(),
            9 => self.select_join(),
            10 => self.select_groupby(),
            11 => self.select_union(),
            12..=13 => self.insert(),
            14..=15 => self.update(),
            16..=17 => self.delete(),
            _ => self.create_table(),
        }
    }

    // -----------------------------------------------------------------
    // CREATE TABLE
    // -----------------------------------------------------------------
    fn create_table(&mut self) -> String {
        let t = self.pick_str(TABLE_POOL);
        // Pick 2-4 distinct column names for this table.
        let ncols = (self.rng.next_u64() % 3) as usize + 2;
        let mut cols: Vec<&str> = COLUMN_POOL.to_vec();
        self.rng.shuffle(&mut cols);
        let mut col_defs = Vec::new();
        // First column is always the primary key as BIGINT.
        let pk = cols[0];
        col_defs.push(format!("{pk} BIGINT NOT NULL"));
        for c in cols.iter().skip(1).take(ncols - 1) {
            let ty = self.pick_str(INT_TYPES);
            col_defs.push(format!("{c} {ty}"));
        }
        format!(
            "CREATE TABLE {t} ({}, PRIMARY KEY ({pk}))",
            col_defs.join(", ")
        )
    }

    // -----------------------------------------------------------------
    // INSERT
    // -----------------------------------------------------------------
    fn insert(&mut self) -> String {
        let t = self.pick_str(TABLE_POOL);
        let col_count = (self.rng.next_u64() % 3) as usize + 1;
        let mut cols: Vec<&str> = COLUMN_POOL.to_vec();
        self.rng.shuffle(&mut cols);
        let picked: &[&str] = &cols[..col_count];
        let vals: Vec<String> = (0..col_count).map(|_| self.value_any()).collect();
        format!(
            "INSERT INTO {t} ({}) VALUES ({})",
            picked.join(", "),
            vals.join(", ")
        )
    }

    // -----------------------------------------------------------------
    // UPDATE
    // -----------------------------------------------------------------
    fn update(&mut self) -> String {
        let t = self.pick_str(TABLE_POOL);
        let c = self.pick_str(COLUMN_POOL);
        let v = self.value_any();
        let pred = self.simple_predicate();
        format!("UPDATE {t} SET {c} = {v} WHERE {pred}")
    }

    // -----------------------------------------------------------------
    // DELETE
    // -----------------------------------------------------------------
    fn delete(&mut self) -> String {
        let t = self.pick_str(TABLE_POOL);
        let pred = self.simple_predicate();
        format!("DELETE FROM {t} WHERE {pred}")
    }

    // -----------------------------------------------------------------
    // SELECT (plain)
    // -----------------------------------------------------------------
    fn select(&mut self) -> String {
        let t = self.pick_str(TABLE_POOL);
        let distinct = if self.rng.next_bool_with_probability(0.15) {
            "DISTINCT "
        } else {
            ""
        };
        let proj = self.projection();
        let mut sql = format!("SELECT {distinct}{proj} FROM {t}");
        if self.rng.next_bool_with_probability(0.7) {
            sql.push_str(&format!(" WHERE {}", self.simple_predicate()));
        }
        if self.rng.next_bool_with_probability(0.3) {
            let c = self.pick_str(COLUMN_POOL);
            let dir = if self.rng.next_bool() { "ASC" } else { "DESC" };
            sql.push_str(&format!(" ORDER BY {c} {dir}"));
        }
        if self.rng.next_bool_with_probability(0.3) {
            let n = (self.rng.next_u64() % 20) + 1;
            sql.push_str(&format!(" LIMIT {n}"));
        }
        sql
    }

    fn projection(&mut self) -> String {
        if self.rng.next_bool_with_probability(0.2) {
            return "*".into();
        }
        let count = (self.rng.next_u64() % 3) as usize + 1;
        let mut cols: Vec<&str> = COLUMN_POOL.to_vec();
        self.rng.shuffle(&mut cols);
        cols[..count].join(", ")
    }

    // -----------------------------------------------------------------
    // SELECT with JOIN
    // -----------------------------------------------------------------
    fn select_join(&mut self) -> String {
        let t1 = self.pick_str(TABLE_POOL);
        let mut t2 = self.pick_str(TABLE_POOL);
        if t1 == t2 {
            // ensure a second distinct table so the join is not on itself
            t2 = "t2";
        }
        let c = self.pick_str(COLUMN_POOL);
        let kind = match self.rng.next_u64() % 3 {
            0 => "INNER JOIN",
            1 => "LEFT JOIN",
            _ => "JOIN",
        };
        let mut sql = format!("SELECT * FROM {t1} {kind} {t2} ON {t1}.{c} = {t2}.{c}");
        if self.rng.next_bool_with_probability(0.5) {
            sql.push_str(&format!(" WHERE {}", self.simple_predicate()));
        }
        sql
    }

    // -----------------------------------------------------------------
    // SELECT with GROUP BY / HAVING
    // -----------------------------------------------------------------
    fn select_groupby(&mut self) -> String {
        let t = self.pick_str(TABLE_POOL);
        let group_col = self.pick_str(COLUMN_POOL);
        let agg = self.pick_str(AGG_FNS);
        let agg_col = if agg == "COUNT" {
            "*"
        } else {
            self.pick_str(COLUMN_POOL)
        };
        let mut sql =
            format!("SELECT {group_col}, {agg}({agg_col}) FROM {t} GROUP BY {group_col}");
        if self.rng.next_bool_with_probability(0.4) {
            sql.push_str(&format!(" HAVING {agg}({agg_col}) > {}", self.int_lit()));
        }
        sql
    }

    // -----------------------------------------------------------------
    // SELECT ... UNION SELECT ...
    // -----------------------------------------------------------------
    fn select_union(&mut self) -> String {
        let t1 = self.pick_str(TABLE_POOL);
        let t2 = self.pick_str(TABLE_POOL);
        let c = self.pick_str(COLUMN_POOL);
        let modifier = if self.rng.next_bool_with_probability(0.5) {
            "UNION ALL"
        } else {
            "UNION"
        };
        format!("SELECT {c} FROM {t1} {modifier} SELECT {c} FROM {t2}")
    }

    // -----------------------------------------------------------------
    // Predicates — schema-free (used by statements above)
    // -----------------------------------------------------------------
    fn simple_predicate(&mut self) -> String {
        // NB: `NOT (<binop>)` is not yet supported by the planner, so
        // the grammar deliberately avoids unary-NOT wrapping. AND/OR
        // composition + IS NULL cover the negation space well enough.
        if self.depth >= MAX_DEPTH {
            return self.leaf_predicate();
        }
        self.depth += 1;
        let p = match self.rng.next_u64() % 12 {
            0..=7 => self.leaf_predicate(),
            8 | 9 => {
                let op = self.pick_str(BOOL_OPS);
                let l = self.simple_predicate();
                let r = self.simple_predicate();
                format!("({l}) {op} ({r})")
            }
            10 => {
                let c = self.pick_str(COLUMN_POOL);
                format!("{c} IS NULL")
            }
            _ => {
                let c = self.pick_str(COLUMN_POOL);
                let lo = self.int_lit();
                let hi = self.int_lit();
                format!("{c} BETWEEN {lo} AND {hi}")
            }
        };
        self.depth -= 1;
        p
    }

    fn leaf_predicate(&mut self) -> String {
        let c = self.pick_str(COLUMN_POOL);
        let op = self.pick_str(CMP_OPS);
        let v = self.int_lit();
        format!("{c} {op} {v}")
    }

    // -----------------------------------------------------------------
    // Predicates — schema-aware (used by generate_predicate)
    // -----------------------------------------------------------------
    fn predicate_against(&mut self, schema: &SeedSchema) -> String {
        // Same shape as `simple_predicate`: NOT-wrapping is excluded
        // because the planner does not accept `NOT (<binop>)`.
        if self.depth >= MAX_DEPTH {
            return self.leaf_predicate_against(schema);
        }
        self.depth += 1;
        let p = match self.rng.next_u64() % 12 {
            0..=7 => self.leaf_predicate_against(schema),
            8 | 9 => {
                let op = self.pick_str(BOOL_OPS);
                let l = self.predicate_against(schema);
                let r = self.predicate_against(schema);
                format!("({l}) {op} ({r})")
            }
            10 => {
                let col = self.rng.choose(&schema.columns).clone();
                format!("{} IS NULL", col.name)
            }
            _ => {
                let col = self.rng.choose(&schema.columns).clone();
                if matches!(col.ty, ColumnType::BigInt | ColumnType::Integer) {
                    let lo = self.int_lit();
                    let hi = self.int_lit();
                    format!("{} BETWEEN {lo} AND {hi}", col.name)
                } else {
                    // fall back to leaf for non-numeric columns
                    self.leaf_predicate_against(schema)
                }
            }
        };
        self.depth -= 1;
        p
    }

    fn leaf_predicate_against(&mut self, schema: &SeedSchema) -> String {
        let col = self.rng.choose(&schema.columns).clone();
        let op = self.pick_str(CMP_OPS);
        // Avoid `col <op> NULL` — the planner rejects comparisons with
        // NULL literals. The `IS NULL` branch covers the nullable case.
        let v = loop {
            let v = self.typed_literal(col.ty);
            if v != "NULL" {
                break v;
            }
        };
        format!("{} {op} {v}", col.name)
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_query::parse_statement;

    #[test]
    fn same_seed_same_output() {
        for seed in [0u64, 1, 42, 1_000_000, u64::MAX] {
            assert_eq!(generate(seed), generate(seed));
        }
    }

    #[test]
    fn different_seeds_usually_differ() {
        // Not a strict invariant (tiny state collisions are possible),
        // but across 64 consecutive seeds we should see variety.
        let outputs: std::collections::HashSet<_> = (0u64..64).map(generate).collect();
        assert!(
            outputs.len() > 32,
            "expected varied output across 64 seeds, got {} uniques",
            outputs.len()
        );
    }

    #[test]
    fn generated_sql_parses() {
        // The grammar is only useful if the parser accepts its output.
        // Scan 1000 seeds; allow a small fraction to reject because
        // a few productions produce legal-looking SQL whose planner
        // rejects (e.g., UNION branches with different arities). The
        // grammar's job is to push past the tokenizer, not to
        // guarantee semantic validity.
        let mut parsed = 0;
        let mut parse_failures = 0;
        for seed in 0u64..1000 {
            let sql = generate(seed);
            match parse_statement(&sql) {
                Ok(_) => parsed += 1,
                Err(_) => parse_failures += 1,
            }
        }
        assert!(
            parsed >= 900,
            "expected ≥90% of grammar output to parse; got {parsed} parsed / {parse_failures} failed"
        );
    }

    #[test]
    fn depth_is_bounded() {
        // Even with an adversarial seed, the generator must not
        // recurse past MAX_DEPTH.
        for seed in 0u64..100 {
            let sql = generate(seed);
            // Count nested parens as a proxy for predicate depth.
            let mut max_nesting = 0i32;
            let mut cur = 0i32;
            for ch in sql.chars() {
                match ch {
                    '(' => {
                        cur += 1;
                        max_nesting = max_nesting.max(cur);
                    }
                    ')' => cur -= 1,
                    _ => {}
                }
            }
            // MAX_DEPTH bounds predicate recursion; structural parens
            // from projection/FROM add ~3 constant slack.
            assert!(
                max_nesting <= MAX_DEPTH as i32 + 4,
                "seed {seed}: max nesting {max_nesting} exceeds bound"
            );
        }
    }

    #[test]
    fn predicate_against_parses_in_where_clause() {
        let schema = SeedSchema::numeric_trio("t");
        let mut parsed = 0;
        for seed in 0u64..200 {
            let pred = generate_predicate(seed, &schema);
            let sql = format!("SELECT * FROM t WHERE {pred}");
            if parse_statement(&sql).is_ok() {
                parsed += 1;
            }
        }
        assert!(
            parsed >= 180,
            "expected ≥90% of schema-aware predicates to parse; got {parsed}/200"
        );
    }

    #[test]
    fn predicate_against_uses_schema_columns() {
        let schema = SeedSchema::numeric_trio("t");
        let pred = generate_predicate(12345, &schema);
        // Must mention at least one real column, not a stray from the
        // general COLUMN_POOL.
        let has_real_col = schema.columns.iter().any(|c| pred.contains(&c.name));
        assert!(
            has_real_col,
            "predicate {pred:?} did not reference any schema column"
        );
    }
}
