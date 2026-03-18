//! Database connector trait for gathering metadata.
//!
//! Defines the interface that all database backends must implement
//! to provide schema information, statistics, and EXPLAIN plans.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::MetadataError;
use crate::explain::ExplainPlan;

/// Information about a database schema gathered from system catalogs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaInfo {
    /// Database name or identifier.
    pub database: String,
    /// Tables in the schema.
    pub tables: Vec<TableInfo>,
    /// Views in the schema.
    pub views: Vec<ViewInfo>,
}

/// Metadata about a table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableInfo {
    /// Schema name (e.g., "public" for `PostgreSQL`).
    pub schema: String,
    /// Table name.
    pub name: String,
    /// Columns in the table.
    pub columns: Vec<ColumnInfo>,
    /// Constraints on the table.
    pub constraints: Vec<ConstraintInfo>,
    /// Indexes on the table.
    pub indexes: Vec<IndexInfo>,
    /// Estimated row count.
    pub estimated_rows: Option<u64>,
}

/// Metadata about a column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnInfo {
    /// Column name.
    pub name: String,
    /// Data type as reported by the database.
    pub data_type: String,
    /// Whether the column allows NULLs.
    pub nullable: bool,
    /// Default value expression, if any.
    pub default_value: Option<String>,
    /// Ordinal position (1-based).
    pub ordinal_position: u32,
}

/// Metadata about a constraint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstraintInfo {
    /// Constraint name.
    pub name: String,
    /// Constraint type.
    pub constraint_type: ConstraintType,
    /// Columns involved in the constraint.
    pub columns: Vec<String>,
    /// Referenced table for foreign keys.
    pub references_table: Option<String>,
    /// Referenced columns for foreign keys.
    pub references_columns: Option<Vec<String>>,
}

/// Types of database constraints.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConstraintType {
    /// Primary key constraint.
    PrimaryKey,
    /// Unique constraint.
    Unique,
    /// Foreign key constraint.
    ForeignKey,
    /// Check constraint.
    Check,
    /// Not null constraint.
    NotNull,
}

/// Metadata about an index.
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
    /// Whether this is the table's primary key index.
    pub primary: bool,
}

/// Metadata about a view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewInfo {
    /// Schema name.
    pub schema: String,
    /// View name.
    pub name: String,
    /// Columns in the view.
    pub columns: Vec<ColumnInfo>,
    /// View definition SQL, if available.
    pub definition: Option<String>,
}

/// Statistics gathered for a specific table from the database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatheredTableStats {
    /// Table name.
    pub table: String,
    /// Row count.
    pub row_count: u64,
    /// Total size in bytes.
    pub total_size_bytes: u64,
    /// Per-column statistics keyed by column name.
    pub columns: HashMap<String, GatheredColumnStats>,
    /// Index statistics keyed by index name.
    pub indexes: HashMap<String, GatheredIndexStats>,
}

/// Column-level statistics from the database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatheredColumnStats {
    /// Number of distinct values.
    pub distinct_count: u64,
    /// Fraction of NULL values (0.0 to 1.0).
    pub null_fraction: f64,
    /// Average column width in bytes.
    pub avg_width: f64,
    /// Correlation with physical row order (-1.0 to 1.0).
    pub correlation: Option<f64>,
    /// Most common values with their frequencies.
    pub most_common_values: Option<Vec<(String, f64)>>,
}

/// Index-level statistics from the database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatheredIndexStats {
    /// Index size in bytes.
    pub size_bytes: u64,
    /// Number of index scans performed.
    pub scans: Option<u64>,
    /// Number of tuples read via index.
    pub tuples_read: Option<u64>,
    /// Number of tuples fetched via index.
    pub tuples_fetched: Option<u64>,
}

/// Parsed connection string identifying a database backend.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionTarget {
    /// `PostgreSQL`: `postgresql://user:pass@host:port/dbname`
    PostgreSql(String),
    /// `MySQL`: `mysql://user:pass@host:port/dbname`
    MySql(String),
    /// `SQLite`: `sqlite:///path/to/db` or just a file path.
    Sqlite(String),
}

/// Parse a connection string into a [`ConnectionTarget`].
///
/// # Errors
///
/// Returns [`MetadataError::InvalidConnectionString`] if the
/// connection string doesn't match any supported format.
///
/// Supported formats:
/// - `postgresql://...` or `postgres://...`
/// - `mysql://...`
/// - `sqlite:///path` or `sqlite://path` or plain file paths
///   ending in `.db`/`.sqlite`
pub fn parse_connection_string(
    conn: &str,
) -> Result<ConnectionTarget, MetadataError> {
    let trimmed = conn.trim();

    if trimmed.starts_with("postgresql://")
        || trimmed.starts_with("postgres://")
    {
        return Ok(ConnectionTarget::PostgreSql(trimmed.to_string()));
    }

    if trimmed.starts_with("mysql://") {
        return Ok(ConnectionTarget::MySql(trimmed.to_string()));
    }

    if let Some(path) = trimmed.strip_prefix("sqlite:///") {
        return Ok(ConnectionTarget::Sqlite(path.to_string()));
    }

    if let Some(path) = trimmed.strip_prefix("sqlite://") {
        return Ok(ConnectionTarget::Sqlite(path.to_string()));
    }

    let path = std::path::Path::new(trimmed);
    if let Some(ext) = path.extension() {
        let ext_lower = ext.to_string_lossy().to_lowercase();
        if ext_lower == "db"
            || ext_lower == "sqlite"
            || ext_lower == "sqlite3"
        {
            return Ok(ConnectionTarget::Sqlite(
                trimmed.to_string(),
            ));
        }
    }

    Err(MetadataError::InvalidConnectionString {
        input: trimmed.to_string(),
        reason: "unsupported scheme; expected postgresql://, mysql://, \
                 or sqlite:// prefix"
            .to_string(),
    })
}

