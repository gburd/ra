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
// Query optimization fundamentally requires converting between integer
// row counts/sizes and floating-point costs. These casts are safe because
// realistic table sizes and cardinalities never approach 2^52.
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]
// Test code legitimately uses expect/unwrap for assertions and setup.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

pub mod adaptive_calibration;
pub mod analysis;
pub mod appliers;
pub mod conditions;
pub mod beam_search;
pub mod cost_model;
#[cfg(feature = "ml")]
pub mod cardinality_cost;
pub mod citus_optimizer;
pub mod column_pruning;
pub mod correlation_analysis;
pub mod consensus_rules;
#[cfg(feature = "metadata")]
pub mod constraint_optimizer;
pub mod convergence;
pub mod cost;
pub mod cost_pruning;
pub mod count_metadata;
pub mod covering_index;
#[cfg(feature = "streaming")]
pub mod differential;
pub mod distributed_optimizer;
pub mod documentdb_optimizer;
pub mod egraph;
pub mod executors;
pub mod extract;
pub mod facts_context;
pub mod federated_cost;
pub mod federated_optimizer;
pub mod fts_cost;
pub mod fts_rules;
pub mod functional_deps;
pub mod genetic_fingerprint;
pub mod provenance;
pub mod incremental_sort;
pub mod index_selection;
pub mod ordering_pass;
pub mod isolation_cost;
pub mod join_graph;
pub mod join_graph_shape;
pub mod plan_advice_emit;
pub mod plan_advice_honor;
pub mod plan_advice_physical;
pub mod plan_advice_validate;
pub mod physical_props;
pub mod partition_pruning;
pub mod join_transformations;
pub mod large_join;
pub mod lazy_rules;
pub mod left_deep;
pub mod memo;
#[cfg(feature = "ml")]
pub mod ml_integration;
pub mod mv_matching;
pub mod mv_rewrite;
pub mod network_cost;
pub mod neural;
pub mod null_simplification;
pub mod parquet_pushdown;
pub mod pattern_fingerprint;
pub mod plan_cache;
pub mod plan_comparison;
pub mod plan_stitch;
pub mod precondition_eval;
pub mod progressive_reopt;
pub mod query_features;
pub mod recursive;
pub mod redundant_join;
pub mod resource_budget;
pub mod resource_profiles;
pub mod rewrite;
pub mod rule_advisor;
pub mod rule_consolidation;
pub mod rule_knowledge;
pub mod rule_metadata;
pub mod rule_priority;
pub mod rule_registry;
pub mod shortcuts;
pub(crate) mod sparsemap;
pub mod state;
pub mod stats_cache;
pub mod subquery_decorrelation;
// Phase 6: Timeline system (deferred)
pub mod hybrid_search;
pub mod oracle_json_duality;
pub mod rum_index;
pub mod runtime_filters;
pub mod selectivity;
pub mod semi_join;
pub mod speculative_router;
pub mod continuation_gate;
pub mod training_coordinator;
#[cfg(feature = "timeline")]
pub mod timeline_config;
#[cfg(feature = "timeline")]
pub mod timeline_facts;
#[cfg(all(feature = "timeline", feature = "streaming"))]
pub mod timeline_optimizer;
#[cfg(feature = "streaming")]
pub mod timely;
#[cfg(feature = "metadata")]
pub mod trigger_optimizer;
pub mod vector_cost;
pub mod vector_rules;
pub mod xml_optimizer;

