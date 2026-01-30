//! SQL parsing for the query engine.
//!
//! Wraps `sqlparser` to parse a minimal SQL subset:
//! - SELECT with column list or *
//! - FROM single table
//! - WHERE with comparison predicates
//! - ORDER BY
//! - LIMIT
//! - CREATE TABLE, DROP TABLE, CREATE INDEX (DDL)
//! - INSERT, UPDATE, DELETE (DML)

use sqlparser::ast::{
    BinaryOperator, ColumnDef as SqlColumnDef, DataType as SqlDataType, Expr, Ident, ObjectName,
    OrderByExpr, Query, Select, SelectItem, SetExpr, Statement, Value as SqlValue,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

use crate::error::{QueryError, Result};
use crate::schema::ColumnName;
use crate::value::Value;

// ============================================================================
// Parsed Statement Types
// ============================================================================

/// Top-level parsed SQL statement.
#[derive(Debug, Clone)]
pub enum ParsedStatement {
    /// SELECT query
    Select(ParsedSelect),
    /// CREATE TABLE DDL
    CreateTable(ParsedCreateTable),
    /// DROP TABLE DDL
    DropTable(String),
    /// CREATE INDEX DDL
    CreateIndex(ParsedCreateIndex),
    /// INSERT DML
    Insert(ParsedInsert),
    /// UPDATE DML
    Update(ParsedUpdate),
    /// DELETE DML
    Delete(ParsedDelete),
}

/// Parsed SELECT statement.
#[derive(Debug, Clone)]
pub struct ParsedSelect {
    /// Table name from FROM clause.
    pub table: String,
    /// Selected columns (None = SELECT *).
    pub columns: Option<Vec<ColumnName>>,
    /// WHERE predicates.
    pub predicates: Vec<Predicate>,
    /// ORDER BY clauses.
    pub order_by: Vec<OrderByClause>,
    /// LIMIT value.
    pub limit: Option<usize>,
}

/// Parsed CREATE TABLE statement.
#[derive(Debug, Clone)]
pub struct ParsedCreateTable {
    pub table_name: String,
    pub columns: Vec<ParsedColumn>,
    pub primary_key: Vec<String>,
}

/// Parsed column definition.
#[derive(Debug, Clone)]
pub struct ParsedColumn {
    pub name: String,
    pub data_type: String, // "BIGINT", "TEXT", "BOOLEAN", "TIMESTAMP", "BYTES"
    pub nullable: bool,
}

/// Parsed CREATE INDEX statement.
#[derive(Debug, Clone)]
pub struct ParsedCreateIndex {
    pub index_name: String,
    pub table_name: String,
    pub columns: Vec<String>,
}

/// Parsed INSERT statement.
#[derive(Debug, Clone)]
pub struct ParsedInsert {
    pub table: String,
    pub columns: Vec<String>,
    pub values: Vec<Value>,
}

/// Parsed UPDATE statement.
#[derive(Debug, Clone)]
pub struct ParsedUpdate {
    pub table: String,
    pub assignments: Vec<(String, Value)>, // column = value pairs
    pub predicates: Vec<Predicate>,
}

/// Parsed DELETE statement.
#[derive(Debug, Clone)]
pub struct ParsedDelete {
    pub table: String,
    pub predicates: Vec<Predicate>,
}

/// A comparison predicate from the WHERE clause.
#[derive(Debug, Clone)]
pub enum Predicate {
    /// column = value or column = $N
    Eq(ColumnName, PredicateValue),
    /// column < value
    Lt(ColumnName, PredicateValue),
    /// column <= value
    Le(ColumnName, PredicateValue),
    /// column > value
    Gt(ColumnName, PredicateValue),
    /// column >= value
    Ge(ColumnName, PredicateValue),
    /// column IN (value, value, ...)
    In(ColumnName, Vec<PredicateValue>),
}

impl Predicate {
    /// Returns the column name this predicate operates on.
    #[allow(dead_code)]
    pub fn column(&self) -> &ColumnName {
        match self {
            Predicate::Eq(col, _)
            | Predicate::Lt(col, _)
            | Predicate::Le(col, _)
            | Predicate::Gt(col, _)
            | Predicate::Ge(col, _)
            | Predicate::In(col, _) => col,
        }
    }
}

/// A value in a predicate (literal or parameter reference).
#[derive(Debug, Clone)]
pub enum PredicateValue {
    /// Literal integer.
    Int(i64),
    /// Literal string.
    String(String),
    /// Literal boolean.
    Bool(bool),
    /// NULL literal.
    Null,
    /// Parameter placeholder ($1, $2, etc.) - 1-indexed.
    Param(usize),
}

/// ORDER BY clause.
#[derive(Debug, Clone)]
pub struct OrderByClause {
    /// Column to order by.
    pub column: ColumnName,
    /// Ascending (true) or descending (false).
    pub ascending: bool,
}

// ============================================================================
// Parser
// ============================================================================

/// Parses a SQL statement string into a `ParsedStatement`.
pub fn parse_statement(sql: &str) -> Result<ParsedStatement> {
    let dialect = GenericDialect {};
    let statements =
        Parser::parse_sql(&dialect, sql).map_err(|e| QueryError::ParseError(e.to_string()))?;

    if statements.len() != 1 {
        return Err(QueryError::ParseError(format!(
            "expected exactly 1 statement, got {}",
            statements.len()
        )));
    }

    match &statements[0] {
        Statement::Query(query) => {
            let select = parse_select_query(query)?;
            Ok(ParsedStatement::Select(select))
        }
        Statement::CreateTable(create_table) => {
            let parsed = parse_create_table(create_table)?;
            Ok(ParsedStatement::CreateTable(parsed))
        }
        Statement::Drop {
            object_type,
            names,
            if_exists: _,
            ..
        } => {
            if !matches!(object_type, sqlparser::ast::ObjectType::Table) {
                return Err(QueryError::UnsupportedFeature(
                    "only DROP TABLE is supported".to_string(),
                ));
            }
            if names.len() != 1 {
                return Err(QueryError::ParseError(
                    "expected exactly 1 table in DROP TABLE".to_string(),
                ));
            }
            let table_name = object_name_to_string(&names[0]);
            Ok(ParsedStatement::DropTable(table_name))
        }
        Statement::CreateIndex(create_index) => {
            let parsed = parse_create_index(create_index)?;
            Ok(ParsedStatement::CreateIndex(parsed))
        }
        Statement::Insert(insert) => {
            let parsed = parse_insert(insert)?;
            Ok(ParsedStatement::Insert(parsed))
        }
        Statement::Update { table, assignments, selection, .. } => {
            let parsed = parse_update(table, assignments, selection.as_ref())?;
            Ok(ParsedStatement::Update(parsed))
        }
        Statement::Delete(delete) => {
            let parsed = parse_delete_stmt(delete)?;
            Ok(ParsedStatement::Delete(parsed))
        }
        other => Err(QueryError::UnsupportedFeature(format!(
            "statement type not supported: {other:?}"
        ))),
    }
}

/// Legacy function for backward compatibility (queries only).
pub fn parse_query(sql: &str) -> Result<ParsedSelect> {
    match parse_statement(sql)? {
        ParsedStatement::Select(select) => Ok(select),
        _ => Err(QueryError::UnsupportedFeature(
            "only SELECT queries are supported in parse_query()".to_string(),
        )),
    }
}

fn parse_select_query(query: &Query) -> Result<ParsedSelect> {
    // Reject CTEs
    if query.with.is_some() {
        return Err(QueryError::UnsupportedFeature(
            "WITH clauses (CTEs) are not supported".to_string(),
        ));
    }

    let SetExpr::Select(select) = query.body.as_ref() else {
        return Err(QueryError::UnsupportedFeature(
            "only simple SELECT queries are supported".to_string(),
        ));
    };

    let parsed_select = parse_select(select)?;

    // Parse ORDER BY from query (not select)
    let order_by = match &query.order_by {
        Some(ob) => parse_order_by(ob)?,
        None => vec![],
    };

    // Parse LIMIT from query
    let limit = parse_limit(query.limit.as_ref())?;

    Ok(ParsedSelect {
        table: parsed_select.table,
        columns: parsed_select.columns,
        predicates: parsed_select.predicates,
        order_by,
        limit,
    })
}

fn parse_select(select: &Select) -> Result<ParsedSelect> {
    // Reject DISTINCT
    if select.distinct.is_some() {
        return Err(QueryError::UnsupportedFeature(
            "DISTINCT is not supported".to_string(),
        ));
    }

    // Parse FROM - must be exactly one table
    if select.from.len() != 1 {
        return Err(QueryError::ParseError(format!(
            "expected exactly 1 table in FROM clause, got {}",
            select.from.len()
        )));
    }

    let from = &select.from[0];

    // Reject JOINs
    if !from.joins.is_empty() {
        return Err(QueryError::UnsupportedFeature(
            "JOINs are not supported".to_string(),
        ));
    }

    let table = match &from.relation {
        sqlparser::ast::TableFactor::Table { name, .. } => object_name_to_string(name),
        other => {
            return Err(QueryError::UnsupportedFeature(format!(
                "unsupported FROM clause: {other:?}"
            )));
        }
    };

    // Parse SELECT columns
    let columns = parse_select_items(&select.projection)?;

    // Parse WHERE predicates
    let predicates = match &select.selection {
        Some(expr) => parse_where_expr(expr)?,
        None => vec![],
    };

    // Reject GROUP BY
    match &select.group_by {
        sqlparser::ast::GroupByExpr::Expressions(exprs, _) if !exprs.is_empty() => {
            return Err(QueryError::UnsupportedFeature(
                "GROUP BY is not supported".to_string(),
            ));
        }
        sqlparser::ast::GroupByExpr::All(_) => {
            return Err(QueryError::UnsupportedFeature(
                "GROUP BY ALL is not supported".to_string(),
            ));
        }
        sqlparser::ast::GroupByExpr::Expressions(_, _) => {}
    }

    // Reject HAVING
    if select.having.is_some() {
        return Err(QueryError::UnsupportedFeature(
            "HAVING is not supported".to_string(),
        ));
    }

    Ok(ParsedSelect {
        table,
        columns,
        predicates,
        order_by: vec![],
        limit: None,
    })
}

fn parse_select_items(items: &[SelectItem]) -> Result<Option<Vec<ColumnName>>> {
    let mut columns = Vec::new();

    for item in items {
        match item {
            SelectItem::Wildcard(_) => {
                // SELECT * - return None to indicate all columns
                return Ok(None);
            }
            SelectItem::UnnamedExpr(Expr::Identifier(ident)) => {
                columns.push(ColumnName::new(ident.value.clone()));
            }
            SelectItem::ExprWithAlias {
                expr: Expr::Identifier(ident),
                alias,
            } => {
                // For now, we ignore aliases and just use the column name
                let _ = alias;
                columns.push(ColumnName::new(ident.value.clone()));
            }
            other => {
                return Err(QueryError::UnsupportedFeature(format!(
                    "unsupported SELECT item: {other:?}"
                )));
            }
        }
    }

    Ok(Some(columns))
}

fn parse_where_expr(expr: &Expr) -> Result<Vec<Predicate>> {
    match expr {
        // AND combines multiple predicates
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            let mut predicates = parse_where_expr(left)?;
            predicates.extend(parse_where_expr(right)?);
            Ok(predicates)
        }

        // Comparison operators
        Expr::BinaryOp { left, op, right } => {
            let predicate = parse_comparison(left, op, right)?;
            Ok(vec![predicate])
        }

        // IN list
        Expr::InList {
            expr,
            list,
            negated,
        } => {
            if *negated {
                return Err(QueryError::UnsupportedFeature(
                    "NOT IN is not supported".to_string(),
                ));
            }

            let column = expr_to_column(expr)?;
            let values: Result<Vec<_>> = list.iter().map(expr_to_predicate_value).collect();
            Ok(vec![Predicate::In(column, values?)])
        }

        // Parenthesized expression
        Expr::Nested(inner) => parse_where_expr(inner),

        other => Err(QueryError::UnsupportedFeature(format!(
            "unsupported WHERE expression: {other:?}"
        ))),
    }
}

