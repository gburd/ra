//! Query optimization engine using egg for equality saturation.
//!
//! This crate provides the core optimization algorithms:
//! - E-graph construction from relational algebra expressions
//! - 50+ rewrite rules (predicate pushdown, join reordering,
//!   expression simplification, `DuckDB`/`SQLite`-inspired rules)
//! - Cost-based plan extraction
//! - E-graph analysis for tracking table references and properties
//! - Memo table for caching optimization results
//! - Incremental optimization via differential dataflow
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
pub mod cost;
pub mod differential;
pub mod egraph;
pub mod extract;
pub mod memo;
pub mod recursive;
pub mod resource_budget;
pub mod resource_profiles;
pub mod rewrite;
pub mod timely;

pub use analysis::RelAnalysis;
pub use cost::{CostCalibration, IntegratedCostFn, IntegratedCostModel};
pub use differential::{IncrementalError, IncrementalOptimizer, RuleChange, RuleId};
pub use egraph::{
    to_rec_expr, EGraphError, OptimizationResult, OptimizationStatus,
    Optimizer, OptimizerConfig, RelLang,
};
pub use extract::{extract_best, extract_best_with_staleness, rec_expr_to_rel_expr, RelCostFn};
pub use memo::{structural_hash, MemoTable};
pub use recursive::{
    ExecutionContext, ExecutionError, ExprEvaluator, RecursiveCTEConfig,
    RecursiveCTEExecutor, RecursionResult, Row, TerminationReason,
};
pub use resource_budget::{
    ExceededResource, OverflowStrategy, ResourceBudget, ResourceCheckResult,
    ResourceTracker, ResourceUsageReport,
};
pub use rewrite::all_rules;
pub use timely::{ComputationStats, TimelyConfig};
