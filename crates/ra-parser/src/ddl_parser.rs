//! SQL DDL parser for timeline schema definitions.
//!
//! Parses CREATE TABLE, CREATE INDEX, and ALTER TABLE statements to extract
//! schema information for timeline configurations.
//!
//! # Example
//!
//! ```rust
//! use ra_parser::ddl_parser::DdlParser;
//!
//! let parser = DdlParser::new();
//! let sql = "CREATE TABLE orders (order_id SERIAL PRIMARY KEY, customer_id INTEGER NOT NULL)";
//! let table = parser.parse_create_table(sql)?;
//! assert_eq!(table.name, "orders");
//! assert_eq!(table.columns.len(), 2);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use sqlparser::ast::{
    AlterTableOperation, ArrayElemTypeDef, ColumnDef, ColumnOption, DataType as SqlDataType,
    ObjectName, Statement, TableConstraint,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use thiserror::Error;

/// Errors from DDL parsing.
#[derive(Debug, Error)]
pub enum DdlParseError {
    /// SQL parsing failed.
    #[error("failed to parse SQL: {0}")]
    SqlParseError(String),

    /// Unsupported statement type.
    #[error("unsupported statement type: expected CREATE TABLE or CREATE INDEX")]
    UnsupportedStatement,

    /// Missing required information.
    #[error("missing required information: {0}")]
    MissingInformation(String),
}

/// SQL DDL parser.
pub struct DdlParser {
    dialect: PostgreSqlDialect,
}

impl DdlParser {
    /// Create a new DDL parser with PostgreSQL dialect.
    pub fn new() -> Self {
        Self {
            dialect: PostgreSqlDialect {},
        }
    }

    /// Parse a CREATE TABLE statement.
    ///
    /// # Errors
    ///
    /// Returns error if SQL is invalid or not a CREATE TABLE statement.
    pub fn parse_create_table(&self, sql: &str) -> Result<TableDefinition, DdlParseError> {
        let statements = Parser::parse_sql(&self.dialect, sql)
            .map_err(|e| DdlParseError::SqlParseError(e.to_string()))?;

        let statement = statements
            .first()
            .ok_or_else(|| DdlParseError::MissingInformation("no statement found".to_string()))?;

        match statement {
            Statement::CreateTable(create_table) => {
                let name = extract_table_name(&create_table.name)?;

                let mut columns = Vec::new();
                let mut primary_key_columns = Vec::new();
                let mut foreign_keys = Vec::new();

                // Extract columns
                for column_def in &create_table.columns {
                    let column = self.convert_column_def(column_def);
                    columns.push(column);

                    // Check for PRIMARY KEY in column options
                    if column_def.options.iter().any(|opt| {
                        matches!(
                            opt.option,
                            ColumnOption::Unique {
                                is_primary: true,
                                ..
                            }
                        )
                    }) {
                        primary_key_columns.push(column_def.name.value.clone());
                    }
                }

                // Extract table constraints
                for constraint in &create_table.constraints {
                    match constraint {
                        TableConstraint::PrimaryKey { columns, .. } => {
                            primary_key_columns.extend(columns.iter().map(|c| c.value.clone()));
                        }
                        TableConstraint::ForeignKey {
                            columns,
                            foreign_table,
                            referred_columns,
                            ..
                        } => {
                            let fk = ForeignKeyDefinition {
                                columns: columns.iter().map(|c| c.value.clone()).collect(),
                                referenced_table: extract_table_name(foreign_table)?,
                                referenced_columns: referred_columns
                                    .iter()
                                    .map(|c| c.value.clone())
                                    .collect(),
                            };
                            foreign_keys.push(fk);
                        }
                        _ => {
                            // Skip other constraints for now
                        }
                    }
                }

                Ok(TableDefinition {
                    name,
                    columns,
                    primary_key: primary_key_columns,
                    foreign_keys,
                    indexes: Vec::new(), // Populated by parse_create_index
                })
            }
            _ => Err(DdlParseError::UnsupportedStatement),
        }
    }

