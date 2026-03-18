//! Function catalog with optimizer metadata for query optimization.
//!
//! This crate provides a comprehensive function catalog modeling
//! SQL function metadata across major database systems. The catalog
//! includes:
//!
//! - **Function Definitions** ([`functions`]): 200+ SQL functions with
//!   signatures, properties, and cost information.
//! - **Optimizer Properties**: Determinism, purity, cost multipliers,
//!   and inlineability for constant folding and pushdown decisions.
//! - **Cross-Database Coverage**: Functions tagged with their availability
//!   across `PostgreSQL`, `MySQL`, `SQLite`, SQL Server, Oracle, and more.
//!
//! # Design Philosophy
//!
//! The function catalog models properties that matter for optimization:
//! - Deterministic functions can be constant-folded
//! - Expensive functions should not be pushed below filters
//! - Pure functions have no side effects and can be freely reordered
//! - Order-sensitive aggregates cannot be parallelized naively
//!
//! # Examples
//!
//! ## Looking up a function
//!
//! ```
//! use ra_catalog::FunctionCatalog;
//!
//! let catalog = FunctionCatalog::with_builtins();
//! let abs = catalog.lookup("ABS").expect("ABS should exist");
//! assert!(abs.properties.deterministic);
//! assert!(abs.properties.constant_foldable);
//! ```
//!
//! ## Checking function properties
//!
//! ```
//! use ra_catalog::FunctionCatalog;
//!
//! let catalog = FunctionCatalog::with_builtins();
//! let random = catalog.lookup("RANDOM").expect("RANDOM exists");
//! assert!(!random.properties.deterministic);
//! assert!(!random.properties.constant_foldable);
//! ```
//!
//! ## Finding expensive functions
//!
//! ```
//! use ra_catalog::FunctionCatalog;
//!
//! let catalog = FunctionCatalog::with_builtins();
//! let expensive = catalog.expensive_functions();
//! assert!(!expensive.is_empty());
//! ```
//!
//! ## Filtering by database
//!
//! ```
//! use ra_catalog::{FunctionCatalog, DatabaseSystem};
//!
//! let catalog = FunctionCatalog::with_builtins();
//! let pg_fns = catalog.by_database(DatabaseSystem::PostgreSQL);
//! assert!(pg_fns.len() > 100);
//! ```

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::too_many_lines)]

pub mod functions;

pub use functions::{
    DatabaseSystem, FunctionCatalog, FunctionCategory, FunctionDefinition, FunctionProperties,
    FunctionSignature, SqlType,
};
