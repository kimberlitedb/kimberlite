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
    /// UNION / UNION ALL of two or more SELECT queries
    Union(ParsedUnion),
    /// CREATE TABLE DDL
    CreateTable(ParsedCreateTable),
    /// DROP TABLE DDL
    DropTable(String),
    /// ALTER TABLE DDL
    AlterTable(ParsedAlterTable),
    /// CREATE INDEX DDL
    CreateIndex(ParsedCreateIndex),
    /// INSERT DML
    Insert(ParsedInsert),
    /// UPDATE DML
    Update(ParsedUpdate),
    /// DELETE DML
    Delete(ParsedDelete),
}

/// Parsed UNION / UNION ALL statement.
#[derive(Debug, Clone)]
pub struct ParsedUnion {
    /// Left side SELECT.
    pub left: ParsedSelect,
    /// Right side SELECT.
    pub right: ParsedSelect,
    /// Whether to keep duplicates (UNION ALL = true, UNION = false).
    pub all: bool,
}

/// Join type for multi-table queries.
#[derive(Debug, Clone)]
pub enum JoinType {
    /// INNER JOIN
    Inner,
    /// LEFT OUTER JOIN
    Left,
    // Right and Full can be added later
}

/// Parsed JOIN clause.
#[derive(Debug, Clone)]
pub struct ParsedJoin {
    /// Table name to join.
    pub table: String,
    /// Join type (INNER or LEFT).
    pub join_type: JoinType,
    /// ON condition predicates.
    pub on_condition: Vec<Predicate>,
}

/// Parsed SELECT statement.
#[derive(Debug, Clone)]
pub struct ParsedSelect {
    /// Table name from FROM clause.
    pub table: String,
    /// JOIN clauses.
    pub joins: Vec<ParsedJoin>,
    /// Selected columns (None = SELECT *).
    pub columns: Option<Vec<ColumnName>>,
    /// WHERE predicates.
    pub predicates: Vec<Predicate>,
    /// ORDER BY clauses.
    pub order_by: Vec<OrderByClause>,
    /// LIMIT value.
    pub limit: Option<usize>,
    /// Aggregate functions in SELECT clause.
    pub aggregates: Vec<AggregateFunction>,
    /// GROUP BY columns.
    pub group_by: Vec<ColumnName>,
    /// Whether DISTINCT is specified.
    pub distinct: bool,
    /// HAVING predicates (applied after GROUP BY aggregation).
    pub having: Vec<HavingCondition>,
}

/// A condition in the HAVING clause.
///
/// HAVING conditions reference aggregate results (e.g., `HAVING COUNT(*) > 5`).
#[derive(Debug, Clone)]
pub enum HavingCondition {
    /// Compare an aggregate function result to a literal value.
    AggregateComparison {
        /// The aggregate function being compared.
        aggregate: AggregateFunction,
        /// Comparison operator.
        op: HavingOp,
        /// Value to compare against.
        value: Value,
    },
}

/// Comparison operators for HAVING conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HavingOp {
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
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

/// Parsed ALTER TABLE statement.
#[derive(Debug, Clone)]
pub struct ParsedAlterTable {
    pub table_name: String,
    pub operation: AlterTableOperation,
}

/// ALTER TABLE operation.
#[derive(Debug, Clone)]
pub enum AlterTableOperation {
    /// ADD COLUMN
    AddColumn(ParsedColumn),
    /// DROP COLUMN
    DropColumn(String),
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
    pub values: Vec<Vec<Value>>,        // Each Vec<Value> is one row
    pub returning: Option<Vec<String>>, // Columns to return after insert
}

/// Parsed UPDATE statement.
#[derive(Debug, Clone)]
pub struct ParsedUpdate {
    pub table: String,
    pub assignments: Vec<(String, Value)>, // column = value pairs
    pub predicates: Vec<Predicate>,
    pub returning: Option<Vec<String>>, // Columns to return after update
}

/// Parsed DELETE statement.
#[derive(Debug, Clone)]
pub struct ParsedDelete {
    pub table: String,
    pub predicates: Vec<Predicate>,
    pub returning: Option<Vec<String>>, // Columns to return after delete
}

/// Aggregate function in SELECT clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregateFunction {
    /// COUNT(*) - count all rows
    CountStar,
    /// COUNT(column) - count non-NULL values in column
    Count(ColumnName),
    /// SUM(column) - sum values in column
    Sum(ColumnName),
    /// AVG(column) - average values in column
    Avg(ColumnName),
    /// MIN(column) - minimum value in column
    Min(ColumnName),
    /// MAX(column) - maximum value in column
    Max(ColumnName),
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
    /// column LIKE 'pattern'
    Like(ColumnName, String),
    /// column IS NULL
    IsNull(ColumnName),
    /// column IS NOT NULL
    IsNotNull(ColumnName),
    /// OR of multiple predicates
    Or(Vec<Predicate>, Vec<Predicate>),
}

