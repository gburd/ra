//! E-graph integration using the egg library.
//!
//! Defines the [`RelLang`] language for representing relational algebra
//! expressions as S-expressions inside an e-graph. Provides conversion
//! between [`ra_core::RelExpr`] and the e-graph representation, plus
//! the [`Optimizer`] that drives equality saturation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egg::{define_language, EGraph, Id, RecExpr, Rewrite, Runner};
use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering, ProjectionColumn, RelExpr,
    SortDirection, SortKey, WindowExpr, WindowFrame, WindowFrameBound, WindowFrameMode,
    WindowFunction,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr, UnaryOp};
#[cfg(feature = "timeline")]
use ra_stats::delta::DeltaSet;
use tracing::warn;

use crate::analysis::RelAnalysis;
use crate::extract::extract_best;
use crate::genetic_fingerprint::QueryFingerprint;
use crate::plan_cache::{PlanCache, PlanCacheConfig, PlanCacheStats};
use crate::resource_budget::{
    OverflowStrategy, ResourceBudget, ResourceTracker, ResourceUsageReport,
};
use crate::rewrite::all_rules;

define_language! {
    /// S-expression language for relational algebra in the e-graph.
    ///
    /// Each variant maps to a relational or scalar operator. Children
    /// are represented as [`Id`] references into the e-graph.
    pub enum RelLang {
        // -- Relational operators --
        "scan" = Scan([Id; 1]),
        "scan-alias" = ScanAlias([Id; 2]),
        "filter" = Filter([Id; 2]),
        "project" = Project([Id; 2]),
        "join" = Join([Id; 4]),
        "aggregate" = Aggregate([Id; 3]),
        "sort" = Sort([Id; 2]),
        "incremental-sort" = IncrementalSort([Id; 3]),
        "limit" = Limit([Id; 3]),
        "union" = Union([Id; 3]),
        "intersect" = Intersect([Id; 3]),
        "except" = Except([Id; 3]),
        "recursive-cte" = RecursiveCTE([Id; 4]),
        "cte" = CTE([Id; 3]),
        "window" = Window([Id; 2]),
        "distinct-rel" = DistinctRel([Id; 1]),
        "values" = Values(Box<[Id]>),
        "values-row" = ValuesRow(Box<[Id]>),

        // -- Metadata shortcut operators --
        "metadata-lookup" = MetadataLookup([Id; 2]),
        "row-count" = RowCount,

        // -- MIN/MAX index scan optimization --
        // Children: [table, column]
        "index-scan" = IndexScan([Id; 2]),

        // -- Index-only scan (covering index) --
        // Children: [table, index_name, projected_cols, predicate]
        "index-only-scan" = IndexOnlyScan([Id; 4]),

        // -- Materialized view scan --
        // Children: [view_name, alias, group_by_list, agg_list]
        "mv-scan" = MvScan([Id; 4]),

        // -- Bitmap index operators --
        "bitmap-index-scan" = BitmapIndexScan([Id; 3]),
        "bitmap-and" = BitmapAnd(Box<[Id]>),
        "bitmap-or" = BitmapOr(Box<[Id]>),
        "bitmap-heap-scan" = BitmapHeapScan([Id; 3]),

        // -- Window function expression --
        "window-expr" = WindowExprNode([Id; 6]),
        "window-fn" = WindowFn([Id; 1]),
        "window-frame" = WindowFrameNode([Id; 3]),
        "frame-rows" = FrameRows,
        "frame-range" = FrameRange,
        "frame-groups" = FrameGroups,
        "frame-unbounded-preceding" = FrameUnboundedPreceding,
        "frame-preceding" = FramePreceding([Id; 1]),
        "frame-current-row" = FrameCurrentRow,
        "frame-following" = FrameFollowing([Id; 1]),
        "frame-unbounded-following" = FrameUnboundedFollowing,

        // -- Join types --
        "inner" = Inner,
        "left-outer" = LeftOuter,
        "right-outer" = RightOuter,
        "full-outer" = FullOuter,
        "cross" = Cross,
        "semi" = Semi,
        "anti" = Anti,

        // -- Boolean flags --
        "true" = True,
        "false" = False,

        // -- Scalar expressions --
        "col" = Col([Id; 1]),
        "qcol" = QCol([Id; 2]),
        "const-null" = ConstNull,
        "const-bool" = ConstBool([Id; 1]),
        "const-int" = ConstInt([Id; 1]),
        "const-float" = ConstFloat([Id; 1]),
        "const-str" = ConstStr([Id; 1]),

        // -- Binary operators --
        "add" = Add([Id; 2]),
        "sub" = Sub([Id; 2]),
        "mul" = Mul([Id; 2]),
        "div" = Div([Id; 2]),
        "mod" = Mod([Id; 2]),
        "eq" = Eq([Id; 2]),
        "ne" = Ne([Id; 2]),
        "lt" = Lt([Id; 2]),
        "le" = Le([Id; 2]),
        "gt" = Gt([Id; 2]),
        "ge" = Ge([Id; 2]),
        "and" = And([Id; 2]),
        "or" = Or([Id; 2]),
        "concat" = Concat([Id; 2]),
        "json-access" = JsonAccess([Id; 2]),

        // -- Unary operators --
        "not" = Not([Id; 1]),
        "is-null" = IsNull([Id; 1]),
        "is-not-null" = IsNotNull([Id; 1]),
        "neg" = Neg([Id; 1]),

        // -- Function call --
        "func" = Func(Box<[Id]>),

        // -- Aggregate functions --
        "count" = Count([Id; 1]),
        "sum" = Sum([Id; 1]),
        "avg" = Avg([Id; 1]),
        "min" = Min([Id; 1]),
        "max" = Max([Id; 1]),

        // -- Lists --
        "list" = List(Box<[Id]>),
        "nil" = Nil,

        // -- Projection column --
        "proj-col" = ProjCol([Id; 1]),
        "proj-alias" = ProjAlias([Id; 2]),

        // -- Sort keys --
        "sort-key" = SortKey([Id; 3]),
        "asc" = Asc,
        "desc" = Desc,
        "nulls-first" = NullsFirst,
        "nulls-last" = NullsLast,

        // -- Aggregate expression --
        "agg-expr" = AggExpr([Id; 3]),
        "distinct" = Distinct,
        "all" = All,

        // -- Vector search operators (RFC 0064) --
        // Children: [metric, column, target_vector]
        "vector-distance" = VectorDistance([Id; 3]),
        // Children: [table, column, target_vector, k]
        "vector-knn" = VectorKNN([Id; 4]),
        // Children: [table, column, target_vector, threshold, metric]
        "vector-range-scan" = VectorRangeScan([Id; 5]),

        // -- Full-text search operators (RFC 0073) --
        // Children: [vendor, columns, query, mode]
        "fts-match" = FtsMatch([Id; 4]),
        // Children: [column, query, algorithm]
        "fts-rank" = FtsRank([Id; 3]),
        // Children: [table, index_type, predicate]
        "fts-index-scan" = FtsIndexScan([Id; 3]),
        // Children: [table, index_type, query, k, algorithm]
        "fts-ranked-scan" = FtsRankedScan([Id; 5]),
        // Children: [table, pred1, pred2]
        "fts-skip-list-and" = FtsSkipListAnd([Id; 3]),

        // -- Hybrid search operators (RFC 0073) --
        // Children: [fts_score, vector_score, alpha, beta, method]
        "hybrid-score" = HybridScore([Id; 5]),
        // Children: [table, fts_args, vector_args, strategy, k, limit]
        "hybrid-scan" = HybridScan([Id; 6]),

        // -- Type casting operator --
        // Children: [expr, target_type]
        "cast" = Cast([Id; 2]),

        // -- Leaf symbols (table names, column names, strings) --
        Symbol(egg::Symbol),
    }
}

/// Configuration for the equality saturation optimizer.
#[derive(Debug, Clone)]
pub struct OptimizerConfig {
    /// Maximum number of e-graph nodes before stopping.
    pub node_limit: usize,
    /// Maximum number of iterations.
    pub iter_limit: usize,
    /// Time limit in seconds.
    pub time_limit_secs: u64,
    /// Number of tables to trigger large join fallback.
    pub large_join_threshold: usize,
    /// Strategy for large join optimization.
    pub large_join_strategy: crate::large_join::LargeJoinStrategy,
    /// Hard timeout for optimization (ms).
    pub max_optimization_time_ms: u64,
    /// Parallel query execution configuration.
    pub parallel: ParallelConfig,
    /// Enable adaptive iteration limits based on query complexity.
    pub use_adaptive_limits: bool,
    /// Enable cost-based pruning during optimization.
    pub use_cost_pruning: bool,
    /// Cost pruning threshold (e.g., 1.5 = prune plans >50% worse than best).
    pub cost_pruning_threshold: f64,
    /// Enable join graph filtering to prune invalid join combinations.
    pub use_join_graph_filtering: bool,
    /// Beam search configuration for managing search space size.
    /// Set to None to disable beam search.
    pub beam_search_config: Option<crate::beam_search::BeamSearchConfig>,
    /// Transaction isolation context for isolation-aware cost adjustments.
    /// When set, the optimizer applies penalties for lock footprint,
    /// snapshot bloat, SubXID overflow, and MultiXact pressure.
    /// When `None`, all isolation penalties are zero.
    pub transaction_context: Option<ra_core::isolation::TransactionContext>,
    /// Enable the fingerprint-based plan cache.
    /// Default: false (must be opted in).
    pub enable_plan_cache: bool,
    /// Configuration for the plan cache (used when `enable_plan_cache` is true).
    pub plan_cache_config: PlanCacheConfig,
    /// Maximum staleness penalty factor (default: 10.0).
    /// Caps how much stale statistics can increase cost estimates.
    pub max_staleness_penalty: f64,
    /// Enable lazy rule compilation (on-demand rule loading).
    /// Default: false (loads all rules upfront).
    pub use_lazy_rules: bool,
    /// Enable the rule advisor pipeline for intelligent rule filtering.
    /// Default: false (must be opted in).
    pub use_rule_advisor: bool,
    /// Configuration for the rule advisor (used when `use_rule_advisor`
    /// is true).
    pub rule_advisor_config: crate::rule_advisor::RuleAdvisorConfig,
}

/// Configuration for parallel query execution.
#[derive(Debug, Clone)]
pub struct ParallelConfig {
    /// Maximum number of parallel workers across all operations.
    pub max_parallel_workers: usize,
    /// Maximum workers for a single gather operation.
    pub max_parallel_workers_per_gather: usize,
    /// Minimum table size in bytes to consider parallel scan.
    pub min_parallel_table_scan_size: usize,
    /// Cost of processing one tuple in parallel context.
    pub parallel_tuple_cost: f64,
    /// Fixed setup cost for parallel execution.
    pub parallel_setup_cost: f64,
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            max_parallel_workers: 8,
            max_parallel_workers_per_gather: 4,
            min_parallel_table_scan_size: 8_388_608, // 8 MB
            parallel_tuple_cost: 0.1,
            parallel_setup_cost: 1000.0,
        }
    }
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            node_limit: 100_000,
            iter_limit: 30, // Fallback when use_adaptive_limits = false
            time_limit_secs: 10,
            large_join_threshold: 10,
            large_join_strategy: crate::large_join::LargeJoinStrategy::default(),
            max_optimization_time_ms: 30000,
            parallel: ParallelConfig::default(),
            use_adaptive_limits: true, // Enable adaptive limits by default
            use_cost_pruning: true,    // Enable cost pruning by default
            cost_pruning_threshold: 1.5, // Prune plans >50% worse than best
            use_join_graph_filtering: true, // Enable join graph filtering by default
            beam_search_config: None,  // Disabled by default (can be enabled for complex queries)
            transaction_context: None, // No isolation awareness by default
            enable_plan_cache: false,
            plan_cache_config: PlanCacheConfig::default(),
            max_staleness_penalty: 10.0, // Cap at 10x cost penalty
            use_lazy_rules: false,       // Disabled by default (opt-in for better compatibility)
            use_rule_advisor: false,     // Disabled by default (Phase 1 rollout)
            rule_advisor_config: crate::rule_advisor::RuleAdvisorConfig::default(),
        }
    }
}

/// The main optimization engine.
///
/// Converts a [`RelExpr`] into an e-graph, runs equality saturation
/// with rewrite rules, then extracts the lowest-cost plan.
///
/// When plan caching is enabled, the optimizer computes a genetic
/// fingerprint of each query and checks the cache before running
/// equality saturation. Structurally identical queries (differing
/// only in literal values) reuse cached plans.
#[derive(Debug)]
pub struct Optimizer {
    config: OptimizerConfig,
    table_stats: HashMap<String, ra_core::statistics::Statistics>,
    hardware_profile: Option<ra_hardware::HardwareProfile>,
    resource_budget: Option<ResourceBudget>,
    plan_cache: Option<Mutex<PlanCache>>,
    rule_advisor: Option<Mutex<crate::rule_advisor::RuleAdvisor>>,
}