fn parse_comparison(left: &Expr, op: &BinaryOperator, right: &Expr) -> Result<Predicate> {
    let column = expr_to_column(left)?;
    let value = expr_to_predicate_value(right)?;

    match op {
        BinaryOperator::Eq => Ok(Predicate::Eq(column, value)),
        BinaryOperator::Lt => Ok(Predicate::Lt(column, value)),
        BinaryOperator::LtEq => Ok(Predicate::Le(column, value)),
        BinaryOperator::Gt => Ok(Predicate::Gt(column, value)),
        BinaryOperator::GtEq => Ok(Predicate::Ge(column, value)),
        other => Err(QueryError::UnsupportedFeature(format!(
            "unsupported operator: {other:?}"
        ))),
    }
}

fn expr_to_column(expr: &Expr) -> Result<ColumnName> {
    match expr {
        Expr::Identifier(ident) => Ok(ColumnName::new(ident.value.clone())),
        Expr::CompoundIdentifier(idents) if idents.len() == 2 => {
            // table.column - ignore table for now
            Ok(ColumnName::new(idents[1].value.clone()))
        }
        other => Err(QueryError::UnsupportedFeature(format!(
            "expected column name, got {other:?}"
        ))),
    }
}

fn expr_to_predicate_value(expr: &Expr) -> Result<PredicateValue> {
    match expr {
        Expr::Value(SqlValue::Number(n, _)) => {
            let v: i64 = n
                .parse()
                .map_err(|_| QueryError::ParseError(format!("invalid integer: {n}")))?;
            Ok(PredicateValue::Int(v))
        }
        Expr::Value(SqlValue::SingleQuotedString(s) | SqlValue::DoubleQuotedString(s)) => {
            Ok(PredicateValue::String(s.clone()))
        }
        Expr::Value(SqlValue::Boolean(b)) => Ok(PredicateValue::Bool(*b)),
        Expr::Value(SqlValue::Null) => Ok(PredicateValue::Null),
        Expr::Value(SqlValue::Placeholder(p)) => {
            // Parse $1, $2, etc.
            if let Some(num_str) = p.strip_prefix('$') {
                let idx: usize = num_str.parse().map_err(|_| {
                    QueryError::ParseError(format!("invalid parameter placeholder: {p}"))
                })?;
                Ok(PredicateValue::Param(idx))
            } else {
                Err(QueryError::ParseError(format!(
                    "unsupported placeholder format: {p}"
                )))
            }
        }
        Expr::UnaryOp {
            op: sqlparser::ast::UnaryOperator::Minus,
            expr,
        } => {
            // Handle negative numbers
            if let Expr::Value(SqlValue::Number(n, _)) = expr.as_ref() {
                let v: i64 = n
                    .parse::<i64>()
                    .map_err(|_| QueryError::ParseError(format!("invalid integer: -{n}")))?;
                Ok(PredicateValue::Int(-v))
            } else {
                Err(QueryError::UnsupportedFeature(format!(
                    "unsupported unary minus operand: {expr:?}"
                )))
            }
        }
        other => Err(QueryError::UnsupportedFeature(format!(
            "unsupported value expression: {other:?}"
        ))),
    }
}

