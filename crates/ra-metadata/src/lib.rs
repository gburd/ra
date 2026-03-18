//! Database metadata integration for the relational algebra system.
//!
//! This crate provides tools to connect to live databases, gather
//! DDL and statistics from system catalogs, parse EXPLAIN plans,
//! and compare optimizer recommendations against actual database
//! query plans.
//!
//! # Supported Databases
//!
//! - **PostgreSQL**: via `pg_class`, `pg_stats`, `pg_indexes` catalogs
//! - **MySQL**: via `information_schema` tables
//! - **SQLite**: via `PRAGMA` commands and `sqlite_stat1`
//!
//! # Architecture
//!
//! The crate is organized as query definitions and result parsers
//! that are independent of any specific database client library.
//! This allows callers to use their preferred driver (sync or async)
//! while reusing the catalog query logic and output parsers.
//!
//! # Modules
//!
//! - [`connector`]: Core types and `DatabaseConnector` trait
//! - [`explain`]: `EXPLAIN` plan parsing for all backends
//! - [`postgres`]: `PostgreSQL` catalog queries and parsers
//! - [`mysql`]: `MySQL` catalog queries and parsers
//! - [`sqlite`]: `SQLite` `PRAGMA` queries and parsers
//! - [`diff`]: Differential validator for plan comparison

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::implicit_hasher)]

pub mod connector;
pub mod diff;
pub mod error;
pub mod explain;
pub mod mysql;
pub mod postgres;
pub mod sqlite;

pub use connector::{
    ConnectionTarget, DatabaseConnector, SchemaInfo,
    parse_connection_string,
};
pub use diff::{DiffReport, compare_plans};
pub use error::MetadataError;
pub use explain::{ExplainPlan, PlanNode};