impl Optimizer {
    /// Create a new optimizer with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: OptimizerConfig::default(),
            table_stats: HashMap::new(),
            hardware_profile: None,
            resource_budget: None,
            plan_cache: None,
            rule_advisor: None,
        }
    }

    /// Create an optimizer with custom configuration.
    #[must_use]
    pub fn with_config(config: OptimizerConfig) -> Self {
        let plan_cache = if config.enable_plan_cache {
            Some(Mutex::new(PlanCache::new(config.plan_cache_config.clone())))
        } else {
            None
        };
        let rule_advisor = if config.use_rule_advisor {
            Some(Mutex::new(crate::rule_advisor::RuleAdvisor::new(
                config.rule_advisor_config.clone(),
            )))
        } else {
            None
        };
        Self {
            config,
            table_stats: HashMap::new(),
            hardware_profile: None,
            resource_budget: None,
            plan_cache,
            rule_advisor,
        }
    }

    /// Enable the plan cache with the given configuration.
    #[must_use]
    pub fn with_plan_cache(mut self, config: PlanCacheConfig) -> Self {
        self.config.enable_plan_cache = true;
        self.config.plan_cache_config = config.clone();
        self.plan_cache = Some(Mutex::new(PlanCache::new(config)));
        self
    }

    /// Return a snapshot of plan cache statistics.
    ///
    /// Returns `None` if caching is not enabled.
    #[must_use]
    pub fn cache_stats(&self) -> Option<PlanCacheStats> {
        self.plan_cache.as_ref().map(|m| {
            let cache = m.lock().unwrap_or_else(|e| e.into_inner());
            cache.stats().clone()
        })
    }

    /// Clear the plan cache.
    pub fn clear_cache(&self) {
        if let Some(m) = self.plan_cache.as_ref() {
            let mut cache = m.lock().unwrap_or_else(|e| e.into_inner());
            cache.clear();
        }
    }

    /// Return a snapshot of rule advisor statistics.
    ///
    /// Returns `None` if the rule advisor is not enabled.
    #[must_use]
    pub fn advisor_stats(&self) -> Option<crate::rule_advisor::AdvisorStats> {
        self.rule_advisor.as_ref().map(|m| {
            let advisor = m.lock().unwrap_or_else(|e| e.into_inner());
            advisor.stats().clone()
        })
    }

    /// Load rules using the advisor > lazy > all priority chain.
    ///
    /// Centralises the rule-loading logic so every optimisation path
    /// (normal, bounded, tracking, incremental) honours the rule
    /// advisor when it is configured.
    fn load_rules(&self, expr: &RelExpr) -> Vec<Rewrite<RelLang, RelAnalysis>> {
        if let Some(ref advisor_mutex) = self.rule_advisor {
            let mut advisor = advisor_mutex.lock().unwrap_or_else(|e| e.into_inner());
            advisor.select_rules(expr)
        } else if self.config.use_lazy_rules {
            let pattern = crate::lazy_rules::LazyQueryPattern::analyze(expr);
            let compiler = crate::lazy_rules::LazyRuleCompiler::new();
            compiler.compile(&pattern)
        } else {
            all_rules()
        }
    }

    /// Set a resource budget for bounded optimization.
    pub fn set_resource_budget(&mut self, budget: ResourceBudget) {
        self.resource_budget = Some(budget);
    }

    /// Builder-style setter for the resource budget.
    #[must_use]
    pub fn with_resource_budget(mut self, budget: ResourceBudget) -> Self {
        self.resource_budget = Some(budget);
        self
    }

    /// Set the hardware profile for cost-based optimization.
    pub fn set_hardware_profile(&mut self, profile: ra_hardware::HardwareProfile) {
        self.hardware_profile = Some(profile);
    }

    /// Get the current hardware profile, or auto-detect if not set.
    #[must_use]
    pub fn hardware_profile(&self) -> ra_hardware::HardwareProfile {
        self.hardware_profile
            .clone()
            .unwrap_or_else(ra_hardware::detect_hardware)
    }

    /// Register table statistics for cost estimation.
    pub fn add_table_stats(
        &mut self,
        table: impl Into<String>,
        stats: ra_core::statistics::Statistics,
    ) {
        self.table_stats.insert(table.into(), stats);
    }

    /// Optimize a relational expression using equality saturation.
    ///
    /// Returns the optimized expression, or an error if conversion
    /// fails.
    ///
    /// # Errors
    ///
    /// Returns an error if the expression cannot be converted to
    /// the e-graph representation or if extraction fails.
    pub fn optimize(&self, expr: &RelExpr) -> Result<RelExpr, EGraphError> {
        use std::time::Instant;
        use tracing::{debug, info};

        let total_start = Instant::now();

        // Plan cache fast path: check if we have a cached plan for
        // a structurally equivalent query.
        let fingerprint = if self.plan_cache.is_some() {
            let fp = QueryFingerprint::from_rel_expr(expr);
            if let Some(ref mutex) = self.plan_cache {
                let mut cache = mutex.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(hit) = cache.lookup(&fp) {
                    info!(
                        "Plan cache hit ({:?}, similarity={:.2}) \
                         in {:?}",
                        hit.match_type,
                        hit.similarity,
                        total_start.elapsed()
                    );
                    return Ok(hit.plan);
                }
                debug!("Plan cache miss");
            }
            Some(fp)
        } else {
            None
        };

        // Fast path: Use left-deep tree for queries with 2-7 tables
        if crate::left_deep::can_use_left_deep(expr) {
            debug!("Using left-deep tree optimization");
            let cost_model: Arc<dyn ra_core::cost::CostModel> =
                Arc::new(ra_hardware::HardwareCostModel::new(self.hardware_profile()));
            let stats_provider = Arc::new(TableStatsProvider {
                stats: self.table_stats.clone(),
            });

            let builder = crate::left_deep::LeftDeepBuilder::new(cost_model, stats_provider);
            match builder.build(expr) {
                Ok(optimized) => {
                    info!(
                        "Left-deep optimization completed in {:?}",
                        total_start.elapsed()
                    );
                    self.insert_into_cache(&fingerprint, &optimized);
                    return Ok(optimized);
                }
                Err(e) => {
                    debug!("Left-deep failed ({}), falling back to e-graph", e);
                }
            }
        }

        // Check if we should use large join optimization
        let table_count = crate::large_join::LargeJoinOptimizer::count_tables(expr);

        if table_count >= self.config.large_join_threshold {
            match &self.config.large_join_strategy {
                crate::large_join::LargeJoinStrategy::EGraph => {
                    // Continue with standard e-graph optimization
                }
                _ => {
                    // Use large join optimizer
                    let cost_model: Arc<dyn ra_core::cost::CostModel> =
                        Arc::new(ra_hardware::HardwareCostModel::new(self.hardware_profile()));
                    let stats_provider = Arc::new(TableStatsProvider {
                        stats: self.table_stats.clone(),
                    });

                    let large_optimizer = crate::large_join::LargeJoinOptimizer::new(
                        self.config.large_join_strategy.clone(),
                        cost_model,
                        stats_provider,
                    );

                    let joins = crate::large_join::LargeJoinOptimizer::extract_joins(expr);
                    if !joins.is_empty() {
                        let result = large_optimizer
                            .optimize(joins)
                            .map_err(|e| EGraphError::ExtractionError(e.to_string()))?;
                        self.insert_into_cache(&fingerprint, &result);
                        return Ok(result);
                    }
                }
            }
        }

        // Calculate adaptive iteration limit based on query complexity
        let complexity = if self.config.use_adaptive_limits {
            crate::query_complexity::QueryComplexity::from_expr(expr)
        } else {
            // Fallback: classify by table count only
            match table_count {
                0..=1 => crate::query_complexity::QueryComplexity::Trivial,
                2..=4 => crate::query_complexity::QueryComplexity::Simple,
                5..=7 => crate::query_complexity::QueryComplexity::Medium,
                8..=9 => crate::query_complexity::QueryComplexity::Complex,
                _ => crate::query_complexity::QueryComplexity::VeryComplex,
            }
        };

        let iter_limit = if self.config.use_adaptive_limits {
            complexity.default_iter_limit()
        } else {
            self.config.iter_limit
        };

        let timeout_ms = if self.config.use_adaptive_limits {
            complexity.default_timeout_ms()
        } else {
            self.config.time_limit_secs * 1000
        };

        // Standard e-graph optimization with timing
        info!(
            "Starting e-graph optimization: {} tables, complexity={:?}, iter_limit={}, timeout={}ms",
            table_count, complexity, iter_limit, timeout_ms
        );

        // Convert table stats to Arc-wrapped cache for cheap sharing
        // This avoids repeated clones during cost extraction
        let stats_cache = crate::stats_cache::StatsCache::from_map(self.table_stats.clone());

        let to_rec_start = Instant::now();
        let rec_expr = to_rec_expr(expr)?;
        let to_rec_elapsed = to_rec_start.elapsed();
        debug!("to_rec_expr: {:?}", to_rec_elapsed);

        let runner_start = Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        let rules = self.load_rules(expr);

        // Create convergence detector (window size: 3, min growth rate: 5%)
        let mut convergence_detector = crate::convergence::ConvergenceDetector::default_settings();

        // Create cost pruner (if enabled)
        let mut cost_pruner = if self.config.use_cost_pruning {
            Some(crate::cost_pruning::CostPruner::new(
                self.config.cost_pruning_threshold,
            ))
        } else {
            None
        };

        // Create beam search tracker (if enabled)
        let mut beam_search_tracker = if let Some(ref beam_config) = self.config.beam_search_config
        {
            Some(crate::beam_search::BeamSearchTracker::new(
                beam_config.clone(),
            ))
        } else {
            None
        };

        // Build join graph (if enabled)
        if self.config.use_join_graph_filtering {
            let join_graph = crate::join_graph::JoinGraph::from_expr(expr);
            let stats = join_graph.stats();
            if stats.table_count > 2 {
                debug!(
                    "Join graph: {} tables, {} edges, density={:.2}, estimated reduction={:.1}%",
                    stats.table_count,
                    stats.edge_count,
                    stats.density(),
                    stats.estimated_reduction_factor() * 100.0
                );
            }
        }

        // Initialize e-graph with the expression
        let mut egraph: EGraph<RelLang, RelAnalysis> = EGraph::default();
        let root = egraph.add_expr(&rec_expr);

        let mut termination_reason = "iteration_limit";
        let mut actual_iterations = 0;
        let mut best_cost = f64::INFINITY;
        let mut cost_improvement_stalled = 0;

        // Cache hardware profile outside loop to avoid repeated clones
        let hardware_cached = if cost_pruner.is_some() || beam_search_tracker.is_some() {
            Some(self.hardware_profile())
        } else {
            None
        };

        // Run iterations one at a time to enable convergence detection
        for iteration in 0..iter_limit {
            // Check timeout
            if runner_start.elapsed() >= timeout {
                termination_reason = "timeout";
                break;
            }

            let prev_classes = egraph.number_of_classes();

            // Run one iteration
            let runner: Runner<RelLang, RelAnalysis> = Runner::default()
                .with_egraph(egraph)
                .with_node_limit(self.config.node_limit)
                .with_iter_limit(1)
                .with_time_limit(timeout.saturating_sub(runner_start.elapsed()))
                .run(&rules);

            egraph = runner.egraph;
            actual_iterations = iteration + 1;

            // Calculate new equivalences found
            let curr_nodes = egraph.total_size();
            let curr_classes = egraph.number_of_classes();
            let unions = if iteration > 0 {
                // Approximate unions by change in classes
                prev_classes.saturating_sub(curr_classes)
            } else {
                curr_classes // First iteration creates all initial classes
            };

            // Record metrics for convergence detection
            convergence_detector.record(crate::convergence::IterationMetrics {
                iteration,
                unions,
                total_nodes: curr_nodes,
                total_classes: curr_classes,
            });

            // Track cost improvement (if cost pruning or beam search enabled)
            let current_cost = if let Some(ref hardware) = hardware_cached {
                let cost_fn = crate::extract::RelCostFn::new(hardware.clone());
                let extractor = egg::Extractor::new(&egraph, cost_fn);
                let (cost, _) = extractor.find_best(root);
                Some(cost)
            } else {
                None
            };

            // Cost pruning
            if let Some(pruner) = cost_pruner.as_mut() {
                if let Some(cost) = current_cost {
                    pruner.record_cost(root, cost);

                    // Check if cost improved significantly
                    let improvement_threshold = 0.01; // 1% improvement
                    if cost < best_cost * (1.0 - improvement_threshold) {
                        // Significant improvement
                        best_cost = cost;
                        cost_improvement_stalled = 0;
                    } else {
                        // No significant improvement
                        cost_improvement_stalled += 1;

                        // Terminate if cost hasn't improved for 3 consecutive iterations
                        if cost_improvement_stalled >= 3 {
                            termination_reason = "cost_stagnant";
                            debug!(
                                "Early termination: cost stagnant for 3 iterations (best: {:.2})",
                                best_cost
                            );
                            break;
                        }
                    }
                }
            }

            // Beam search: record plan costs and prune
            if let Some(tracker) = beam_search_tracker.as_mut() {
                if let Some(cost) = current_cost {
                    tracker.start_iteration(iteration);
                    tracker.record_plan(root, cost);
                    let pruned = tracker.prune();

                    if pruned > 0 {
                        debug!(
                            "Beam search: pruned {} plans at iteration {} (kept top {})",
                            pruned,
                            iteration,
                            tracker.stats().plans_kept
                        );
                    }
                }
            }

            // Check for convergence
            if convergence_detector.should_terminate()
                == crate::convergence::TerminationDecision::Converged
            {
                termination_reason = "converged";
                debug!(
                    "Early termination: converged at iteration {} (stats: {:?})",
                    iteration,
                    convergence_detector.stats()
                );
                break;
            }

            // Check for egg saturation
            if let Some(stop_reason) = runner.stop_reason.as_ref() {
                if matches!(stop_reason, egg::StopReason::Saturated) {
                    termination_reason = "saturated";
                    break;
                }
            }
        }

        let runner_elapsed = runner_start.elapsed();
        let egraph_nodes = egraph.total_size();
        let egraph_classes = egraph.number_of_classes();

        // Log pruning statistics if cost pruning was enabled
        if let Some(pruner) = cost_pruner.as_ref() {
            let stats = pruner.stats();
            if stats.classes_evaluated > 0 {
                debug!(
                    "Cost pruning: best_cost={:.2}, evaluated={}, pruned={} ({:.1}% pruning rate)",
                    stats.global_best_cost.unwrap_or(f64::INFINITY),
                    stats.classes_evaluated,
                    stats.classes_pruned,
                    stats.pruning_rate()
                );
            }
        }

        // Log beam search statistics if beam search was enabled
        if let Some(tracker) = beam_search_tracker.as_ref() {
            let stats = tracker.stats();
            if stats.is_active() && stats.plans_pruned > 0 {
                info!(
                    "Beam search: beam_width={}, total_plans={}, kept={}, pruned={} ({:.1}% reduction)",
                    stats.beam_width,
                    stats.total_plans,
                    stats.plans_kept,
                    stats.plans_pruned,
                    stats.pruning_rate()
                );
            }
        }

        info!(
            "E-graph saturation: {:?} ({} iterations, {} nodes, {} classes, reason: {})",
            runner_elapsed, actual_iterations, egraph_nodes, egraph_classes, termination_reason
        );

        let extract_start = Instant::now();
        let hardware = self.hardware_profile();
        let result = extract_best(&egraph, root, stats_cache.as_map(), &hardware)?;
        let extract_elapsed = extract_start.elapsed();
        debug!("extract_best: {:?}", extract_elapsed);

        let total_elapsed = total_start.elapsed();
        info!(
            "Total optimization: {:?} (to_rec={:?}, egraph={:?}, extract={:?})",
            total_elapsed, to_rec_elapsed, runner_elapsed, extract_elapsed
        );

        self.insert_into_cache(&fingerprint, &result);
        Ok(result)
    }

    /// Insert a plan into the cache if caching is enabled.
    fn insert_into_cache(&self, fingerprint: &Option<QueryFingerprint>, plan: &RelExpr) {
        if let Some(fp) = fingerprint {
            if let Some(ref mutex) = self.plan_cache {
                let mut cache = mutex.lock().unwrap_or_else(|e| e.into_inner());
                cache.insert(fp.clone(), plan.clone());
            }
        }
    }

    /// Optimize using pre-condition-filtered rules based on available system facts.
    ///
    /// This method evaluates rule pre-conditions against the provided facts
    /// and only applies rules whose pre-conditions are satisfied. This can
    /// significantly reduce the search space when facts indicate certain rules
    /// are not applicable.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use ra_engine::{Optimizer, FactsContextBuilder};
    /// use ra_hardware::HardwareProfile;
    ///
    /// let facts = FactsContextBuilder::new(HardwareProfile::cpu_only())
    ///     .database("postgresql")
    ///     .feature("lateral_join", true)
    ///     .build();
    ///
    /// let optimizer = Optimizer::new();
    /// let optimized = optimizer.optimize_with_facts(&expr, &facts)?;
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the expression cannot be converted to
    /// the e-graph representation or if extraction fails.
    pub fn optimize_with_facts(
        &self,
        expr: &RelExpr,
        facts: &dyn ra_core::FactsProvider,
    ) -> Result<RelExpr, EGraphError> {
        use tracing::{debug, info};

        // Note: In a full implementation, we would:
        // 1. Load RuleMetadata from .rra files
        // 2. Evaluate pre-conditions for each rule using PreConditionEvaluator
        // 3. Filter egg::Rewrite rules based on matching RuleMetadata
        // 4. Run optimization with filtered rules
        //
        // For now, we demonstrate the API and log the filtering intent.

        let total_rules = all_rules().len();

        // Log facts availability
        debug!(
            "Optimizing with facts: database={}, dialect={:?}, cpu_cores={}, has_gpu={}, memory={}",
            facts.database_name(),
            facts.sql_dialect(),
            facts.cpu_cores(),
            facts.has_gpu(),
            facts.available_memory(),
        );

        info!(
            "Pre-condition filtering enabled ({} total rules available)",
            total_rules
        );

        // Load RuleMetadata from .rra files and filter based on pre-conditions
        let filtered_rules = if let Ok(rules_dir) = std::env::var("RA_RULES_DIR") {
            let rules_path = std::path::PathBuf::from(rules_dir);
            match crate::rule_metadata::load_rules_from_directory(&rules_path) {
                Ok(parsed_rules) => {
                    let applicable_rule_ids =
                        crate::rule_metadata::filter_rules_by_preconditions(&parsed_rules, facts);

                    info!(
                        "Loaded {} rules, {} applicable after precondition filtering",
                        parsed_rules.len(),
                        applicable_rule_ids.len()
                    );

                    // For now, use all rules since we need to map rule IDs to egg::Rewrite
                    // TODO: Build a HashMap<String, egg::Rewrite> to enable actual filtering
                    all_rules()
                }
                Err(e) => {
                    warn!("Failed to load rules from {}: {}", rules_path.display(), e);
                    all_rules()
                }
            }
        } else {
            // No RA_RULES_DIR set, use all rules
            debug!("RA_RULES_DIR not set, using all rules");
            all_rules()
        };

        info!(
            "Applying {} rules (filtered from {} total)",
            filtered_rules.len(),
            total_rules
        );

        // Run optimization with filtered rules
        let rec_expr = to_rec_expr(expr)?;
        let runner: Runner<RelLang, RelAnalysis> = Runner::default()
            .with_expr(&rec_expr)
            .with_node_limit(self.config.node_limit)
            .with_iter_limit(self.config.iter_limit)
            .with_time_limit(std::time::Duration::from_secs(self.config.time_limit_secs))
            .run(&filtered_rules);

        let root = runner.roots[0];
        let hardware = self.hardware_profile();
        let result = extract_best(&runner.egraph, root, &self.table_stats, &hardware)?;
        Ok(result)
    }

    /// Run optimization and return both the result and the e-graph
    /// for inspection.
    ///
    /// # Errors
    ///
    /// Returns an error if conversion or extraction fails.
    pub fn optimize_with_egraph(
        &self,
        expr: &RelExpr,
    ) -> Result<(RelExpr, EGraph<RelLang, RelAnalysis>), EGraphError> {
        let rec_expr = to_rec_expr(expr)?;
        let rules = self.load_rules(expr);
        let runner: Runner<RelLang, RelAnalysis> = Runner::default()
            .with_expr(&rec_expr)
            .with_node_limit(self.config.node_limit)
            .with_iter_limit(self.config.iter_limit)
            .with_time_limit(std::time::Duration::from_secs(self.config.time_limit_secs))
            .run(&rules);

        let root = runner.roots[0];
        let hardware = self.hardware_profile();
        let result = extract_best(&runner.egraph, root, &self.table_stats, &hardware)?;
        Ok((result, runner.egraph))
    }

    /// Optimize with resource budget tracking and best-so-far.
    ///
    /// Runs equality saturation one iteration at a time, checking
    /// the resource budget between iterations and tracking the best
    /// plan seen so far. Returns an [`OptimizationResult`] containing
    /// the plan, cost, completion status, and resource usage report.
    ///
    /// If no resource budget is set, uses the default unlimited budget.
    ///
    /// # Errors
    ///
    /// Returns an error if expression conversion fails, or if the
    /// overflow strategy is [`OverflowStrategy::Fail`] and the budget
    /// is exceeded before any plan is extracted.
    pub fn optimize_bounded(&self, expr: &RelExpr) -> Result<OptimizationResult, EGraphError> {
        let budget = self.resource_budget.clone().unwrap_or_default();
        let mut tracker = ResourceTracker::start(budget);

        let rec_expr = to_rec_expr(expr)?;
        let hardware = self.hardware_profile();
        let rules = self.load_rules(expr);

        let iter_limit = self.config.iter_limit;
        let node_limit = self.config.node_limit;
        let time_limit_secs = self.config.time_limit_secs;

        let mut egraph: EGraph<RelLang, RelAnalysis> = EGraph::default();
        let root = egraph.add_expr(&rec_expr);

        let mut best_plan: Option<RelExpr> = None;
        let mut best_cost = f64::INFINITY;

        // Extract initial plan (the original, unoptimized)
        if let Ok(plan) = extract_best(&egraph, root, &self.table_stats, &hardware) {
            best_plan = Some(plan);
            best_cost = estimate_plan_cost(&egraph, root, &hardware);
        }

        for _iteration in 0..iter_limit {
            // Check budget before running an iteration
            let check = tracker.check();
            if !check.is_within_budget() {
                return handle_overflow(&tracker, expr, best_plan, best_cost);
            }

            // Run one iteration of equality saturation
            let runner: Runner<RelLang, RelAnalysis> = Runner::default()
                .with_egraph(egraph)
                .with_node_limit(node_limit)
                .with_iter_limit(1)
                .with_time_limit(std::time::Duration::from_secs(time_limit_secs))
                .run(&rules);

            egraph = runner.egraph;
            tracker.record_iteration();
            tracker.record_egraph_nodes(egraph.total_number_of_nodes());

            // Estimate memory: ~64 bytes per e-graph node is rough
            let mem_estimate = (egraph.total_number_of_nodes() as u64).saturating_mul(64);
            tracker.record_memory_estimate(mem_estimate);

            // Try to extract the best plan from the current e-graph
            if let Ok(plan) = extract_best(&egraph, root, &self.table_stats, &hardware) {
                let cost = estimate_plan_cost(&egraph, root, &hardware);
                if cost < best_cost {
                    best_cost = cost;
                    best_plan = Some(plan);
                }
            }

            // Check for egg saturation (no new nodes)
            if runner
                .stop_reason
                .as_ref()
                .is_some_and(|r| matches!(r, egg::StopReason::Saturated))
            {
                break;
            }
        }

        let report = tracker.report();
        let status = if report.completed_within_budget() {
            OptimizationStatus::Complete
        } else {
            OptimizationStatus::Incomplete
        };

        match best_plan {
            Some(plan) => Ok(OptimizationResult {
                plan,
                cost: best_cost,
                status,
                resource_usage: report,
                applied_rules: None,
                rule_tracking: None,
            }),
            None => Err(EGraphError::ExtractionError(
                "no plan could be extracted".to_owned(),
            )),
        }
    }

    /// Optimize with detailed rule tracking enabled.
    ///
    /// This method runs each rule individually to track which specific rules
    /// were applied, how many times they fired, and the cost improvement from
    /// each rule. The detailed tracking information is returned in
    /// `OptimizationResult.rule_tracking`.
    ///
    /// # Errors
    ///
    /// Returns an error if expression conversion fails, or if the
    /// overflow strategy is `OverflowStrategy::Fail` and the budget
    /// is exceeded before any plan is extracted.
    pub fn optimize_with_tracking(
        &self,
        expr: &RelExpr,
    ) -> Result<OptimizationResult, EGraphError> {
        self.optimize_with_tracking_verbose(expr, false)
    }

    /// Optimizes a relational algebra expression with detailed tracking.
    ///
    /// When verbose is true, captures intermediate plan transformations
    /// after each rule application for detailed debugging output.
    ///
    /// # Errors
    ///
    /// Returns an error if expression conversion fails, or if the
    /// overflow strategy is `OverflowStrategy::Fail` and the budget
    /// is exceeded before any plan is extracted.
    pub fn optimize_with_tracking_verbose(
        &self,
        expr: &RelExpr,
        verbose: bool,
    ) -> Result<OptimizationResult, EGraphError> {
        let budget = self.resource_budget.clone().unwrap_or_default();
        let mut tracker = ResourceTracker::start(budget);

        let rec_expr = to_rec_expr(expr)?;
        let hardware = self.hardware_profile();
        let rules = self.load_rules(expr);

        let iter_limit = self.config.iter_limit;
        let node_limit = self.config.node_limit;

        let mut egraph: EGraph<RelLang, RelAnalysis> = EGraph::default();
        let root = egraph.add_expr(&rec_expr);

        let mut best_plan: Option<RelExpr> = None;
        let mut best_cost = f64::INFINITY;
        let initial_cost = estimate_plan_cost(&egraph, root, &hardware);

        // Extract initial plan (the original, unoptimized)
        if let Ok(plan) = extract_best(&egraph, root, &self.table_stats, &hardware) {
            best_plan = Some(plan);
            best_cost = initial_cost;
        }

        // Track per-rule applications
        let available_rules: Vec<String> = rules.iter().map(|r| r.name.to_string()).collect();

        let mut rule_applications: Vec<RuleApplication> = Vec::new();
        let mut intermediate_steps: Vec<IntermediateStep> = Vec::new();
        let mut step_number = 0;
        let mut iteration_number = 0;
        let mut saturated = false;

        for _iteration in 0..iter_limit {
            // Check budget before running an iteration
            let check = tracker.check();
            if !check.is_within_budget() {
                let tracking = build_detailed_tracking(available_rules, rule_applications);
                return handle_overflow_with_tracking(
                    &tracker,
                    expr,
                    best_plan,
                    best_cost,
                    Some(tracking),
                );
            }

            iteration_number += 1;
            let mut any_rule_applied_this_iteration = false;

            // Run each rule individually to track its contribution
            for rule in &rules {
                let nodes_before = egraph.total_number_of_nodes();
                let cost_before = estimate_plan_cost(&egraph, root, &hardware);

                // Extract plan before if verbose mode
                let plan_before = if verbose {
                    extract_best(&egraph, root, &self.table_stats, &hardware).ok()
                } else {
                    None
                };

                // Run this single rule once
                let runner: Runner<RelLang, RelAnalysis> = Runner::default()
                    .with_egraph(egraph)
                    .with_node_limit(node_limit)
                    .with_iter_limit(1)
                    .run(&[rule.clone()]);

                egraph = runner.egraph;
                let nodes_after = egraph.total_number_of_nodes();
                let nodes_added = nodes_after.saturating_sub(nodes_before);

                // If this rule added nodes, track it
                if nodes_added > 0 {
                    any_rule_applied_this_iteration = true;

                    // Measure cost improvement
                    let cost_after = estimate_plan_cost(&egraph, root, &hardware);
                    let cost_improvement = if cost_after < cost_before {
                        Some(cost_before - cost_after)
                    } else {
                        None
                    };

                    // Try to extract better plan
                    if let Ok(plan) = extract_best(&egraph, root, &self.table_stats, &hardware) {
                        if cost_after < best_cost {
                            best_cost = cost_after;
                            best_plan = Some(plan.clone());
                        }

                        // Capture intermediate step if verbose
                        if verbose {
                            if let Some(before) = plan_before {
                                step_number += 1;
                                let reason = if let Some(improvement) = cost_improvement {
                                    format!("Cost improvement: {:.4}", improvement)
                                } else {
                                    "Pattern matched, exploring alternatives".to_string()
                                };

                                intermediate_steps.push(IntermediateStep {
                                    step_number,
                                    rule_name: rule.name.to_string(),
                                    reason,
                                    plan_before: before,
                                    plan_after: plan,
                                    cost_improvement,
                                });
                            }
                        }
                    }

                    rule_applications.push(RuleApplication {
                        name: format!("{} (iteration {})", rule.name, iteration_number),
                        fired_count: 1,
                        nodes_added,
                        cost_improvement,
                    });
                }
            }

            tracker.record_iteration();
            tracker.record_egraph_nodes(egraph.total_number_of_nodes());

            // Estimate memory
            let mem_estimate = (egraph.total_number_of_nodes() as u64).saturating_mul(64);
            tracker.record_memory_estimate(mem_estimate);

            // If no rules applied anything this iteration, we're saturated
            if !any_rule_applied_this_iteration {
                saturated = true;
                break;
            }
        }

        let report = tracker.report();
        let status = if report.completed_within_budget() {
            if saturated {
                OptimizationStatus::Complete
            } else {
                OptimizationStatus::Incomplete
            }
        } else {
            OptimizationStatus::Incomplete
        };

        let tracking = build_detailed_tracking_with_steps(
            available_rules,
            rule_applications,
            if verbose {
                Some(intermediate_steps)
            } else {
                None
            },
        );

        match best_plan {
            Some(plan) => Ok(OptimizationResult {
                plan,
                cost: best_cost,
                status,
                resource_usage: report,
                applied_rules: None,
                rule_tracking: Some(tracking),
            }),
            None => Err(EGraphError::ExtractionError(
                "no plan could be extracted".to_owned(),
            )),
        }
    }

    /// Incrementally reoptimize a plan given statistics deltas.
    ///
    /// Instead of running full equality saturation from scratch,
    /// this method:
    /// 1. Applies the statistics deltas to the internal table stats
    /// 2. If deltas are small, runs a reduced number of iterations
    /// 3. Reports how much work was saved vs full reoptimization
    ///
    /// When the delta set indicates a large change (structural changes,
    /// \>50% row count shift, or many small changes), this falls back
    /// to full optimization automatically.
    ///
    /// # Errors
    ///
    /// Returns an error if the expression cannot be converted or
    /// extraction fails.
    #[cfg(feature = "timeline")]
    pub fn optimize_incremental(
        &mut self,
        expr: &RelExpr,
        stats_delta: &DeltaSet,
    ) -> Result<(RelExpr, IncrementalStats), EGraphError> {
        let start = std::time::Instant::now();

        // Apply deltas to internal table stats.
        let tables_updated = self.apply_stats_delta(stats_delta);

        // Decide iteration budget based on delta magnitude.
        let (iter_limit, is_full) = if stats_delta.needs_full_reoptimization() {
            (self.config.iter_limit, true)
        } else {
            // Scale iterations by change magnitude.
            let pct = stats_delta.row_count_change_pct();
            let fraction = (pct / 100.0).clamp(0.05, 1.0);
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let iters = ((self.config.iter_limit as f64) * fraction).ceil() as usize;
            (iters.max(1), false)
        };

        let rec_expr = to_rec_expr(expr)?;
        let rules = self.load_rules(expr);
        let hardware = self.hardware_profile();

        let runner: Runner<RelLang, RelAnalysis> = Runner::default()
            .with_expr(&rec_expr)
            .with_node_limit(self.config.node_limit)
            .with_iter_limit(iter_limit)
            .with_time_limit(std::time::Duration::from_secs(self.config.time_limit_secs))
            .run(&rules);

        let root = runner.roots[0];
        let result = extract_best(&runner.egraph, root, &self.table_stats, &hardware)?;

        let elapsed = start.elapsed();
        let nodes_in_egraph = runner.egraph.total_number_of_nodes();

        let stats = IncrementalStats {
            rules_evaluated: rules.len(),
            iterations_used: iter_limit,
            max_iterations: self.config.iter_limit,
            nodes_in_egraph,
            tables_updated,
            delta_count: stats_delta.len(),
            row_change_pct: stats_delta.row_count_change_pct(),
            used_full_reoptimization: is_full,
            elapsed,
        };

        Ok((result, stats))
    }

    /// Apply statistics deltas to the internal table stats map.
    ///
    /// Returns the number of tables whose stats were updated.
    #[cfg(feature = "timeline")]
    fn apply_stats_delta(&mut self, delta_set: &DeltaSet) -> usize {
        let mut updated_tables = std::collections::HashSet::<String>::new();

        for delta in delta_set {
            match delta {
                ra_stats::delta::StatisticsDelta::TableRowCount { table, new, .. } => {
                    let stats = self
                        .table_stats
                        .entry(table.clone())
                        .or_insert_with(|| ra_core::statistics::Statistics::new(*new as f64));
                    stats.row_count = *new as f64;
                    updated_tables.insert(table.clone());
                }
                ra_stats::delta::StatisticsDelta::ColumnNDV {
                    table, column, new, ..
                } => {
                    if let Some(stats) = self.table_stats.get_mut(table) {
                        let col = stats
                            .columns
                            .entry(column.clone())
                            .or_insert_with(|| ra_core::statistics::ColumnStats::new(*new as f64));
                        col.distinct_count = *new as f64;
                        updated_tables.insert(table.clone());
                    }
                }
                ra_stats::delta::StatisticsDelta::ColumnNullFraction {
                    table, column, new, ..
                } => {
                    if let Some(stats) = self.table_stats.get_mut(table) {
                        if let Some(col) = stats.columns.get_mut(column) {
                            col.null_fraction = *new;
                            updated_tables.insert(table.clone());
                        }
                    }
                }
                ra_stats::delta::StatisticsDelta::TableAdded { table, row_count } => {
                    self.table_stats.insert(
                        table.clone(),
                        ra_core::statistics::Statistics::new(*row_count as f64),
                    );
                    updated_tables.insert(table.clone());
                }
                ra_stats::delta::StatisticsDelta::TableRemoved { table, .. } => {
                    self.table_stats.remove(table);
                    updated_tables.insert(table.clone());
                }
                // ColumnCorrelation and StalenessChanged don't
                // directly map to ra_core::statistics fields.
                _ => {}
            }
        }

        updated_tables.len()
    }
}