fn parse_order_by(order_by: &sqlparser::ast::OrderBy) -> Result<Vec<OrderByClause>> {
    let mut clauses = Vec::new();

    for expr in &order_by.exprs {
        clauses.push(parse_order_by_expr(expr)?);
    }

    Ok(clauses)
}

fn parse_order_by_expr(expr: &OrderByExpr) -> Result<OrderByClause> {
    let column = match &expr.expr {
        Expr::Identifier(ident) => ColumnName::new(ident.value.clone()),
        other => {
            return Err(QueryError::UnsupportedFeature(format!(
                "unsupported ORDER BY expression: {other:?}"
            )));
        }
    };

    let ascending = expr.asc.unwrap_or(true);

    Ok(OrderByClause { column, ascending })
}

fn parse_limit(limit: Option<&Expr>) -> Result<Option<usize>> {
    match limit {
        None => Ok(None),
        Some(Expr::Value(SqlValue::Number(n, _))) => {
            let v: usize = n
                .parse()
                .map_err(|_| QueryError::ParseError(format!("invalid LIMIT value: {n}")))?;
            Ok(Some(v))
        }
        Some(other) => Err(QueryError::UnsupportedFeature(format!(
            "unsupported LIMIT expression: {other:?}"
        ))),
    }
}

fn object_name_to_string(name: &ObjectName) -> String {
    name.0
        .iter()
        .map(|i: &Ident| i.value.clone())
        .collect::<Vec<_>>()
        .join(".")
}

