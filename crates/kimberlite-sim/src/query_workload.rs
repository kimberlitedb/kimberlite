//! Query workload generator for simulation testing.
//!
//! Generates random but valid SQL queries to exercise the query engine
//! under deterministic simulation.

use crate::rng::SimRng;

// ============================================================================
// Schema Definition
// ============================================================================

/// A simple table schema for workload generation.
#[derive(Debug, Clone)]
pub struct TableSchema {
    /// Table name.
    pub name: String,
    /// Column definitions: (name, type).
    pub columns: Vec<(String, String)>,
    /// Primary key column names.
    pub primary_key: Vec<String>,
}

impl TableSchema {
    /// Creates a new table schema.
    pub fn new(name: &str, columns: Vec<(&str, &str)>, primary_key: Vec<&str>) -> Self {
        Self {
            name: name.to_string(),
            columns: columns
                .into_iter()
                .map(|(n, t)| (n.to_string(), t.to_string()))
                .collect(),
            primary_key: primary_key.into_iter().map(String::from).collect(),
        }
    }

    /// Returns a column name by index.
    pub fn column_name(&self, index: usize) -> Option<&str> {
        self.columns.get(index).map(|(n, _)| n.as_str())
    }

    /// Returns a column type by name.
    pub fn column_type(&self, name: &str) -> Option<&str> {
        self.columns
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, t)| t.as_str())
    }

    /// Returns the number of columns.
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }
}

// ============================================================================
// Query Workload Generator
// ============================================================================

/// Generates random but valid SQL queries for testing.
///
/// Queries are deterministic based on the RNG seed, enabling
/// reproducible test runs.
#[derive(Debug)]
pub struct QueryWorkloadGenerator {
    /// Deterministic RNG.
    rng: SimRng,
    /// Table schemas available for query generation.
    schemas: Vec<TableSchema>,
    /// Counter for generating unique IDs.
    id_counter: u64,
}

impl QueryWorkloadGenerator {
    /// Creates a new workload generator with the given seed.
    pub fn new(seed: u64) -> Self {
        Self {
            rng: SimRng::new(seed),
            schemas: Vec::new(),
            id_counter: 0,
        }
    }

    /// Adds a table schema to the generator.
    pub fn add_schema(&mut self, schema: TableSchema) {
        self.schemas.push(schema);
    }

    /// Generates a random INSERT statement.
    pub fn generate_insert(&mut self) -> Option<String> {
        if self.schemas.is_empty() {
            return None;
        }

        // Clone schema to avoid borrow checker issues
        let schema = self.schemas[self.rng.next_usize(self.schemas.len())].clone();
        let id = self.next_id();

        let mut values = Vec::new();
        for (col_name, col_type) in &schema.columns {
            let value = if schema.primary_key.contains(&col_name) {
                // Use sequential IDs for primary keys
                id.to_string()
            } else {
                self.generate_value(&col_type)
            };
            values.push(value);
        }

        Some(format!(
            "INSERT INTO {} VALUES ({})",
            schema.name,
            values.join(", ")
        ))
    }

    /// Generates a random SELECT statement.
    pub fn generate_select(&mut self) -> Option<String> {
        if self.schemas.is_empty() {
            return None;
        }

        // Clone schema to avoid borrow checker issues
        let schema = self.schemas[self.rng.next_usize(self.schemas.len())].clone();

        // Random projection: * or specific columns
        let projection = if self.rng.next_bool() {
            "*".to_string()
        } else {
            // Select 1-3 columns
            let max_cols = 3.min(schema.column_count());
            let count = if max_cols == 1 {
                1
            } else {
                self.rng.next_u64_range(1, max_cols as u64 + 1) as usize
            };
            let mut cols = Vec::new();
            for _ in 0..count {
                let col = schema.column_name(self.rng.next_usize(schema.column_count()))?;
                if !cols.contains(&col) {
                    cols.push(col);
                }
            }
            cols.join(", ")
        };

        // Random WHERE clause (50% chance)
        let where_clause = if self.rng.next_bool() {
            let col_idx = self.rng.next_usize(schema.column_count());
            let (col_name, col_type) = &schema.columns[col_idx];
            let value = self.generate_value(col_type);
            format!(" WHERE {} = {}", col_name, value)
        } else {
            String::new()
        };

        // Random ORDER BY (30% chance)
        let order_by_clause = if self.rng.next_u64_range(0, 10) < 3 {
            let col = schema.column_name(self.rng.next_usize(schema.column_count()))?;
            let direction = if self.rng.next_bool() { "ASC" } else { "DESC" };
            format!(" ORDER BY {} {}", col, direction)
        } else {
            String::new()
        };

        // Random LIMIT (20% chance)
        let limit_clause = if self.rng.next_u64_range(0, 10) < 2 {
            let limit = self.rng.next_u64_range(1, 20);
            format!(" LIMIT {}", limit)
        } else {
            String::new()
        };

        Some(format!(
            "SELECT {} FROM {}{}{}{}",
            projection, schema.name, where_clause, order_by_clause, limit_clause
        ))
    }