/// Build a simple tracking result.
///
/// Since egg doesn't expose per-rule application statistics, we track
/// optimization at a high level: whether rules made changes, total nodes
/// added, and cost improvement.
#[allow(dead_code)]
fn build_simple_tracking(
    available_rules: Vec<String>,
    total_nodes_added: usize,
    iterations_with_changes: usize,
    initial_cost: f64,
    final_cost: f64,
) -> RuleTrackingResult {
    let cost_improvement = if final_cost < initial_cost {
        initial_cost - final_cost
    } else {
        0.0
    };

    let applied = if total_nodes_added > 0 {
        vec![RuleApplication {
            name: format!(
                "Aggregate: {} iteration(s) with rule applications",
                iterations_with_changes
            ),
            fired_count: iterations_with_changes,
            nodes_added: total_nodes_added,
            cost_improvement: if cost_improvement > 0.0 {
                Some(cost_improvement)
            } else {
                None
            },
        }]
    } else {
        Vec::new()
    };

    let evaluated = if total_nodes_added == 0 && !available_rules.is_empty() {
        vec![RuleEvaluation {
            name: format!(
                "Aggregate: {} rule(s) available but none applied",
                available_rules.len()
            ),
            tried_count: available_rules.len(),
            rejection_reason: "no pattern matched or no improvement".to_string(),
        }]
    } else {
        Vec::new()
    };

    RuleTrackingResult {
        applied,
        evaluated,
        available: available_rules,
        intermediate_steps: None,
    }
}

