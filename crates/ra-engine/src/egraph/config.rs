use crate::plan_cache::PlanCacheConfig;

/// Configuration for the equality saturation optimizer.
#[derive(Debug, Clone)]
#[expect(clippy::struct_excessive_bools, reason = "configuration struct")]
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
    /// snapshot bloat, `SubXID` overflow, and `MultiXact` pressure.
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
    /// Cumulative cap on e-graph node growth across all saturation
    /// iterations. Unlike `node_limit` (which is a per-egg-runner-step
    /// ceiling), this bounds the total number of nodes added to the
    /// e-graph for the whole optimization. When `use_adaptive_limits`
    /// is true the active [`crate::speculative_router::OptRoute`]
    /// supplies a route-specific budget; this field is the fallback
    /// hard cap.
    ///
    /// 0 disables the check; defaults to a large value sized for the
    /// tail of analytical queries.
    pub max_node_growth: usize,
    /// Cumulative cap on the number of successful rewrite applications
    /// across all saturation iterations. Each entry in egg's
    /// `Iteration.applied` map contributes to this count.
    ///
    /// 0 disables the check; defaults to a large value sized for the
    /// tail of analytical queries. Like `max_node_growth`, the
    /// adaptive path overrides this with a route-specific budget.
    pub max_rule_applications: usize,
    /// Enable [`crate::join_graph_shape::JoinGraphShape`]-based
    /// advisory rule filtering. When true, rule groups that the
    /// shape predicate marks as "redundant for this query" (e.g.
    /// join-reordering rules on a query whose join graph admits
    /// no reorderings) are removed from the loaded rule set
    /// before saturation. Purely advisory: removing these rules
    /// affects plan cost but not correctness.
    pub use_shape_aware_filtering: bool,
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
            // Generous defaults sized for analytical-query tail; the
            // adaptive path narrows these per `OptRoute`. Set to 0 to
            // disable cumulative budgeting entirely.
            max_node_growth: 1_000_000,
            max_rule_applications: 100_000,
            // Shape-aware filtering is cheap (one O(n) walk of the
            // RelExpr) and purely advisory; default on.
            use_shape_aware_filtering: true,
        }
    }
}