impl Predicate {
    /// Returns the column name this predicate operates on.
    ///
    /// Returns None for OR predicates which may reference multiple columns.
    #[allow(dead_code)]
    pub fn column(&self) -> Option<&ColumnName> {
        match self {
            Predicate::Eq(col, _)
            | Predicate::Lt(col, _)
            | Predicate::Le(col, _)
            | Predicate::Gt(col, _)
            | Predicate::Ge(col, _)
            | Predicate::In(col, _)
            | Predicate::Like(col, _)
            | Predicate::IsNull(col)
            | Predicate::IsNotNull(col) => Some(col),
            Predicate::Or(_, _) => None,
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
    /// Literal value (for any type).
    Literal(Value),
    /// Column reference (for JOIN predicates): table.column or just column.
    /// Format: "table.column" or "column"
    ColumnRef(String),
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
        Statement::Query(query) => parse_query_to_statement(query),
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
        Statement::Update {
            table,
            assignments,
            selection,
            returning,
            ..
        } => {
            let parsed = parse_update(table, assignments, selection.as_ref(), returning.as_ref())?;
            Ok(ParsedStatement::Update(parsed))
        }
        Statement::Delete(delete) => {
            let parsed = parse_delete_stmt(delete)?;
            Ok(ParsedStatement::Delete(parsed))
        }
        Statement::AlterTable { name, operations, .. } => {
            let parsed = parse_alter_table(name, operations)?;
            Ok(ParsedStatement::AlterTable(parsed))
        }
        other => Err(QueryError::UnsupportedFeature(format!(
            "statement type not supported: {other:?}"
        ))),
    }
}

/// Parses a query, returning either a Select or Union statement.
fn parse_query_to_statement(query: &Query) -> Result<ParsedStatement> {
    // Reject CTEs
    if query.with.is_some() {
        return Err(QueryError::UnsupportedFeature(
            "WITH clauses (CTEs) are not supported".to_string(),
        ));
    }

    match query.body.as_ref() {
        SetExpr::Select(select) => {
            let parsed_select = parse_select(select)?;

            // Parse ORDER BY from query (not select)
            let order_by = match &query.order_by {
                Some(ob) => parse_order_by(ob)?,
                None => vec![],
            };

            // Parse LIMIT from query
            let limit = parse_limit(query.limit.as_ref())?;

            Ok(ParsedStatement::Select(ParsedSelect {
                table: parsed_select.table,
                joins: parsed_select.joins,
                columns: parsed_select.columns,
                predicates: parsed_select.predicates,
                order_by,
                limit,
                aggregates: parsed_select.aggregates,
                group_by: parsed_select.group_by,
                distinct: parsed_select.distinct,
                having: parsed_select.having,
            }))
        }
        SetExpr::SetOperation {
            op,
            set_quantifier,
            left,
            right,
        } => {
            use sqlparser::ast::SetOperator;
            use sqlparser::ast::SetQuantifier;

            if !matches!(op, SetOperator::Union) {
                return Err(QueryError::UnsupportedFeature(format!(
                    "set operation not supported: {op:?} (only UNION is supported)"
                )));
            }

            let all = matches!(set_quantifier, SetQuantifier::All);

            // Parse left and right as simple SELECTs
            let left_select = match left.as_ref() {
                SetExpr::Select(s) => parse_select(s)?,
                _ => {
                    return Err(QueryError::UnsupportedFeature(
                        "nested set operations not supported".to_string(),
                    ));
                }
            };
            let right_select = match right.as_ref() {
                SetExpr::Select(s) => parse_select(s)?,
                _ => {
                    return Err(QueryError::UnsupportedFeature(
                        "nested set operations not supported".to_string(),
                    ));
                }
            };

            Ok(ParsedStatement::Union(ParsedUnion {
                left: left_select,
                right: right_select,
                all,
            }))
        }
        other => Err(QueryError::UnsupportedFeature(format!(
            "unsupported query type: {other:?}"
        ))),
    }
}

/// Parses a JOIN clause from the AST.
fn parse_join(join: &sqlparser::ast::Join) -> Result<ParsedJoin> {
    use sqlparser::ast::{JoinConstraint, JoinOperator};

    // Extract join type
    let join_type = match &join.join_operator {
        JoinOperator::Inner(_) => JoinType::Inner,
        JoinOperator::LeftOuter(_) => JoinType::Left,
        other => {
            return Err(QueryError::UnsupportedFeature(format!(
                "join type not supported: {other:?}"
            )));
        }
    };

    // Extract table name
    let table = match &join.relation {
        sqlparser::ast::TableFactor::Table { name, .. } => object_name_to_string(name),
        _ => {
            return Err(QueryError::UnsupportedFeature(
                "subquery joins not supported".to_string(),
            ));
        }
    };

    // Extract ON condition
    let on_condition = match &join.join_operator {
        JoinOperator::Inner(JoinConstraint::On(expr))
        | JoinOperator::LeftOuter(JoinConstraint::On(expr)) => parse_join_condition(expr)?,
        JoinOperator::Inner(JoinConstraint::Using(_))
        | JoinOperator::LeftOuter(JoinConstraint::Using(_)) => {
            return Err(QueryError::UnsupportedFeature(
                "USING clause not supported".to_string(),
            ));
        }
        _ => {
            return Err(QueryError::UnsupportedFeature(
                "join without ON clause not supported".to_string(),
            ));
        }
    };

    Ok(ParsedJoin {
        table,
        join_type,
        on_condition,
    })
}

/// Parses a JOIN ON condition into a list of predicates.
/// Handles AND combinations: ON a.id = b.id AND a.status = 'active'
fn parse_join_condition(expr: &Expr) -> Result<Vec<Predicate>> {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            let mut predicates = parse_join_condition(left)?;
            predicates.extend(parse_join_condition(right)?);
            Ok(predicates)
        }
        _ => {
            // Single predicate - reuse existing WHERE parser logic
            parse_where_expr(expr)
        }
    }
}