    /// Parse a CREATE INDEX statement.
    ///
    /// # Errors
    ///
    /// Returns error if SQL is invalid or not a CREATE INDEX statement.
    pub fn parse_create_index(&self, sql: &str) -> Result<IndexDefinition, DdlParseError> {
        let statements = Parser::parse_sql(&self.dialect, sql)
            .map_err(|e| DdlParseError::SqlParseError(e.to_string()))?;

        let statement = statements
            .first()
            .ok_or_else(|| DdlParseError::MissingInformation("no statement found".to_string()))?;

        match statement {
            Statement::CreateIndex(create_index) => {
                let name = create_index
                    .name
                    .as_ref()
                    .map_or_else(|| "unnamed_index".to_string(), |n| n.to_string());

                let table_name = extract_table_name(&create_index.table_name)?;

                let columns: Vec<String> = create_index
                    .columns
                    .iter()
                    .filter_map(|col| {
                        // Extract column name from order by expression
                        if let sqlparser::ast::Expr::Identifier(ident) = &col.expr {
                            Some(ident.value.clone())
                        } else {
                            None
                        }
                    })
                    .collect();

                let index_type = if create_index.using.is_some() {
                    // Parse index type (BTREE, HASH, etc.)
                    "btree".to_string() // Default, can be extended
                } else {
                    "btree".to_string()
                };

                Ok(IndexDefinition {
                    name,
                    table: table_name,
                    index_type,
                    columns,
                    is_unique: create_index.unique,
                    included_columns: Vec::new(), // PostgreSQL INCLUDE clause
                })
            }
            _ => Err(DdlParseError::UnsupportedStatement),
        }
    }

    /// Parse an ALTER TABLE statement.
    ///
    /// # Errors
    ///
    /// Returns error if SQL is invalid or not an ALTER TABLE statement.
    pub fn parse_alter_table(&self, sql: &str) -> Result<AlterTableDefinition, DdlParseError> {
        let statements = Parser::parse_sql(&self.dialect, sql)
            .map_err(|e| DdlParseError::SqlParseError(e.to_string()))?;

        let statement = statements
            .first()
            .ok_or_else(|| DdlParseError::MissingInformation("no statement found".to_string()))?;

        match statement {
            Statement::AlterTable {
                name, operations, ..
            } => {
                let table_name = extract_table_name(name)?;
                let mut changes = Vec::new();

                for operation in operations {
                    match operation {
                        AlterTableOperation::AddColumn {
                            column_keyword: _,
                            if_not_exists: _,
                            column_def,
                            column_position: _,
                        } => {
                            changes.push(AlterOperation::AddColumn(
                                self.convert_column_def(column_def),
                            ));
                        }
                        AlterTableOperation::DropColumn {
                            column_name,
                            if_exists: _,
                            cascade: _,
                        } => {
                            changes.push(AlterOperation::DropColumn(column_name.value.clone()));
                        }
                        _ => {
                            // Skip unsupported operations for now
                        }
                    }
                }

                Ok(AlterTableDefinition {
                    table: table_name,
                    operations: changes,
                })
            }
            _ => Err(DdlParseError::UnsupportedStatement),
        }
    }

    /// Convert a sqlparser ColumnDef to our ColumnDefinition.
    fn convert_column_def(&self, column_def: &ColumnDef) -> ColumnDefinition {
        let name = column_def.name.value.clone();
        let data_type = self.convert_data_type(&column_def.data_type);
        let nullable = !column_def.options.iter().any(|opt| {
            matches!(
                opt.option,
                ColumnOption::NotNull
                    | ColumnOption::Unique {
                        is_primary: true,
                        ..
                    }
            )
        });

        ColumnDefinition {
            name,
            data_type,
            nullable,
        }
    }