// ============================================================================
// DDL Parsers
// ============================================================================

fn parse_create_table(create_table: &sqlparser::ast::CreateTable) -> Result<ParsedCreateTable> {
    let table_name = object_name_to_string(&create_table.name);

    // Extract column definitions
    let mut columns = Vec::new();
    for col_def in &create_table.columns {
        let parsed_col = parse_column_def(col_def)?;
        columns.push(parsed_col);
    }

    // Extract primary key from constraints
    let mut primary_key = Vec::new();
    for constraint in &create_table.constraints {
        if let sqlparser::ast::TableConstraint::PrimaryKey { columns: pk_cols, .. } = constraint {
            for col in pk_cols {
                primary_key.push(col.value.clone());
            }
        }
    }

    // If no explicit PRIMARY KEY constraint, check for PRIMARY KEY in column definitions
    if primary_key.is_empty() {
        for col_def in &create_table.columns {
            for option in &col_def.options {
                if matches!(
                    &option.option,
                    sqlparser::ast::ColumnOption::Unique { is_primary, .. } if *is_primary
                ) {
                    primary_key.push(col_def.name.value.clone());
                }
            }
        }
    }

    Ok(ParsedCreateTable {
        table_name,
        columns,
        primary_key,
    })
}

fn parse_column_def(col_def: &SqlColumnDef) -> Result<ParsedColumn> {
    let name = col_def.name.value.clone();

    // Map SQL data type to string
    let data_type = match &col_def.data_type {
        SqlDataType::BigInt(_) => "BIGINT",
        SqlDataType::Int(_) | SqlDataType::Integer(_) => "BIGINT", // Normalize to BIGINT
        SqlDataType::Text | SqlDataType::Varchar(_) | SqlDataType::String(_) => "TEXT",
        SqlDataType::Boolean | SqlDataType::Bool => "BOOLEAN",
        SqlDataType::Timestamp(_, _) => "TIMESTAMP",
        SqlDataType::Binary(_) | SqlDataType::Varbinary(_) | SqlDataType::Blob(_) => "BYTES",
        other => {
            return Err(QueryError::UnsupportedFeature(format!(
                "unsupported data type: {other:?}"
            )))
        }
    }
    .to_string();

    // Check for NOT NULL constraint
    let mut nullable = true;
    for option in &col_def.options {
        if matches!(option.option, sqlparser::ast::ColumnOption::NotNull) {
            nullable = false;
        }
    }

    Ok(ParsedColumn {
        name,
        data_type,
        nullable,
    })
}

