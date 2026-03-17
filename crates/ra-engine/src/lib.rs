//! Query optimization engine using egg for equality saturation.
//!
//! This crate provides the core optimization algorithms:
//! - E-graph construction from relational algebra expressions
//! - 50+ rewrite rules (predicate pushdown, join reordering,
//!   expression simplification, `DuckDB`/`SQLite`-inspired rules)
//! - Cost-based plan extraction
//! - E-graph analysis for tracking table references and properties
//! - Memo table for caching optimization results
//!
//! # Usage
//!
//! ```
//! use ra_engine::Optimizer;
//! use ra_core::algebra::RelExpr;
//!
//! let optimizer = Optimizer::new();
//! let plan = RelExpr::scan("users");
//! let optimized = optimizer.optimize(&plan).unwrap();
//! ```

// The egg define_language! macro generates enum variants without
// doc comments, which triggers missing_docs. Allowing at crate level
// is the only option since the attribute cannot be placed on the macro.
#![allow(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod analysis;
pub mod egraph;
pub mod extract;
pub mod memo;
pub mod rewrite;

pub use analysis::RelAnalysis;
pub use egraph::{to_rec_expr, EGraphError, Optimizer, OptimizerConfig, RelLang};
pub use extract::{extract_best, rec_expr_to_rel_expr, RelCostFn};
pub use memo::{structural_hash, MemoTable};
pub use rewrite::all_rules;
