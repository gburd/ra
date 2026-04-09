//! Database connection configuration for ra-web.

use serde::{Deserialize, Serialize};

/// Database configuration for executing queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DatabaseConfig {
    /// PostgreSQL database.
    PostgreSQL {
        /// Connection string (e.g., "postgresql://user:pass@host/db").
        connection_string: String,
        /// Maximum number of connections in the pool.
        #[serde(default = "default_pool_size")]
        pool_size: u32,
    },
    /// MySQL database.
    MySQL {
        /// Connection string (e.g., "mysql://user:pass@host/db").
        connection_string: String,
        /// Maximum number of connections in the pool.
        #[serde(default = "default_pool_size")]
        pool_size: u32,
    },
    /// SQLite database.
    SQLite {
        /// Path to SQLite database file.
        database_path: String,
    },
    /// DuckDB database.
    DuckDB {
        /// Path to DuckDB database file (":memory:" for in-memory).
        database_path: String,
    },
}

fn default_pool_size() -> u32 {
    20
}

impl DatabaseConfig {
    /// Create a PostgreSQL configuration.
    #[must_use]
    pub fn postgres(connection_string: impl Into<String>) -> Self {
        Self::PostgreSQL {
            connection_string: connection_string.into(),
            pool_size: default_pool_size(),
        }
    }

    /// Create a MySQL configuration.
    #[must_use]
    pub fn mysql(connection_string: impl Into<String>) -> Self {
        Self::MySQL {
            connection_string: connection_string.into(),
            pool_size: default_pool_size(),
        }
    }

    /// Create a SQLite configuration.
    #[must_use]
    pub fn sqlite(database_path: impl Into<String>) -> Self {
        Self::SQLite {
            database_path: database_path.into(),
        }
    }

    /// Create a DuckDB configuration.
    #[must_use]
    pub fn duckdb(database_path: impl Into<String>) -> Self {
        Self::DuckDB {
            database_path: database_path.into(),
        }
    }
}
