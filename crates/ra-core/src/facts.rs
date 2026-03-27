//! Unified interface for accessing system facts.
//!
//! This module provides the `FactsProvider` trait which gives pre-condition
//! evaluators access to all system facts needed for rule filtering:
//! - Statistics (cardinality, NDV, selectivity, histograms)
//! - Hardware profile (CPU, memory, GPU, SIMD capabilities)
//! - Schema information (tables, columns, indexes, constraints)
//! - Runtime statistics (actual cardinality, execution time, memory usage)
//! - Database capabilities (supported features, SQL dialect)
//!
//! # Example
//!
//! ```
//! use ra_core::facts::FactsProvider;
//!
//! fn check_large_table(facts: &dyn FactsProvider, table: &str) -> bool {
//!     if let Some(stats) = facts.get_table_stats(table) {
//!         stats.row_count > 1_000_000.0
//!     } else {
//!         false
//!     }
//! }
//! ```

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Statistics for a single table
#[derive(Debug, Clone)]
pub struct TableStats {
    /// Number of rows
    pub row_count: f64,
    /// Number of pages/blocks
    pub page_count: u64,
    /// Average row size in bytes
    pub average_row_size: f64,
    /// Total table size in bytes
    pub table_size_bytes: u64,
    /// Live tuples (excluding deleted)
    pub live_tuples: Option<f64>,
    /// Dead tuples (deleted but not vacuumed)
    pub dead_tuples: Option<f64>,
    /// Unix timestamp of last ANALYZE
    pub last_analyzed: Option<i64>,
    /// Confidence in these statistics (0.0 to 1.0)
    pub confidence: f64,
}

// Re-export ColumnStats from statistics module
pub use crate::statistics::ColumnStats;

/// Data type of a column
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataType {
    /// Integer types
    Integer,
    /// Floating point types
    Float,
    /// String/text types
    String,
    /// Boolean type
    Boolean,
    /// Date/time types
    Timestamp,
    /// Binary data
    Binary,
    /// JSON/JSONB
    Json,
    /// Array types
    Array(Box<DataType>),
    /// Other/unknown type
    Other(String),
}

impl DataType {
    /// Check if this is a numeric type
    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Integer | Self::Float)
    }

    /// Check if this is a string type
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String)
    }

    /// Check if this is a temporal type
    pub fn is_temporal(&self) -> bool {
        matches!(self, Self::Timestamp)
    }
}

impl std::fmt::Display for DataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Integer => write!(f, "integer"),
            Self::Float => write!(f, "float"),
            Self::String => write!(f, "string"),
            Self::Boolean => write!(f, "boolean"),
            Self::Timestamp => write!(f, "timestamp"),
            Self::Binary => write!(f, "binary"),
            Self::Json => write!(f, "json"),
            Self::Array(inner) => write!(f, "array[{inner}]"),
            Self::Other(name) => write!(f, "{name}"),
        }
    }
}

/// Type of index
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexType {
    /// B-tree index
    BTree,
    /// Hash index
    Hash,
    /// GiST (Generalized Search Tree)
    Gist,
    /// GIN (Generalized Inverted Index)
    Gin,
    /// SP-GiST (Space-Partitioned GiST)
    SpGist,
    /// BRIN (Block Range Index)
    Brin,
    /// RUM (GIN extension with distance ordering)
    Rum,
    /// Bitmap index
    Bitmap,
    /// Unknown or unsupported index type
    Unknown,
}

/// Storage format for a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StorageFormat {
    /// Row-based storage (heap tables, MyISAM)
    RowBased,
    /// Column-based storage (columnar)
    Columnar,
    /// Parquet files
    Parquet,
    /// ORC files
    Orc,
    /// Arrow IPC
    ArrowIpc,
    /// Unknown or mixed format
    #[default]
    Unknown,
}

impl StorageFormat {
    /// Check if this format is Parquet.
    #[must_use]
    pub fn is_parquet(&self) -> bool {
        matches!(self, Self::Parquet)
    }

    /// Check if this format is columnar (Parquet, ORC, Arrow, or generic columnar).
    #[must_use]
    pub fn is_columnar(&self) -> bool {
        matches!(
            self,
            Self::Columnar | Self::Parquet | Self::Orc | Self::ArrowIpc
        )
    }

    /// Check if this format supports predicate pushdown via metadata.
    #[must_use]
    pub fn supports_metadata_pushdown(&self) -> bool {
        matches!(self, Self::Parquet | Self::Orc)
    }
}

