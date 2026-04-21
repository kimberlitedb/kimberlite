//! Lightweight SQL query builder.
//!
//! Not an ORM — this is a thin string builder that emits SQL + bound
//! parameters. It exists so callers don't hand-concatenate strings and
//! leak SQL-injection vectors.
//!
//! # Example
//!
//! ```ignore
//! use kimberlite_client::{Query, Client};
//!
//! let (sql, params) = Query::from("patients")
//!     .select(&["id", "name", "dob"])
//!     .where_eq("tenant_id", 42.into())
//!     .where_eq("active", true.into())
//!     .order_by("name")
//!     .limit(100)
//!     .build();
//!
//! let result = client.query(&sql, &params)?;
//! ```

use std::fmt::Write as _;

use kimberlite_wire::QueryParam;

/// A fluent SQL SELECT / UPDATE / DELETE builder.
#[derive(Debug, Clone)]
pub struct Query {
    from: String,
    columns: Vec<String>,
    wheres: Vec<(String, QueryParam, Cmp)>,
    order_by: Option<String>,
    order_desc: bool,
    limit: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
enum Cmp {
    Eq,
    Lt,
    Gt,
    Le,
    Ge,
    Ne,
}

impl Cmp {
    fn sql(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Lt => "<",
            Self::Gt => ">",
            Self::Le => "<=",
            Self::Ge => ">=",
            Self::Ne => "!=",
        }
    }
}

impl Query {
    /// Start a query over `table`.
    pub fn from(table: impl Into<String>) -> Self {
        Self {
            from: table.into(),
            columns: Vec::new(),
            wheres: Vec::new(),
            order_by: None,
            order_desc: false,
            limit: None,
        }
    }

    /// Select specific columns. When omitted, the builder emits `SELECT *`.
    pub fn select(mut self, columns: &[&str]) -> Self {
        self.columns = columns.iter().map(|c| (*c).to_string()).collect();
        self
    }

    /// Add an equality predicate: `WHERE column = $n`.
    pub fn where_eq(mut self, column: impl Into<String>, value: QueryParam) -> Self {
        self.wheres.push((column.into(), value, Cmp::Eq));
        self
    }

    pub fn where_lt(mut self, column: impl Into<String>, value: QueryParam) -> Self {
        self.wheres.push((column.into(), value, Cmp::Lt));
        self
    }

    pub fn where_gt(mut self, column: impl Into<String>, value: QueryParam) -> Self {
        self.wheres.push((column.into(), value, Cmp::Gt));
        self
    }

    pub fn where_le(mut self, column: impl Into<String>, value: QueryParam) -> Self {
        self.wheres.push((column.into(), value, Cmp::Le));
        self
    }

    pub fn where_ge(mut self, column: impl Into<String>, value: QueryParam) -> Self {
        self.wheres.push((column.into(), value, Cmp::Ge));
        self
    }

    pub fn where_ne(mut self, column: impl Into<String>, value: QueryParam) -> Self {
        self.wheres.push((column.into(), value, Cmp::Ne));
        self
    }

    pub fn order_by(mut self, column: impl Into<String>) -> Self {
        self.order_by = Some(column.into());
        self.order_desc = false;
        self
    }

    pub fn order_by_desc(mut self, column: impl Into<String>) -> Self {
        self.order_by = Some(column.into());
        self.order_desc = true;
        self
    }

    pub fn limit(mut self, n: u64) -> Self {
        self.limit = Some(n);
        self
    }

    /// Build the final SQL and positional parameter vector.
    pub fn build(self) -> (String, Vec<QueryParam>) {
        let mut sql = String::new();

        // SELECT <cols> FROM <table>
        sql.push_str("SELECT ");
        if self.columns.is_empty() {
            sql.push('*');
        } else {
            sql.push_str(&self.columns.join(", "));
        }
        sql.push_str(" FROM ");
        sql.push_str(&self.from);

        // WHERE clause
        let mut params = Vec::with_capacity(self.wheres.len());
        for (i, (col, value, cmp)) in self.wheres.into_iter().enumerate() {
            if i == 0 {
                sql.push_str(" WHERE ");
            } else {
                sql.push_str(" AND ");
            }
            sql.push_str(&col);
            sql.push(' ');
            sql.push_str(cmp.sql());
            let _ = write!(sql, " ${}", i + 1);
            params.push(value);
        }

        if let Some(col) = self.order_by {
            sql.push_str(" ORDER BY ");
            sql.push_str(&col);
            if self.order_desc {
                sql.push_str(" DESC");
            }
        }

        if let Some(n) = self.limit {
            let _ = write!(sql, " LIMIT {n}");
        }

        (sql, params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_star() {
        let (sql, params) = Query::from("patients").build();
        assert_eq!(sql, "SELECT * FROM patients");
        assert!(params.is_empty());
    }

    #[test]
    fn select_specific_columns() {
        let (sql, params) = Query::from("patients").select(&["id", "name"]).build();
        assert_eq!(sql, "SELECT id, name FROM patients");
        assert!(params.is_empty());
    }

    #[test]
    fn where_eq_single() {
        let (sql, params) = Query::from("patients")
            .where_eq("id", QueryParam::BigInt(42))
            .build();
        assert_eq!(sql, "SELECT * FROM patients WHERE id = $1");
        assert_eq!(params.len(), 1);
        assert!(matches!(params[0], QueryParam::BigInt(42)));
    }

    #[test]
    fn multi_predicate() {
        let (sql, _) = Query::from("patients")
            .where_eq("tenant_id", QueryParam::BigInt(1))
            .where_gt("age", QueryParam::BigInt(18))
            .build();
        assert_eq!(
            sql,
            "SELECT * FROM patients WHERE tenant_id = $1 AND age > $2"
        );
    }

    #[test]
    fn order_and_limit() {
        let (sql, _) = Query::from("patients").order_by("name").limit(10).build();
        assert_eq!(sql, "SELECT * FROM patients ORDER BY name LIMIT 10");
    }

    #[test]
    fn order_by_desc() {
        let (sql, _) = Query::from("events").order_by_desc("timestamp").build();
        assert_eq!(sql, "SELECT * FROM events ORDER BY timestamp DESC");
    }

    #[test]
    fn full_query() {
        let (sql, params) = Query::from("patients")
            .select(&["id", "name"])
            .where_eq("active", QueryParam::Boolean(true))
            .where_ne("status", QueryParam::Text("deleted".into()))
            .order_by_desc("created_at")
            .limit(50)
            .build();
        assert_eq!(
            sql,
            "SELECT id, name FROM patients WHERE active = $1 AND status != $2 ORDER BY created_at DESC LIMIT 50"
        );
        assert_eq!(params.len(), 2);
    }
}
