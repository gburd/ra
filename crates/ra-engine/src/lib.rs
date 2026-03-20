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

pub mod adaptive_calibration;
pub mod analysis;
pub mod column_pruning;
pub mod constraint_optimizer;
pub mod cost;
pub mod differential;
pub mod distributed_optimizer;
pub mod egraph;
pub mod executors;
pub mod extract;
pub mod facts_context;
pub mod functional_deps;
pub mod incremental_sort;
pub mod join_transformations;
pub mod federated_cost;
pub mod federated_optimizer;
pub mod memo;
pub mod network_cost;
pub mod null_simplification;
pub mod precondition_eval;
pub mod recursive;
pub mod redundant_join;
pub mod resource_budget;
pub mod resource_profiles;
pub mod rewrite;
pub mod runtime_filters;
pub mod semi_join;
pub mod timely;
pub mod trigger_optimizer;

pub use adaptive_calibration::{
    AdaptiveCalibrator, CalibrationConfig, CalibrationState,
    CostFeedback, OperatorKind,
};
pub use analysis::RelAnalysis;
pub use cost::{CostCalibration, IntegratedCostFn, IntegratedCostModel};
pub use distributed_optimizer::{
    AggStrategyResult, ClusterTopology, DistributedOptimizer,
    DistributedOptimizerConfig, DistributedOptimizerError,
};
pub use federated_cost::FederatedCostModel;
pub use federated_optimizer::{
    FederatedAnalysis, FederatedError, FederatedOptimizer,
};
pub use differential::{IncrementalError, IncrementalOptimizer, RuleChange, RuleId};
pub use egraph::{
    to_rec_expr, EGraphError, IncrementalStats, OptimizationResult,
    OptimizationStatus, Optimizer, OptimizerConfig, RelLang,
};
pub use extract::{extract_best, extract_best_with_staleness, rec_expr_to_rel_expr, RelCostFn};
pub use memo::{structural_hash, MemoTable};
pub use network_cost::{
    DistributionStrategy, JoinSides, NetworkCostEstimate, NetworkCostModel,
};
pub use recursive::{
    ExecutionContext, ExecutionError, ExprEvaluator, RecursiveCTEConfig,
    RecursiveCTEExecutor, RecursionResult, Row, TerminationReason,
};
pub use resource_budget::{
    ExceededResource, OverflowStrategy, ResourceBudget, ResourceCheckResult,
    ResourceTracker, ResourceUsageReport,
};
pub use constraint_optimizer::{
    optimize_with_constraints, ConstraintOptResult,
};
pub use facts_context::{FactsContext, FactsContextBuilder};
pub use incremental_sort::{
    IncrementalSortCost, PrefixMatch, detect_prefix_match,
    estimate_costs as estimate_incremental_sort_costs,
    try_incremental_sort,
};
pub use join_transformations::{
    SchemaInfo, SelfJoinMatch, UniqueConstraint,
    apply_join_transformations, can_eliminate_self_join,
    detect_self_join, is_null_rejecting, outer_to_inner_conversion,
    try_convert_outer_to_inner, try_eliminate_self_join,
};
pub use rewrite::all_rules;
pub use precondition_eval::{EvaluationError, PreConditionEvaluator};
pub use timely::{ComputationStats, TimelyConfig};
pub use trigger_optimizer::{
    analyze_dml_cost, detect_cascade, CascadeWarning,
    DmlCostEstimate, TriggerAnalysis,
};
pub use executors::{
    LateralJoinExecutor, MultiUnnestExecutor, TableFunctionExecutor,
    UnnestExecutor,
};
pub use runtime_filters::{
    BloomFilterState, FilterBuilder, FilterConfig, FilterEffectiveness,
    FilterOpportunity, FilterStrategy, InListFilterState,
    MinMaxFilterState, RuntimeFilter, RuntimeFilterCost,
    estimate_filter_cost, identify_filter_opportunities,
    should_apply_filter,
};