fn parse_select(select: &Select) -> Result<ParsedSelect> {
    // Parse DISTINCT flag
    let distinct = select.distinct.is_some();

    // Parse FROM - must be exactly one table
    if select.from.len() != 1 {
        return Err(QueryError::ParseError(format!(
            "expected exactly 1 table in FROM clause, got {}",
            select.from.len()
        )));
    }

    let from = &select.from[0];

    // Parse JOINs
    let joins: Result<Vec<_>> = from.joins.iter().map(parse_join).collect();
    let joins = joins?;

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

    // Parse GROUP BY clause
    let group_by = match &select.group_by {
        sqlparser::ast::GroupByExpr::Expressions(exprs, _) if !exprs.is_empty() => {
            parse_group_by_expr(exprs)?
        }
        sqlparser::ast::GroupByExpr::All(_) => {
            return Err(QueryError::UnsupportedFeature(
                "GROUP BY ALL is not supported".to_string(),
            ));
        }
        sqlparser::ast::GroupByExpr::Expressions(_, _) => vec![],
    };

    // Parse aggregates from SELECT clause
    let aggregates = parse_aggregates_from_select_items(&select.projection)?;

    // Parse HAVING clause
    let having = match &select.having {
        Some(expr) => parse_having_expr(expr)?,
        None => vec![],
    };

    Ok(ParsedSelect {
        table,
        joins,
        columns,
        predicates,
        order_by: vec![],
        limit: None,
        aggregates,
        group_by,
        distinct,
        having,
    })
}

/// Parses a HAVING clause expression into conditions.
///
/// Supports: `HAVING aggregate_fn(col) op value` with AND combinations.
/// Example: `HAVING COUNT(*) > 5 AND SUM(amount) < 1000`
fn parse_having_expr(expr: &Expr) -> Result<Vec<HavingCondition>> {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            let mut conditions = parse_having_expr(left)?;
            conditions.extend(parse_having_expr(right)?);
            Ok(conditions)
        }
        Expr::BinaryOp { left, op, right } => {
            // Left side must be an aggregate function
            let aggregate = match left.as_ref() {
                Expr::Function(_) => {
                    try_parse_aggregate(left)?.ok_or_else(|| {
                        QueryError::UnsupportedFeature(
                            "HAVING requires aggregate functions (COUNT, SUM, AVG, MIN, MAX)"
                                .to_string(),
                        )
                    })?
                }
                _ => {
                    return Err(QueryError::UnsupportedFeature(
                        "HAVING clause must reference aggregate functions".to_string(),
                    ));
                }
            };

            // Right side must be a literal value
            let value = expr_to_value(right)?;

            // Map the operator
            let having_op = match op {
                BinaryOperator::Eq => HavingOp::Eq,
                BinaryOperator::Lt => HavingOp::Lt,
                BinaryOperator::LtEq => HavingOp::Le,
                BinaryOperator::Gt => HavingOp::Gt,
                BinaryOperator::GtEq => HavingOp::Ge,
                other => {
                    return Err(QueryError::UnsupportedFeature(format!(
                        "unsupported HAVING operator: {other:?}"
                    )));
                }
            };

            Ok(vec![HavingCondition::AggregateComparison {
                aggregate,
                op: having_op,
                value,
            }])
        }
        Expr::Nested(inner) => parse_having_expr(inner),
        other => Err(QueryError::UnsupportedFeature(format!(
            "unsupported HAVING expression: {other:?}"
        ))),
    }
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
            SelectItem::UnnamedExpr(Expr::CompoundIdentifier(idents)) if idents.len() == 2 => {
                // table.column - just use the column name
                columns.push(ColumnName::new(idents[1].value.clone()));
            }
            SelectItem::ExprWithAlias {
                expr: Expr::Identifier(ident),
                alias,
            } => {
                // For now, we ignore aliases and just use the column name
                let _ = alias;
                columns.push(ColumnName::new(ident.value.clone()));
            }
            SelectItem::ExprWithAlias {
                expr: Expr::CompoundIdentifier(idents),
                alias,
            } if idents.len() == 2 => {
                // table.column AS alias - use the column name (ignoring alias for now)
                let _ = alias;
                columns.push(ColumnName::new(idents[1].value.clone()));
            }
            SelectItem::UnnamedExpr(Expr::Function(_))
            | SelectItem::ExprWithAlias {
                expr: Expr::Function(_),
                ..
            } => {
                // Aggregate functions are handled separately by parse_aggregates_from_select_items
                // Skip them here
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

/// Parses aggregate functions from SELECT items.
fn parse_aggregates_from_select_items(items: &[SelectItem]) -> Result<Vec<AggregateFunction>> {
    let mut aggregates = Vec::new();

    for item in items {
        match item {
            SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                if let Some(agg) = try_parse_aggregate(expr)? {
                    aggregates.push(agg);
                }
            }
            _ => {
                // SELECT * has no aggregates; ignore other select items (Wildcard, QualifiedWildcard, etc.)
            }
        }
    }

    Ok(aggregates)
}