/// Table schema information
#[derive(Debug, Clone)]
pub struct TableInfo {
    /// Table name
    pub name: String,
    /// Column names and types
    pub columns: Vec<(String, DataType)>,
    /// Primary key columns
    pub primary_key: Vec<String>,
    /// Foreign key constraints
    pub foreign_keys: Vec<ForeignKey>,
    /// Available indexes
    pub indexes: Vec<IndexInfo>,
    /// Storage format for this table
    pub storage_format: StorageFormat,
}

/// Foreign key constraint
#[derive(Debug, Clone)]
pub struct ForeignKey {
    /// Columns in this table
    pub columns: Vec<String>,
    /// Referenced table
    pub referenced_table: String,
    /// Referenced columns
    pub referenced_columns: Vec<String>,
}

/// Index information
#[derive(Debug, Clone)]
pub struct IndexInfo {
    /// Index name
    pub name: String,
    /// Index type
    pub index_type: IndexType,
    /// Indexed columns (key columns)
    pub columns: Vec<String>,
    /// Included (non-key) columns (SQL Server INCLUDE style)
    pub included_columns: Vec<String>,
    /// Whether the index is unique
    pub is_unique: bool,
}

/// Hardware profile
#[derive(Debug, Clone)]
pub struct HardwareProfile {
    /// Number of CPU cores
    pub cpu_cores: u32,
    /// Available memory in bytes
    pub available_memory: u64,
    /// Total memory in bytes
    pub total_memory: u64,
    /// SIMD width in bits (128 for SSE, 256 for AVX2, 512 for AVX-512)
    pub simd_width: u32,
    /// Whether GPU is available
    pub has_gpu: bool,
    /// GPU memory in bytes (if available)
    pub gpu_memory: Option<u64>,
    /// L1 cache size in bytes
    pub l1_cache_size: u64,
    /// L2 cache size in bytes
    pub l2_cache_size: u64,
    /// L3 cache size in bytes
    pub l3_cache_size: u64,
}

/// Runtime statistics for an operator
#[derive(Debug, Clone)]
pub struct OperatorStats {
    /// Operator ID
    pub operator_id: String,
    /// Actual number of rows produced
    pub actual_rows: f64,
    /// Estimated number of rows
    pub estimated_rows: f64,
    /// Actual execution time
    pub execution_time: Duration,
    /// Memory used in bytes
    pub memory_used: u64,
    /// Whether skew was detected
    pub skew_detected: bool,
}

/// SQL dialect
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlDialect {
    /// PostgreSQL
    Postgres,
    /// MySQL
    Mysql,
    /// Oracle
    Oracle,
    /// Microsoft SQL Server
    SqlServer,
    /// SQLite
    Sqlite,
    /// DuckDB
    DuckDb,
    /// Generic SQL
    Generic,
}

impl std::fmt::Display for SqlDialect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Postgres => write!(f, "postgresql"),
            Self::Mysql => write!(f, "mysql"),
            Self::Oracle => write!(f, "oracle"),
            Self::SqlServer => write!(f, "sqlserver"),
            Self::Sqlite => write!(f, "sqlite"),
            Self::DuckDb => write!(f, "duckdb"),
            Self::Generic => write!(f, "generic"),
        }
    }
}

/// Unified interface for querying system facts
pub trait FactsProvider: Send + Sync {
    /// Get statistics for a table
    fn get_table_stats(&self, table: &str) -> Option<&TableStats>;

    /// Get statistics for a column
    fn get_column_stats(&self, table: &str, column: &str) -> Option<&ColumnStats>;

    /// Get hardware profile
    fn hardware_profile(&self) -> &HardwareProfile;

    /// Get available memory in bytes
    fn available_memory(&self) -> u64 {
        self.hardware_profile().available_memory
    }

    /// Get number of CPU cores
    fn cpu_cores(&self) -> u32 {
        self.hardware_profile().cpu_cores
    }

    /// Check if GPU is available
    fn has_gpu(&self) -> bool {
        self.hardware_profile().has_gpu
    }

    /// Get SIMD width in bits
    fn simd_width(&self) -> u32 {
        self.hardware_profile().simd_width
    }

    /// Get schema information for a table
    fn get_schema(&self, table: &str) -> Option<&TableInfo>;

    /// Get column data type
    fn column_type(&self, table: &str, column: &str) -> Option<DataType> {
        self.get_schema(table).and_then(|schema| {
            schema
                .columns
                .iter()
                .find(|(name, _)| name == column)
                .map(|(_, dtype)| dtype.clone())
        })
    }