    /// Convert sqlparser DataType to simple string representation.
    fn convert_data_type(&self, data_type: &SqlDataType) -> String {
        match data_type {
            SqlDataType::TinyInt(_)
            | SqlDataType::SmallInt(_)
            | SqlDataType::MediumInt(_)
            | SqlDataType::Int(_)
            | SqlDataType::Int2(_)
            | SqlDataType::Int4(_)
            | SqlDataType::Int8(_)
            | SqlDataType::Integer(_)
            | SqlDataType::BigInt(_) => "integer".to_string(),
            SqlDataType::Real
            | SqlDataType::Float(_)
            | SqlDataType::Float4
            | SqlDataType::Float8
            | SqlDataType::Double
            | SqlDataType::DoublePrecision
            | SqlDataType::Dec(_)
            | SqlDataType::Decimal(_)
            | SqlDataType::Numeric(_) => "float".to_string(),
            SqlDataType::Varchar(_)
            | SqlDataType::Char(_)
            | SqlDataType::Character(_)
            | SqlDataType::CharVarying(_)
            | SqlDataType::CharacterVarying(_)
            | SqlDataType::Nvarchar(_)
            | SqlDataType::Text
            | SqlDataType::String(_)
            | SqlDataType::Clob(_) => "string".to_string(),
            SqlDataType::Bool | SqlDataType::Boolean => "boolean".to_string(),
            SqlDataType::Timestamp(_, _)
            | SqlDataType::Date
            | SqlDataType::Datetime(_)
            | SqlDataType::Time(_, _) => "timestamp".to_string(),
            SqlDataType::Bytea
            | SqlDataType::Binary(_)
            | SqlDataType::Varbinary(_)
            | SqlDataType::Blob(_) => "binary".to_string(),
            SqlDataType::JSON | SqlDataType::JSONB => "json".to_string(),
            SqlDataType::Uuid => "uuid".to_string(),
            SqlDataType::Array(array_def) => match array_def {
                ArrayElemTypeDef::None => "array".to_string(),
                ArrayElemTypeDef::SquareBracket(inner, _)
                | ArrayElemTypeDef::AngleBracket(inner)
                | ArrayElemTypeDef::Parenthesis(inner) => {
                    format!("array[{}]", self.convert_data_type(inner))
                }
            },
            SqlDataType::Custom(name, _) => {
                let upper = name.to_string().to_uppercase();
                if upper == "SERIAL" || upper == "BIGSERIAL" || upper == "SMALLSERIAL" {
                    "integer".to_string()
                } else {
                    name.to_string()
                }
            }
            _ => "other".to_string(),
        }
    }
}

impl Default for DdlParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract table name from ObjectName.
fn extract_table_name(name: &ObjectName) -> Result<String, DdlParseError> {
    name.0
        .last()
        .map(|ident| ident.value.clone())
        .ok_or_else(|| DdlParseError::MissingInformation("table name".to_string()))
}

/// Table definition extracted from DDL.
#[derive(Debug, Clone, PartialEq)]
pub struct TableDefinition {
    /// Table name.
    pub name: String,
    /// Column definitions.
    pub columns: Vec<ColumnDefinition>,
    /// Primary key columns.
    pub primary_key: Vec<String>,
    /// Foreign key constraints.
    pub foreign_keys: Vec<ForeignKeyDefinition>,
    /// Indexes (populated separately).
    pub indexes: Vec<IndexDefinition>,
}

/// Column definition extracted from DDL.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDefinition {
    /// Column name.
    pub name: String,
    /// Data type (simplified).
    pub data_type: String,
    /// Whether column is nullable.
    pub nullable: bool,
}

/// Index definition extracted from DDL.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexDefinition {
    /// Index name.
    pub name: String,
    /// Table name.
    pub table: String,
    /// Index type (btree, hash, etc.).
    pub index_type: String,
    /// Indexed columns.
    pub columns: Vec<String>,
    /// Whether index is unique.
    pub is_unique: bool,
    /// Included (non-key) columns.
    pub included_columns: Vec<String>,
}

/// Foreign key definition extracted from DDL.
#[derive(Debug, Clone, PartialEq)]
pub struct ForeignKeyDefinition {
    /// Columns in this table.
    pub columns: Vec<String>,
    /// Referenced table.
    pub referenced_table: String,
    /// Referenced columns.
    pub referenced_columns: Vec<String>,
}

/// ALTER TABLE definition extracted from DDL.
#[derive(Debug, Clone, PartialEq)]
pub struct AlterTableDefinition {
    /// Table being altered.
    pub table: String,
    /// Operations to perform.
    pub operations: Vec<AlterOperation>,
}