/// Tries to parse an expression as an aggregate function.
/// Returns None if the expression is not an aggregate function.
fn try_parse_aggregate(expr: &Expr) -> Result<Option<AggregateFunction>> {
    match expr {
        Expr::Function(func) => {
            let func_name = func.name.to_string().to_uppercase();

            // Extract function arguments from the FunctionArguments enum
            let args = match &func.args {
                sqlparser::ast::FunctionArguments::List(list) => &list.args,
                _ => {
                    return Err(QueryError::UnsupportedFeature(
                        "non-list function arguments not supported".to_string(),
                    ));
                }
            };

            match func_name.as_str() {
                "COUNT" => {
                    // COUNT(*) or COUNT(column)
                    if args.len() == 1 {
                        match &args[0] {
                            sqlparser::ast::FunctionArg::Unnamed(arg_expr) => match arg_expr {
                                sqlparser::ast::FunctionArgExpr::Wildcard => {
                                    Ok(Some(AggregateFunction::CountStar))
                                }
                                sqlparser::ast::FunctionArgExpr::Expr(Expr::Identifier(ident)) => {
                                    Ok(Some(AggregateFunction::Count(ColumnName::new(
                                        ident.value.clone(),
                                    ))))
                                }
                                _ => Err(QueryError::UnsupportedFeature(
                                    "COUNT with complex expression not supported".to_string(),
                                )),
                            },
                            _ => Err(QueryError::UnsupportedFeature(
                                "named function arguments not supported".to_string(),
                            )),
                        }
                    } else {
                        Err(QueryError::ParseError(format!(
                            "COUNT expects 1 argument, got {}",
                            args.len()
                        )))
                    }
                }
                "SUM" | "AVG" | "MIN" | "MAX" => {
                    // SUM/AVG/MIN/MAX(column)
                    if args.len() != 1 {
                        return Err(QueryError::ParseError(format!(
                            "{} expects 1 argument, got {}",
                            func_name,
                            args.len()
                        )));
                    }

                    match &args[0] {
                        sqlparser::ast::FunctionArg::Unnamed(arg_expr) => match arg_expr {
                            sqlparser::ast::FunctionArgExpr::Expr(Expr::Identifier(ident)) => {
                                let column = ColumnName::new(ident.value.clone());
                                match func_name.as_str() {
                                    "SUM" => Ok(Some(AggregateFunction::Sum(column))),
                                    "AVG" => Ok(Some(AggregateFunction::Avg(column))),
                                    "MIN" => Ok(Some(AggregateFunction::Min(column))),
                                    "MAX" => Ok(Some(AggregateFunction::Max(column))),
                                    _ => unreachable!(),
                                }
                            }
                            _ => Err(QueryError::UnsupportedFeature(format!(
                                "{func_name} with complex expression not supported"
                            ))),
                        },
                        _ => Err(QueryError::UnsupportedFeature(
                            "named function arguments not supported".to_string(),
                        )),
                    }
                }
                _ => {
                    // Not an aggregate function
                    Ok(None)
                }
            }
        }
        _ => {
            // Not a function call
            Ok(None)
        }
    }
}

/// Parses GROUP BY expressions into column names.
fn parse_group_by_expr(exprs: &[Expr]) -> Result<Vec<ColumnName>> {
    let mut columns = Vec::new();

    for expr in exprs {
        match expr {
            Expr::Identifier(ident) => {
                columns.push(ColumnName::new(ident.value.clone()));
            }
            _ => {
                return Err(QueryError::UnsupportedFeature(
                    "complex GROUP BY expressions not supported".to_string(),
                ));
            }
        }
    }

    Ok(columns)
}