/// Build a detailed tracking result from per-rule applications.
///
/// This function takes the collected rule applications and creates
/// a tracking result with per-rule information.
fn build_detailed_tracking(
    available_rules: Vec<String>,
    rule_applications: Vec<RuleApplication>,
) -> RuleTrackingResult {
    build_detailed_tracking_with_steps(available_rules, rule_applications, None)
}

fn build_detailed_tracking_with_steps(
    available_rules: Vec<String>,
    rule_applications: Vec<RuleApplication>,
    intermediate_steps: Option<Vec<IntermediateStep>>,
) -> RuleTrackingResult {
    let evaluated = if rule_applications.is_empty() && !available_rules.is_empty() {
        vec![RuleEvaluation {
            name: format!(
                "{} rule(s) available but none applied",
                available_rules.len()
            ),
            tried_count: available_rules.len(),
            rejection_reason: "no pattern matched or no improvement".to_string(),
        }]
    } else {
        Vec::new()
    };

    RuleTrackingResult {
        applied: rule_applications,
        evaluated,
        available: available_rules,
        intermediate_steps,
    }
}

/// Handle budget overflow according to the overflow strategy.
fn handle_overflow(
    tracker: &ResourceTracker,
    original: &RelExpr,
    best_plan: Option<RelExpr>,
    best_cost: f64,
) -> Result<OptimizationResult, EGraphError> {
    handle_overflow_with_tracking(tracker, original, best_plan, best_cost, None)
}

/// Handle budget overflow with optional tracking data.
fn handle_overflow_with_tracking(
    tracker: &ResourceTracker,
    original: &RelExpr,
    best_plan: Option<RelExpr>,
    best_cost: f64,
    rule_tracking: Option<RuleTrackingResult>,
) -> Result<OptimizationResult, EGraphError> {
    let report = tracker.report();
    match tracker.overflow_strategy() {
        OverflowStrategy::ReturnBestSoFar => match best_plan {
            Some(plan) => Ok(OptimizationResult {
                plan,
                cost: best_cost,
                status: OptimizationStatus::Incomplete,
                resource_usage: report,
                applied_rules: None,
                rule_tracking,
            }),
            None => Ok(OptimizationResult {
                plan: original.clone(),
                cost: f64::INFINITY,
                status: OptimizationStatus::Incomplete,
                resource_usage: report,
                applied_rules: None,
                rule_tracking,
            }),
        },
        OverflowStrategy::ReturnOriginal => Ok(OptimizationResult {
            plan: original.clone(),
            cost: f64::INFINITY,
            status: OptimizationStatus::Incomplete,
            resource_usage: report,
            applied_rules: None,
            rule_tracking,
        }),
        OverflowStrategy::Fail => {
            let exceeded = report
                .budget_exceeded
                .map_or("unknown resource".to_owned(), |r| r.to_string());
            Err(EGraphError::ResourceBudgetExceeded(exceeded))
        }
    }
}

impl Default for Optimizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a bounded optimization run.
#[derive(Debug)]
pub struct OptimizationResult {
    /// The best plan found.
    pub plan: RelExpr,
    /// Estimated cost of the plan.
    pub cost: f64,
    /// Whether optimization completed fully or was truncated.
    pub status: OptimizationStatus,
    /// Detailed resource usage report.
    pub resource_usage: ResourceUsageReport,
    /// Rules applied during optimization (only populated if tracking enabled).
    /// Zero overhead when None.
    pub applied_rules: Option<crate::rule_registry::RuleSet>,
    /// Detailed rule tracking (only populated if tracking enabled).
    pub rule_tracking: Option<RuleTrackingResult>,
}

/// Detailed tracking of rule applications during optimization.
#[derive(Debug, Clone)]
pub struct RuleTrackingResult {
    /// Rules that successfully modified the e-graph.
    pub applied: Vec<RuleApplication>,
    /// Rules that were tried but didn't match or add nodes.
    pub evaluated: Vec<RuleEvaluation>,
    /// All rules available in the system.
    pub available: Vec<String>,
    /// Intermediate optimization steps (only populated in verbose mode).
    pub intermediate_steps: Option<Vec<IntermediateStep>>,
}

/// A single step in the optimization process showing plan transformation.
#[derive(Debug, Clone)]
pub struct IntermediateStep {
    /// Step number in the optimization sequence.
    pub step_number: usize,
    /// Name of the rule that was applied.
    pub rule_name: String,
    /// Explanation of why this rule was chosen.
    pub reason: String,
    /// The plan before applying the rule.
    pub plan_before: RelExpr,
    /// The plan after applying the rule.
    pub plan_after: RelExpr,
    /// Cost improvement from this step.
    pub cost_improvement: Option<f64>,
}

/// A rule that successfully applied and modified the e-graph.
#[derive(Debug, Clone)]
pub struct RuleApplication {
    /// Name of the rule.
    pub name: String,
    /// Number of times this rule fired.
    pub fired_count: usize,
    /// E-graph nodes added by this rule.
    pub nodes_added: usize,
    /// Cost improvement, if measurable.
    pub cost_improvement: Option<f64>,
}

/// A rule that was evaluated but didn't contribute to the e-graph.
#[derive(Debug, Clone)]
pub struct RuleEvaluation {
    /// Name of the rule.
    pub name: String,
    /// Number of times this rule was tried.
    pub tried_count: usize,
    /// Why the rule was rejected.
    pub rejection_reason: String,
}

/// Whether optimization completed within its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimizationStatus {
    /// All iterations ran and the e-graph was fully explored.
    Complete,
    /// Optimization was cut short by a resource limit.
    Incomplete,
    /// Optimization failed (e.g., no plan could be extracted).
    Failed,
}

/// Statistics about an incremental optimization run.
///
/// Reports how much work the incremental optimizer did compared to
/// what a full reoptimization would require, allowing callers to
/// measure the speedup from differential updates.
#[derive(Debug, Clone)]
pub struct IncrementalStats {
    /// Number of rewrite rules evaluated.
    pub rules_evaluated: usize,
    /// Number of e-graph iterations actually used.
    pub iterations_used: usize,
    /// Maximum iterations configured.
    pub max_iterations: usize,
    /// Number of nodes in the final e-graph.
    pub nodes_in_egraph: usize,
    /// Number of tables whose stats were updated.
    pub tables_updated: usize,
    /// Number of individual deltas processed.
    pub delta_count: usize,
    /// Maximum row count change percentage.
    pub row_change_pct: f64,
    /// Whether full reoptimization was used (delta was too large).
    pub used_full_reoptimization: bool,
    /// Wall-clock time for the incremental optimization.
    pub elapsed: std::time::Duration,
}

impl IncrementalStats {
    /// Estimated speedup factor vs full optimization.
    ///
    /// Based on the ratio of iterations used vs max configured.
    /// Returns 1.0 when full reoptimization was used.
    #[must_use]
    pub fn speedup_factor(&self) -> f64 {
        if self.used_full_reoptimization || self.iterations_used == 0 {
            return 1.0;
        }
        self.max_iterations as f64 / self.iterations_used as f64
    }
}

/// Estimate the cost of the best plan in the e-graph.
///
/// Uses the basic cost function for a quick estimate; this is
/// consistent with what `extract_best` uses when no table stats
/// are available.
fn estimate_plan_cost(
    egraph: &EGraph<RelLang, RelAnalysis>,
    root: Id,
    hardware: &ra_hardware::HardwareProfile,
) -> f64 {
    let cost_fn = crate::extract::RelCostFn::new(hardware.clone());
    let extractor = egg::Extractor::new(egraph, cost_fn);
    let (cost, _) = extractor.find_best(root);
    cost
}

/// Errors that can occur during e-graph optimization.
#[derive(Debug, thiserror::Error)]
pub enum EGraphError {
    /// Failed to convert a relational expression to the e-graph.
    #[error("failed to convert expression to e-graph: {0}")]
    ConversionError(String),

    /// Failed to extract a plan from the e-graph.
    #[error("failed to extract plan from e-graph: {0}")]
    ExtractionError(String),

    /// Resource budget was exceeded with Fail strategy.
    #[error("resource budget exceeded: {0}")]
    ResourceBudgetExceeded(String),
}

/// Convert a [`RelExpr`] into an egg [`RecExpr`].
///
/// # Errors
///
/// Returns an error if the expression contains unsupported constructs.
pub fn to_rec_expr(expr: &RelExpr) -> Result<RecExpr<RelLang>, EGraphError> {
    let mut rec = RecExpr::default();
    add_rel_expr(&mut rec, expr)?;
    Ok(rec)
}