fn parse_create_index(create_index: &sqlparser::ast::CreateIndex) -> Result<ParsedCreateIndex> {
    let index_name = match &create_index.name {
        Some(name) => object_name_to_string(name),
        None => {
            return Err(QueryError::ParseError(
                "CREATE INDEX requires an index name".to_string(),
            ))
        }
    };

    let table_name = object_name_to_string(&create_index.table_name);

    let mut columns = Vec::new();
    for col in &create_index.columns {
        columns.push(col.expr.to_string());
    }

    Ok(ParsedCreateIndex {
        index_name,
        table_name,
        columns,
    })
}

// ============================================================================
// DML Parsers
// ============================================================================

fn parse_insert(insert: &sqlparser::ast::Insert) -> Result<ParsedInsert> {
    // TableObject might be ObjectName directly - convert to string
    let table = insert.table.to_string();

    // Extract column names
    let columns: Vec<String> = insert.columns.iter().map(|c| c.value.clone()).collect();

    // Extract values from the first row
    // For simplicity, we only support single-row inserts
    let values = match insert.source.as_ref().map(|s| s.body.as_ref()) {
        Some(SetExpr::Values(values)) => {
            if values.rows.len() != 1 {
                return Err(QueryError::UnsupportedFeature(
                    "only single-row INSERT is supported".to_string(),
                ));
            }
            let row = &values.rows[0];
            let mut parsed_values = Vec::new();
            for expr in row {
                let val = expr_to_value(expr)?;
                parsed_values.push(val);
            }
            parsed_values
        }
        _ => {
            return Err(QueryError::UnsupportedFeature(
                "only VALUES clause is supported in INSERT".to_string(),
            ))
        }
    };

    Ok(ParsedInsert {
        table,
        columns,
        values,
    })
}

