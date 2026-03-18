//! Schema metadata types for database introspection.
//!
//! These types represent the structure of a database as discovered
//! through system catalog queries: tables, columns, constraints,
//! indexes, and their properties.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Full schema information for a database or schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaInfo {
    /// Database backend that produced this schema.
    pub kind: DatabaseKind,
    /// Schema or database name.
    pub schema_name: String,
    /// Tables, keyed by table name.
    pub tables: HashMap<String, TableInfo>,
}

impl SchemaInfo {
    /// Look up a table by name.
    #[must_use]
    pub fn get_table(&self, name: &str) -> Option<&TableInfo> {
        self.tables.get(name)
    }

    /// Returns the number of tables in this schema.
    #[must_use]
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }
}

/// Supported database backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DatabaseKind {
    /// `PostgreSQL` (9.6+).
    PostgreSQL,
    /// `MySQL` (5.7+ / 8.0+).
    MySQL,
    /// `SQLite` (3.x).
    SQLite,
}

impl std::fmt::Display for DatabaseKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PostgreSQL => write!(f, "PostgreSQL"),
            Self::MySQL => write!(f, "MySQL"),
            Self::SQLite => write!(f, "SQLite"),
        }
    }
}

/// Information about a single table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableInfo {
    /// Table name.
    pub name: String,
    /// Columns in declaration order.
    pub columns: Vec<ColumnInfo>,
    /// Constraints (primary key, foreign key, unique, check).
    pub constraints: Vec<ConstraintInfo>,
    /// Indexes on this table.
    pub indexes: Vec<IndexInfo>,
    /// Estimated row count (from catalog statistics).
    pub estimated_rows: Option<f64>,
}

impl TableInfo {
    /// Look up a column by name.
    #[must_use]
    pub fn get_column(&self, name: &str) -> Option<&ColumnInfo> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// Returns the primary key columns, if a primary key exists.
    #[must_use]
    pub fn primary_key_columns(&self) -> Vec<&str> {
        for constraint in &self.constraints {
            if constraint.kind == ConstraintKind::PrimaryKey {
                return constraint.columns.iter().map(String::as_str).collect();
            }
        }
        Vec::new()
    }

    /// Returns the number of columns.
    #[must_use]
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }
}

/// Information about a single column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnInfo {
    /// Column name.
    pub name: String,
    /// Data type as reported by the database.
    pub data_type: String,
    /// Whether the column accepts NULL values.
    pub nullable: bool,
    /// Ordinal position (1-based).
    pub ordinal: u32,
    /// Default value expression, if any.
    pub default_value: Option<String>,
}

/// A table constraint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstraintInfo {
    /// Constraint name (may be auto-generated).
    pub name: String,
    /// Constraint kind.
    pub kind: ConstraintKind,
    /// Columns involved.
    pub columns: Vec<String>,
    /// For foreign keys, the referenced table.
    pub referenced_table: Option<String>,
    /// For foreign keys, the referenced columns.
    pub referenced_columns: Vec<String>,
}

/// Kinds of table constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConstraintKind {
    /// Primary key.
    PrimaryKey,
    /// Foreign key.
    ForeignKey,
    /// Unique constraint.
    Unique,
    /// Check constraint.
    Check,
}

impl std::fmt::Display for ConstraintKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PrimaryKey => write!(f, "PRIMARY KEY"),
            Self::ForeignKey => write!(f, "FOREIGN KEY"),
            Self::Unique => write!(f, "UNIQUE"),
            Self::Check => write!(f, "CHECK"),
        }
    }
}

/// Information about an index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexInfo {
    /// Index name.
    pub name: String,
    /// Columns in the index.
    pub columns: Vec<String>,
    /// Whether the index enforces uniqueness.
    pub unique: bool,
    /// Index type (btree, hash, gin, gist, etc.).
    pub index_type: String,
}

/// Statistics gathered for a single table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableStats {
    /// Table name.
    pub table_name: String,
    /// Estimated total row count.
    pub row_count: f64,
    /// Total size in bytes (data + indexes).
    pub total_bytes: u64,
    /// Per-column statistics, keyed by column name.
    pub columns: HashMap<String, ColumnStatistics>,
}

impl TableStats {
    /// Convert to the core statistics type used by the optimizer.
    #[must_use]
    pub fn to_core_statistics(&self) -> ra_core::Statistics {
        let mut stats = ra_core::Statistics::new(self.row_count);
        stats.total_size = self.total_bytes;

        for (col_name, col_stats) in &self.columns {
            let mut core_col = ra_core::ColumnStats::new(col_stats.distinct_count);
            core_col.null_fraction = col_stats.null_fraction;
            core_col.avg_length = col_stats.avg_width;
            stats.columns.insert(col_name.clone(), core_col);
        }

        stats
    }
}