/// ALTER TABLE operation.
#[derive(Debug, Clone, PartialEq)]
pub enum AlterOperation {
    /// Add column.
    AddColumn(ColumnDefinition),
    /// Drop column.
    DropColumn(String),
    /// Add constraint (future).
    AddConstraint(String),
    /// Drop constraint (future).
    DropConstraint(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_create_table() {
        let parser = DdlParser::new();
        let sql =
            "CREATE TABLE orders (order_id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL)";
        let table = parser.parse_create_table(sql).unwrap();

        assert_eq!(table.name, "orders");
        assert_eq!(table.columns.len(), 2);
        assert_eq!(table.columns[0].name, "order_id");
        assert_eq!(table.columns[0].data_type, "integer");
        assert!(!table.columns[0].nullable);
        assert_eq!(table.primary_key, vec!["order_id"]);
    }

    #[test]
    fn parse_create_table_with_types() {
        let parser = DdlParser::new();
        let sql = r#"
        CREATE TABLE users (
            id SERIAL PRIMARY KEY,
            name VARCHAR(100) NOT NULL,
            email TEXT,
            age INTEGER,
            balance DECIMAL(10, 2),
            is_active BOOLEAN,
            created_at TIMESTAMP
        )
        "#;
        let table = parser.parse_create_table(sql).unwrap();

        assert_eq!(table.name, "users");
        assert_eq!(table.columns.len(), 7);
        assert_eq!(table.columns[1].data_type, "string"); // VARCHAR
        assert_eq!(table.columns[4].data_type, "float"); // DECIMAL
        assert_eq!(table.columns[5].data_type, "boolean");
        assert_eq!(table.columns[6].data_type, "timestamp");
    }

    #[test]
    fn parse_create_index() {
        let parser = DdlParser::new();
        let sql = "CREATE INDEX idx_orders_customer ON orders(customer_id)";
        let index = parser.parse_create_index(sql).unwrap();

        assert_eq!(index.name, "idx_orders_customer");
        assert_eq!(index.table, "orders");
        assert_eq!(index.columns, vec!["customer_id"]);
        assert!(!index.is_unique);
    }

    #[test]
    fn parse_create_unique_index() {
        let parser = DdlParser::new();
        let sql = "CREATE UNIQUE INDEX idx_users_email ON users(email)";
        let index = parser.parse_create_index(sql).unwrap();

        assert_eq!(index.name, "idx_users_email");
        assert!(index.is_unique);
    }

    #[test]
    fn parse_composite_index() {
        let parser = DdlParser::new();
        let sql = "CREATE INDEX idx_orders_cust_status ON orders(customer_id, status)";
        let index = parser.parse_create_index(sql).unwrap();

        assert_eq!(index.columns, vec!["customer_id", "status"]);
    }

    #[test]
    fn parse_alter_table_add_column() {
        let parser = DdlParser::new();
        let sql = "ALTER TABLE orders ADD COLUMN priority INTEGER";
        let alter = parser.parse_alter_table(sql).unwrap();

        assert_eq!(alter.table, "orders");
        assert_eq!(alter.operations.len(), 1);
        match &alter.operations[0] {
            AlterOperation::AddColumn(col) => {
                assert_eq!(col.name, "priority");
                assert_eq!(col.data_type, "integer");
            }
            _ => panic!("Expected AddColumn"),
        }
    }

    #[test]
    fn parse_alter_table_drop_column() {
        let parser = DdlParser::new();
        let sql = "ALTER TABLE orders DROP COLUMN status";
        let alter = parser.parse_alter_table(sql).unwrap();

        assert_eq!(alter.operations.len(), 1);
        match &alter.operations[0] {
            AlterOperation::DropColumn(name) => {
                assert_eq!(name, "status");
            }
            _ => panic!("Expected DropColumn"),
        }
    }

    #[test]
    fn parse_foreign_key() {
        let parser = DdlParser::new();
        let sql = r#"
        CREATE TABLE orders (
            order_id INTEGER PRIMARY KEY,
            customer_id INTEGER NOT NULL,
            FOREIGN KEY (customer_id) REFERENCES customers(customer_id)
        )
        "#;
        let table = parser.parse_create_table(sql).unwrap();

        assert_eq!(table.foreign_keys.len(), 1);
        let fk = &table.foreign_keys[0];
        assert_eq!(fk.columns, vec!["customer_id"]);
        assert_eq!(fk.referenced_table, "customers");
        assert_eq!(fk.referenced_columns, vec!["customer_id"]);
    }
}
