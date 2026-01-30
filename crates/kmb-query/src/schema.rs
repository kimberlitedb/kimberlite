//! Schema definitions for query planning.
//!
//! Provides explicit mapping of SQL table/column names to store types.

use std::collections::BTreeMap;
use std::fmt::{self, Debug, Display};

use kmb_store::TableId;

// ============================================================================
// Names
// ============================================================================

/// SQL table name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TableName(String);

impl TableName {
    /// Creates a new table name.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Returns the table name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for TableName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TableName({:?})", self.0)
    }
}

impl Display for TableName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for TableName {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for TableName {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

/// SQL column name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ColumnName(String);

impl ColumnName {
    /// Creates a new column name.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Returns the column name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for ColumnName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ColumnName({:?})", self.0)
    }
}

impl Display for ColumnName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ColumnName {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for ColumnName {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

// ============================================================================
// Data Types
// ============================================================================

/// SQL data types supported by the query engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataType {
    // ===== Integer Types =====
    /// 8-bit signed integer (-128 to 127).
    TinyInt,
    /// 16-bit signed integer (-32,768 to 32,767).
    SmallInt,
    /// 32-bit signed integer (-2^31 to 2^31-1).
    Integer,
    /// 64-bit signed integer (-2^63 to 2^63-1).
    BigInt,

    // ===== Numeric Types =====
    /// 64-bit floating point number (IEEE 754 double precision).
    Real,
    /// Fixed-precision decimal number.
    ///
    /// Stored internally as i128 in smallest units.
    /// Example: DECIMAL(10,2) stores 123.45 as 12345.
    Decimal {
        /// Total number of digits (1-38).
        precision: u8,
        /// Number of digits after decimal point (0-precision).
        scale: u8,
    },

    // ===== String Types =====
    /// Variable-length UTF-8 text.
    Text,

    // ===== Binary Types =====
    /// Variable-length binary data.
    Bytes,

    // ===== Boolean Type =====
    /// Boolean value (true/false).
    Boolean,

    // ===== Date/Time Types =====
    /// Date (days since Unix epoch, i32).
    Date,
    /// Time of day (nanoseconds within day, i64).
    Time,
    /// Timestamp (nanoseconds since Unix epoch, u64).
    Timestamp,

    // ===== Structured Types =====
    /// UUID (RFC 4122, 128-bit).
    Uuid,
    /// JSON document (validated, stored as text).
    Json,
}

impl Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataType::TinyInt => write!(f, "TINYINT"),
            DataType::SmallInt => write!(f, "SMALLINT"),
            DataType::Integer => write!(f, "INTEGER"),
            DataType::BigInt => write!(f, "BIGINT"),
            DataType::Real => write!(f, "REAL"),
            DataType::Decimal { precision, scale } => write!(f, "DECIMAL({precision},{scale})"),
            DataType::Text => write!(f, "TEXT"),
            DataType::Bytes => write!(f, "BYTES"),
            DataType::Boolean => write!(f, "BOOLEAN"),
            DataType::Date => write!(f, "DATE"),
            DataType::Time => write!(f, "TIME"),
            DataType::Timestamp => write!(f, "TIMESTAMP"),
            DataType::Uuid => write!(f, "UUID"),
            DataType::Json => write!(f, "JSON"),
        }
    }
}

// ============================================================================
// Column Definition
// ============================================================================

/// Definition of a table column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDef {
    /// Column name.
    pub name: ColumnName,
    /// Column data type.
    pub data_type: DataType,
    /// Whether the column can contain NULL values.
    pub nullable: bool,
}

impl ColumnDef {
    /// Creates a new column definition.
    pub fn new(name: impl Into<ColumnName>, data_type: DataType) -> Self {
        Self {
            name: name.into(),
            data_type,
            nullable: true,
        }
    }

    /// Makes this column non-nullable.
    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }
}

// ============================================================================
// Index Definition
// ============================================================================

/// Definition of a secondary index on a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDef {
    /// Index ID in the store.
    pub index_id: u64,
    /// Index name.
    pub name: String,
    /// Indexed column names (in order).
    pub columns: Vec<ColumnName>,
}