    /// Check if an index exists on the given columns
    fn has_index(&self, table: &str, columns: &[&str], index_type: Option<IndexType>) -> bool {
        if let Some(schema) = self.get_schema(table) {
            schema.indexes.iter().any(|idx| {
                let cols_match = idx.columns.len() == columns.len()
                    && idx
                        .columns
                        .iter()
                        .zip(columns.iter())
                        .all(|(a, b)| a == b);

                let type_match = index_type.map_or(true, |t| idx.index_type == t);

                cols_match && type_match
            })
        } else {
            false
        }
    }

    /// Check if a table has a primary key
    fn has_primary_key(&self, table: &str) -> bool {
        self.get_schema(table)
            .map_or(false, |schema| !schema.primary_key.is_empty())
    }

    /// Get foreign keys for a table
    fn foreign_keys(&self, table: &str) -> Vec<&ForeignKey> {
        self.get_schema(table)
            .map_or_else(Vec::new, |schema| schema.foreign_keys.iter().collect())
    }

    /// Get indexes that cover all the requested columns.
    ///
    /// A covering index includes all `needed_columns` in either its
    /// key columns or its included columns, allowing an index-only
    /// scan without heap access.
    fn get_covering_indexes(
        &self,
        table: &str,
        needed_columns: &[&str],
    ) -> Vec<&IndexInfo> {
        let Some(schema) = self.get_schema(table) else {
            return Vec::new();
        };
        schema
            .indexes
            .iter()
            .filter(|idx| {
                needed_columns.iter().all(|col| {
                    idx.columns.iter().any(|c| c == col)
                        || idx.included_columns.iter().any(|c| c == col)
                })
            })
            .collect()
    }

    /// Check whether a covering index exists for the given columns.
    fn has_covering_index(
        &self,
        table: &str,
        needed_columns: &[&str],
    ) -> bool {
        !self.get_covering_indexes(table, needed_columns).is_empty()
    }

    /// Get runtime statistics for an operator
    fn runtime_stats(&self, operator_id: &str) -> Option<&OperatorStats>;

    /// Get cardinality estimation error for an operator
    fn cardinality_error(&self, operator_id: &str) -> Option<f64> {
        self.runtime_stats(operator_id).map(|stats| {
            if stats.estimated_rows > 0.0 {
                (stats.actual_rows / stats.estimated_rows).max(stats.estimated_rows / stats.actual_rows)
            } else {
                f64::INFINITY
            }
        })
    }

    /// Get database name
    fn database_name(&self) -> &str;

    /// Check if a feature is supported
    fn supports_feature(&self, feature: &str) -> bool;

    /// Get SQL dialect
    fn sql_dialect(&self) -> SqlDialect;

    /// Get memory limit (if configured)
    fn memory_limit(&self) -> Option<u64>;

    /// Get optimizer timeout
    fn optimizer_timeout(&self) -> Duration;
}

/// Empty facts provider for testing
#[derive(Debug, Clone)]
pub struct EmptyFactsProvider {
    hardware: HardwareProfile,
}

impl EmptyFactsProvider {
    /// Create a new empty facts provider with default hardware profile
    pub fn new() -> Self {
        Self {
            hardware: HardwareProfile {
                cpu_cores: 8,
                available_memory: 16 * 1024 * 1024 * 1024,
                total_memory: 16 * 1024 * 1024 * 1024,
                simd_width: 256,
                has_gpu: false,
                gpu_memory: None,
                l1_cache_size: 32 * 1024,
                l2_cache_size: 256 * 1024,
                l3_cache_size: 8 * 1024 * 1024,
            },
        }
    }
}

impl Default for EmptyFactsProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FactsProvider for EmptyFactsProvider {
    fn get_table_stats(&self, _table: &str) -> Option<&TableStats> {
        None
    }

    fn get_column_stats(&self, _table: &str, _column: &str) -> Option<&ColumnStats> {
        None
    }

    fn hardware_profile(&self) -> &HardwareProfile {
        &self.hardware
    }

    fn get_schema(&self, _table: &str) -> Option<&TableInfo> {
        None
    }

    fn runtime_stats(&self, _operator_id: &str) -> Option<&OperatorStats> {
        None
    }

    fn database_name(&self) -> &str {
        "generic"
    }

    fn supports_feature(&self, _feature: &str) -> bool {
        false
    }

    fn sql_dialect(&self) -> SqlDialect {
        SqlDialect::Generic
    }

    fn memory_limit(&self) -> Option<u64> {
        None
    }

