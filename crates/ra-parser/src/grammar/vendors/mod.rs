//! Vendor-specific SQL grammar extensions.
//!
//! This module contains vendor-specific syntax extensions for major database systems.
//! Each vendor module focuses on syntax unique to that database, building upon
//! the standard SQL grammar.

pub mod postgresql;
pub mod mysql;
pub mod oracle;
pub mod sqlserver;

pub use postgresql::PostgreSQLExtension;
pub use mysql::MySQLExtension;
pub use oracle::OracleExtension;
pub use sqlserver::SQLServerExtension;