/// Maximum nesting depth for WHERE clause expressions.
///
/// Prevents stack overflow from deeply nested queries like:
/// `WHERE ((((...(a = 1)...))))`
///
/// 100 levels is sufficient for all practical queries while preventing
/// malicious or pathological input from exhausting the stack.
const MAX_WHERE_DEPTH: usize = 100;

fn parse_where_expr(expr: &Expr) -> Result<Vec<Predicate>> {
    parse_where_expr_inner(expr, 0)
}

fn parse_where_expr_inner(expr: &Expr, depth: usize) -> Result<Vec<Predicate>> {
    if depth >= MAX_WHERE_DEPTH {
        return Err(QueryError::ParseError(format!(
            "WHERE clause nesting exceeds maximum depth of {MAX_WHERE_DEPTH}"
        )));
    }

    match expr {
        // AND combines multiple predicates
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            let mut predicates = parse_where_expr_inner(left, depth + 1)?;
            predicates.extend(parse_where_expr_inner(right, depth + 1)?);
            Ok(predicates)
        }

        // OR creates a disjunction
        Expr::BinaryOp {
            left,
            op: BinaryOperator::Or,
            right,
        } => {
            let left_preds = parse_where_expr_inner(left, depth + 1)?;
            let right_preds = parse_where_expr_inner(right, depth + 1)?;
            Ok(vec![Predicate::Or(left_preds, right_preds)])
        }

        // LIKE pattern matching
        Expr::Like {
            expr,
            pattern,
            negated,
            ..
        } => {
            if *negated {
                return Err(QueryError::UnsupportedFeature(
                    "NOT LIKE is not supported".to_string(),
                ));
            }

            let column = expr_to_column(expr)?;
            let pattern_value = expr_to_predicate_value(pattern)?;

            match pattern_value {
                PredicateValue::String(pattern_str)
                | PredicateValue::Literal(Value::Text(pattern_str)) => {
                    Ok(vec![Predicate::Like(column, pattern_str)])
                }
                _ => Err(QueryError::UnsupportedFeature(
                    "LIKE pattern must be a string literal".to_string(),
                )),
            }
        }

        // IS NULL / IS NOT NULL
        Expr::IsNull(expr) => {
            let column = expr_to_column(expr)?;
            Ok(vec![Predicate::IsNull(column)])
        }

        Expr::IsNotNull(expr) => {
            let column = expr_to_column(expr)?;
            Ok(vec![Predicate::IsNotNull(column)])
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
        Expr::Nested(inner) => parse_where_expr_inner(inner, depth + 1),

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
        // Handle column references (for JOIN conditions like users.id = orders.user_id)
        Expr::Identifier(ident) => {
            // Unqualified column reference
            Ok(PredicateValue::ColumnRef(ident.value.clone()))
        }
        Expr::CompoundIdentifier(idents) if idents.len() == 2 => {
            // Qualified column reference: table.column
            Ok(PredicateValue::ColumnRef(format!(
                "{}.{}",
                idents[0].value, idents[1].value
            )))
        }
        Expr::Value(SqlValue::Number(n, _)) => {
            let value = parse_number_literal(n)?;
            match value {
                Value::BigInt(v) => Ok(PredicateValue::Int(v)),
                Value::Decimal(_, _) => Ok(PredicateValue::Literal(value)),
                _ => unreachable!("parse_number_literal only returns BigInt or Decimal"),
            }
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
                // SQL parameters are 1-indexed, reject $0
                if idx == 0 {
                    return Err(QueryError::ParseError(
                        "parameter indices start at $1, not $0".to_string(),
                    ));
                }
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
                let value = parse_number_literal(n)?;
                match value {
                    Value::BigInt(v) => Ok(PredicateValue::Int(-v)),
                    Value::Decimal(v, scale) => {
                        Ok(PredicateValue::Literal(Value::Decimal(-v, scale)))
                    }
                    _ => unreachable!("parse_number_literal only returns BigInt or Decimal"),
                }
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
        if let sqlparser::ast::TableConstraint::PrimaryKey {
            columns: pk_cols, ..
        } = constraint
        {
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
    // For DECIMAL, we need to handle precision/scale specially
    let data_type = match &col_def.data_type {
        // Integer types
        SqlDataType::TinyInt(_) => "TINYINT".to_string(),
        SqlDataType::SmallInt(_) => "SMALLINT".to_string(),
        SqlDataType::Int(_) | SqlDataType::Integer(_) => "INTEGER".to_string(),
        SqlDataType::BigInt(_) => "BIGINT".to_string(),

        // Numeric types
        SqlDataType::Real | SqlDataType::Float(_) | SqlDataType::Double(_) => "REAL".to_string(),
        SqlDataType::Decimal(precision_opt) => match precision_opt {
            sqlparser::ast::ExactNumberInfo::PrecisionAndScale(p, s) => {
                format!("DECIMAL({p},{s})")
            }
            sqlparser::ast::ExactNumberInfo::Precision(p) => {
                format!("DECIMAL({p},0)")
            }
            sqlparser::ast::ExactNumberInfo::None => "DECIMAL(18,2)".to_string(),
        },

        // String types
        SqlDataType::Text | SqlDataType::Varchar(_) | SqlDataType::String(_) => "TEXT".to_string(),

        // Binary types
        SqlDataType::Binary(_) | SqlDataType::Varbinary(_) | SqlDataType::Blob(_) => {
            "BYTES".to_string()
        }

        // Boolean type
        SqlDataType::Boolean | SqlDataType::Bool => "BOOLEAN".to_string(),

        // Date/Time types
        SqlDataType::Date => "DATE".to_string(),
        SqlDataType::Time(_, _) => "TIME".to_string(),
        SqlDataType::Timestamp(_, _) => "TIMESTAMP".to_string(),

        // Structured types
        SqlDataType::Uuid => "UUID".to_string(),
        SqlDataType::JSON => "JSON".to_string(),

        other => {
            return Err(QueryError::UnsupportedFeature(format!(
                "unsupported data type: {other:?}"
            )));
        }
    };

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

fn parse_alter_table(
    name: &sqlparser::ast::ObjectName,
    operations: &[sqlparser::ast::AlterTableOperation],
) -> Result<ParsedAlterTable> {
    let table_name = object_name_to_string(name);

    // Only support one operation at a time
    if operations.len() != 1 {
        return Err(QueryError::UnsupportedFeature(
            "ALTER TABLE supports only one operation at a time".to_string(),
        ));
    }

    let operation = match &operations[0] {
        sqlparser::ast::AlterTableOperation::AddColumn { column_def, .. } => {
            let parsed_col = parse_column_def(column_def)?;
            AlterTableOperation::AddColumn(parsed_col)
        }
        sqlparser::ast::AlterTableOperation::DropColumn {
            column_name,
            if_exists: _,
            ..
        } => {
            let col_name = column_name.value.clone();
            AlterTableOperation::DropColumn(col_name)
        }
        other => {
            return Err(QueryError::UnsupportedFeature(format!(
                "ALTER TABLE operation not supported: {other:?}"
            )));
        }
    };

    Ok(ParsedAlterTable {
        table_name,
        operation,
    })
}

fn parse_create_index(create_index: &sqlparser::ast::CreateIndex) -> Result<ParsedCreateIndex> {
    let index_name = match &create_index.name {
        Some(name) => object_name_to_string(name),
        None => {
            return Err(QueryError::ParseError(
                "CREATE INDEX requires an index name".to_string(),
            ));
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

    // Extract values from all rows
    let values = match insert.source.as_ref().map(|s| s.body.as_ref()) {
        Some(SetExpr::Values(values)) => {
            let mut all_rows = Vec::new();
            for row in &values.rows {
                let mut parsed_row = Vec::new();
                for expr in row {
                    let val = expr_to_value(expr)?;
                    parsed_row.push(val);
                }
                all_rows.push(parsed_row);
            }
            all_rows
        }
        _ => {
            return Err(QueryError::UnsupportedFeature(
                "only VALUES clause is supported in INSERT".to_string(),
            ));
        }
    };

    // Parse RETURNING clause
    let returning = parse_returning(insert.returning.as_ref())?;

    Ok(ParsedInsert {
        table,
        columns,
        values,
        returning,
    })
}

fn parse_update(
    table: &sqlparser::ast::TableWithJoins,
    assignments: &[sqlparser::ast::Assignment],
    selection: Option<&Expr>,
    returning: Option<&Vec<SelectItem>>,
) -> Result<ParsedUpdate> {
    let table_name = match &table.relation {
        sqlparser::ast::TableFactor::Table { name, .. } => object_name_to_string(name),
        other => {
            return Err(QueryError::UnsupportedFeature(format!(
                "unsupported table in UPDATE: {other:?}"
            )));
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

    // Parse RETURNING clause
    let returning_cols = parse_returning(returning)?;

    Ok(ParsedUpdate {
        table: table_name,
        assignments: parsed_assignments,
        predicates,
        returning: returning_cols,
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
                    ));
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
                    ));
                }
            }
        }
    };

    // Parse WHERE clause
    let predicates = match &delete.selection {
        Some(expr) => parse_where_expr(expr)?,
        None => vec![],
    };

    // Parse RETURNING clause
    let returning_cols = parse_returning(delete.returning.as_ref())?;

    Ok(ParsedDelete {
        table: table_name,
        predicates,
        returning: returning_cols,
    })
}

/// Parses a RETURNING clause into a list of column names.
fn parse_returning(returning: Option<&Vec<SelectItem>>) -> Result<Option<Vec<String>>> {
    match returning {
        None => Ok(None),
        Some(items) => {
            let mut columns = Vec::new();
            for item in items {
                match item {
                    SelectItem::UnnamedExpr(Expr::Identifier(ident)) => {
                        columns.push(ident.value.clone());
                    }
                    SelectItem::UnnamedExpr(Expr::CompoundIdentifier(parts)) => {
                        // Handle table.column format - just take the column name
                        if let Some(last) = parts.last() {
                            columns.push(last.value.clone());
                        } else {
                            return Err(QueryError::ParseError(
                                "invalid column in RETURNING clause".to_string(),
                            ));
                        }
                    }
                    _ => {
                        return Err(QueryError::UnsupportedFeature(
                            "only simple column names supported in RETURNING clause".to_string(),
                        ));
                    }
                }
            }
            Ok(Some(columns))
        }
    }
}

/// Parses a number literal as either an integer or decimal.
///
/// Uses `rust_decimal` for robust decimal parsing (handles all edge cases correctly).
fn parse_number_literal(n: &str) -> Result<Value> {
    use rust_decimal::Decimal;
    use std::str::FromStr;

    if n.contains('.') {
        // Parse as DECIMAL using rust_decimal for correct handling
        let decimal = Decimal::from_str(n)
            .map_err(|e| QueryError::ParseError(format!("invalid decimal '{n}': {e}")))?;

        // Get scale (number of decimal places)
        let scale = decimal.scale() as u8;

        if scale > 38 {
            return Err(QueryError::ParseError(format!(
                "decimal scale too large (max 38): {n}"
            )));
        }

        // Convert to i128 representation: mantissa * 10^scale
        // rust_decimal stores internally as i128 mantissa with scale
        let mantissa = decimal.mantissa();

        Ok(Value::Decimal(mantissa, scale))
    } else {
        // Parse as integer (BigInt)
        let v: i64 = n
            .parse()
            .map_err(|_| QueryError::ParseError(format!("invalid integer: {n}")))?;
        Ok(Value::BigInt(v))
    }
}

/// Converts a SQL expression to a Value.
fn expr_to_value(expr: &Expr) -> Result<Value> {
    match expr {
        Expr::Value(SqlValue::Number(n, _)) => parse_number_literal(n),
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
                // SQL parameters are 1-indexed, reject $0
                if idx == 0 {
                    return Err(QueryError::ParseError(
                        "parameter indices start at $1, not $0".to_string(),
                    ));
                }
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
                let value = parse_number_literal(n)?;
                match value {
                    Value::BigInt(v) => Ok(Value::BigInt(-v)),
                    Value::Decimal(v, scale) => Ok(Value::Decimal(-v, scale)),
                    _ => unreachable!("parse_number_literal only returns BigInt or Decimal"),
                }
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

    fn parse_test_select(sql: &str) -> ParsedSelect {
        match parse_statement(sql).unwrap() {
            ParsedStatement::Select(s) => s,
            _ => panic!("expected SELECT statement"),
        }
    }

    #[test]
    fn test_parse_simple_select() {
        let result = parse_test_select("SELECT id, name FROM users");
        assert_eq!(result.table, "users");
        assert_eq!(
            result.columns,
            Some(vec![ColumnName::new("id"), ColumnName::new("name")])
        );
        assert!(result.predicates.is_empty());
    }

    #[test]
    fn test_parse_select_star() {
        let result = parse_test_select("SELECT * FROM users");
        assert_eq!(result.table, "users");
        assert!(result.columns.is_none());
    }

    #[test]
    fn test_parse_where_eq() {
        let result = parse_test_select("SELECT * FROM users WHERE id = 42");
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
        let result = parse_test_select("SELECT * FROM users WHERE name = 'alice'");
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
        let result = parse_test_select("SELECT * FROM users WHERE id = 1 AND name = 'bob'");
        assert_eq!(result.predicates.len(), 2);
    }

    #[test]
    fn test_parse_where_in() {
        let result = parse_test_select("SELECT * FROM users WHERE id IN (1, 2, 3)");
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
        let result = parse_test_select("SELECT * FROM users ORDER BY name ASC, id DESC");
        assert_eq!(result.order_by.len(), 2);
        assert_eq!(result.order_by[0].column.as_str(), "name");
        assert!(result.order_by[0].ascending);
        assert_eq!(result.order_by[1].column.as_str(), "id");
        assert!(!result.order_by[1].ascending);
    }

    #[test]
    fn test_parse_limit() {
        let result = parse_test_select("SELECT * FROM users LIMIT 10");
        assert_eq!(result.limit, Some(10));
    }

    #[test]
    fn test_parse_param() {
        let result = parse_test_select("SELECT * FROM users WHERE id = $1");
        match &result.predicates[0] {
            Predicate::Eq(_, PredicateValue::Param(1)) => {}
            other => panic!("unexpected predicate: {other:?}"),
        }
    }

    #[test]
    fn test_parse_inner_join() {
        let result =
            parse_statement("SELECT * FROM users JOIN orders ON users.id = orders.user_id");
        if let Err(ref e) = result {
            eprintln!("Parse error: {e:?}");
        }
        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::Select(s) => {
                assert_eq!(s.table, "users");
                assert_eq!(s.joins.len(), 1);
                assert_eq!(s.joins[0].table, "orders");
                assert!(matches!(s.joins[0].join_type, JoinType::Inner));
            }
            _ => panic!("expected SELECT statement"),
        }
    }

    #[test]
    fn test_parse_left_join() {
        let result = parse_statement(
            "SELECT * FROM users LEFT JOIN orders ON users.id = orders.user_id",
        );
        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::Select(s) => {
                assert_eq!(s.table, "users");
                assert_eq!(s.joins.len(), 1);
                assert_eq!(s.joins[0].table, "orders");
                assert!(matches!(s.joins[0].join_type, JoinType::Left));
            }
            _ => panic!("expected SELECT statement"),
        }
    }

    #[test]
    fn test_parse_multi_join() {
        let result = parse_statement(
            "SELECT * FROM users \
             JOIN orders ON users.id = orders.user_id \
             JOIN products ON orders.product_id = products.id",
        );
        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::Select(s) => {
                assert_eq!(s.table, "users");
                assert_eq!(s.joins.len(), 2);
                assert_eq!(s.joins[0].table, "orders");
                assert_eq!(s.joins[1].table, "products");
            }
            _ => panic!("expected SELECT statement"),
        }
    }

    #[test]
    fn test_reject_subquery() {
        let result = parse_statement("SELECT * FROM (SELECT * FROM users)");
        assert!(result.is_err());
    }

    #[test]
    fn test_where_depth_within_limit() {
        // Test reasonable nesting depth (stays within sqlparser limits)
        // Build a query with nested AND/OR to test our depth tracking
        let mut sql = String::from("SELECT * FROM users WHERE ");
        for i in 0..10 {
            if i > 0 {
                sql.push_str(" AND ");
            }
            sql.push('(');
            sql.push_str("id = ");
            sql.push_str(&i.to_string());
            sql.push(')');
        }

        let result = parse_statement(&sql);
        assert!(
            result.is_ok(),
            "Moderate nesting should succeed, but got: {result:?}"
        );
    }

    #[test]
    fn test_where_depth_nested_parens() {
        // Test nested parentheses (this will hit sqlparser limit before ours)
        // Just verify that excessive nesting is rejected by some limit
        let mut sql = String::from("SELECT * FROM users WHERE ");
        for _ in 0..200 {
            sql.push('(');
        }
        sql.push_str("id = 1");
        for _ in 0..200 {
            sql.push(')');
        }

        let result = parse_statement(&sql);
        assert!(
            result.is_err(),
            "Excessive parenthesis nesting should be rejected"
        );
    }

    #[test]
    fn test_where_depth_complex_and_or() {
        // Test complex AND/OR nesting patterns
        let sql = "SELECT * FROM users WHERE \
                   ((id = 1 AND name = 'a') OR (id = 2 AND name = 'b')) AND \
                   ((age > 10 AND age < 20) OR (age > 30 AND age < 40))";

        let result = parse_statement(sql);
        assert!(result.is_ok(), "Complex AND/OR should succeed");
    }

    #[test]
    fn test_parse_having() {
        let result =
            parse_test_select("SELECT name, COUNT(*) FROM users GROUP BY name HAVING COUNT(*) > 5");
        assert_eq!(result.group_by.len(), 1);
        assert_eq!(result.having.len(), 1);
        match &result.having[0] {
            HavingCondition::AggregateComparison {
                aggregate,
                op,
                value,
            } => {
                assert!(matches!(aggregate, AggregateFunction::CountStar));
                assert_eq!(*op, HavingOp::Gt);
                assert_eq!(*value, Value::BigInt(5));
            }
        }
    }

    #[test]
    fn test_parse_having_multiple() {
        let result = parse_test_select(
            "SELECT name, COUNT(*), SUM(age) FROM users GROUP BY name HAVING COUNT(*) > 1 AND SUM(age) < 100",
        );
        assert_eq!(result.having.len(), 2);
    }

    #[test]
    fn test_parse_having_without_group_by() {
        let result = parse_test_select("SELECT COUNT(*) FROM users HAVING COUNT(*) > 0");
        assert!(result.group_by.is_empty());
        assert_eq!(result.having.len(), 1);
    }

    #[test]
    fn test_parse_union() {
        let result = parse_statement("SELECT id FROM users UNION SELECT id FROM orders");
        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::Union(u) => {
                assert_eq!(u.left.table, "users");
                assert_eq!(u.right.table, "orders");
                assert!(!u.all);
            }
            _ => panic!("expected UNION statement"),
        }
    }

    #[test]
    fn test_parse_union_all() {
        let result = parse_statement("SELECT id FROM users UNION ALL SELECT id FROM orders");
        assert!(result.is_ok());
        match result.unwrap() {
            ParsedStatement::Union(u) => {
                assert_eq!(u.left.table, "users");
                assert_eq!(u.right.table, "orders");
                assert!(u.all);
            }
            _ => panic!("expected UNION ALL statement"),
        }
    }
}