pub use adaptive_calibration::{
    AdaptiveCalibrator, CalibrationConfig, CalibrationState, CostFeedback, OperatorKind,
};
pub use analysis::RelAnalysis;
pub use beam_search::{BeamSearchConfig, BeamSearchStats, BeamSearchTracker};
#[cfg(feature = "ml")]
pub use cardinality_cost::CardinalityAwareCostFn;
pub use citus_optimizer::{
    analyze_shard_pruning, columnar_scan_cost_factor, CitusMetadata, CitusOptimizedPlan,
    CitusOptimizer, CitusOptimizerConfig, CitusOptimizerError, CitusStrategy, CitusWorkerNode,
    ColumnarCostParams, ColumnarTableInfo, DistributedTableInfo, DistributionMethod,
    ExecutionLocation as CitusExecutionLocation, ShardPruningResult, StorageType,
};
pub use consensus_rules::consensus_rules;
#[cfg(feature = "metadata")]
pub use constraint_optimizer::{optimize_with_constraints, ConstraintOptResult};
pub use convergence::{
    ConvergenceDetector, ConvergenceStats, IterationMetrics, TerminationDecision,
};
pub use cost::{CostCalibration, IntegratedCostFn, IntegratedCostModel, LiveConditions};
pub use cost_pruning::{CostPruner, PruningStats};
pub use covering_index::{covering_index_rules, index_only_scan_cost_factor};
#[cfg(feature = "streaming")]
pub use differential::{
    change_ratio, ChangeSource, FactChange, HistogramDigest, IncrementalError,
    IncrementalOptimizer, IndexChange, PlanDependencies, ResourceId, RuleChange, RuleId,
    StalenessThresholds, StatisticsChange,
};
pub use distributed_optimizer::{
    AggStrategyResult, ClusterTopology, DistributedOptimizer, DistributedOptimizerConfig,
    DistributedOptimizerError,
};
pub use documentdb_optimizer::{
    bson_op_benefits_from_rum, bson_op_to_rum_opfamily, combine_selectivities,
    compound_gin_scan_cost, documentdb_rewrite_rules, estimate_selectivity,
    evaluate_rum_bson_recommendation, gin_bson_equivalent_cost, gin_bson_scan_cost_factor,
    gin_scan_cost, gin_vs_sequential_ratio, recommend_gin_indexes, rum_bson_array_scan_cost,
    rum_bson_near_scan_cost, rum_bson_scan_cost_factor, rum_bson_text_scan_cost,
    rum_vs_gin_bson_near_ratio, rum_vs_gin_bson_text_ratio, sequential_scan_cost, BsonOperator,
    BsonPredicate, BsonRumOpfamily, DocumentDbError, DocumentDbRumError, GinBsonCostParams,
    GinIndexRecommendation, QueryPattern, RumBsonCostParams, RumBsonIndexRecommendation,
    SelectivityEstimate, SelectivitySource,
};
pub use egraph::{
    to_rec_expr, EGraphError, IncrementalStats, OptimizationResult, OptimizationStatus, Optimizer,
    OptimizerConfig, RelLang, RuleApplication, RuleEvaluation, RuleTrackingResult,
};
pub use executors::{
    LateralJoinExecutor, MultiUnnestExecutor, TableFunctionExecutor, UnnestExecutor,
};
pub use cost_model::BitNetCostModel;
pub use extract::{
    extract_best, extract_best_bitnet, extract_best_with_staleness, rec_expr_to_rel_expr,
    HybridCostFn, RelCostFn,
};
#[cfg(feature = "ml")]
pub use extract::extract_best_with_cardinality;
pub use facts_context::{FactsContext, FactsContextBuilder};
pub use federated_cost::FederatedCostModel;
pub use federated_optimizer::{FederatedAnalysis, FederatedError, FederatedOptimizer};
pub use fts_cost::{
    boolean_query_cost, fulltext_scan_cost, index_vs_seqscan_speedup, inverted_index_lookup_cost,
    rum_scan_cost as fts_rum_scan_cost, select_fts_index_type, skip_list_intersection_cost,
    top_k_ranking_cost, BooleanOperator as FtsBooleanOperator, FtsIndexType, RankingAlgorithm,
};
pub use fts_rules::{
    boolean_query_to_skip_list, fts_filter_pushdown, fts_index_scan_introduction,
    fts_multi_column_index, fts_optimization_rules, optimize_top_k_fts, rank_aware_top_k,
    OptimizationDecision,
};
pub use genetic_fingerprint::QueryFingerprint;
pub use provenance::PlanProvenance;
pub use hybrid_search::{
    choose_hybrid_strategy, fuse_scores, hybrid_fts_first_cost_factor, hybrid_parallel_cost_factor,
    hybrid_scan_cost_factor, hybrid_search_rules, hybrid_vector_first_cost_factor, HybridStrategy,
    ScoreFusion,
};
pub use incremental_sort::{
    detect_prefix_match, estimate_costs as estimate_incremental_sort_costs, try_incremental_sort,
    IncrementalSortCost, PrefixMatch,
};
pub use isolation_cost::{isolation_cost_adjustment, IsolationCostConfig, PlanEstimates};
pub use ordering_pass::propagate_ordering;
pub use join_graph::{JoinGraph, JoinGraphStats};
pub use join_transformations::{
    apply_join_transformations, can_eliminate_self_join, detect_self_join, is_null_rejecting,
    outer_to_inner_conversion, try_convert_outer_to_inner, try_eliminate_self_join, SchemaInfo,
    SelfJoinMatch, UniqueConstraint,
};
pub use large_join::{JoinNode, LargeJoinOptimizer, LargeJoinStrategy};
pub use lazy_rules::{LazyQueryComplexity, LazyQueryPattern, LazyRuleCompiler, RuleCategory};
pub use left_deep::{can_use_left_deep, LeftDeepBuilder};
pub use memo::{structural_hash, MemoTable};
pub use mv_matching::{
    match_query_with_mv, view_benefit, MatchType, MaterializedViewInfo, MvCatalog, MvMatch,
};
pub use shortcuts::fast_path::{
    can_use_fast_path, FastPathDecision, FastPathKind, FastPathSelector, SimpleAggFunction,
};
pub use mv_rewrite::{mv_rewrite_rules, mv_scan_cost_factor};
pub use network_cost::{DistributionStrategy, JoinSides, NetworkCostEstimate, NetworkCostModel};
pub use neural::{NeuralConvergenceDetector, NeuralRuleSelector, RuleStallingTracker};
pub use state::{AtomicFingerprint, FingerprintReader, SystemFingerprint};
pub use oracle_json_duality::{
    benchmark_access_patterns, choose_access_path, duality_document_scan_cost_factor,
    duality_rewrite_rules, eliminable_joins, estimate_document_cost, estimate_relational_cost,
    estimate_update_cost, join_elimination_savings, predicate_target, pushdown_selectivity_benefit,
    AccessPath, AccessPathDecision, DualityCostParams, DualityError, DualityField,
    DualityFieldMapping, DualityView, PredicateTarget, Updatability,
};
pub use parquet_pushdown::{
    evaluate_predicate, filter_row_groups, parquet_pushdown_rules, pruning_selectivity, CompareOp,
    ParquetMetadataRegistry, PushdownPredicate, RowGroupMatch,
};
pub use pattern_fingerprint::PlanFingerprint;
pub use plan_cache::{
    CacheLookupResult, CacheMatchType, PlanCache, PlanCacheConfig, PlanCacheStats,
};
pub use plan_comparison::{ComparisonMetrics, PlanComparisonResult};
pub use plan_stitch::{
    count_stitch_points, differential_verify, find_deepest_join, replace_subtree, stitch_multi,
    stitch_plans, verify_join_order_equivalence, DifferentialResult, OperatorState, StitchResult,
};
pub use precondition_eval::{EvaluationError, PreConditionEvaluator};
pub use progressive_reopt::{
    divergence_factor, estimate_remaining_cost, estimate_stitch_cost, evaluate_reopt_decision,
    insert_stitch_points, is_switch_worthwhile, join_transfer_kind, progressive_optimize,
    should_reoptimize, BackgroundReoptimizer, DivergenceInfo, JoinImplKind, ReoptConfig,
    ReoptDecision, ReoptError, ReoptResult, ReoptimizeFn, RuntimeStatistics, StitchPointKind,
    StitchPointMeta, StitchTransferKind,
};
pub use speculative_router::{OptimizationFeatures, OptRoute, RoutePrediction, SpeculativeRouter};
pub use continuation_gate::{ContinuationDecision, ContinuationGate};
pub use training_coordinator::{
    SharedTrainingCoordinator, TrainingCoordinator, TrainingStats,
};
pub use query_features::QueryFeatureSet;
pub use recursive::{
    ExecutionContext, ExecutionError, ExprEvaluator, RecursionResult, RecursiveCTEConfig,
    RecursiveCTEExecutor, Row, TerminationReason,
};
pub use resource_budget::{
    ConvergenceBehavior, ExceededResource, FastPathPreferences, OverflowStrategy, ResourceBudget,
    ResourceCheckResult, ResourceTracker, ResourceUsageReport, RuleSelectionBehavior,
};
pub use rewrite::{
    all_rules, all_rules_annotated, all_rules_unsorted, generated_rules, AnnotatedRuleGroup,
    RuleAnnotation,
};
pub use rule_advisor::{
    AdaptiveState, AdvisorStats, RuleAdvisor, RuleAdvisorConfig, RuleApplicability, RuleOutcome,
    RuleTier, RuleUsageStats, TierSummary, TierThresholds,
};
pub use rule_consolidation::{
    default_consolidator, known_rule_dependencies, ConflictResolution, ConsolidationConfig,
    ConsolidationMetrics, ConsolidationResult, DependencyKind, MergeRecord, RuleConflict,
    RuleConsolidator, RuleDependency, RuleEffectivenessRecord, RuleEffectivenessReport,
};
pub use rule_knowledge::{RuleKnowledge, ShapeKeyBucket};
pub use rule_metadata::{
    filter_rules_by_preconditions, load_rules_from_directory, parse_rra_file, BenefitRange,
    ComplexityClass, ParsedRule, Precondition, RuleMetadata,
};
pub use rule_priority::{compute_priority, sort_rules_by_priority, RulePriority};
pub use rum_index::{
    evaluate_rum_recommendation, rum_boolean_cost_factor_vs_gin, rum_phrase_scan_cost,
    rum_ranked_scan_cost, rum_rewrite_rules, rum_scan_cost, rum_scan_cost_factor, rum_vs_gin_ratio,
    rum_vs_sequential_ratio, RumCostParams, RumError, RumIndexRecommendation, RumOpclass,
    RumQueryType,
};
pub use runtime_filters::{
    estimate_filter_cost, identify_filter_opportunities, should_apply_filter, BloomFilterState,
    FilterBuilder, FilterConfig, FilterEffectiveness, FilterOpportunity, FilterStrategy,
    InListFilterState, MinMaxFilterState, RuntimeFilter, RuntimeFilterCost,
};
pub use selectivity::estimate_selectivity as estimate_predicate_selectivity;
pub use stats_cache::{StatsCache, StatsCacheBuilder};
#[cfg(feature = "timeline")]
pub use timeline_config::{
    ColumnStatsDef, DataTypeDef, EventKind, FactsSnapshot, FingerPrintSnapshot, HardwareProfileDef,
    IndexDef, IndexTypeDef, SchemaSnapshot, StatisticsSnapshot, StorageFormatDef, TableStatsDef,
    TestExpectation, TimelineConfig, TimelineConfigError, TimelineEvent, TimelineMetadata,
};
#[cfg(feature = "timeline")]
pub use timeline_facts::SnapshotFactsProvider;
#[cfg(all(feature = "timeline", feature = "streaming"))]
pub use timeline_optimizer::{
    detect_fact_changes, detect_hardware_changes, detect_schema_changes, detect_stats_changes,
    ChangeDescription, ChangeSeverity, ChangeType, SnapshotResult, TimelineOptimizationResult,
    TimelineOptimizer,
};
#[cfg(feature = "streaming")]
pub use timely::{ComputationStats, TimelyConfig};
#[cfg(feature = "metadata")]
pub use trigger_optimizer::{
    analyze_dml_cost, detect_cascade, CascadeWarning, DmlCostEstimate, TriggerAnalysis,
};
pub use vector_cost::{
    hnsw_search_cost, ivfflat_search_cost, select_vector_index_type, vector_distance_cost,
    vector_sequential_scan_cost, QueryFrequency, VectorIndexRecommendation, VectorIndexType,
    VectorMetric,
};
pub use vector_rules::{
    estimate_vector_query_cost, optimize_vector_filter_order, vector_rewrite_rules,
    VectorIndexParams, VectorQueryCost,
};
pub use xml_optimizer::{
    classify_xml_function, estimate_xpath_cost, parse_xpath, simplify_xpath,
    xml_optimization_rules, NodeTest, PositionPredicate, XPathAxis, XPathCompareOp, XPathExpr,
    XPathPredicate, XPathStep, XmlCostParams, XmlFunctionCall, XmlFunctionKind, XmlIndexInfo,
    XmlIndexType, XmlOptimizerError, XmlPlatform, XmlValueType,
};

/// Parse a string into an egg [`Var`] for use in e-graph rewrite patterns.
///
/// Only call with known-valid pattern variable literals (e.g. `"?x"`).
///
/// # Panics
///
/// Panics on invalid input since these are programmer-controlled constants.
#[expect(clippy::expect_used)]
#[must_use]
pub fn parse_var(s: &str) -> egg::Var {
    s.parse()
        .expect("invalid egg::Var literal; pattern variables must start with '?'")
}