impl IndexDef {
    /// Creates a new index definition.
    pub fn new(index_id: u64, name: impl Into<String>, columns: Vec<ColumnName>) -> Self {
        Self {
            index_id,
            name: name.into(),
            columns,
        }
    }
}

// ============================================================================
// Table Definition
// ============================================================================

/// Definition of a table in the schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDef {
    /// Underlying store table ID.
    pub table_id: TableId,
    /// Column definitions in order.
    pub columns: Vec<ColumnDef>,
    /// Primary key column names (in order).
    pub primary_key: Vec<ColumnName>,
    /// Secondary indexes on this table.
    pub indexes: Vec<IndexDef>,
}

impl TableDef {
    /// Creates a new table definition.
    pub fn new(table_id: TableId, columns: Vec<ColumnDef>, primary_key: Vec<ColumnName>) -> Self {
        // Validate primary key columns exist
        for pk_col in &primary_key {
            debug_assert!(
                columns.iter().any(|c| &c.name == pk_col),
                "primary key column '{pk_col}' not found in columns"
            );
        }

        Self {
            table_id,
            columns,
            primary_key,
            indexes: Vec::new(),
        }
    }

    /// Adds an index to this table definition.
    pub fn with_index(mut self, index: IndexDef) -> Self {
        self.indexes.push(index);
        self
    }

    /// Returns all indexes for this table.
    pub fn indexes(&self) -> &[IndexDef] {
        &self.indexes
    }

    /// Finds an index that can be used for the given column.
    pub fn find_index_for_column(&self, column: &ColumnName) -> Option<&IndexDef> {
        self.indexes
            .iter()
            .find(|idx| !idx.columns.is_empty() && &idx.columns[0] == column)
    }

    /// Finds a column by name.
    pub fn find_column(&self, name: &ColumnName) -> Option<(usize, &ColumnDef)> {
        self.columns
            .iter()
            .enumerate()
            .find(|(_, c)| &c.name == name)
    }

    /// Returns true if the given column is part of the primary key.
    pub fn is_primary_key(&self, name: &ColumnName) -> bool {
        self.primary_key.contains(name)
    }

    /// Returns the index of a column in the primary key.
    pub fn primary_key_position(&self, name: &ColumnName) -> Option<usize> {
        self.primary_key.iter().position(|pk| pk == name)
    }

    /// Returns the column indices that form the primary key.
    pub fn primary_key_indices(&self) -> Vec<usize> {
        self.primary_key
            .iter()
            .filter_map(|pk| self.find_column(pk).map(|(idx, _)| idx))
            .collect()
    }
}

// ============================================================================
// Schema
// ============================================================================

/// Schema registry mapping SQL names to store types.
#[derive(Debug, Clone, Default)]
pub struct Schema {
    tables: BTreeMap<TableName, TableDef>,
}

impl Schema {
    /// Creates an empty schema.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a table to the schema.
    pub fn add_table(&mut self, name: impl Into<TableName>, def: TableDef) {
        self.tables.insert(name.into(), def);
    }

    /// Looks up a table by name.
    pub fn get_table(&self, name: &TableName) -> Option<&TableDef> {
        self.tables.get(name)
    }

    /// Returns all table names.
    pub fn table_names(&self) -> impl Iterator<Item = &TableName> {
        self.tables.keys()
    }

    /// Returns the number of tables.
    pub fn len(&self) -> usize {
        self.tables.len()
    }

    /// Returns true if the schema has no tables.
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }
}

// ============================================================================
// Builder
// ============================================================================

/// Builder for constructing schemas fluently.
#[derive(Debug, Default)]
pub struct SchemaBuilder {
    schema: Schema,
}

impl SchemaBuilder {
    /// Creates a new schema builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a table to the schema.
    pub fn table(
        mut self,
        name: impl Into<TableName>,
        table_id: TableId,
        columns: Vec<ColumnDef>,
        primary_key: Vec<ColumnName>,
    ) -> Self {
        let def = TableDef::new(table_id, columns, primary_key);
        self.schema.add_table(name, def);
        self
    }

    /// Builds the schema.
    pub fn build(self) -> Schema {
        self.schema
    }
}