/// Statistics for a single column, gathered from the database catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnStatistics {
    /// Column name.
    pub column_name: String,
    /// Number of distinct values (NDV).
    pub distinct_count: f64,
    /// Fraction of NULL values in [0.0, 1.0].
    pub null_fraction: f64,
    /// Average width in bytes.
    pub avg_width: Option<f64>,
    /// Most common values (value, frequency).
    pub most_common_values: Vec<(String, f64)>,
    /// Histogram bounds (for equi-depth histograms).
    pub histogram_bounds: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_kind_display() {
        assert_eq!(DatabaseKind::PostgreSQL.to_string(), "PostgreSQL");
        assert_eq!(DatabaseKind::MySQL.to_string(), "MySQL");
        assert_eq!(DatabaseKind::SQLite.to_string(), "SQLite");
    }

    #[test]
    fn constraint_kind_display() {
        assert_eq!(ConstraintKind::PrimaryKey.to_string(), "PRIMARY KEY");
        assert_eq!(ConstraintKind::ForeignKey.to_string(), "FOREIGN KEY");
        assert_eq!(ConstraintKind::Unique.to_string(), "UNIQUE");
        assert_eq!(ConstraintKind::Check.to_string(), "CHECK");
    }

    #[test]
    fn table_stats_to_core() {
        let mut columns = HashMap::new();
        columns.insert(
            "id".to_owned(),
            ColumnStatistics {
                column_name: "id".to_owned(),
                distinct_count: 1000.0,
                null_fraction: 0.0,
                avg_width: Some(4.0),
                most_common_values: vec![],
                histogram_bounds: vec![],
            },
        );
        columns.insert(
            "name".to_owned(),
            ColumnStatistics {
                column_name: "name".to_owned(),
                distinct_count: 800.0,
                null_fraction: 0.05,
                avg_width: Some(32.0),
                most_common_values: vec![],
                histogram_bounds: vec![],
            },
        );

        let table_stats = TableStats {
            table_name: "users".to_owned(),
            row_count: 1000.0,
            total_bytes: 64000,
            columns,
        };

        let core = table_stats.to_core_statistics();
        assert!((core.row_count - 1000.0).abs() < f64::EPSILON);
        assert_eq!(core.total_size, 64000);
        assert_eq!(core.columns.len(), 2);

        let id_stats = core.columns.get("id").expect("id column");
        assert!((id_stats.distinct_count - 1000.0).abs() < f64::EPSILON);
        assert!(id_stats.null_fraction.abs() < f64::EPSILON);

        let name_stats = core.columns.get("name").expect("name column");
        assert!((name_stats.null_fraction - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn schema_info_serialize_roundtrip() {
        let schema = SchemaInfo {
            kind: DatabaseKind::PostgreSQL,
            schema_name: "public".to_owned(),
            tables: HashMap::new(),
        };

        let json =
            serde_json::to_string(&schema).expect("serialization should succeed");
        let deserialized: SchemaInfo =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(schema, deserialized);
    }

    #[test]
    fn table_info_with_columns_and_constraints() {
        let table = TableInfo {
            name: "orders".to_owned(),
            columns: vec![
                ColumnInfo {
                    name: "id".to_owned(),
                    data_type: "integer".to_owned(),
                    nullable: false,
                    ordinal: 1,
                    default_value: None,
                },
                ColumnInfo {
                    name: "amount".to_owned(),
                    data_type: "numeric(10,2)".to_owned(),
                    nullable: true,
                    ordinal: 2,
                    default_value: Some("0.00".to_owned()),
                },
            ],
            constraints: vec![ConstraintInfo {
                name: "orders_pkey".to_owned(),
                kind: ConstraintKind::PrimaryKey,
                columns: vec!["id".to_owned()],
                referenced_table: None,
                referenced_columns: vec![],
            }],
            indexes: vec![IndexInfo {
                name: "orders_pkey".to_owned(),
                columns: vec!["id".to_owned()],
                unique: true,
                index_type: "btree".to_owned(),
            }],
            estimated_rows: Some(50000.0),
        };

        assert_eq!(table.columns.len(), 2);
        assert_eq!(table.constraints.len(), 1);
        assert_eq!(table.indexes.len(), 1);
    }

    #[test]
    fn table_info_get_column() {
        let table = TableInfo {
            name: "test".to_owned(),
            columns: vec![
                ColumnInfo {
                    name: "id".to_owned(),
                    data_type: "integer".to_owned(),
                    nullable: false,
                    ordinal: 1,
                    default_value: None,
                },
                ColumnInfo {
                    name: "name".to_owned(),
                    data_type: "text".to_owned(),
                    nullable: true,
                    ordinal: 2,
                    default_value: None,
                },
            ],
            constraints: vec![],
            indexes: vec![],
            estimated_rows: None,
        };

        assert!(table.get_column("id").is_some());
        assert!(table.get_column("missing").is_none());
        assert_eq!(table.column_count(), 2);
    }

    #[test]
    fn table_info_primary_key_columns() {
        let table = TableInfo {
            name: "test".to_owned(),
            columns: vec![],
            constraints: vec![
                ConstraintInfo {
                    name: "test_pkey".to_owned(),
                    kind: ConstraintKind::PrimaryKey,
                    columns: vec!["id".to_owned(), "tenant_id".to_owned()],
                    referenced_table: None,
                    referenced_columns: vec![],
                },
                ConstraintInfo {
                    name: "test_name_unique".to_owned(),
                    kind: ConstraintKind::Unique,
                    columns: vec!["name".to_owned()],
                    referenced_table: None,
                    referenced_columns: vec![],
                },
            ],
            indexes: vec![],
            estimated_rows: None,
        };

        let pk = table.primary_key_columns();
        assert_eq!(pk, vec!["id", "tenant_id"]);
    }

    #[test]
    fn table_info_no_primary_key() {
        let table = TableInfo {
            name: "heap_table".to_owned(),
            columns: vec![],
            constraints: vec![],
            indexes: vec![],
            estimated_rows: None,
        };

        assert!(table.primary_key_columns().is_empty());
    }

    #[test]
    fn schema_info_get_table() {
        let mut tables = HashMap::new();
        tables.insert(
            "users".to_owned(),
            TableInfo {
                name: "users".to_owned(),
                columns: vec![],
                constraints: vec![],
                indexes: vec![],
                estimated_rows: Some(1000.0),
            },
        );

        let schema = SchemaInfo {
            kind: DatabaseKind::SQLite,
            schema_name: "main".to_owned(),
            tables,
        };

        assert!(schema.get_table("users").is_some());
        assert!(schema.get_table("missing").is_none());
        assert_eq!(schema.table_count(), 1);
    }

    #[test]
    fn column_statistics_most_common_values() {
        let stats = ColumnStatistics {
            column_name: "status".to_owned(),
            distinct_count: 3.0,
            null_fraction: 0.0,
            avg_width: Some(8.0),
            most_common_values: vec![
                ("active".to_owned(), 0.7),
                ("pending".to_owned(), 0.2),
                ("closed".to_owned(), 0.1),
            ],
            histogram_bounds: vec![],
        };

        assert_eq!(stats.most_common_values.len(), 3);
        assert!((stats.most_common_values[0].1 - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn column_statistics_histogram_bounds() {
        let stats = ColumnStatistics {
            column_name: "age".to_owned(),
            distinct_count: 80.0,
            null_fraction: 0.01,
            avg_width: Some(4.0),
            most_common_values: vec![],
            histogram_bounds: vec![
                "18".to_owned(),
                "30".to_owned(),
                "45".to_owned(),
                "65".to_owned(),
                "99".to_owned(),
            ],
        };

        assert_eq!(stats.histogram_bounds.len(), 5);
    }

    #[test]
    fn table_stats_empty_columns() {
        let stats = TableStats {
            table_name: "empty_stats".to_owned(),
            row_count: 0.0,
            total_bytes: 0,
            columns: HashMap::new(),
        };

        let core = stats.to_core_statistics();
        assert!(core.row_count.abs() < f64::EPSILON);
        assert!(core.columns.is_empty());
    }

    #[test]
    fn index_info_fields() {
        let idx = IndexInfo {
            name: "idx_users_email".to_owned(),
            columns: vec!["email".to_owned()],
            unique: true,
            index_type: "btree".to_owned(),
        };

        assert!(idx.unique);
        assert_eq!(idx.index_type, "btree");
    }

    #[test]
    fn constraint_info_foreign_key() {
        let fk = ConstraintInfo {
            name: "orders_user_fk".to_owned(),
            kind: ConstraintKind::ForeignKey,
            columns: vec!["user_id".to_owned()],
            referenced_table: Some("users".to_owned()),
            referenced_columns: vec!["id".to_owned()],
        };

        assert_eq!(fk.kind, ConstraintKind::ForeignKey);
        assert_eq!(fk.referenced_table.as_deref(), Some("users"));
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn table_stats_serialize_roundtrip() {
        let stats = TableStats {
            table_name: "test".to_owned(),
            row_count: 500.0,
            total_bytes: 32000,
            columns: HashMap::new(),
        };

        let json = serde_json::to_string(&stats).expect("serialization should succeed");
        let deserialized: TableStats =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(stats, deserialized);
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn column_info_serialize_roundtrip() {
        let col = ColumnInfo {
            name: "email".to_owned(),
            data_type: "varchar(255)".to_owned(),
            nullable: false,
            ordinal: 3,
            default_value: None,
        };

        let json = serde_json::to_string(&col).expect("serialization should succeed");
        let deserialized: ColumnInfo =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(col, deserialized);
    }
}