fn parse_update(
    table: &sqlparser::ast::TableWithJoins,
    assignments: &[sqlparser::ast::Assignment],
    selection: Option<&Expr>,
) -> Result<ParsedUpdate> {
    let table_name = match &table.relation {
        sqlparser::ast::TableFactor::Table { name, .. } => object_name_to_string(name),
        other => {
            return Err(QueryError::UnsupportedFeature(format!(
                "unsupported table in UPDATE: {other:?}"
            )))
        }
    };

    // Parse assignments (SET clauses)
    let mut parsed_assignments = Vec::new();
    for assignment in assignments {
        let col_name = assignment.target.to_string();
        let value = expr_to_value(&assignment.value)?;
        parsed_assignments.push((col_name, value));
    }

    // Parse WHERE clause
    let predicates = match selection {
        Some(expr) => parse_where_expr(expr)?,
        None => vec![],
    };

    Ok(ParsedUpdate {
        table: table_name,
        assignments: parsed_assignments,
        predicates,
    })
}

fn parse_delete_stmt(delete: &sqlparser::ast::Delete) -> Result<ParsedDelete> {
    // In sqlparser 0.54, DELETE FROM uses a single `from` table
    use sqlparser::ast::FromTable;

    let table_name = match &delete.from {
        FromTable::WithFromKeyword(tables) => {
            if tables.len() != 1 {
                return Err(QueryError::ParseError(
                    "expected exactly 1 table in DELETE FROM".to_string(),
                ));
            }

            match &tables[0].relation {
                sqlparser::ast::TableFactor::Table { name, .. } => object_name_to_string(name),
                _ => {
                    return Err(QueryError::ParseError(
                        "DELETE only supports simple table names".to_string(),
                    ))
                }
            }
        }
        FromTable::WithoutKeyword(tables) => {
            if tables.len() != 1 {
                return Err(QueryError::ParseError(
                    "expected exactly 1 table in DELETE".to_string(),
                ));
            }

            match &tables[0].relation {
                sqlparser::ast::TableFactor::Table { name, .. } => object_name_to_string(name),
                _ => {
                    return Err(QueryError::ParseError(
                        "DELETE only supports simple table names".to_string(),
                    ))
                }
            }
        }
    };

    // Parse WHERE clause
    let predicates = match &delete.selection {
        Some(expr) => parse_where_expr(expr)?,
        None => vec![],
    };

    Ok(ParsedDelete {
        table: table_name,
        predicates,
    })
}