    /// Generates a random UPDATE statement.
    pub fn generate_update(&mut self) -> Option<String> {
        if self.schemas.is_empty() {
            return None;
        }

        // Clone schema to avoid borrow checker issues
        let schema = self.schemas[self.rng.next_usize(self.schemas.len())].clone();

        // Pick a non-primary-key column to update
        let non_pk_cols: Vec<_> = schema
            .columns
            .iter()
            .filter(|(name, _)| !schema.primary_key.contains(name))
            .collect();

        if non_pk_cols.is_empty() {
            return None;
        }

        let (col_name, col_type) = non_pk_cols[self.rng.next_usize(non_pk_cols.len())];
        let new_value = self.generate_value(col_type);

        // WHERE clause on primary key
        let pk_col = &schema.primary_key[0];
        let max_id = (self.id_counter + 1).max(10);
        let pk_value = if max_id == 1 {
            1
        } else {
            self.rng.next_u64_range(1, max_id)
        };

        Some(format!(
            "UPDATE {} SET {} = {} WHERE {} = {}",
            schema.name, col_name, new_value, pk_col, pk_value
        ))
    }

    /// Generates a random DELETE statement.
    pub fn generate_delete(&mut self) -> Option<String> {
        if self.schemas.is_empty() {
            return None;
        }

        // Clone schema to avoid borrow checker issues
        let schema = self.schemas[self.rng.next_usize(self.schemas.len())].clone();

        // WHERE clause on primary key (50% chance of specific ID, 50% range)
        let max_id = (self.id_counter + 1).max(10);
        let where_clause = if self.rng.next_bool() {
            let pk_col = &schema.primary_key[0];
            let pk_value = if max_id == 1 {
                1
            } else {
                self.rng.next_u64_range(1, max_id)
            };
            format!(" WHERE {} = {}", pk_col, pk_value)
        } else {
            let pk_col = &schema.primary_key[0];
            let threshold = if max_id == 1 {
                1
            } else {
                self.rng.next_u64_range(1, max_id)
            };
            format!(" WHERE {} < {}", pk_col, threshold)
        };

        Some(format!("DELETE FROM {}{}", schema.name, where_clause))
    }

    /// Generates a random aggregate query.
    pub fn generate_aggregate(&mut self) -> Option<String> {
        if self.schemas.is_empty() {
            return None;
        }

        // Clone schema to avoid borrow checker issues
        let schema = self.schemas[self.rng.next_usize(self.schemas.len())].clone();

        // Pick an aggregate function
        let agg_func = match self.rng.next_u64_range(0, 4) {
            0 => "COUNT(*)".to_string(),
            1 => {
                let col = schema.column_name(self.rng.next_usize(schema.column_count()))?;
                format!("COUNT({})", col)
            }
            2 => {
                // SUM on numeric column
                let numeric_cols: Vec<_> = schema
                    .columns
                    .iter()
                    .filter(|(_, t)| {
                        matches!(
                            t.as_str(),
                            "TINYINT" | "SMALLINT" | "INTEGER" | "BIGINT" | "REAL" | "DECIMAL"
                        )
                    })
                    .collect();
                if numeric_cols.is_empty() {
                    return self.generate_aggregate();
                }
                let (col, _) = numeric_cols[self.rng.next_usize(numeric_cols.len())];
                format!("SUM({})", col)
            }
            _ => {
                let col = schema.column_name(self.rng.next_usize(schema.column_count()))?;
                if self.rng.next_bool() {
                    format!("MIN({})", col)
                } else {
                    format!("MAX({})", col)
                }
            }
        };

        // Optional WHERE clause
        let where_clause = if self.rng.next_bool() {
            let col_idx = self.rng.next_usize(schema.column_count());
            let (col_name, col_type) = &schema.columns[col_idx];
            let value = self.generate_value(col_type);
            format!(" WHERE {} > {}", col_name, value)
        } else {
            String::new()
        };

        Some(format!(
            "SELECT {} FROM {}{}",
            agg_func, schema.name, where_clause
        ))
    }

