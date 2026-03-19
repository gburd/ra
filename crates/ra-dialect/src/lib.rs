//! SQL dialect translation for cross-database compatibility.
//!
//! This crate provides tools to translate SQL statements between
//! different database dialects, handling differences in syntax,
//! function names, operators, and feature support.
//!
//! # Supported dialects
//!
//! - `PostgreSQL`
//! - `MySQL`
//! - `SQLite`
//! - `DuckDB`
//! - Microsoft SQL Server
//! - Oracle
//!
//! # Usage
//!
//! ```
//! use ra_dialect::{Dialect, DialectTranslator};
//!
//! let translator = DialectTranslator::new(
//!     Dialect::PostgreSql,
//!     Dialect::MySql,
//! );
//! let result = translator
//!     .translate(
//!         "SELECT first_name || ' ' || last_name FROM users",
//!     )
//!     .unwrap();
//! // MySQL uses CONCAT() instead of ||
//! assert!(result.sql.contains("CONCAT"));
//! ```
//!
//! # Compatibility matrix
//!
//! ```
//! use ra_dialect::CompatibilityMatrix;
//!
//! let matrix = CompatibilityMatrix::build();
//! let table = matrix.to_table();
//! // Prints a human-readable compatibility table
//! ```

#![warn(missing_docs)]

pub mod dialect;
pub mod error;
pub mod functions;
pub mod matrix;
pub mod translator;

pub use dialect::{feature_support, Dialect, FeatureSupport, SqlFeature};
pub use error::{TranslationError, TranslationWarning, WarningSeverity};
pub use matrix::CompatibilityMatrix;
pub use translator::{
    DialectTranslator, DialectVersion, TranslationResult,
};
