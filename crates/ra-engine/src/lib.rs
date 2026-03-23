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
pub mod bayesian_pruning;
pub mod cardinality_cost;
// pub mod column_pruning; // TODO: incomplete, has invalid egg syntax
pub mod consensus_rules;
pub mod constraint_optimizer;
pub mod cost;
pub mod count_metadata;
pub mod covering_index;
pub mod differential;
pub mod distributed_optimizer;
pub mod egraph;
pub mod executors;
pub mod extract;
pub mod facts_context;
// pub mod functional_deps; // TODO: incomplete, has invalid egg syntax
pub mod incremental_sort;
pub mod join_transformations;
pub mod large_join;
pub mod left_deep;
pub mod federated_cost;
pub mod federated_optimizer;
pub mod memo;
pub mod network_cost;
pub mod null_simplification;
pub mod parquet_pushdown;
pub mod pattern_fingerprint;
pub mod plan_stitch;
pub mod progressive_reopt;
pub mod query_complexity;
pub mod beam_search;
pub mod convergence;
pub mod cost_pruning;
pub mod join_graph;
pub mod precondition_eval;
pub mod stats_cache;
pub mod recursive;
// pub mod redundant_join; // TODO: incomplete, has invalid egg syntax
pub mod resource_budget;
pub mod resource_profiles;
pub mod rewrite;
pub mod shortcuts;
pub mod rule_metadata;
pub mod rule_registry;
pub mod runtime_filters;
// pub mod semi_join; // TODO: incomplete, has invalid egg syntax
pub mod timely;
pub mod trigger_optimizer;

pub use adaptive_calibration::{
    AdaptiveCalibrator, CalibrationConfig, CalibrationState,
    CostFeedback, OperatorKind,
};
pub use analysis::RelAnalysis;
pub use cardinality_cost::CardinalityAwareCostFn;
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
pub use extract::{extract_best, extract_best_with_staleness, extract_best_with_cardinality, rec_expr_to_rel_expr, RelCostFn};
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
pub use large_join::{
    JoinNode, LargeJoinOptimizer, LargeJoinStrategy,
};
pub use left_deep::{LeftDeepBuilder, can_use_left_deep};
pub use consensus_rules::consensus_rules;
pub use rewrite::all_rules;
pub use rule_metadata::{
    filter_rules_by_preconditions, load_rules_from_directory, parse_rra_file, ParsedRule,
    Precondition, RuleMetadata,
};
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
pub use parquet_pushdown::{
    CompareOp, ParquetMetadataRegistry, PushdownPredicate,
    RowGroupMatch, evaluate_predicate, filter_row_groups,
    parquet_pushdown_rules, pruning_selectivity,
};
pub use runtime_filters::{
    BloomFilterState, FilterBuilder, FilterConfig, FilterEffectiveness,
    FilterOpportunity, FilterStrategy, InListFilterState,
    MinMaxFilterState, RuntimeFilter, RuntimeFilterCost,
    estimate_filter_cost, identify_filter_opportunities,
    should_apply_filter,
};
pub use covering_index::{
    covering_index_rules, index_only_scan_cost_factor,
};
pub use query_complexity::QueryComplexity;
pub use convergence::{
    ConvergenceDetector, ConvergenceStats, IterationMetrics, TerminationDecision,
};
pub use beam_search::{BeamSearchConfig, BeamSearchStats, BeamSearchTracker};
pub use cost_pruning::{CostPruner, PruningStats};
pub use join_graph::{JoinGraph, JoinGraphStats};
pub use stats_cache::{StatsCache, StatsCacheBuilder};
pub use progressive_reopt::{
    DivergenceInfo, ReoptConfig, ReoptDecision, RuntimeStatistics,
    StitchPointKind, StitchPointMeta, StitchTransferKind, JoinImplKind,
    divergence_factor, estimate_remaining_cost, estimate_stitch_cost,
    evaluate_reopt_decision, insert_stitch_points, is_switch_worthwhile,
    join_transfer_kind, should_reoptimize,
};
pub use plan_stitch::{
    OperatorState, StitchResult, count_stitch_points,
    find_deepest_join, stitch_plans,
};
pub use bayesian_pruning::{
    BayesianPruner, BucketStats, PruningConfig, PruningOutcome,
    PruningSummary,
};
pub use pattern_fingerprint::PlanFingerprint;