/// Converts a SQL expression to a Value.
fn expr_to_value(expr: &Expr) -> Result<Value> {
    match expr {
        Expr::Value(SqlValue::Number(n, _)) => {
            let v: i64 = n
                .parse()
                .map_err(|_| QueryError::ParseError(format!("invalid integer: {n}")))?;
            Ok(Value::BigInt(v))
        }
        Expr::Value(SqlValue::SingleQuotedString(s) | SqlValue::DoubleQuotedString(s)) => {
            Ok(Value::Text(s.clone()))
        }
        Expr::Value(SqlValue::Boolean(b)) => Ok(Value::Boolean(*b)),
        Expr::Value(SqlValue::Null) => Ok(Value::Null),
        Expr::Value(SqlValue::Placeholder(p)) => {
            // Parse $1, $2, etc.
            if let Some(num_str) = p.strip_prefix('$') {
                let idx: usize = num_str.parse().map_err(|_| {
                    QueryError::ParseError(format!("invalid parameter placeholder: {p}"))
                })?;
                Ok(Value::Placeholder(idx))
            } else {
                Err(QueryError::ParseError(format!(
                    "unsupported placeholder format: {p}"
                )))
            }
        }
        Expr::UnaryOp {
            op: sqlparser::ast::UnaryOperator::Minus,
            expr,
        } => {
            // Handle negative numbers
            if let Expr::Value(SqlValue::Number(n, _)) = expr.as_ref() {
                let v: i64 = n
                    .parse::<i64>()
                    .map_err(|_| QueryError::ParseError(format!("invalid integer: -{n}")))?;
                Ok(Value::BigInt(-v))
            } else {
                Err(QueryError::UnsupportedFeature(format!(
                    "unsupported unary minus operand: {expr:?}"
                )))
            }
        }
        other => Err(QueryError::UnsupportedFeature(format!(
            "unsupported value expression: {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_select() {
        let result = parse_query("SELECT id, name FROM users").unwrap();
        assert_eq!(result.table, "users");
        assert_eq!(
            result.columns,
            Some(vec![ColumnName::new("id"), ColumnName::new("name")])
        );
        assert!(result.predicates.is_empty());
    }

    #[test]
    fn test_parse_select_star() {
        let result = parse_query("SELECT * FROM users").unwrap();
        assert_eq!(result.table, "users");
        assert!(result.columns.is_none());
    }

    #[test]
    fn test_parse_where_eq() {
        let result = parse_query("SELECT * FROM users WHERE id = 42").unwrap();
        assert_eq!(result.predicates.len(), 1);
        match &result.predicates[0] {
            Predicate::Eq(col, PredicateValue::Int(42)) => {
                assert_eq!(col.as_str(), "id");
            }
            other => panic!("unexpected predicate: {other:?}"),
        }
    }

    #[test]
    fn test_parse_where_string() {
        let result = parse_query("SELECT * FROM users WHERE name = 'alice'").unwrap();
        match &result.predicates[0] {
            Predicate::Eq(col, PredicateValue::String(s)) => {
                assert_eq!(col.as_str(), "name");
                assert_eq!(s, "alice");
            }
            other => panic!("unexpected predicate: {other:?}"),
        }
    }

    #[test]
    fn test_parse_where_and() {
        let result = parse_query("SELECT * FROM users WHERE id = 1 AND name = 'bob'").unwrap();
        assert_eq!(result.predicates.len(), 2);
    }

    #[test]
    fn test_parse_where_in() {
        let result = parse_query("SELECT * FROM users WHERE id IN (1, 2, 3)").unwrap();
        match &result.predicates[0] {
            Predicate::In(col, values) => {
                assert_eq!(col.as_str(), "id");
                assert_eq!(values.len(), 3);
            }
            other => panic!("unexpected predicate: {other:?}"),
        }
    }

    #[test]
    fn test_parse_order_by() {
        let result = parse_query("SELECT * FROM users ORDER BY name ASC, id DESC").unwrap();
        assert_eq!(result.order_by.len(), 2);
        assert_eq!(result.order_by[0].column.as_str(), "name");
        assert!(result.order_by[0].ascending);
        assert_eq!(result.order_by[1].column.as_str(), "id");
        assert!(!result.order_by[1].ascending);
    }

    #[test]
    fn test_parse_limit() {
        let result = parse_query("SELECT * FROM users LIMIT 10").unwrap();
        assert_eq!(result.limit, Some(10));
    }

    #[test]
    fn test_parse_param() {
        let result = parse_query("SELECT * FROM users WHERE id = $1").unwrap();
        match &result.predicates[0] {
            Predicate::Eq(_, PredicateValue::Param(1)) => {}
            other => panic!("unexpected predicate: {other:?}"),
        }
    }

    #[test]
    fn test_reject_join() {
        let result = parse_query("SELECT * FROM users JOIN orders ON users.id = orders.user_id");
        assert!(result.is_err());
    }

    #[test]
    fn test_reject_subquery() {
        let result = parse_query("SELECT * FROM (SELECT * FROM users)");
        assert!(result.is_err());
    }
}