/// Trait for gathering metadata from a database.
///
/// Each database backend implements this trait to provide
/// schema information, table statistics, and `EXPLAIN` plan output.
pub trait DatabaseConnector {
    /// Gather the full schema from the connected database.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] if the catalog query fails.
    fn gather_schema(&self) -> Result<SchemaInfo, MetadataError>;

    /// Gather statistics for a specific table.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] if the table is not found or
    /// the statistics query fails.
    fn gather_statistics(
        &self,
        table: &str,
    ) -> Result<GatheredTableStats, MetadataError>;

    /// Run `EXPLAIN` on a SQL query and parse the output.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] if the query fails or the
    /// output cannot be parsed.
    fn explain_query(
        &self,
        sql: &str,
    ) -> Result<ExplainPlan, MetadataError>;

    /// Return the database engine name (e.g., "`PostgreSQL`", "`MySQL`").
    fn engine_name(&self) -> &str;

    /// Close the connection (best effort).
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] if the connection cannot be closed.
    fn close(&self) -> Result<(), MetadataError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_postgresql_connection() {
        let target = parse_connection_string(
            "postgresql://user:pass@localhost:5432/mydb",
        );
        assert!(matches!(target, Ok(ConnectionTarget::PostgreSql(_))));
    }

    #[test]
    fn parse_postgres_shorthand() {
        let target = parse_connection_string(
            "postgres://user:pass@localhost/mydb",
        );
        assert!(matches!(target, Ok(ConnectionTarget::PostgreSql(_))));
    }

    #[test]
    fn parse_mysql_connection() {
        let target = parse_connection_string(
            "mysql://user:pass@localhost:3306/mydb",
        );
        assert!(matches!(target, Ok(ConnectionTarget::MySql(_))));
    }

    #[test]
    fn parse_sqlite_triple_slash() {
        let target =
            parse_connection_string("sqlite:///tmp/test.db");
        match target {
            Ok(ConnectionTarget::Sqlite(path)) => {
                assert_eq!(path, "tmp/test.db");
            }
            other => panic!("expected Sqlite, got {other:?}"),
        }
    }

    #[test]
    fn parse_sqlite_double_slash() {
        let target =
            parse_connection_string("sqlite://path/to/test.db");
        match target {
            Ok(ConnectionTarget::Sqlite(path)) => {
                assert_eq!(path, "path/to/test.db");
            }
            other => panic!("expected Sqlite, got {other:?}"),
        }
    }

    #[test]
    fn parse_sqlite_file_extension() {
        let target =
            parse_connection_string("/var/data/app.sqlite3");
        match target {
            Ok(ConnectionTarget::Sqlite(path)) => {
                assert_eq!(path, "/var/data/app.sqlite3");
            }
            other => panic!("expected Sqlite, got {other:?}"),
        }
    }

    #[test]
    fn parse_invalid_scheme() {
        let target =
            parse_connection_string("ftp://server/db");
        assert!(matches!(
            target,
            Err(MetadataError::InvalidConnectionString { .. })
        ));
    }

    #[test]
    fn schema_info_serialization() {
        let schema = SchemaInfo {
            database: "test".to_string(),
            tables: vec![],
            views: vec![],
        };
        let json = serde_json::to_string(&schema)
            .expect("serialization should succeed");
        let roundtrip: SchemaInfo = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(schema, roundtrip);
    }

    #[test]
    fn constraint_type_variants() {
        let types = [
            ConstraintType::PrimaryKey,
            ConstraintType::Unique,
            ConstraintType::ForeignKey,
            ConstraintType::Check,
            ConstraintType::NotNull,
        ];
        assert_eq!(types.len(), 5);
    }

    #[test]
    fn gathered_table_stats_roundtrip() {
        let stats = GatheredTableStats {
            table: "users".to_string(),
            row_count: 10_000,
            total_size_bytes: 1_048_576,
            columns: HashMap::new(),
            indexes: HashMap::new(),
        };
        let json = serde_json::to_string(&stats)
            .expect("serialization should succeed");
        let roundtrip: GatheredTableStats =
            serde_json::from_str(&json)
                .expect("deserialization should succeed");
        assert_eq!(stats, roundtrip);
    }
}