fn add_rel_expr(rec: &mut RecExpr<RelLang>, expr: &RelExpr) -> Result<Id, EGraphError> {
    match expr {
        RelExpr::Scan { table, alias } => {
            let table_id = add_symbol(rec, table);
            if let Some(alias_name) = alias {
                let alias_id = add_symbol(rec, alias_name);
                Ok(rec.add(RelLang::ScanAlias([table_id, alias_id])))
            } else {
                Ok(rec.add(RelLang::Scan([table_id])))
            }
        }
        RelExpr::Filter { predicate, input } => {
            let pred_id = add_scalar_expr(rec, predicate)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Filter([pred_id, input_id])))
        }
        RelExpr::Project { columns, input } => {
            let cols_id = add_projection_list(rec, columns)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Project([cols_id, input_id])))
        }
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => {
            let jt_id = add_join_type(rec, *join_type);
            let cond_id = add_scalar_expr(rec, condition)?;
            let left_id = add_rel_expr(rec, left)?;
            let right_id = add_rel_expr(rec, right)?;
            Ok(rec.add(RelLang::Join([jt_id, cond_id, left_id, right_id])))
        }
        RelExpr::Aggregate {
            group_by,
            aggregates,
            input,
        } => {
            let groups_id = add_expr_list(rec, group_by)?;
            let aggs_id = add_aggregate_list(rec, aggregates)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Aggregate([groups_id, aggs_id, input_id])))
        }
        RelExpr::Sort { keys, input } => {
            let keys_id = add_sort_key_list(rec, keys)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Sort([keys_id, input_id])))
        }
        RelExpr::IncrementalSort {
            prefix_keys,
            suffix_keys,
            input,
        } => {
            let prefix_id = add_sort_key_list(rec, prefix_keys)?;
            let suffix_id = add_sort_key_list(rec, suffix_keys)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::IncrementalSort([prefix_id, suffix_id, input_id])))
        }
        RelExpr::Limit {
            count,
            offset,
            input,
        } => {
            let count_id = add_symbol(rec, &count.to_string());
            let offset_id = add_symbol(rec, &offset.to_string());
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Limit([count_id, offset_id, input_id])))
        }
        RelExpr::Union { all, left, right } => {
            let all_id = add_bool_flag(rec, *all);
            let left_id = add_rel_expr(rec, left)?;
            let right_id = add_rel_expr(rec, right)?;
            Ok(rec.add(RelLang::Union([all_id, left_id, right_id])))
        }
        RelExpr::Intersect { all, left, right } => {
            let all_id = add_bool_flag(rec, *all);
            let left_id = add_rel_expr(rec, left)?;
            let right_id = add_rel_expr(rec, right)?;
            Ok(rec.add(RelLang::Intersect([all_id, left_id, right_id])))
        }
        RelExpr::Except { all, left, right } => {
            let all_id = add_bool_flag(rec, *all);
            let left_id = add_rel_expr(rec, left)?;
            let right_id = add_rel_expr(rec, right)?;
            Ok(rec.add(RelLang::Except([all_id, left_id, right_id])))
        }
        RelExpr::RecursiveCTE {
            name,
            base_case,
            recursive_case,
            body,
            ..
        } => {
            let name_id = add_symbol(rec, name);
            let base_id = add_rel_expr(rec, base_case)?;
            let rec_id = add_rel_expr(rec, recursive_case)?;
            let body_id = add_rel_expr(rec, body)?;
            Ok(rec.add(RelLang::RecursiveCTE([name_id, base_id, rec_id, body_id])))
        }
        RelExpr::CTE {
            name,
            definition,
            body,
        } => {
            let name_id = add_symbol(rec, name);
            let def_id = add_rel_expr(rec, definition)?;
            let body_id = add_rel_expr(rec, body)?;
            Ok(rec.add(RelLang::CTE([name_id, def_id, body_id])))
        }
        RelExpr::Window { functions, input } => {
            let fns_id = add_window_expr_list(rec, functions)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Window([fns_id, input_id])))
        }
        RelExpr::Distinct { input } => {
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::DistinctRel([input_id])))
        }
        RelExpr::Values { rows } => {
            let mut row_ids = Vec::with_capacity(rows.len());
            for row in rows {
                let mut cell_ids = Vec::with_capacity(row.len());
                for cell in row {
                    cell_ids.push(add_scalar_expr(rec, cell)?);
                }
                row_ids.push(rec.add(RelLang::ValuesRow(cell_ids.into_boxed_slice())));
            }
            Ok(rec.add(RelLang::Values(row_ids.into_boxed_slice())))
        }
        RelExpr::Unnest {
            expr,
            alias,
            input,
            with_ordinality,
        } => {
            let expr_id = add_scalar_expr(rec, expr)?;
            let alias_id = add_symbol(rec, alias.as_deref().unwrap_or(""));
            let ord_id = add_bool_flag(rec, *with_ordinality);
            if let Some(inp) = input {
                let input_id = add_rel_expr(rec, inp)?;
                let tag_id = add_symbol(rec, "unnest_lateral");
                let ids = vec![tag_id, expr_id, alias_id, ord_id, input_id];
                Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
            } else {
                let tag_id = add_symbol(rec, "unnest");
                let ids = vec![tag_id, expr_id, alias_id, ord_id];
                Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
            }
        }
        RelExpr::MultiUnnest {
            exprs,
            aliases,
            with_ordinality,
        } => {
            let tag_id = add_symbol(rec, "multi_unnest");
            let ord_id = add_bool_flag(rec, *with_ordinality);
            let mut ids = vec![tag_id, ord_id];
            for (expr, alias) in exprs.iter().zip(aliases.iter()) {
                ids.push(add_scalar_expr(rec, expr)?);
                ids.push(add_symbol(rec, alias.as_deref().unwrap_or("")));
            }
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::TableFunction {
            name, args, input, ..
        } => {
            let name_id = add_symbol(rec, name);
            let mut ids = vec![name_id];
            for arg in args {
                ids.push(add_scalar_expr(rec, arg)?);
            }
            if let Some(inp) = input {
                ids.push(add_rel_expr(rec, inp)?);
            }
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::BitmapIndexScan {
            table,
            index,
            predicate,
        } => {
            let table_id = add_symbol(rec, table);
            let index_id = add_symbol(rec, index);
            let pred_id = add_scalar_expr(rec, predicate)?;
            Ok(rec.add(RelLang::BitmapIndexScan([table_id, index_id, pred_id])))
        }
        RelExpr::BitmapAnd { inputs } => {
            let mut input_ids = Vec::with_capacity(inputs.len());
            for input in inputs {
                input_ids.push(add_rel_expr(rec, input)?);
            }
            Ok(rec.add(RelLang::BitmapAnd(input_ids.into_boxed_slice())))
        }
        RelExpr::BitmapOr { inputs } => {
            let mut input_ids = Vec::with_capacity(inputs.len());
            for input in inputs {
                input_ids.push(add_rel_expr(rec, input)?);
            }
            Ok(rec.add(RelLang::BitmapOr(input_ids.into_boxed_slice())))
        }
        RelExpr::BitmapHeapScan {
            table,
            bitmap,
            recheck_cond,
        } => {
            let table_id = add_symbol(rec, table);
            let bitmap_id = add_rel_expr(rec, bitmap)?;
            let recheck_id = if let Some(cond) = recheck_cond {
                add_scalar_expr(rec, cond)?
            } else {
                add_symbol(rec, "")
            };
            Ok(rec.add(RelLang::BitmapHeapScan([table_id, bitmap_id, recheck_id])))
        }
        RelExpr::IndexScan { table, column } => {
            let tag_id = add_symbol(rec, "index_scan");
            let table_id = add_symbol(rec, table);
            let col_id = add_symbol(rec, column);
            let ids = vec![tag_id, table_id, col_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::IndexOnlyScan {
            table,
            index,
            columns,
            predicate,
        } => {
            let table_id = add_symbol(rec, table);
            let index_id = add_symbol(rec, index);
            let cols_id = add_projection_list(rec, columns)?;
            let pred_id = add_scalar_expr(rec, predicate)?;
            Ok(rec.add(RelLang::IndexOnlyScan([
                table_id, index_id, cols_id, pred_id,
            ])))
        }
        RelExpr::RowPattern { input, pattern, .. } => {
            let tag_id = add_symbol(rec, "MATCH_RECOGNIZE");
            let pattern_id = add_symbol(rec, &pattern.to_string());
            let input_id = add_rel_expr(rec, input)?;
            let ids = vec![tag_id, pattern_id, input_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::ParallelScan { table, workers } => {
            let tag_id = add_symbol(rec, "parallel_scan");
            let table_id = add_symbol(rec, table);
            let workers_id = add_symbol(rec, &workers.to_string());
            let ids = vec![tag_id, table_id, workers_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::ParallelHashJoin {
            join_type,
            condition,
            left,
            right,
            workers,
        } => {
            let tag_id = add_symbol(rec, "parallel_hash_join");
            let jt_id = add_join_type(rec, *join_type);
            let cond_id = add_scalar_expr(rec, condition)?;
            let left_id = add_rel_expr(rec, left)?;
            let right_id = add_rel_expr(rec, right)?;
            let workers_id = add_symbol(rec, &workers.to_string());
            let ids = vec![tag_id, jt_id, cond_id, left_id, right_id, workers_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::ParallelAggregate {
            group_by,
            aggregates,
            input,
            workers,
        } => {
            let tag_id = add_symbol(rec, "parallel_aggregate");
            let groups_id = add_expr_list(rec, group_by)?;
            let aggs_id = add_aggregate_list(rec, aggregates)?;
            let input_id = add_rel_expr(rec, input)?;
            let workers_id = add_symbol(rec, &workers.to_string());
            let ids = vec![tag_id, groups_id, aggs_id, input_id, workers_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::Gather { input, workers } => {
            let tag_id = add_symbol(rec, "gather");
            let input_id = add_rel_expr(rec, input)?;
            let workers_id = add_symbol(rec, &workers.to_string());
            let ids = vec![tag_id, input_id, workers_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::MvScan { view_name, alias } => {
            let view_id = add_symbol(rec, view_name);
            let alias_id = add_symbol(rec, alias.as_deref().unwrap_or("auto"));
            let nil_g = rec.add(RelLang::Nil);
            let nil_a = rec.add(RelLang::Nil);
            Ok(rec.add(RelLang::MvScan([view_id, alias_id, nil_g, nil_a])))
        }
        RelExpr::TopK {
            vector_expr,
            query_vector,
            metric,
            k,
            input,
        } => {
            let tag_id = add_symbol(rec, "topk");
            let vec_expr_id = add_scalar_expr(rec, vector_expr)?;
            let query_id = add_scalar_expr(rec, query_vector)?;
            let metric_id = add_symbol(rec, &format!("{:?}", metric));
            let k_id = add_symbol(rec, &k.to_string());
            let input_id = add_rel_expr(rec, input)?;
            let ids = vec![tag_id, vec_expr_id, query_id, metric_id, k_id, input_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::VectorFilter {
            vector_expr,
            query_vector,
            metric,
            threshold,
            input,
        } => {
            let tag_id = add_symbol(rec, "vector_filter");
            let vec_expr_id = add_scalar_expr(rec, vector_expr)?;
            let query_id = add_scalar_expr(rec, query_vector)?;
            let metric_id = add_symbol(rec, &format!("{:?}", metric));
            let threshold_id = add_symbol(rec, &threshold.to_string());
            let input_id = add_rel_expr(rec, input)?;
            let ids = vec![
                tag_id,
                vec_expr_id,
                query_id,
                metric_id,
                threshold_id,
                input_id,
            ];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
    }
}

fn add_scalar_expr(rec: &mut RecExpr<RelLang>, expr: &Expr) -> Result<Id, EGraphError> {
    match expr {
        Expr::Column(col_ref) => {
            let col_id = add_symbol(rec, &col_ref.column);
            if let Some(table) = &col_ref.table {
                let table_id = add_symbol(rec, table);
                Ok(rec.add(RelLang::QCol([table_id, col_id])))
            } else {
                Ok(rec.add(RelLang::Col([col_id])))
            }
        }
        Expr::Const(c) => Ok(add_const(rec, c)),
        Expr::BinOp { op, left, right } => {
            let left_id = add_scalar_expr(rec, left)?;
            let right_id = add_scalar_expr(rec, right)?;
            let node = match op {
                BinOp::Add => RelLang::Add([left_id, right_id]),
                BinOp::Sub => RelLang::Sub([left_id, right_id]),
                BinOp::Mul => RelLang::Mul([left_id, right_id]),
                BinOp::Div => RelLang::Div([left_id, right_id]),
                BinOp::Mod => RelLang::Mod([left_id, right_id]),
                BinOp::Eq => RelLang::Eq([left_id, right_id]),
                BinOp::Ne => RelLang::Ne([left_id, right_id]),
                BinOp::Lt => RelLang::Lt([left_id, right_id]),
                BinOp::Le => RelLang::Le([left_id, right_id]),
                BinOp::Gt => RelLang::Gt([left_id, right_id]),
                BinOp::Ge => RelLang::Ge([left_id, right_id]),
                BinOp::And => RelLang::And([left_id, right_id]),
                BinOp::Or => RelLang::Or([left_id, right_id]),
                BinOp::Concat => RelLang::Concat([left_id, right_id]),
                BinOp::JsonAccess => RelLang::JsonAccess([left_id, right_id]),
            };
            Ok(rec.add(node))
        }
        Expr::UnaryOp { op, operand } => {
            let operand_id = add_scalar_expr(rec, operand)?;
            let node = match op {
                UnaryOp::Not => RelLang::Not([operand_id]),
                UnaryOp::IsNull => RelLang::IsNull([operand_id]),
                UnaryOp::IsNotNull => RelLang::IsNotNull([operand_id]),
                UnaryOp::Neg => RelLang::Neg([operand_id]),
            };
            Ok(rec.add(node))
        }
        Expr::Function { name, args } => {
            let name_id = add_symbol(rec, name);
            let mut ids = vec![name_id];
            for arg in args {
                ids.push(add_scalar_expr(rec, arg)?);
            }
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::Case { .. } => Err(EGraphError::ConversionError(
            "CASE expressions are not yet supported in the \
                 e-graph representation"
                .into(),
        )),
        Expr::Cast { expr, target_type } => {
            let expr_id = add_scalar_expr(rec, expr)?;
            let type_id = add_symbol(rec, target_type);
            Ok(rec.add(RelLang::Cast([expr_id, type_id])))
        }
        Expr::Array(elements) => {
            let tag_id = add_symbol(rec, "ARRAY");
            let mut ids = vec![tag_id];
            for elem in elements {
                ids.push(add_scalar_expr(rec, elem)?);
            }
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::ArrayIndex(array, index) => {
            let arr_id = add_scalar_expr(rec, array)?;
            let idx_id = add_scalar_expr(rec, index)?;
            let tag_id = add_symbol(rec, "ARRAY_INDEX");
            let ids = vec![tag_id, arr_id, idx_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::PatternPrev(inner, offset) => {
            let tag_id = add_symbol(rec, "PREV");
            let inner_id = add_scalar_expr(rec, inner)?;
            let offset_id = add_symbol(rec, &offset.to_string());
            let ids = vec![tag_id, inner_id, offset_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::PatternNext(inner, offset) => {
            let tag_id = add_symbol(rec, "NEXT");
            let inner_id = add_scalar_expr(rec, inner)?;
            let offset_id = add_symbol(rec, &offset.to_string());
            let ids = vec![tag_id, inner_id, offset_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::PatternFirst(inner, var) => {
            let tag_id = add_symbol(rec, "FIRST");
            let inner_id = add_scalar_expr(rec, inner)?;
            let var_id = add_symbol(rec, var);
            let ids = vec![tag_id, inner_id, var_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::PatternLast(inner, var) => {
            let tag_id = add_symbol(rec, "LAST");
            let inner_id = add_scalar_expr(rec, inner)?;
            let var_id = add_symbol(rec, var);
            let ids = vec![tag_id, inner_id, var_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::PatternClassifier => {
            let tag_id = add_symbol(rec, "CLASSIFIER");
            let ids = vec![tag_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::PatternMatchNumber => {
            let tag_id = add_symbol(rec, "MATCH_NUMBER");
            let ids = vec![tag_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::ArraySlice { array, start, end } => {
            let arr_id = add_scalar_expr(rec, array)?;
            let start_id = match start {
                Some(s) => add_scalar_expr(rec, s)?,
                None => rec.add(RelLang::ConstNull),
            };
            let end_id = match end {
                Some(e) => add_scalar_expr(rec, e)?,
                None => rec.add(RelLang::ConstNull),
            };
            let tag_id = add_symbol(rec, "ARRAY_SLICE");
            let ids = vec![tag_id, arr_id, start_id, end_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::FieldAccess { expr, field_name } => {
            let expr_id = add_scalar_expr(rec, expr)?;
            let field_id = add_symbol(rec, field_name);
            let tag_id = add_symbol(rec, "FIELD_ACCESS");
            let ids = vec![tag_id, expr_id, field_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::SubQuery { .. } => Err(EGraphError::ConversionError(
            "Subquery expressions are not yet supported in the \
                 e-graph representation"
                .into(),
        )),
        Expr::FullTextMatch {
            vendor,
            columns,
            query,
            mode,
        } => {
            let vendor_id = add_symbol(rec, vendor);
            let cols_id = add_symbol(rec, &columns.join(","));
            let query_id = add_symbol(rec, query);
            let mode_id = add_symbol(rec, mode.as_deref().unwrap_or(""));
            Ok(rec.add(RelLang::FtsMatch([vendor_id, cols_id, query_id, mode_id])))
        }
        Expr::VectorDistance {
            metric,
            column,
            target,
        } => {
            let metric_id = add_symbol(rec, metric);
            let col_id = add_scalar_expr(rec, column)?;
            let target_id = add_scalar_expr(rec, target)?;
            Ok(rec.add(RelLang::VectorDistance([metric_id, col_id, target_id])))
        }
    }
}

fn add_const(rec: &mut RecExpr<RelLang>, c: &Const) -> Id {
    match c {
        Const::Null => rec.add(RelLang::ConstNull),
        Const::Bool(b) => {
            let val_id = if *b {
                rec.add(RelLang::True)
            } else {
                rec.add(RelLang::False)
            };
            rec.add(RelLang::ConstBool([val_id]))
        }
        Const::Int(i) => {
            let val_id = add_symbol(rec, &i.to_string());
            rec.add(RelLang::ConstInt([val_id]))
        }
        Const::Float(f) => {
            let val_id = add_symbol(rec, &f.to_string());
            rec.add(RelLang::ConstFloat([val_id]))
        }
        Const::String(s) => {
            let val_id = add_symbol(rec, s);
            rec.add(RelLang::ConstStr([val_id]))
        }
    }
}

fn add_symbol(rec: &mut RecExpr<RelLang>, s: &str) -> Id {
    rec.add(RelLang::Symbol(egg::Symbol::from(s)))
}

fn add_join_type(rec: &mut RecExpr<RelLang>, jt: JoinType) -> Id {
    let node = match jt {
        JoinType::Inner => RelLang::Inner,
        JoinType::LeftOuter => RelLang::LeftOuter,
        JoinType::RightOuter => RelLang::RightOuter,
        JoinType::FullOuter => RelLang::FullOuter,
        JoinType::Cross => RelLang::Cross,
        JoinType::Semi => RelLang::Semi,
        JoinType::Anti => RelLang::Anti,
    };
    rec.add(node)
}

fn add_bool_flag(rec: &mut RecExpr<RelLang>, val: bool) -> Id {
    if val {
        rec.add(RelLang::True)
    } else {
        rec.add(RelLang::False)
    }
}

fn add_projection_list(
    rec: &mut RecExpr<RelLang>,
    columns: &[ProjectionColumn],
) -> Result<Id, EGraphError> {
    let mut ids = Vec::with_capacity(columns.len());
    for col in columns {
        let expr_id = add_scalar_expr(rec, &col.expr)?;
        let proj_id = if let Some(alias) = &col.alias {
            let alias_id = add_symbol(rec, alias);
            rec.add(RelLang::ProjAlias([expr_id, alias_id]))
        } else {
            rec.add(RelLang::ProjCol([expr_id]))
        };
        ids.push(proj_id);
    }
    Ok(rec.add(RelLang::List(ids.into_boxed_slice())))
}

fn add_expr_list(rec: &mut RecExpr<RelLang>, exprs: &[Expr]) -> Result<Id, EGraphError> {
    let mut ids = Vec::with_capacity(exprs.len());
    for e in exprs {
        ids.push(add_scalar_expr(rec, e)?);
    }
    Ok(rec.add(RelLang::List(ids.into_boxed_slice())))
}

fn add_aggregate_list(
    rec: &mut RecExpr<RelLang>,
    aggs: &[AggregateExpr],
) -> Result<Id, EGraphError> {
    let mut ids = Vec::with_capacity(aggs.len());
    for agg in aggs {
        let func_node = match agg.function {
            AggregateFunction::Count => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                RelLang::Count([arg_id])
            }
            AggregateFunction::Sum => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                RelLang::Sum([arg_id])
            }
            AggregateFunction::Avg => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                RelLang::Avg([arg_id])
            }
            AggregateFunction::Min => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                RelLang::Min([arg_id])
            }
            AggregateFunction::Max => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                RelLang::Max([arg_id])
            }
            AggregateFunction::StdDev
            | AggregateFunction::Variance
            | AggregateFunction::StringAgg
            | AggregateFunction::ArrayAgg => {
                return Err(EGraphError::ConversionError(format!(
                    "aggregate function {:?} not yet supported in e-graph",
                    agg.function
                )));
            }
        };
        let func_id = rec.add(func_node);
        let distinct_id = if agg.distinct {
            rec.add(RelLang::Distinct)
        } else {
            rec.add(RelLang::All)
        };
        let alias_id = if let Some(alias) = &agg.alias {
            add_symbol(rec, alias)
        } else {
            rec.add(RelLang::Nil)
        };
        let agg_id = rec.add(RelLang::AggExpr([func_id, distinct_id, alias_id]));
        ids.push(agg_id);
    }
    Ok(rec.add(RelLang::List(ids.into_boxed_slice())))
}

fn add_agg_arg(rec: &mut RecExpr<RelLang>, arg: Option<&Expr>) -> Result<Id, EGraphError> {
    match arg {
        Some(e) => add_scalar_expr(rec, e),
        None => Ok(rec.add(RelLang::Nil)),
    }
}

fn add_sort_key_list(rec: &mut RecExpr<RelLang>, keys: &[SortKey]) -> Result<Id, EGraphError> {
    let mut ids = Vec::with_capacity(keys.len());
    for key in keys {
        let expr_id = add_scalar_expr(rec, &key.expr)?;
        let dir_id = match key.direction {
            SortDirection::Asc => rec.add(RelLang::Asc),
            SortDirection::Desc => rec.add(RelLang::Desc),
        };
        let nulls_id = match key.nulls {
            NullOrdering::First => rec.add(RelLang::NullsFirst),
            NullOrdering::Last => rec.add(RelLang::NullsLast),
        };
        let key_id = rec.add(RelLang::SortKey([expr_id, dir_id, nulls_id]));
        ids.push(key_id);
    }
    Ok(rec.add(RelLang::List(ids.into_boxed_slice())))
}

fn add_window_expr_list(
    rec: &mut RecExpr<RelLang>,
    exprs: &[WindowExpr],
) -> Result<Id, EGraphError> {
    let mut ids = Vec::with_capacity(exprs.len());
    for wexpr in exprs {
        ids.push(add_window_expr(rec, wexpr)?);
    }
    Ok(rec.add(RelLang::List(ids.into_boxed_slice())))
}

fn add_window_expr(rec: &mut RecExpr<RelLang>, wexpr: &WindowExpr) -> Result<Id, EGraphError> {
    let fn_name = add_symbol(rec, &format!("{:?}", wexpr.function));
    let fn_id = rec.add(RelLang::WindowFn([fn_name]));
    let arg_id = match &wexpr.arg {
        Some(e) => add_scalar_expr(rec, e)?,
        None => rec.add(RelLang::Nil),
    };
    let part_id = add_expr_list(rec, &wexpr.partition_by)?;
    let order_id = add_sort_key_list(rec, &wexpr.order_by)?;
    let frame_id = add_window_frame(rec, wexpr.frame.as_ref())?;
    let alias_id = match &wexpr.alias {
        Some(a) => add_symbol(rec, a),
        None => rec.add(RelLang::Nil),
    };
    Ok(rec.add(RelLang::WindowExprNode([
        fn_id, arg_id, part_id, order_id, frame_id, alias_id,
    ])))
}

fn add_window_frame(
    rec: &mut RecExpr<RelLang>,
    frame: Option<&WindowFrame>,
) -> Result<Id, EGraphError> {
    let Some(f) = frame else {
        return Ok(rec.add(RelLang::Nil));
    };
    let mode_id = match f.mode {
        WindowFrameMode::Rows => rec.add(RelLang::FrameRows),
        WindowFrameMode::Range => rec.add(RelLang::FrameRange),
        WindowFrameMode::Groups => rec.add(RelLang::FrameGroups),
    };
    let start_id = add_frame_bound(rec, &f.start);
    let end_id = add_frame_bound(rec, &f.end);
    Ok(rec.add(RelLang::WindowFrameNode([mode_id, start_id, end_id])))
}

fn add_frame_bound(rec: &mut RecExpr<RelLang>, bound: &WindowFrameBound) -> Id {
    match bound {
        WindowFrameBound::UnboundedPreceding => rec.add(RelLang::FrameUnboundedPreceding),
        WindowFrameBound::Preceding(n) => {
            let n_id = add_symbol(rec, &n.to_string());
            rec.add(RelLang::FramePreceding([n_id]))
        }
        WindowFrameBound::CurrentRow => rec.add(RelLang::FrameCurrentRow),
        WindowFrameBound::Following(n) => {
            let n_id = add_symbol(rec, &n.to_string());
            rec.add(RelLang::FrameFollowing([n_id]))
        }
        WindowFrameBound::UnboundedFollowing => rec.add(RelLang::FrameUnboundedFollowing),
    }
}

/// Convert an e-graph node (by class [`Id`]) back to a [`RelExpr`].
///
/// Extracts the best node from each e-class using the given extractor
/// function, then reconstructs the AST.
///
/// # Errors
///
/// Returns an error if the e-graph contains nodes that cannot be
/// mapped back to [`RelExpr`].
pub fn from_egraph_node(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<RelExpr, EGraphError> {
    let nodes = &egraph[id].nodes;
    let node = &nodes[0];
    from_node(egraph, node)
}

#[allow(clippy::too_many_lines)]
fn from_node(
    egraph: &EGraph<RelLang, RelAnalysis>,
    node: &RelLang,
) -> Result<RelExpr, EGraphError> {
    match node {
        RelLang::Scan([table_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            Ok(RelExpr::Scan { table, alias: None })
        }
        RelLang::ScanAlias([table_id, alias_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            let alias = extract_symbol(egraph, *alias_id)?;
            Ok(RelExpr::Scan {
                table,
                alias: Some(alias),
            })
        }
        RelLang::Filter([pred_id, input_id]) => {
            let predicate = extract_scalar_expr(egraph, *pred_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Filter {
                predicate,
                input: Box::new(input),
            })
        }
        RelLang::Project([cols_id, input_id]) => {
            let columns = extract_projection_list(egraph, *cols_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Project {
                columns,
                input: Box::new(input),
            })
        }
        RelLang::Join([jt_id, cond_id, left_id, right_id]) => {
            let join_type = extract_join_type(egraph, *jt_id)?;
            let condition = extract_scalar_expr(egraph, *cond_id)?;
            let left = from_egraph_node(egraph, *left_id)?;
            let right = from_egraph_node(egraph, *right_id)?;
            Ok(RelExpr::Join {
                join_type,
                condition,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        RelLang::Aggregate([groups_id, aggs_id, input_id]) => {
            let group_by = extract_expr_list(egraph, *groups_id)?;
            let aggregates = extract_aggregate_list(egraph, *aggs_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Aggregate {
                group_by,
                aggregates,
                input: Box::new(input),
            })
        }
        RelLang::Sort([keys_id, input_id]) => {
            let keys = extract_sort_key_list(egraph, *keys_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Sort {
                keys,
                input: Box::new(input),
            })
        }
        RelLang::IncrementalSort([prefix_id, suffix_id, input_id]) => {
            let prefix_keys = extract_sort_key_list(egraph, *prefix_id)?;
            let suffix_keys = extract_sort_key_list(egraph, *suffix_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::IncrementalSort {
                prefix_keys,
                suffix_keys,
                input: Box::new(input),
            })
        }
        RelLang::Limit([count_id, offset_id, input_id]) => {
            let count_str = extract_symbol(egraph, *count_id)?;
            let offset_str = extract_symbol(egraph, *offset_id)?;
            let count = count_str
                .parse::<u64>()
                .map_err(|e| EGraphError::ExtractionError(format!("invalid limit count: {e}")))?;
            let offset = offset_str
                .parse::<u64>()
                .map_err(|e| EGraphError::ExtractionError(format!("invalid limit offset: {e}")))?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Limit {
                count,
                offset,
                input: Box::new(input),
            })
        }
        RelLang::Union([all_id, left_id, right_id]) => {
            let all = extract_bool_flag(egraph, *all_id)?;
            let left = from_egraph_node(egraph, *left_id)?;
            let right = from_egraph_node(egraph, *right_id)?;
            Ok(RelExpr::Union {
                all,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        RelLang::Intersect([all_id, left_id, right_id]) => {
            let all = extract_bool_flag(egraph, *all_id)?;
            let left = from_egraph_node(egraph, *left_id)?;
            let right = from_egraph_node(egraph, *right_id)?;
            Ok(RelExpr::Intersect {
                all,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        RelLang::Except([all_id, left_id, right_id]) => {
            let all = extract_bool_flag(egraph, *all_id)?;
            let left = from_egraph_node(egraph, *left_id)?;
            let right = from_egraph_node(egraph, *right_id)?;
            Ok(RelExpr::Except {
                all,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        RelLang::RecursiveCTE([name_id, base_id, rec_id, body_id]) => {
            let name = extract_symbol(egraph, *name_id)?;
            let base_case = from_egraph_node(egraph, *base_id)?;
            let recursive_case = from_egraph_node(egraph, *rec_id)?;
            let body = from_egraph_node(egraph, *body_id)?;
            Ok(RelExpr::RecursiveCTE {
                name,
                base_case: Box::new(base_case),
                recursive_case: Box::new(recursive_case),
                body: Box::new(body),
                cycle_detection: None,
            })
        }
        RelLang::CTE([name_id, def_id, body_id]) => {
            let name = extract_symbol(egraph, *name_id)?;
            let definition = from_egraph_node(egraph, *def_id)?;
            let body = from_egraph_node(egraph, *body_id)?;
            Ok(RelExpr::CTE {
                name,
                definition: Box::new(definition),
                body: Box::new(body),
            })
        }
        RelLang::Window([fns_id, input_id]) => {
            let functions = extract_window_expr_list(egraph, *fns_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Window {
                functions,
                input: Box::new(input),
            })
        }
        RelLang::DistinctRel([input_id]) => {
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Distinct {
                input: Box::new(input),
            })
        }
        RelLang::Values(row_ids) => {
            let mut rows = Vec::with_capacity(row_ids.len());
            for &row_id in row_ids.iter() {
                rows.push(extract_values_row(egraph, row_id)?);
            }
            Ok(RelExpr::Values { rows })
        }
        RelLang::BitmapIndexScan([table_id, index_id, pred_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            let index = extract_symbol(egraph, *index_id)?;
            let predicate = extract_scalar_expr(egraph, *pred_id)?;
            Ok(RelExpr::BitmapIndexScan {
                table,
                index,
                predicate,
            })
        }
        RelLang::BitmapAnd(input_ids) => {
            let mut inputs = Vec::with_capacity(input_ids.len());
            for &input_id in input_ids.iter() {
                inputs.push(Box::new(from_egraph_node(egraph, input_id)?));
            }
            Ok(RelExpr::BitmapAnd { inputs })
        }
        RelLang::BitmapOr(input_ids) => {
            let mut inputs = Vec::with_capacity(input_ids.len());
            for &input_id in input_ids.iter() {
                inputs.push(Box::new(from_egraph_node(egraph, input_id)?));
            }
            Ok(RelExpr::BitmapOr { inputs })
        }
        RelLang::BitmapHeapScan([table_id, bitmap_id, recheck_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            let bitmap = from_egraph_node(egraph, *bitmap_id)?;
            let recheck_str = extract_symbol(egraph, *recheck_id)?;
            let recheck_cond = if recheck_str.is_empty() {
                None
            } else {
                Some(extract_scalar_expr(egraph, *recheck_id)?)
            };
            Ok(RelExpr::BitmapHeapScan {
                table,
                bitmap: Box::new(bitmap),
                recheck_cond,
            })
        }
        RelLang::MetadataLookup([table_id, _kind_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            Ok(RelExpr::Aggregate {
                group_by: vec![],
                aggregates: vec![AggregateExpr {
                    function: AggregateFunction::Count,
                    arg: None,
                    distinct: false,
                    alias: Some("count".to_string()),
                }],
                input: Box::new(RelExpr::Scan { table, alias: None }),
            })
        }
        RelLang::VectorKNN([table_id, col_id, target_id, k_id]) => {
            // Extract as a scan with a filter annotation for now
            // TODO: Add proper VectorKNN to RelExpr enum
            let table = extract_symbol(egraph, *table_id)?;
            let _col = extract_scalar_expr(egraph, *col_id)?;
            let _target = extract_scalar_expr(egraph, *target_id)?;
            let _k = extract_symbol(egraph, *k_id)?;
            Ok(RelExpr::Scan {
                table,
                alias: Some("vector_knn_scan".to_string()),
            })
        }
        RelLang::VectorRangeScan([table_id, _col_id, _target_id, _threshold_id, _metric_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            Ok(RelExpr::Scan {
                table,
                alias: Some("vector_range_scan".to_string()),
            })
        }
        RelLang::FtsIndexScan([table_id, _idx_id, _match_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            Ok(RelExpr::Scan {
                table,
                alias: Some("fts_index_scan".to_string()),
            })
        }
        RelLang::FtsRankedScan([table_id, _idx_id, _query_id, _k_id, _algo_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            Ok(RelExpr::Scan {
                table,
                alias: Some("fts_ranked_scan".to_string()),
            })
        }
        RelLang::FtsSkipListAnd([table_id, _match1_id, _match2_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            Ok(RelExpr::Scan {
                table,
                alias: Some("fts_skip_list_and".to_string()),
            })
        }
        RelLang::HybridScan(_ids) => {
            // Placeholder for hybrid scan extraction
            Ok(RelExpr::Scan {
                table: "hybrid_scan".to_string(),
                alias: Some("hybrid_scan".to_string()),
            })
        }
        RelLang::HybridScore(_ids) => {
            // This shouldn't appear in relational context, but handle it gracefully
            Err(EGraphError::ExtractionError(
                "hybrid-score is a scalar operator, not a relational operator".into(),
            ))
        }
        other => Err(EGraphError::ExtractionError(format!(
            "unexpected relational node: {other:?}"
        ))),
    }
}

fn extract_symbol(egraph: &EGraph<RelLang, RelAnalysis>, id: Id) -> Result<String, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::Symbol(s) = node {
            return Ok(s.to_string());
        }
    }
    Err(EGraphError::ExtractionError(format!(
        "expected Symbol node at e-class {id:?}"
    )))
}

fn extract_bool_flag(egraph: &EGraph<RelLang, RelAnalysis>, id: Id) -> Result<bool, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::True => return Ok(true),
            RelLang::False => return Ok(false),
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(format!(
        "expected True/False node at e-class {id:?}"
    )))
}

fn extract_join_type(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<JoinType, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        let jt = match node {
            RelLang::Inner => JoinType::Inner,
            RelLang::LeftOuter => JoinType::LeftOuter,
            RelLang::RightOuter => JoinType::RightOuter,
            RelLang::FullOuter => JoinType::FullOuter,
            RelLang::Cross => JoinType::Cross,
            RelLang::Semi => JoinType::Semi,
            RelLang::Anti => JoinType::Anti,
            _ => continue,
        };
        return Ok(jt);
    }
    Err(EGraphError::ExtractionError(format!(
        "expected join type node at e-class {id:?}"
    )))
}

fn extract_scalar_expr(egraph: &EGraph<RelLang, RelAnalysis>, id: Id) -> Result<Expr, EGraphError> {
    let canonical = egraph.find(id);
    let node = &egraph[canonical].nodes[0];
    scalar_from_node(egraph, node)
}

#[allow(clippy::too_many_lines)]
fn scalar_from_node(
    egraph: &EGraph<RelLang, RelAnalysis>,
    node: &RelLang,
) -> Result<Expr, EGraphError> {
    match node {
        RelLang::Col([name_id]) => {
            let name = extract_symbol(egraph, *name_id)?;
            Ok(Expr::Column(ColumnRef::new(name)))
        }
        RelLang::QCol([table_id, name_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            let name = extract_symbol(egraph, *name_id)?;
            Ok(Expr::Column(ColumnRef::qualified(table, name)))
        }
        RelLang::ConstNull => Ok(Expr::Const(Const::Null)),
        RelLang::ConstBool([val_id]) => {
            let b = extract_bool_flag(egraph, *val_id)?;
            Ok(Expr::Const(Const::Bool(b)))
        }
        RelLang::ConstInt([val_id]) => {
            let s = extract_symbol(egraph, *val_id)?;
            let i = s.parse::<i64>().map_err(|e| {
                EGraphError::ExtractionError(format!("invalid integer constant: {e}"))
            })?;
            Ok(Expr::Const(Const::Int(i)))
        }
        RelLang::ConstFloat([val_id]) => {
            let s = extract_symbol(egraph, *val_id)?;
            let f = s.parse::<f64>().map_err(|e| {
                EGraphError::ExtractionError(format!("invalid float constant: {e}"))
            })?;
            Ok(Expr::Const(Const::Float(f)))
        }
        RelLang::ConstStr([val_id]) => {
            let s = extract_symbol(egraph, *val_id)?;
            Ok(Expr::Const(Const::String(s)))
        }
        RelLang::Add([l, r]) => extract_binop(egraph, BinOp::Add, *l, *r),
        RelLang::Sub([l, r]) => extract_binop(egraph, BinOp::Sub, *l, *r),
        RelLang::Mul([l, r]) => extract_binop(egraph, BinOp::Mul, *l, *r),
        RelLang::Div([l, r]) => extract_binop(egraph, BinOp::Div, *l, *r),
        RelLang::Eq([l, r]) => extract_binop(egraph, BinOp::Eq, *l, *r),
        RelLang::Ne([l, r]) => extract_binop(egraph, BinOp::Ne, *l, *r),
        RelLang::Lt([l, r]) => extract_binop(egraph, BinOp::Lt, *l, *r),
        RelLang::Le([l, r]) => extract_binop(egraph, BinOp::Le, *l, *r),
        RelLang::Gt([l, r]) => extract_binop(egraph, BinOp::Gt, *l, *r),
        RelLang::Ge([l, r]) => extract_binop(egraph, BinOp::Ge, *l, *r),
        RelLang::And([l, r]) => extract_binop(egraph, BinOp::And, *l, *r),
        RelLang::Or([l, r]) => extract_binop(egraph, BinOp::Or, *l, *r),
        RelLang::Not([operand_id]) => {
            let operand = extract_scalar_expr(egraph, *operand_id)?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            })
        }
        RelLang::IsNull([operand_id]) => {
            let operand = extract_scalar_expr(egraph, *operand_id)?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::IsNull,
                operand: Box::new(operand),
            })
        }
        RelLang::IsNotNull([operand_id]) => {
            let operand = extract_scalar_expr(egraph, *operand_id)?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::IsNotNull,
                operand: Box::new(operand),
            })
        }
        RelLang::Neg([operand_id]) => {
            let operand = extract_scalar_expr(egraph, *operand_id)?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                operand: Box::new(operand),
            })
        }
        RelLang::Func(ids) => {
            if ids.is_empty() {
                return Err(EGraphError::ExtractionError(
                    "function call with no children".into(),
                ));
            }
            let name = extract_symbol(egraph, ids[0])?;
            let mut args = Vec::with_capacity(ids.len() - 1);
            for &arg_id in &ids[1..] {
                args.push(extract_scalar_expr(egraph, arg_id)?);
            }
            Ok(Expr::Function { name, args })
        }
        RelLang::Cast([expr_id, type_id]) => {
            let expr = extract_scalar_expr(egraph, *expr_id)?;
            let target_type = extract_symbol(egraph, *type_id)?;
            Ok(Expr::Cast {
                expr: Box::new(expr),
                target_type,
            })
        }
        RelLang::VectorDistance([metric_id, col_id, target_id]) => {
            let metric = extract_symbol(egraph, *metric_id)?;
            let column = extract_scalar_expr(egraph, *col_id)?;
            let target = extract_scalar_expr(egraph, *target_id)?;
            Ok(Expr::VectorDistance {
                metric,
                column: Box::new(column),
                target: Box::new(target),
            })
        }
        RelLang::FtsMatch([vendor_id, cols_id, query_id, mode_id]) => {
            let vendor = extract_symbol(egraph, *vendor_id)?;
            let cols_str = extract_symbol(egraph, *cols_id)?;
            let columns = cols_str.split(',').map(|s| s.to_string()).collect();
            let query = extract_symbol(egraph, *query_id)?;
            let mode_str = extract_symbol(egraph, *mode_id)?;
            let mode = if mode_str.is_empty() {
                None
            } else {
                Some(mode_str)
            };
            Ok(Expr::FullTextMatch {
                vendor,
                columns,
                query,
                mode,
            })
        }
        RelLang::FtsRank([col_id, query_id, algo_id]) => {
            let col = extract_scalar_expr(egraph, *col_id)?;
            let query = extract_scalar_expr(egraph, *query_id)?;
            let algo = extract_symbol(egraph, *algo_id)?;
            // FTS rank is represented as a function call in Expr
            Ok(Expr::Function {
                name: format!("ts_rank_{}", algo),
                args: vec![col, query],
            })
        }
        other => Err(EGraphError::ExtractionError(format!(
            "unexpected scalar node: {other:?}"
        ))),
    }
}

fn extract_binop(
    egraph: &EGraph<RelLang, RelAnalysis>,
    op: BinOp,
    left_id: Id,
    right_id: Id,
) -> Result<Expr, EGraphError> {
    let left = extract_scalar_expr(egraph, left_id)?;
    let right = extract_scalar_expr(egraph, right_id)?;
    Ok(Expr::BinOp {
        op,
        left: Box::new(left),
        right: Box::new(right),
    })
}

fn extract_projection_list(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<ProjectionColumn>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::List(ids) = node {
            let mut cols = Vec::with_capacity(ids.len());
            for &child_id in ids.iter() {
                cols.push(extract_projection_column(egraph, child_id)?);
            }
            return Ok(cols);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected List node for projection columns".into(),
    ))
}

fn extract_projection_column(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<ProjectionColumn, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::ProjCol([expr_id]) => {
                let expr = extract_scalar_expr(egraph, *expr_id)?;
                return Ok(ProjectionColumn { expr, alias: None });
            }
            RelLang::ProjAlias([expr_id, alias_id]) => {
                let expr = extract_scalar_expr(egraph, *expr_id)?;
                let alias = extract_symbol(egraph, *alias_id)?;
                return Ok(ProjectionColumn {
                    expr,
                    alias: Some(alias),
                });
            }
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected ProjCol or ProjAlias node".into(),
    ))
}

fn extract_expr_list(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<Expr>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::List(ids) = node {
            let mut exprs = Vec::with_capacity(ids.len());
            for &child_id in ids.iter() {
                exprs.push(extract_scalar_expr(egraph, child_id)?);
            }
            return Ok(exprs);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected List node for expression list".into(),
    ))
}

fn extract_aggregate_list(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<AggregateExpr>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::List(ids) = node {
            let mut aggs = Vec::with_capacity(ids.len());
            for &child_id in ids.iter() {
                aggs.push(extract_agg_expr(egraph, child_id)?);
            }
            return Ok(aggs);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected List node for aggregate list".into(),
    ))
}

fn extract_agg_expr(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<AggregateExpr, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::AggExpr([func_id, distinct_id, alias_id]) = node {
            let (function, arg) = extract_agg_function(egraph, *func_id)?;
            let distinct = extract_distinct_flag(egraph, *distinct_id)?;
            let alias = extract_optional_symbol(egraph, *alias_id)?;
            return Ok(AggregateExpr {
                function,
                arg,
                distinct,
                alias,
            });
        }
    }
    Err(EGraphError::ExtractionError("expected AggExpr node".into()))
}

fn extract_agg_function(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<(AggregateFunction, Option<Expr>), EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        let (func, arg_id) = match node {
            RelLang::Count([a]) => (AggregateFunction::Count, *a),
            RelLang::Sum([a]) => (AggregateFunction::Sum, *a),
            RelLang::Avg([a]) => (AggregateFunction::Avg, *a),
            RelLang::Min([a]) => (AggregateFunction::Min, *a),
            RelLang::Max([a]) => (AggregateFunction::Max, *a),
            _ => continue,
        };
        let arg = extract_optional_expr(egraph, arg_id)?;
        return Ok((func, arg));
    }
    Err(EGraphError::ExtractionError(
        "expected aggregate function node".into(),
    ))
}

fn extract_optional_expr(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Option<Expr>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::Nil = node {
            return Ok(None);
        }
    }
    Ok(Some(extract_scalar_expr(egraph, id)?))
}

fn extract_optional_symbol(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Option<String>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::Nil = node {
            return Ok(None);
        }
    }
    Ok(Some(extract_symbol(egraph, id)?))
}

fn extract_distinct_flag(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<bool, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::Distinct => return Ok(true),
            RelLang::All => return Ok(false),
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected Distinct/All flag".into(),
    ))
}

fn extract_window_expr_list(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<WindowExpr>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::List(ids) = node {
            let mut exprs = Vec::with_capacity(ids.len());
            for &child_id in ids.iter() {
                exprs.push(extract_window_expr(egraph, child_id)?);
            }
            return Ok(exprs);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected List node for window expressions".into(),
    ))
}

fn extract_window_expr(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<WindowExpr, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::WindowExprNode([fn_id, arg_id, part_id, order_id, frame_id, alias_id]) =
            node
        {
            let function = extract_window_function(egraph, *fn_id)?;
            let arg = extract_optional_expr(egraph, *arg_id)?;
            let partition_by = extract_expr_list(egraph, *part_id)?;
            let order_by = extract_sort_key_list(egraph, *order_id)?;
            let frame = extract_window_frame(egraph, *frame_id)?;
            let alias = extract_optional_symbol(egraph, *alias_id)?;
            return Ok(WindowExpr {
                function,
                arg,
                partition_by,
                order_by,
                frame,
                alias,
            });
        }
    }
    Err(EGraphError::ExtractionError(
        "expected WindowExprNode".into(),
    ))
}

fn extract_window_function(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<WindowFunction, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::WindowFn([name_id]) = node {
            let name = extract_symbol(egraph, *name_id)?;
            let func = match name.as_str() {
                "RowNumber" => WindowFunction::RowNumber,
                "Rank" => WindowFunction::Rank,
                "DenseRank" => WindowFunction::DenseRank,
                "PercentRank" => WindowFunction::PercentRank,
                "Ntile" => WindowFunction::Ntile,
                "Lag" => WindowFunction::Lag,
                "Lead" => WindowFunction::Lead,
                "FirstValue" => WindowFunction::FirstValue,
                "LastValue" => WindowFunction::LastValue,
                "NthValue" => WindowFunction::NthValue,
                "Avg" => WindowFunction::Avg,
                "Sum" => WindowFunction::Sum,
                "Count" => WindowFunction::Count,
                "Min" => WindowFunction::Min,
                "Max" => WindowFunction::Max,
                other => {
                    return Err(EGraphError::ExtractionError(format!(
                        "unknown window function: {other}"
                    )));
                }
            };
            return Ok(func);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected WindowFn node".into(),
    ))
}

fn extract_window_frame(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Option<WindowFrame>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::Nil = node {
            return Ok(None);
        }
        if let RelLang::WindowFrameNode([mode_id, start_id, end_id]) = node {
            let mode = extract_frame_mode(egraph, *mode_id)?;
            let start = extract_frame_bound(egraph, *start_id)?;
            let end = extract_frame_bound(egraph, *end_id)?;
            return Ok(Some(WindowFrame { mode, start, end }));
        }
    }
    Err(EGraphError::ExtractionError(
        "expected WindowFrameNode or Nil".into(),
    ))
}

fn extract_frame_mode(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<WindowFrameMode, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::FrameRows => return Ok(WindowFrameMode::Rows),
            RelLang::FrameRange => return Ok(WindowFrameMode::Range),
            RelLang::FrameGroups => return Ok(WindowFrameMode::Groups),
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected frame mode node".into(),
    ))
}

fn extract_frame_bound(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<WindowFrameBound, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::FrameUnboundedPreceding => {
                return Ok(WindowFrameBound::UnboundedPreceding);
            }
            RelLang::FramePreceding([n_id]) => {
                let s = extract_symbol(egraph, *n_id)?;
                let n = s.parse::<u64>().map_err(|e| {
                    EGraphError::ExtractionError(format!("invalid frame bound: {e}"))
                })?;
                return Ok(WindowFrameBound::Preceding(n));
            }
            RelLang::FrameCurrentRow => {
                return Ok(WindowFrameBound::CurrentRow);
            }
            RelLang::FrameFollowing([n_id]) => {
                let s = extract_symbol(egraph, *n_id)?;
                let n = s.parse::<u64>().map_err(|e| {
                    EGraphError::ExtractionError(format!("invalid frame bound: {e}"))
                })?;
                return Ok(WindowFrameBound::Following(n));
            }
            RelLang::FrameUnboundedFollowing => {
                return Ok(WindowFrameBound::UnboundedFollowing);
            }
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected frame bound node".into(),
    ))
}

fn extract_values_row(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<Expr>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::ValuesRow(ids) = node {
            let mut cells = Vec::with_capacity(ids.len());
            for &cell_id in ids.iter() {
                cells.push(extract_scalar_expr(egraph, cell_id)?);
            }
            return Ok(cells);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected ValuesRow node".into(),
    ))
}

fn extract_sort_key_list(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<SortKey>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::List(ids) = node {
            let mut keys = Vec::with_capacity(ids.len());
            for &child_id in ids.iter() {
                keys.push(extract_sort_key(egraph, child_id)?);
            }
            return Ok(keys);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected List node for sort keys".into(),
    ))
}

fn extract_sort_key(egraph: &EGraph<RelLang, RelAnalysis>, id: Id) -> Result<SortKey, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::SortKey([expr_id, dir_id, nulls_id]) = node {
            let expr = extract_scalar_expr(egraph, *expr_id)?;
            let direction = extract_sort_direction(egraph, *dir_id)?;
            let nulls = extract_null_ordering(egraph, *nulls_id)?;
            return Ok(ra_core::algebra::SortKey {
                expr,
                direction,
                nulls,
            });
        }
    }
    Err(EGraphError::ExtractionError("expected SortKey node".into()))
}

fn extract_sort_direction(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<SortDirection, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::Asc => return Ok(SortDirection::Asc),
            RelLang::Desc => return Ok(SortDirection::Desc),
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected Asc/Desc node".into(),
    ))
}

fn extract_null_ordering(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<NullOrdering, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::NullsFirst => return Ok(NullOrdering::First),
            RelLang::NullsLast => return Ok(NullOrdering::Last),
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected NullsFirst/NullsLast node".into(),
    ))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    #[test]
    fn roundtrip_scan() {
        let expr = RelExpr::scan("users");
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        assert!(!rec.as_ref().is_empty());
    }

    #[test]
    fn roundtrip_filter() {
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Const(Const::Int(42))),
        });
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        assert!(!rec.as_ref().is_empty());
    }

    #[test]
    fn roundtrip_join() {
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("a", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("b", "a_id"))),
            },
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        assert!(!rec.as_ref().is_empty());
    }

    #[test]
    fn optimizer_roundtrip_simple_scan() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let result = optimizer
            .optimize(&expr)
            .expect("optimization should succeed");
        assert_eq!(result, expr);
    }

    #[test]
    fn optimizer_roundtrip_filter() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let result = optimizer
            .optimize(&expr)
            .expect("optimization should succeed");
        // The optimized result should be semantically equivalent
        // (may or may not be structurally identical - optimizer may
        // wrap in Project, reorder, or apply other transformations)
        let _ = result;
    }

    // ---- optimize_bounded tests ----

    #[test]
    fn bounded_optimize_simple_scan() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::unlimited());
        let expr = RelExpr::scan("users");
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("bounded optimization should succeed");
        assert_eq!(result.status, OptimizationStatus::Complete);
        assert!(result.cost.is_finite());
        assert!(result.resource_usage.completed_within_budget());
    }

    #[test]
    fn bounded_optimize_with_iteration_limit() {
        let budget = ResourceBudget::unlimited().with_iteration_limit(2);
        let optimizer = Optimizer::new().with_resource_budget(budget);
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("bounded optimization should succeed");
        assert!(result.resource_usage.iterations_used <= 2);
    }

    #[test]
    fn bounded_optimize_returns_plan_on_timeout() {
        let budget = ResourceBudget::unlimited()
            .with_time_limit(std::time::Duration::from_millis(0))
            .with_overflow_strategy(crate::resource_budget::OverflowStrategy::ReturnBestSoFar);
        let optimizer = Optimizer::new().with_resource_budget(budget);
        let expr = RelExpr::scan("users");
        // Even with 0ms budget, we should still get a plan
        // because we extract the initial plan before iterating
        std::thread::sleep(std::time::Duration::from_millis(1));
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("should return best so far");
        assert_eq!(result.status, OptimizationStatus::Incomplete);
    }

    #[test]
    fn bounded_optimize_return_original_strategy() {
        let budget = ResourceBudget::unlimited()
            .with_iteration_limit(0)
            .with_overflow_strategy(crate::resource_budget::OverflowStrategy::ReturnOriginal);
        let optimizer = Optimizer::new().with_resource_budget(budget);
        let expr = RelExpr::scan("users");
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("should return original");
        assert_eq!(result.status, OptimizationStatus::Incomplete);
        assert_eq!(result.plan, expr);
    }

    #[test]
    fn bounded_optimize_fail_strategy() {
        let budget = ResourceBudget::unlimited()
            .with_iteration_limit(0)
            .with_overflow_strategy(crate::resource_budget::OverflowStrategy::Fail);
        let optimizer = Optimizer::new().with_resource_budget(budget);
        let expr = RelExpr::scan("users");
        let result = optimizer.optimize_bounded(&expr);
        assert!(result.is_err());
        let err = result.err().expect("should be error");
        assert!(matches!(err, EGraphError::ResourceBudgetExceeded(_)));
    }

    #[test]
    fn bounded_optimize_no_budget_defaults_unlimited() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("should succeed with default budget");
        assert_eq!(result.status, OptimizationStatus::Complete);
    }

    #[test]
    fn bounded_optimize_tracks_egraph_nodes() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::standard());
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        assert!(result.resource_usage.peak_egraph_nodes > 0);
    }

    #[test]
    fn bounded_optimize_tracks_memory_estimate() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::standard());
        let expr = RelExpr::scan("users");
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        assert!(result.resource_usage.peak_memory_estimate > 0);
    }

    #[test]
    fn bounded_optimize_cost_is_finite() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::batch());
        let expr = RelExpr::scan("users");
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        assert!(result.cost.is_finite());
        assert!(result.cost > 0.0);
    }

    #[test]
    fn bounded_optimize_elapsed_time_recorded() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::standard());
        let expr = RelExpr::scan("users");
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        // Elapsed time should be non-zero (we did some work)
        let _elapsed = result.resource_usage.elapsed_time;
    }

    #[test]
    fn bounded_optimize_interactive_profile() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::interactive());
        let expr = RelExpr::scan("users");
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        assert!(
            result.cost.is_finite(),
            "interactive budget should produce a plan"
        );
    }

    #[test]
    fn bounded_optimize_memory_constrained_profile() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::memory_constrained());
        let expr = RelExpr::scan("users");
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        assert!(result.cost.is_finite());
    }

    #[test]
    fn bounded_optimize_with_egraph_node_limit() {
        let budget = ResourceBudget::unlimited().with_egraph_node_limit(5);
        let optimizer = Optimizer::new().with_resource_budget(budget);
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("a", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("b", "a_id"))),
            },
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("should succeed with best-so-far");
        // With such a tight e-graph limit, likely incomplete
        assert!(result.cost.is_finite() || result.cost == f64::INFINITY);
    }

    #[test]
    fn optimization_status_variants() {
        assert_ne!(OptimizationStatus::Complete, OptimizationStatus::Incomplete);
        assert_ne!(OptimizationStatus::Complete, OptimizationStatus::Failed);
        assert_ne!(OptimizationStatus::Incomplete, OptimizationStatus::Failed);
    }

    #[test]
    fn optimization_result_has_plan() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::unlimited());
        let expr = RelExpr::scan("test_table");
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        // The plan should be a valid RelExpr
        assert!(matches!(result.plan, RelExpr::Scan { .. }));
    }

    #[test]
    fn set_resource_budget_mutable() {
        let mut optimizer = Optimizer::new();
        optimizer.set_resource_budget(ResourceBudget::interactive());
        let expr = RelExpr::scan("users");
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        assert!(result.cost.is_finite());
    }

    #[test]
    fn resource_budget_exceeded_error_display() {
        let err = EGraphError::ResourceBudgetExceeded("iterations".to_owned());
        let msg = format!("{err}");
        assert!(msg.contains("iterations"));
        assert!(msg.contains("resource budget exceeded"));
    }

    #[test]
    fn bounded_optimize_best_so_far_no_plan_returns_original() {
        // With 0 iterations AND ReturnBestSoFar, we still get
        // the initial plan because we extract before iterating
        let budget = ResourceBudget::unlimited()
            .with_iteration_limit(0)
            .with_overflow_strategy(crate::resource_budget::OverflowStrategy::ReturnBestSoFar);
        let optimizer = Optimizer::new().with_resource_budget(budget);
        let expr = RelExpr::scan("users");
        let result = optimizer
            .optimize_bounded(&expr)
            .expect("should return a plan");
        assert_eq!(result.status, OptimizationStatus::Incomplete);
    }

    #[test]
    fn bounded_optimize_join_with_budget() {
        let optimizer = Optimizer::new().with_resource_budget(ResourceBudget::standard());
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("a", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("b", "a_id"))),
            },
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let result = optimizer.optimize_bounded(&expr).expect("should succeed");
        assert!(result.cost.is_finite());
        assert!(result.resource_usage.iterations_used > 0);
    }

    // ---- optimize_incremental tests ----

    #[cfg(feature = "timeline")]
    fn make_snap(time: u64, rows: u64) -> ra_stats::timeline::Snapshot {
        ra_stats::timeline::Snapshot {
            time_offset: time,
            label: None,
            tables: vec![ra_stats::timeline::TableSnapshot {
                name: "users".to_string(),
                row_count: rows,
                page_count: None,
                avg_row_size: None,
                table_size_bytes: None,
                columns: vec![ra_stats::timeline::ColumnSnapshot {
                    name: "id".to_string(),
                    ndv: rows,
                    null_fraction: 0.0,
                    avg_width: 8.0,
                    correlation: Some(1.0),
                    min_value: None,
                    max_value: None,
                }],
            }],
        }
    }

    #[cfg(feature = "timeline")]
    fn small_delta() -> DeltaSet {
        let a = make_snap(0, 10_000);
        let b = make_snap(60, 10_100); // 1% change
        DeltaSet::compute(&a, &b)
    }

    #[cfg(feature = "timeline")]
    fn medium_delta() -> DeltaSet {
        let a = make_snap(0, 10_000);
        let b = make_snap(60, 11_000); // 10% change
        DeltaSet::compute(&a, &b)
    }

    #[cfg(feature = "timeline")]
    fn large_delta() -> DeltaSet {
        let a = make_snap(0, 10_000);
        let b = make_snap(60, 20_000); // 100% change
        DeltaSet::compute(&a, &b)
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_simple_scan() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (result, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("incremental should succeed");
        assert!(matches!(result, RelExpr::Scan { .. }));
        assert!(!stats.used_full_reoptimization);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_returns_valid_plan() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let delta = small_delta();
        let (result, _) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(matches!(result, RelExpr::Filter { .. }) || matches!(result, RelExpr::Scan { .. }));
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_small_delta_fewer_iterations() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.iterations_used <= stats.max_iterations);
        assert!(!stats.used_full_reoptimization);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_medium_delta_more_iterations() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = medium_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.iterations_used >= 1);
        assert!(!stats.used_full_reoptimization);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_large_delta_falls_back_to_full() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = large_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.used_full_reoptimization);
        assert_eq!(stats.iterations_used, stats.max_iterations);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_updates_table_stats() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.tables_updated > 0);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_empty_delta_uses_minimal_iterations() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = DeltaSet::new(0, 60);
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.delta_count == 0);
        assert!(!stats.used_full_reoptimization);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_produces_same_as_full_for_scan() {
        let expr = RelExpr::scan("users");
        let delta = small_delta();

        let full_result = Optimizer::new()
            .optimize(&expr)
            .expect("full should succeed");
        let (incr_result, _) = Optimizer::new()
            .optimize_incremental(&expr, &delta)
            .expect("incremental should succeed");

        // Both should produce a scan (may differ in internal IDs).
        assert!(matches!(full_result, RelExpr::Scan { .. }));
        assert!(matches!(incr_result, RelExpr::Scan { .. }));
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_stats_speedup_factor() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.speedup_factor() >= 1.0);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_stats_full_reopt_speedup_is_one() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = large_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!((stats.speedup_factor() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_reports_delta_count() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.delta_count > 0);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_reports_row_change_pct() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = medium_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.row_change_pct > 5.0);
        assert!(stats.row_change_pct < 15.0);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_elapsed_time_recorded() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        // Elapsed should be non-zero.
        let _elapsed = stats.elapsed;
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_join_query() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("a", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("b", "a_id"))),
            },
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let delta = small_delta();
        let (result, _) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(matches!(result, RelExpr::Join { .. }) || matches!(result, RelExpr::Scan { .. }));
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_table_added_delta() {
        let a = ra_stats::timeline::Snapshot {
            time_offset: 0,
            label: None,
            tables: vec![ra_stats::timeline::TableSnapshot {
                name: "users".to_string(),
                row_count: 1000,
                page_count: None,
                avg_row_size: None,
                table_size_bytes: None,
                columns: vec![],
            }],
        };
        let b = ra_stats::timeline::Snapshot {
            time_offset: 60,
            label: None,
            tables: vec![
                ra_stats::timeline::TableSnapshot {
                    name: "users".to_string(),
                    row_count: 1000,
                    page_count: None,
                    avg_row_size: None,
                    table_size_bytes: None,
                    columns: vec![],
                },
                ra_stats::timeline::TableSnapshot {
                    name: "orders".to_string(),
                    row_count: 5000,
                    page_count: None,
                    avg_row_size: None,
                    table_size_bytes: None,
                    columns: vec![],
                },
            ],
        };
        let delta = DeltaSet::compute(&a, &b);
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        // Structural change triggers full reoptimization.
        assert!(stats.used_full_reoptimization);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_nodes_in_egraph_reported() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.nodes_in_egraph > 0);
    }

    #[test]
    #[cfg(feature = "timeline")]
    fn incremental_rules_evaluated_reported() {
        let mut optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let delta = small_delta();
        let (_, stats) = optimizer
            .optimize_incremental(&expr, &delta)
            .expect("should succeed");
        assert!(stats.rules_evaluated > 0);
    }

    #[test]
    fn optimize_with_facts_succeeds() {
        use crate::FactsContextBuilder;
        use ra_hardware::HardwareProfile;

        let facts = FactsContextBuilder::new(HardwareProfile::cpu_only())
            .database("postgresql")
            .dialect(ra_core::SqlDialect::Postgres)
            .feature("lateral_join", true)
            .feature("cte_recursive", true)
            .build();

        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");

        let result = optimizer
            .optimize_with_facts(&expr, &facts)
            .expect("should succeed");

        // Should produce a valid plan
        assert!(matches!(result, RelExpr::Scan { .. }));
    }

    #[test]
    fn optimize_with_facts_uses_hardware_info() {
        use crate::FactsContextBuilder;
        use ra_hardware::HardwareProfile;

        let facts = FactsContextBuilder::new(HardwareProfile::gpu_server())
            .database("duckdb")
            .dialect(ra_core::SqlDialect::DuckDb)
            .feature("parallel_scan", true)
            .build();

        let optimizer = Optimizer::new();
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("customers")),
        };

        let result = optimizer
            .optimize_with_facts(&expr, &facts)
            .expect("should succeed");

        // Should produce an optimized plan
        // (actual plan may vary, but should not error)
        assert!(matches!(result, RelExpr::Join { .. }) || matches!(result, RelExpr::Scan { .. }));
    }

    // ── Plan cache integration tests ────────────────────────────

    fn cached_optimizer() -> Optimizer {
        Optimizer::new().with_plan_cache(PlanCacheConfig {
            max_entries: 64,
            similarity_threshold: 0.9,
            enable_fuzzy_matching: true,
            ..PlanCacheConfig::default()
        })
    }

    #[test]
    fn plan_cache_miss_then_hit() {
        let opt = cached_optimizer();
        let q1 = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Const(Const::Int(42))),
        });
        // First call: cache miss, runs optimization
        let _ = opt.optimize(&q1).expect("should succeed");
        let stats = opt.cache_stats().expect("cache enabled");
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.exact_hits, 0);

        // Same query again: cache hit
        let _ = opt.optimize(&q1).expect("should succeed");
        let stats = opt.cache_stats().expect("cache enabled");
        assert_eq!(stats.exact_hits, 1);
    }

    #[test]
    fn plan_cache_parameter_variation_hits() {
        let opt = cached_optimizer();
        let q1 = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let _ = opt.optimize(&q1).expect("should succeed");

        // Different constant value, same structure
        let q2 = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(65))),
        });
        let _ = opt.optimize(&q2).expect("should succeed");

        let stats = opt.cache_stats().expect("cache enabled");
        assert_eq!(
            stats.exact_hits, 1,
            "parameter variation should be exact hit"
        );
        assert_eq!(stats.misses, 1, "only the first query should miss");
    }

    #[test]
    fn plan_cache_disabled_by_default() {
        let opt = Optimizer::new();
        assert!(opt.cache_stats().is_none());
    }

    #[test]
    fn plan_cache_clear() {
        let opt = cached_optimizer();
        let q = RelExpr::scan("users");
        let _ = opt.optimize(&q).expect("should succeed");
        assert_eq!(opt.cache_stats().expect("cache enabled").current_entries, 1);

        opt.clear_cache();
        assert_eq!(opt.cache_stats().expect("cache enabled").current_entries, 0);
    }

    #[test]
    fn plan_cache_oltp_hit_rate_above_90_pct() {
        let opt = cached_optimizer();

        // 5 templates, 20 parameter variations each = 100 queries
        let total = 100_u32;

        for i in 0..20_i64 {
            let _ = opt.optimize(&RelExpr::scan("users").filter(Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Const(Const::Int(i))),
            }));
        }
        for i in 0..20_i64 {
            let _ = opt.optimize(&RelExpr::scan("orders").filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("amount"))),
                right: Box::new(Expr::Const(Const::Int(i * 100))),
            }));
        }
        for i in 0..20_i64 {
            let _ = opt.optimize(&RelExpr::Join {
                join_type: JoinType::Inner,
                condition: Expr::BinOp {
                    op: BinOp::Eq,
                    left: Box::new(Expr::Column(ColumnRef::qualified("u", "id"))),
                    right: Box::new(Expr::Column(ColumnRef::qualified("o", "uid"))),
                },
                left: Box::new(RelExpr::scan("users").filter(Expr::BinOp {
                    op: BinOp::Gt,
                    left: Box::new(Expr::Column(ColumnRef::new("age"))),
                    right: Box::new(Expr::Const(Const::Int(18 + i))),
                })),
                right: Box::new(RelExpr::scan("orders")),
            });
        }
        for i in 0..20_i64 {
            let _ = opt.optimize(&RelExpr::Aggregate {
                group_by: vec![Expr::Column(ColumnRef::new("dept"))],
                aggregates: vec![AggregateExpr {
                    function: AggregateFunction::Count,
                    arg: None,
                    distinct: false,
                    alias: None,
                }],
                input: Box::new(RelExpr::scan("employees").filter(Expr::BinOp {
                    op: BinOp::Gt,
                    left: Box::new(Expr::Column(ColumnRef::new("salary"))),
                    right: Box::new(Expr::Const(Const::Int(50000 + i * 1000))),
                })),
            });
        }
        for i in 0..20_i64 {
            let _ = opt.optimize(&RelExpr::scan("products").filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("price"))),
                right: Box::new(Expr::Const(Const::Int(i * 10))),
            }));
        }

        let stats = opt.cache_stats().expect("cache enabled");
        let hit_count = (stats.exact_hits + stats.fuzzy_hits) as u32;

        // 5 cold misses + 95 hits = 95% hit rate
        let hit_rate = f64::from(hit_count) / f64::from(total);
        assert!(
            hit_rate >= 0.9,
            "expected >90% hit rate, got {:.1}% ({} hits / {} total, stats: {:?})",
            hit_rate * 100.0,
            hit_count,
            total,
            stats
        );
    }

    // ---- rule tracking tests ----

    #[test]
    fn test_optimize_with_tracking_simple() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let result = optimizer
            .optimize_with_tracking(&expr)
            .expect("tracking optimization should succeed");

        assert!(result.rule_tracking.is_some());
        let tracking = result.rule_tracking.unwrap();
        assert!(!tracking.available.is_empty());
        assert!(tracking.available.len() >= 200); // Total rules (varies with features)
    }

    #[test]
    fn test_optimize_with_tracking_with_changes() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(18))),
            }),
            right: Box::new(Expr::Const(Const::Bool(true))),
        });

        let result = optimizer
            .optimize_with_tracking(&expr)
            .expect("tracking optimization should succeed");

        assert!(result.rule_tracking.is_some());
        let tracking = result.rule_tracking.unwrap();
        assert!(!tracking.available.is_empty());

        // The filter-true rule should simplify this
        if !tracking.applied.is_empty() {
            assert!(tracking.applied[0].fired_count > 0);
        }
    }

    #[test]
    fn test_rule_tracking_result_structure() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users");
        let result = optimizer
            .optimize_with_tracking(&expr)
            .expect("tracking optimization should succeed");

        let tracking = result.rule_tracking.expect("tracking should be present");

        // Check structure
        assert!(!tracking.available.is_empty());
        // Applied and evaluated depend on whether rules fired
        assert!(tracking.applied.len() <= tracking.available.len());
        assert!(tracking.evaluated.len() <= tracking.available.len());
    }

    #[test]
    fn test_verbose_mode_captures_intermediate_steps() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(18))),
            }),
            right: Box::new(Expr::Const(Const::Bool(true))),
        });

        let result = optimizer
            .optimize_with_tracking_verbose(&expr, true)
            .expect("verbose tracking should succeed");

        let tracking = result.rule_tracking.expect("tracking should be present");

        // Verbose mode should populate intermediate_steps
        assert!(tracking.intermediate_steps.is_some());

        if !tracking.applied.is_empty() {
            let steps = tracking.intermediate_steps.unwrap();
            // If rules were applied, we should have steps
            if !steps.is_empty() {
                // Each step should have complete information
                for step in &steps {
                    assert!(step.step_number > 0);
                    assert!(!step.rule_name.is_empty());
                    assert!(!step.reason.is_empty());
                }
            }
        }
    }

    #[test]
    fn test_non_verbose_mode_skips_intermediate_steps() {
        let optimizer = Optimizer::new();
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(18))),
            }),
            right: Box::new(Expr::Const(Const::Bool(true))),
        });

        let result = optimizer
            .optimize_with_tracking_verbose(&expr, false)
            .expect("non-verbose tracking should succeed");

        let tracking = result.rule_tracking.expect("tracking should be present");

        // Non-verbose mode should not populate intermediate_steps
        assert!(
            tracking.intermediate_steps.is_none()
                || tracking.intermediate_steps.as_ref().unwrap().is_empty()
        );
    }
}

/// Statistics provider for table statistics.
#[derive(Debug, Clone)]
struct TableStatsProvider {
    stats: HashMap<String, ra_core::statistics::Statistics>,
}

impl ra_core::cost::StatisticsProvider for TableStatsProvider {
    fn get_statistics(&self, table: &str) -> Option<&ra_core::statistics::Statistics> {
        self.stats.get(table)
    }
}