    /// Generates a mixed workload of queries.
    pub fn generate_mixed_workload(&mut self, count: usize) -> Vec<String> {
        let mut queries = Vec::new();

        for _ in 0..count {
            let query = match self.rng.next_u64_range(0, 5) {
                0 => self.generate_insert(),
                1 => self.generate_select(),
                2 => self.generate_update(),
                3 => self.generate_delete(),
                _ => self.generate_aggregate(),
            };

            if let Some(q) = query {
                queries.push(q);
            }
        }

        queries
    }

    /// Generates a value of the specified type.
    fn generate_value(&mut self, col_type: &str) -> String {
        match col_type {
            "TINYINT" => {
                let val = self.rng.next_u64_range(0, 256) as i16 - 128;
                val.to_string()
            }
            "SMALLINT" => {
                let val = self.rng.next_u64_range(0, 65536) as i32 - 32768;
                val.to_string()
            }
            "INTEGER" => {
                let val = self.rng.next_u64_range(0, 2000) as i32 - 1000;
                val.to_string()
            }
            "BIGINT" => {
                let val = self.rng.next_u64_range(0, 20000) as i64 - 10000;
                val.to_string()
            }
            "REAL" => {
                let val = (self.rng.next_u64_range(0, 2000) as i64 - 1000) as f64 / 10.0;
                val.to_string()
            }
            "DECIMAL" => {
                let val = self.rng.next_u64_range(0, 2000) as i32 - 1000;
                format!("{}.{:02}", val / 100, (val % 100).abs())
            }
            "TEXT" => {
                // Generate random strings
                let words = ["Alice", "Bob", "Charlie", "Diana", "Eve", "Frank"];
                format!("'{}'", words[self.rng.next_usize(words.len())])
            }
            "BOOLEAN" => if self.rng.next_bool() {
                "TRUE"
            } else {
                "FALSE"
            }
            .to_string(),
            "DATE" => "'2024-01-01'".to_string(),
            "TIME" => "'12:00:00'".to_string(),
            "TIMESTAMP" => "'2024-01-01 12:00:00'".to_string(),
            "UUID" => "'550e8400-e29b-41d4-a716-446655440000'".to_string(),
            "JSON" => "'{\"key\": \"value\"}'".to_string(),
            _ => "NULL".to_string(),
        }
    }

    /// Returns the next unique ID.
    fn next_id(&mut self) -> u64 {
        let id = self.id_counter;
        self.id_counter += 1;
        id
    }

