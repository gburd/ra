//! Database adapters for integrating external databases with the RA optimizer.
//!
//! This crate provides the `DatabaseAdapter` trait and implementations for
//! various database systems (`PostgreSQL`, Stoolap, etc.) to gather statistics,
//! schema information, and capabilities for use by the pre-condition system.

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::unnecessary_literal_bound)]

use anyhow::Result;
use ra_core::{FactsProvider, SqlDialect};
use ra_stats::types::{ColumnStats, TableStats};
use std::collections::HashMap;
use thiserror::Error;

pub mod postgres;
pub mod stoolap;

// Re-exports
pub use postgres::PostgresAdapter;
pub use stoolap::StoolapAdapter;

/// Errors that can occur during database adapter operations.
#[derive(Debug, Error)]
pub enum AdapterError {
    /// Failed to connect to the database.
    #[error("Connection failed: {0}")]
    ConnectionError(String),

    /// Failed to query database metadata.
    #[error("Query failed: {0}")]
    QueryError(String),

    /// Unsupported database feature.
    #[error("Unsupported feature: {0}")]
    UnsupportedFeature(String),

    /// Invalid connection string or configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
}

/// Unified interface for connecting to external databases and gathering facts.
///
/// Database adapters implement this trait to provide statistics, schema information,
/// and capability detection for use by the pre-condition system. This enables the
/// optimizer to make informed decisions based on actual database characteristics.
///
/// # Example
///
/// ```no_run
/// use ra_adapters::{DatabaseAdapter, PostgresAdapter};
/// use ra_core::FactsProvider;
///
/// # fn example() -> anyhow::Result<()> {
/// let mut adapter = PostgresAdapter::new();
/// adapter.connect("postgresql://localhost/mydb")?;
///
/// let stats = adapter.gather_statistics()?;
/// let schema = adapter.get_schema_info()?;
///
/// // Use adapter as FactsProvider
/// let facts_provider: &dyn FactsProvider = adapter.as_facts_provider();
/// # Ok(())
/// # }
/// ```
pub trait DatabaseAdapter: Send + Sync {
    /// Connect to the database using the given connection string.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or the connection string is invalid.
    fn connect(&mut self, connection_string: &str) -> Result<(), AdapterError>;

    /// Gather statistics for all tables in the database.
    ///
    /// This retrieves row counts, data sizes, and other table-level statistics
    /// from the database's system catalogs.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails or statistics are unavailable.
    fn gather_statistics(&self) -> Result<HashMap<String, TableStats>, AdapterError>;

    /// Gather column-level statistics for a specific table.
    ///
    /// This retrieves distinct counts, null fractions, histograms, and other
    /// column-level statistics.
    ///
    /// # Errors
    ///
    /// Returns an error if the table doesn't exist or statistics are unavailable.
    fn gather_column_stats(&self, table: &str) -> Result<HashMap<String, ColumnStats>, AdapterError>;

    /// Get schema information for all tables.
    ///
    /// This retrieves table definitions, column types, constraints, and indexes.
    ///
    /// # Errors
    ///
    /// Returns an error if schema information cannot be queried.
    fn get_schema_info(&self) -> Result<SchemaInfo, AdapterError>;

    /// Query database capabilities and supported features.
    ///
    /// This detects which SQL features, index types, and optimizer hints
    /// are supported by the database.
    ///
    /// # Errors
    ///
    /// Returns an error if capability detection fails.
    fn get_capabilities(&self) -> Result<DatabaseCapabilities, AdapterError>;

    /// Check if a specific feature is supported.
    ///
    /// # Errors
    ///
    /// Returns an error if the feature check query fails.
    fn supports_feature(&self, feature: &str) -> Result<bool, AdapterError>;

    /// Get the SQL dialect used by this database.
    fn sql_dialect(&self) -> SqlDialect;

    /// Get the database name/type.
    fn database_name(&self) -> &str;

    /// Convert this adapter into a [`FactsProvider`].
    ///
    /// This allows the adapter to be used directly by the pre-condition evaluator.
    fn as_facts_provider(&self) -> &dyn FactsProvider;
}

/// Schema information gathered from a database.
#[derive(Debug, Clone)]
pub struct SchemaInfo {
    /// Table definitions by name.
    pub tables: HashMap<String, TableInfo>,
}

/// Information about a single table.
#[derive(Debug, Clone)]
pub struct TableInfo {
    /// Table name.
    pub name: String,
    /// Column definitions.
    pub columns: Vec<ColumnInfo>,
    /// Primary key columns.
    pub primary_key: Vec<String>,
    /// Foreign key constraints.
    pub foreign_keys: Vec<ForeignKeyInfo>,
    /// Indexes on this table.
    pub indexes: Vec<IndexInfo>,
}

/// Information about a single column.
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    /// Column name.
    pub name: String,
    /// Data type (SQL type name).
    pub data_type: String,
    /// Whether the column is nullable.
    pub nullable: bool,
    /// Default value expression (if any).
    pub default_value: Option<String>,
}

/// Foreign key constraint information.
#[derive(Debug, Clone)]
pub struct ForeignKeyInfo {
    /// Foreign key constraint name.
    pub name: String,
    /// Columns in this table.
    pub columns: Vec<String>,
    /// Referenced table name.
    pub referenced_table: String,
    /// Referenced columns.
    pub referenced_columns: Vec<String>,
}

/// Index information.
#[derive(Debug, Clone)]
pub struct IndexInfo {
    /// Index name.
    pub name: String,
    /// Indexed columns.
    pub columns: Vec<String>,
    /// Whether this is a unique index.
    pub unique: bool,
    /// Index type (btree, hash, gin, gist, etc.).
    pub index_type: String,
}

/// Database capabilities and supported features.
#[derive(Debug, Clone)]
pub struct DatabaseCapabilities {
    /// Database name/type.
    pub database_name: String,
    /// SQL dialect.
    pub dialect: SqlDialect,
    /// Supported SQL features.
    pub features: HashMap<String, bool>,
    /// Supported index types.
    pub index_types: Vec<String>,
    /// Maximum identifier length.
    pub max_identifier_length: usize,
}

impl DatabaseCapabilities {
    /// Check if a feature is supported.
    #[must_use]
    pub fn supports(&self, feature: &str) -> bool {
        self.features.get(feature).copied().unwrap_or(false)
    }
}
