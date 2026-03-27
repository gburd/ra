//! Database metadata integration.
//!
//! Provides a uniform interface for gathering schema metadata,
//! statistics, and EXPLAIN plans from `PostgreSQL`, `MySQL`, `SQLite`,
//! and optionally `DuckDB`, SQL Server, Oracle, and `MonetDB`.
//!
//! - [`connector`]: `DatabaseConnector` trait.
//! - [`schema`]: Schema types (`SchemaInfo`, `TableInfo`, `ColumnInfo`, etc.).
//! - [`explain`]: EXPLAIN plan tree and parsers for each backend.
//! - [`diff_validator`]: Differential plan comparison.
//! - [`explain_gen`]: EXPLAIN format generators for database-specific output.
//! - [`error`]: Shared error types.
//!
//! Optional backends are enabled via Cargo features:
//! - `duckdb-support`: `DuckDB` via the `duckdb` crate.
//! - `sqlserver-support`: SQL Server via the `tiberius` crate.
//! - `oracle-support`: Oracle via the `oracle` crate.
//! - `monetdb-support`: `MonetDB` via the `odbc-api` crate.

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
pub mod explain_gen;
pub mod factory;
pub mod mysql;
pub mod postgres;
pub mod schema;
pub mod sqlite;

#[cfg(feature = "duckdb-support")]
pub mod duckdb;

#[cfg(feature = "sqlserver-support")]
pub mod sqlserver;

#[cfg(feature = "oracle-support")]
pub mod oracle;

#[cfg(feature = "monetdb-support")]
pub mod monetdb;

pub use connector::{DatabaseConnector, MetadataResult};
pub use error::MetadataError;
pub use explain::{
    format_mysql_explain, format_postgres_explain, format_sqlite_explain, parse_mysql_explain,
    parse_postgres_explain, parse_sqlite_explain, relexpr_to_explain_node, ExplainNode,
    ExplainPlan, JoinType, NodeType,
};
pub use explain_gen::{from_relexpr, DatabaseCostParams, ExplainFormat};
pub use factory::{connect, detect_kind, AnyConnector};
pub use schema::{
    ColumnInfo, ColumnStatistics, ConstraintInfo, ConstraintKind, DatabaseKind, IndexInfo,
    SchemaInfo, TableInfo, TableStats, TriggerEvent, TriggerInfo, TriggerScope, TriggerTiming,
};
