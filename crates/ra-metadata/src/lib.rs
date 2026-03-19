//! Database metadata integration.
//!
//! Provides a uniform interface for gathering schema metadata,
//! statistics, and EXPLAIN plans from `PostgreSQL`, `MySQL`, and `SQLite`.
//!
//! - [`connector`]: `DatabaseConnector` trait.
//! - [`schema`]: Schema types (`SchemaInfo`, `TableInfo`, `ColumnInfo`, etc.).
//! - [`explain`]: EXPLAIN plan tree and parsers for each backend.
//! - [`diff_validator`]: Differential plan comparison.
//! - [`error`]: Shared error types.

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::must_use_candidate)]
#![cfg_attr(test, allow(clippy::float_cmp))]

pub mod connector;
pub mod diff_validator;
pub mod error;
pub mod explain;
pub mod factory;
pub mod mysql;
pub mod postgres;
pub mod schema;
pub mod sqlite;

pub use connector::{DatabaseConnector, MetadataResult};
pub use error::MetadataError;
pub use explain::{
    parse_mysql_explain, parse_postgres_explain,
    parse_sqlite_explain, ExplainNode, ExplainPlan, JoinType,
    NodeType,
};
pub use factory::{AnyConnector, connect, detect_kind};
pub use schema::{
    ColumnInfo, ColumnStatistics, ConstraintInfo, ConstraintKind,
    DatabaseKind, IndexInfo, SchemaInfo, TableInfo, TableStats,
};
