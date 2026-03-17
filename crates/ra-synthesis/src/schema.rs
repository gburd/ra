//! Schema encoding for query synthesis.
//!
//! Describes the database schema so the synthesis engine can resolve
//! table names, column names, and relationships from natural language.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Complete schema description for synthesis.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchemaInfo {
    /// Tables indexed by name (lowercased).
    pub tables: HashMap<String, TableInfo>,
}

/// Metadata about a single table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInfo {
    /// Canonical table name.
    pub name: String,
    /// Columns in the table, in ordinal order.
    pub columns: Vec<ColumnInfo>,
    /// Foreign key relationships originating from this table.
    pub foreign_keys: Vec<ForeignKey>,
}

/// Metadata about a single column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    /// Column name.
    pub name: String,
    /// SQL type name (e.g., "INTEGER", "TEXT", "REAL").
    pub data_type: String,
    /// Whether this column is part of the primary key.
    pub is_primary_key: bool,
    /// Whether this column allows NULLs.
    pub nullable: bool,
}

/// A foreign key relationship.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKey {
    /// Column(s) in this table.
    pub columns: Vec<String>,
    /// Referenced table.
    pub referenced_table: String,
    /// Referenced column(s).
    pub referenced_columns: Vec<String>,
}

impl SchemaInfo {
    /// Create a new empty schema.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a table to the schema.
    pub fn add_table(&mut self, table: TableInfo) {
        self.tables.insert(table.name.to_lowercase(), table);
    }

    /// Look up a table by name (case-insensitive).
    #[must_use]
    pub fn find_table(&self, name: &str) -> Option<&TableInfo> {
        self.tables.get(&name.to_lowercase())
    }

    /// Find all tables that contain a column with the given name.
    #[must_use]
    pub fn tables_with_column(&self, column: &str) -> Vec<&TableInfo> {
        let lower = column.to_lowercase();
        self.tables
            .values()
            .filter(|t| {
                t.columns
                    .iter()
                    .any(|c| c.name.to_lowercase() == lower)
            })
            .collect()
    }

    /// Return all table names in the schema.
    #[must_use]
    pub fn table_names(&self) -> Vec<&str> {
        self.tables.values().map(|t| t.name.as_str()).collect()
    }
}

impl TableInfo {
    /// Create a new table with columns.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        columns: Vec<ColumnInfo>,
    ) -> Self {
        Self {
            name: name.into(),
            columns,
            foreign_keys: Vec::new(),
        }
    }

    /// Find a column by name (case-insensitive).
    #[must_use]
    pub fn find_column(&self, name: &str) -> Option<&ColumnInfo> {
        let lower = name.to_lowercase();
        self.columns
            .iter()
            .find(|c| c.name.to_lowercase() == lower)
    }

    /// Return all column names.
    #[must_use]
    pub fn column_names(&self) -> Vec<&str> {
        self.columns.iter().map(|c| c.name.as_str()).collect()
    }

    /// Add a foreign key relationship.
    pub fn add_foreign_key(&mut self, fk: ForeignKey) {
        self.foreign_keys.push(fk);
    }
}

impl ColumnInfo {
    /// Create a new column descriptor.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        data_type: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            data_type: data_type.into(),
            is_primary_key: false,
            nullable: true,
        }
    }

    /// Mark this column as a primary key.
    #[must_use]
    pub fn primary_key(mut self) -> Self {
        self.is_primary_key = true;
        self.nullable = false;
        self
    }

    /// Mark this column as not nullable.
    #[must_use]
    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }

    /// Whether this column holds numeric data.
    #[must_use]
    pub fn is_numeric(&self) -> bool {
        let dt = self.data_type.to_uppercase();
        dt.contains("INT")
            || dt.contains("REAL")
            || dt.contains("FLOAT")
            || dt.contains("DOUBLE")
            || dt.contains("NUMERIC")
            || dt.contains("DECIMAL")
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn sample_schema() -> SchemaInfo {
        let mut schema = SchemaInfo::new();
        schema.add_table(TableInfo::new(
            "users",
            vec![
                ColumnInfo::new("id", "INTEGER").primary_key(),
                ColumnInfo::new("name", "TEXT").not_null(),
                ColumnInfo::new("email", "TEXT"),
                ColumnInfo::new("age", "INTEGER"),
            ],
        ));
        let mut orders = TableInfo::new(
            "orders",
            vec![
                ColumnInfo::new("id", "INTEGER").primary_key(),
                ColumnInfo::new("user_id", "INTEGER").not_null(),
                ColumnInfo::new("amount", "REAL").not_null(),
                ColumnInfo::new("status", "TEXT"),
            ],
        );
        orders.add_foreign_key(ForeignKey {
            columns: vec!["user_id".into()],
            referenced_table: "users".into(),
            referenced_columns: vec!["id".into()],
        });
        schema.add_table(orders);
        schema
    }

    #[test]
    fn find_table_case_insensitive() {
        let schema = sample_schema();
        assert!(schema.find_table("Users").is_some());
        assert!(schema.find_table("ORDERS").is_some());
        assert!(schema.find_table("missing").is_none());
    }

    #[test]
    fn tables_with_column_finds_shared() {
        let schema = sample_schema();
        let tables = schema.tables_with_column("id");
        assert_eq!(tables.len(), 2);
    }

    #[test]
    fn column_is_numeric() {
        assert!(ColumnInfo::new("x", "INTEGER").is_numeric());
        assert!(ColumnInfo::new("x", "REAL").is_numeric());
        assert!(!ColumnInfo::new("x", "TEXT").is_numeric());
    }

    #[test]
    fn table_column_names() {
        let schema = sample_schema();
        let users = schema.find_table("users").expect("test");
        assert_eq!(
            users.column_names(),
            vec!["id", "name", "email", "age"]
        );
    }

    #[test]
    fn find_column_case_insensitive() {
        let schema = sample_schema();
        let users = schema.find_table("users").expect("test");
        assert!(users.find_column("NAME").is_some());
        assert!(users.find_column("missing").is_none());
    }

    #[test]
    fn primary_key_not_nullable() {
        let col = ColumnInfo::new("id", "INTEGER").primary_key();
        assert!(col.is_primary_key);
        assert!(!col.nullable);
    }

    #[test]
    fn schema_table_names() {
        let schema = sample_schema();
        let mut names = schema.table_names();
        names.sort_unstable();
        assert_eq!(names, vec!["orders", "users"]);
    }

    #[test]
    fn serialize_roundtrip() {
        let schema = sample_schema();
        let json = serde_json::to_string(&schema).expect("test");
        let deserialized: SchemaInfo =
            serde_json::from_str(&json).expect("test");
        assert_eq!(deserialized.tables.len(), 2);
    }
}
