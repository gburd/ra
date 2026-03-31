//! SQL Standards grammar modules.
//!
//! This module contains grammar definitions for SQL standards from SQL-86 through SQL:2023,
//! showing the evolution of the SQL language over time.
//!
//! # SQL Standards Evolution
//!
//! - **SQL-86 / SQL-87**: First ANSI/ISO standard (basic SELECT, INSERT, UPDATE, DELETE)
//! - **SQL-89**: Minor revision (referential integrity)
//! - **SQL-92**: Major revision (joins, transactions, cursors) - Foundation for modern SQL
//! - **SQL:1999**: Major extensions (WITH/CTEs, CASE, triggers, stored procedures)
//! - **SQL:2003**: Window functions, XML support, sequences, identity columns
//! - **SQL:2006**: XML amendments
//! - **SQL:2008**: MERGE statement, enhanced datetime, TRUNCATE
//! - **SQL:2011**: Temporal tables (system-versioned, application-time)
//! - **SQL:2016**: JSON support (JSON_TABLE, JSON_QUERY, JSON_VALUE)
//! - **SQL:2023**: Property Graph Queries (GRAPH_TABLE, MATCH patterns)
//!
//! # Architecture
//!
//! Each standard module implements the [`GrammarExtension`](super::extension::GrammarExtension) trait,
//! providing:
//! - New keywords introduced in that standard
//! - New operators
//! - New built-in functions
//! - Statement parsing for new constructs
//!
//! Profiles can enable specific standards (e.g., PostgreSQL 17 supports SQL:1999, SQL:2003, SQL:2016).

pub mod sql_92;
pub mod sql_1999;
pub mod sql_2003;
pub mod sql_2008;
pub mod sql_2011;
pub mod sql_2016;
pub mod sql_2023;

pub use sql_92::SQL92Extension;
pub use sql_1999::SQL1999Extension;
pub use sql_2003::SQL2003Extension;
pub use sql_2008::SQL2008Extension;
pub use sql_2011::SQL2011Extension;
pub use sql_2016::SQL2016Extension;
pub use sql_2023::SQL2023Extension;