    fn optimizer_timeout(&self) -> Duration {
        Duration::from_secs(60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_type_checks() {
        assert!(DataType::Integer.is_numeric());
        assert!(DataType::Float.is_numeric());
        assert!(!DataType::String.is_numeric());

        assert!(DataType::String.is_string());
        assert!(!DataType::Integer.is_string());

        assert!(DataType::Timestamp.is_temporal());
        assert!(!DataType::String.is_temporal());
    }

    #[test]
    fn empty_facts_provider() {
        let facts = EmptyFactsProvider::new();
        assert_eq!(facts.cpu_cores(), 8);
        assert_eq!(facts.simd_width(), 256);
        assert!(!facts.has_gpu());
        assert_eq!(facts.database_name(), "generic");
        assert!(!facts.supports_feature("lateral_join"));
    }

    #[test]
    fn facts_provider_column_type() {
        let facts = EmptyFactsProvider::new();
        assert!(facts.column_type("users", "id").is_none());
    }

    #[test]
    fn facts_provider_has_index() {
        let facts = EmptyFactsProvider::new();
        assert!(!facts.has_index("users", &["id"], None));
        assert!(!facts.has_index("users", &["id"], Some(IndexType::BTree)));
    }

    struct SchemaFacts {
        inner: EmptyFactsProvider,
        schemas: Vec<TableInfo>,
    }
    impl SchemaFacts {
        fn with_table(info: TableInfo) -> Self {
            Self { inner: EmptyFactsProvider::new(), schemas: vec![info] }
        }
    }
    impl FactsProvider for SchemaFacts {
        fn get_table_stats(&self, t: &str) -> Option<&TableStats> { self.inner.get_table_stats(t) }
        fn get_column_stats(&self, t: &str, c: &str) -> Option<&ColumnStats> { self.inner.get_column_stats(t, c) }
        fn hardware_profile(&self) -> &HardwareProfile { self.inner.hardware_profile() }
        fn get_schema(&self, table: &str) -> Option<&TableInfo> { self.schemas.iter().find(|s| s.name == table) }
        fn runtime_stats(&self, id: &str) -> Option<&OperatorStats> { self.inner.runtime_stats(id) }
        fn database_name(&self) -> &str { self.inner.database_name() }
        fn supports_feature(&self, f: &str) -> bool { self.inner.supports_feature(f) }
        fn sql_dialect(&self) -> SqlDialect { self.inner.sql_dialect() }
        fn memory_limit(&self) -> Option<u64> { self.inner.memory_limit() }
        fn optimizer_timeout(&self) -> Duration { self.inner.optimizer_timeout() }
    }

    #[test]
    fn empty_facts_no_covering_indexes() {
        let facts = EmptyFactsProvider::new();
        assert!(!facts.has_covering_index("orders", &["id", "amount"]));
        assert!(facts.get_covering_indexes("orders", &["id"]).is_empty());
    }

    #[test]
    fn covering_index_with_key_columns() {
        let facts = SchemaFacts::with_table(TableInfo {
            name: "orders".to_string(),
            columns: vec![("id".into(), DataType::Integer), ("customer_id".into(), DataType::Integer), ("amount".into(), DataType::Float)],
            primary_key: vec!["id".into()],
            foreign_keys: vec![],
            indexes: vec![IndexInfo {
                name: "idx_orders_cust_amt".into(),
                index_type: IndexType::BTree,
                columns: vec!["customer_id".into(), "amount".into()],
                is_unique: false,
                included_columns: vec![],
            }],
            storage_format: StorageFormat::RowBased,
        });
        assert!(facts.has_covering_index("orders", &["customer_id", "amount"]));
        assert!(!facts.has_covering_index("orders", &["customer_id", "id"]));
    }

    #[test]
    fn covering_index_with_included_columns() {
        let facts = SchemaFacts::with_table(TableInfo {
            name: "orders".to_string(),
            columns: vec![("id".into(), DataType::Integer), ("customer_id".into(), DataType::Integer), ("order_date".into(), DataType::Timestamp)],
            primary_key: vec!["id".into()],
            foreign_keys: vec![],
            indexes: vec![IndexInfo {
                name: "idx_cust_incl_date".into(),
                index_type: IndexType::BTree,
                columns: vec!["customer_id".into()],
                is_unique: false,
                included_columns: vec!["order_date".into()],
            }],
            storage_format: StorageFormat::RowBased,
        });
        assert!(facts.has_covering_index("orders", &["customer_id", "order_date"]));
        assert!(!facts.has_covering_index("orders", &["customer_id", "id"]));
    }
}