    /// Returns the current ID counter (number of INSERTs generated).
    pub fn id_counter(&self) -> u64 {
        self.id_counter
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_insert_basic() {
        let mut generator = QueryWorkloadGenerator::new(42);
        generator.add_schema(TableSchema::new(
            "users",
            vec![("id", "BIGINT"), ("name", "TEXT"), ("age", "INTEGER")],
            vec!["id"],
        ));

        let insert = generator.generate_insert().expect("should generate INSERT");
        assert!(insert.starts_with("INSERT INTO users VALUES ("));
        assert!(insert.contains("0")); // First ID is 0
    }

    #[test]
    fn generate_select_basic() {
        let mut generator = QueryWorkloadGenerator::new(42);
        generator.add_schema(TableSchema::new(
            "users",
            vec![("id", "BIGINT"), ("name", "TEXT")],
            vec!["id"],
        ));

        let select = generator.generate_select().expect("should generate SELECT");
        assert!(select.starts_with("SELECT"));
        assert!(select.contains("FROM users"));
    }

    #[test]
    fn generate_update_basic() {
        let mut generator = QueryWorkloadGenerator::new(42);
        generator.add_schema(TableSchema::new(
            "users",
            vec![("id", "BIGINT"), ("name", "TEXT"), ("age", "INTEGER")],
            vec!["id"],
        ));

        let update = generator.generate_update().expect("should generate UPDATE");
        assert!(update.starts_with("UPDATE users SET"));
        assert!(update.contains("WHERE id ="));
    }

    #[test]
    fn generate_delete_basic() {
        let mut generator = QueryWorkloadGenerator::new(42);
        generator.add_schema(TableSchema::new(
            "users",
            vec![("id", "BIGINT"), ("name", "TEXT")],
            vec!["id"],
        ));

        let delete = generator.generate_delete().expect("should generate DELETE");
        assert!(delete.starts_with("DELETE FROM users"));
        assert!(delete.contains("WHERE"));
    }

    #[test]
    fn generate_aggregate_basic() {
        let mut generator = QueryWorkloadGenerator::new(42);
        generator.add_schema(TableSchema::new(
            "users",
            vec![("id", "BIGINT"), ("age", "INTEGER")],
            vec!["id"],
        ));

        let agg = generator
            .generate_aggregate()
            .expect("should generate aggregate");
        assert!(agg.starts_with("SELECT"));
        assert!(agg.contains("FROM users"));
        assert!(
            agg.contains("COUNT")
                || agg.contains("SUM")
                || agg.contains("MIN")
                || agg.contains("MAX")
        );
    }

    #[test]
    fn generate_mixed_workload() {
        let mut generator = QueryWorkloadGenerator::new(123);
        generator.add_schema(TableSchema::new(
            "users",
            vec![("id", "BIGINT"), ("name", "TEXT"), ("age", "INTEGER")],
            vec!["id"],
        ));

        let queries = generator.generate_mixed_workload(20);
        assert_eq!(queries.len(), 20);

        // Should have a mix of different query types
        let has_insert = queries.iter().any(|q| q.starts_with("INSERT"));
        let has_select = queries.iter().any(|q| q.starts_with("SELECT"));

        assert!(has_insert, "should have INSERT queries");
        assert!(has_select, "should have SELECT queries");
    }

    #[test]
    fn deterministic_generation() {
        let mut generator1 = QueryWorkloadGenerator::new(999);
        generator1.add_schema(TableSchema::new(
            "users",
            vec![("id", "BIGINT"), ("name", "TEXT")],
            vec!["id"],
        ));

        let mut generator2 = QueryWorkloadGenerator::new(999);
        generator2.add_schema(TableSchema::new(
            "users",
            vec![("id", "BIGINT"), ("name", "TEXT")],
            vec!["id"],
        ));

        let queries1 = generator1.generate_mixed_workload(10);
        let queries2 = generator2.generate_mixed_workload(10);

        // Same seed → same queries
        assert_eq!(queries1, queries2);
    }

    #[test]
    fn no_schema_returns_none() {
        let mut generator = QueryWorkloadGenerator::new(42);

        assert!(generator.generate_insert().is_none());
        assert!(generator.generate_select().is_none());
        assert!(generator.generate_update().is_none());
        assert!(generator.generate_delete().is_none());
    }

    #[test]
    fn id_counter_increments() {
        let mut generator = QueryWorkloadGenerator::new(42);
        generator.add_schema(TableSchema::new(
            "users",
            vec![("id", "BIGINT"), ("name", "TEXT")],
            vec!["id"],
        ));

        assert_eq!(generator.id_counter(), 0);

        generator.generate_insert();
        assert_eq!(generator.id_counter(), 1);

        generator.generate_insert();
        assert_eq!(generator.id_counter(), 2);
    }
}
