use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use egg::{EGraph, Id, Rewrite, Runner};
use ra_core::algebra::RelExpr;
#[cfg(feature = "timeline")]
use ra_stats::delta::DeltaSet;

use crate::analysis::RelAnalysis;
use crate::continuation_gate::{ContinuationDecision, ContinuationGate};
use crate::cost_model::BitNetCostModel;
use crate::cost_model::feedback::OptimizationTrace;
use crate::extract::{extract_best, extract_best_bitnet};
use crate::training_coordinator::SharedTrainingCoordinator;
use crate::genetic_fingerprint::QueryFingerprint;
use crate::plan_cache::{PlanCache, PlanCacheConfig, PlanCacheStats};
use crate::resource_budget::{
    ConvergenceBehavior, OverflowStrategy, ResourceBudget, ResourceTracker,
};
use crate::rewrite::all_rules;
use crate::speculative_router::{OptRoute, OptimizationFeatures, SpeculativeRouter};
use crate::state::FingerprintReader;

use super::config::OptimizerConfig;
use super::errors::EGraphError;
use super::lang::RelLang;
#[cfg(feature = "timeline")]
use super::result::IncrementalStats;
use super::result::{OptimizationResult, OptimizationStatus};
use super::to_rec::to_rec_expr;
use super::tracking::{
    build_detailed_tracking, build_detailed_tracking_with_steps, IntermediateStep, RuleApplication,
    RuleTrackingResult,
};

/// Default iteration limit based on table count (replaces `QueryComplexity`).
fn default_iter_limit_for_tables(table_count: usize) -> usize {
    match table_count {
        0..=1 => 3,
        2..=4 => 5,
        5..=7 => 10,
        8..=9 => 15,
        _ => 20,
    }
}

/// Default timeout based on table count (replaces `QueryComplexity`).
fn default_timeout_ms_for_tables(table_count: usize) -> u64 {
    match table_count {
        0..=1 => 50,
        2..=4 => 200,
        5..=7 => 500,
        8..=9 => 2000,
        _ => 5000,
    }
}

/// Whether a query is "simple" for convergence purposes.
fn is_simple_query(table_count: usize) -> bool {
    table_count <= 4
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
    table_stats: Arc<HashMap<String, ra_core::statistics::Statistics>>,
    hardware_profile: Option<ra_hardware::HardwareProfile>,
    resource_budget: Option<ResourceBudget>,
    plan_cache: Option<Mutex<PlanCache>>,
    rule_advisor: Option<Mutex<crate::rule_advisor::RuleAdvisor>>,
    cost_model: Option<Arc<BitNetCostModel>>,
    fingerprint_reader: Option<FingerprintReader>,
    speculative_router: Option<SpeculativeRouter>,
    training_coordinator: Option<SharedTrainingCoordinator>,
}

/// Result of running e-graph equality saturation.
struct SaturationResult {
    egraph: EGraph<RelLang, RelAnalysis>,
    root: Id,
    actual_iterations: usize,
    termination_reason: &'static str,
    runner_elapsed: std::time::Duration,
    egraph_nodes: usize,
    continuation_gate: Option<ContinuationGate>,
    stats_cache: crate::stats_cache::StatsCache,
}

/// Mutable state carried through the saturation loop.
struct LoopContext {
    convergence_detector: crate::convergence::ConvergenceDetector,
    cost_pruner: Option<crate::cost_pruning::CostPruner>,
    beam_search_tracker: Option<crate::beam_search::BeamSearchTracker>,
    hardware_cached: Option<ra_hardware::HardwareProfile>,
    best_cost: f64,
    cost_improvement_stalled: u32,
}

impl Optimizer {
    /// Create a new optimizer with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: OptimizerConfig::default(),
            table_stats: Arc::new(HashMap::new()),
            hardware_profile: None,
            resource_budget: None,
            plan_cache: None,
            rule_advisor: None,
            cost_model: None,
            fingerprint_reader: None,
            speculative_router: None,
            training_coordinator: None,
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
            table_stats: Arc::new(HashMap::new()),
            hardware_profile: None,
            resource_budget: None,
            plan_cache,
            rule_advisor,
            cost_model: None,
            fingerprint_reader: None,
            speculative_router: None,
            training_coordinator: None,
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
            let cache = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            cache.stats().clone()
        })
    }

    /// Clear the plan cache.
    pub fn clear_cache(&self) {
        if let Some(m) = self.plan_cache.as_ref() {
            let mut cache = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            cache.clear();
        }
    }

    /// Return a snapshot of rule advisor statistics.
    ///
    /// Returns `None` if the rule advisor is not enabled.
    #[must_use]
    pub fn advisor_stats(&self) -> Option<crate::rule_advisor::AdvisorStats> {
        self.rule_advisor.as_ref().map(|m| {
            let advisor = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            advisor.stats().clone()
        })
    }

    /// Load rules using the advisor > lazy > all priority chain.
    ///
    /// Centralises the rule-loading logic so every optimisation path
    /// (normal, bounded, tracking, incremental) honours the rule
    /// advisor when it is configured. When a [`ResourceBudget`] is
    /// set, passes it to the advisor so rule selection respects the
    /// budget's [`RuleSelectionBehavior`].
    fn load_rules(&self, expr: &RelExpr) -> Vec<Rewrite<RelLang, RelAnalysis>> {
        if let Some(ref advisor_mutex) = self.rule_advisor {
            let mut advisor = advisor_mutex
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(ref budget) = self.resource_budget {
                advisor.select_rules_with_budget(expr, budget)
            } else {
                advisor.select_rules(expr)
            }
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
        Arc::make_mut(&mut self.table_stats).insert(table.into(), stats);
    }

    /// Builder-style setter for the neural cost model.
    #[must_use]
    pub fn with_cost_model(mut self, model: Arc<BitNetCostModel>) -> Self {
        self.cost_model = Some(model);
        if self.fingerprint_reader.is_none() {
            self.fingerprint_reader = Some(FingerprintReader::new());
        }
        self
    }

    /// Builder-style setter for the fingerprint reader.
    #[must_use]
    pub fn with_fingerprint_reader(mut self, reader: FingerprintReader) -> Self {
        self.fingerprint_reader = Some(reader);
        self
    }

    /// Enable speculative routing with the given cost model.
    ///
    /// When enabled, the optimizer uses a `BitNet` forward pass (~87ns)
    /// to predict the optimal optimization strategy before running
    /// the e-graph. This can route simple queries (equi-join chains)
    /// directly to left-deep construction, bypassing e-graph entirely.
    #[must_use]
    pub fn with_speculative_router(mut self, model: Arc<BitNetCostModel>) -> Self {
        self.speculative_router = Some(SpeculativeRouter::new(model));
        self
    }

    /// Enable speculative routing using the already-loaded cost model.
    ///
    /// Reuses the existing model if one is loaded. No-op if no model exists.
    pub fn enable_speculative_routing(&mut self) {
        if let Some(ref model) = self.cost_model {
            self.speculative_router = Some(SpeculativeRouter::new(Arc::clone(model)));
        }
    }

    /// Enable online training: each optimization run feeds back to the model.
    ///
    /// When enabled, the optimizer records `OptimizationTrace` for every
    /// e-graph run and feeds batches to the `BitNetTrainer`. The model
    /// improves over time based on observed optimization difficulty.
    #[must_use]
    pub fn with_training(mut self, coordinator: SharedTrainingCoordinator) -> Self {
        self.training_coordinator = Some(coordinator);
        self
    }

    /// Enable online training with a new coordinator.
    pub fn enable_training(&mut self) {
        self.training_coordinator =
            Some(crate::training_coordinator::shared_coordinator());
    }

    /// Get training statistics (if training is enabled).
    #[must_use]
    pub fn training_stats(&self) -> Option<crate::training_coordinator::TrainingStats> {
        self.training_coordinator.as_ref().and_then(|c| {
            c.lock().ok().map(|coord| coord.stats())
        })
    }

    /// Get a reference to the training coordinator handle.
    #[must_use]
    pub fn training_coordinator_handle(&self) -> Option<&SharedTrainingCoordinator> {
        self.training_coordinator.as_ref()
    }

    /// Load a `BitNet` cost model from a JSON file.
    ///
    /// The path can be overridden via the `RA_MODEL_PATH` environment variable.
    /// Falls back to `models/cost_model.bitnet.json`. If no model file exists,
    /// the optimizer uses traditional costing only.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be parsed.
    pub fn load_model(&mut self) -> Result<(), EGraphError> {
        let path = std::env::var("RA_MODEL_PATH")
            .unwrap_or_else(|_| "models/cost_model.bitnet.json".to_string());
        let path = Path::new(&path);
        if !path.exists() {
            tracing::debug!("No cost model at {}, using traditional costing", path.display());
            return Ok(());
        }
        let model = BitNetCostModel::load_from_file(
            path.to_str().unwrap_or("models/cost_model.bitnet.json"),
        )
        .map_err(|e| EGraphError::ExtractionError(format!("model load failed: {e}")))?;
        tracing::info!(
            samples_trained = model.samples_trained,
            "Loaded BitNet cost model from {}",
            path.display()
        );
        self.cost_model = Some(Arc::new(model));
        if self.fingerprint_reader.is_none() {
            self.fingerprint_reader = Some(FingerprintReader::new());
        }
        Ok(())
    }

    /// Extract the best plan using hybrid neural/traditional cost when available.
    ///
    /// Falls back to `extract_best` when no neural model is loaded.
    fn extract_with_hybrid_fallback<S: std::hash::BuildHasher>(
        &self,
        egraph: &egg::EGraph<RelLang, RelAnalysis>,
        root: Id,
        table_stats: &HashMap<String, ra_core::statistics::Statistics, S>,
        hardware: &ra_hardware::HardwareProfile,
    ) -> Result<RelExpr, EGraphError> {
        if let (Some(model), Some(reader)) = (&self.cost_model, &self.fingerprint_reader) {
            let fingerprint = reader.read();
            let staleness_map: HashMap<String, ra_stats::accuracy::Staleness> = table_stats
                .keys()
                .map(|k| (k.clone(), ra_stats::accuracy::Staleness::Fresh))
                .collect();
            extract_best_bitnet(egraph, root, table_stats, &staleness_map, hardware, model, &fingerprint)
        } else {
            extract_best(egraph, root, table_stats, hardware)
        }
    }

    /// Returns true if the query is trivial enough to skip the e-graph
    /// entirely. A trivial query has no joins, no subqueries, no CTEs,
    /// no window functions, and at most one table reference.
    fn is_trivial_query(expr: &RelExpr) -> bool {
        match expr {
            RelExpr::Scan { .. } | RelExpr::Values { .. } => true,
            RelExpr::Filter { input, predicate, .. } => {
                !crate::subquery_decorrelation::contains_subquery(predicate)
                    && Self::is_trivial_query(input)
            }
            RelExpr::Project { input, columns, .. } => {
                !columns.iter().any(|c| {
                    crate::subquery_decorrelation::contains_subquery(&c.expr)
                }) && Self::is_trivial_query(input)
            }
            RelExpr::Sort { input, .. }
            | RelExpr::Limit { input, .. }
            | RelExpr::Distinct { input } => Self::is_trivial_query(input),
            RelExpr::Aggregate { input, .. } => Self::is_trivial_query(input),
            // Joins, CTEs, window functions, set ops → not trivial
            _ => false,
        }
    }

    /// Optimize DML sub-relations without putting the DML envelope
    /// through equality saturation.
    ///
    /// Returns `Some(optimized)` for DML statements, `None` for queries.
    fn try_optimize_dml(
        &self,
        expr: &RelExpr,
    ) -> Result<Option<RelExpr>, EGraphError> {
        match expr {
            RelExpr::Insert {
                table,
                columns,
                source,
                on_conflict,
                returning,
            } => {
                let optimized_source = self.optimize(source)?;
                Ok(Some(RelExpr::Insert {
                    table: table.clone(),
                    columns: columns.clone(),
                    source: Box::new(optimized_source),
                    on_conflict: on_conflict.clone(),
                    returning: returning.clone(),
                }))
            }
            RelExpr::Update {
                table,
                assignments,
                filter,
                from,
                returning,
            } => {
                let optimized_from = from
                    .as_deref()
                    .map(|f| self.optimize(f))
                    .transpose()?
                    .map(Box::new);
                Ok(Some(RelExpr::Update {
                    table: table.clone(),
                    assignments: assignments.clone(),
                    filter: filter.clone(),
                    from: optimized_from,
                    returning: returning.clone(),
                }))
            }
            RelExpr::Delete {
                table,
                filter,
                using,
                returning,
            } => {
                let optimized_using = using
                    .as_deref()
                    .map(|u| self.optimize(u))
                    .transpose()?
                    .map(Box::new);
                Ok(Some(RelExpr::Delete {
                    table: table.clone(),
                    filter: filter.clone(),
                    using: optimized_using,
                    returning: returning.clone(),
                }))
            }
            // CTE with DML body: optimize definition through e-graph,
            // handle DML body via DML fast-path recursion.
            RelExpr::CTE {
                name,
                definition,
                body,
            } if matches!(
                body.as_ref(),
                RelExpr::Update { .. } | RelExpr::Delete { .. } | RelExpr::Insert { .. }
            ) =>
            {
                let optimized_def = self.optimize(definition)?;
                let optimized_body = self.optimize(body)?;
                Ok(Some(RelExpr::CTE {
                    name: name.clone(),
                    definition: Box::new(optimized_def),
                    body: Box::new(optimized_body),
                }))
            }
            _ => Ok(None),
        }
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

        // 1. Plan cache lookup
        let fingerprint = if self.plan_cache.is_some() {
            let fp = QueryFingerprint::from_rel_expr(expr);
            if let Some(ref mutex) = self.plan_cache {
                let mut cache = mutex
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(hit) = cache.lookup(&fp) {
                    info!(
                        "Plan cache hit ({:?}, similarity={:.2}) in {:?}",
                        hit.match_type, hit.similarity, total_start.elapsed()
                    );
                    return Ok(hit.plan);
                }
                debug!("Plan cache miss");
            }
            Some(fp)
        } else {
            None
        };

        // 2a. DML fast-path: optimize sub-relations, preserve DML structure
        if let Some(result) = self.try_optimize_dml(expr)? {
            debug!("DML optimization completed");
            self.insert_into_cache(fingerprint.as_ref(), &result);
            return Ok(result);
        }

        // 2. Trivial fast-path
        if Self::is_trivial_query(expr) {
            debug!("Trivial query fast-path: skipping e-graph");
            self.record_fast_path_trace(expr, "trivial_fast_path", 0.0);
            self.insert_into_cache(fingerprint.as_ref(), expr);
            return Ok(expr.clone());
        }

        // 3. Speculative routing
        let opt_features = OptimizationFeatures::from_expr(expr)
            .with_table_stats(&self.table_stats);

        let mut route_prediction = if let Some(ref router) = self.speculative_router {
            router.predict(&opt_features)
        } else {
            SpeculativeRouter::heuristic_fallback(&opt_features)
        };

        if let Some(result) =
            self.try_fast_route(expr, &mut route_prediction, &fingerprint, total_start)?
        {
            return Ok(result);
        }

        // 4. Large join check
        let table_count = crate::large_join::LargeJoinOptimizer::count_tables(expr);
        if let Some(result) = self.try_large_join(expr, table_count, &fingerprint)? {
            return Ok(result);
        }

        // 5. Calculate iteration/timeout limits
        let (iter_limit, timeout_ms) =
            self.compute_egraph_limits(&route_prediction, table_count);

        info!(
            "Starting e-graph optimization: {} tables, iter_limit={}, timeout={}ms",
            table_count, iter_limit, timeout_ms
        );

        // 6. Run e-graph saturation
        let sat = self.run_egraph_saturation(
            expr, &opt_features, &route_prediction, iter_limit, timeout_ms, table_count,
        )?;

        // 7. Extract result
        let extract_start = Instant::now();
        let hardware = self.hardware_profile();
        let result = self.extract_with_hybrid_fallback(
            &sat.egraph, sat.root, sat.stats_cache.as_map(), &hardware,
        )?;
        debug!("extract_best: {:?}", extract_start.elapsed());

        info!(
            "Total optimization: {:?} (egraph={:?}, extract={:?})",
            total_start.elapsed(), sat.runner_elapsed, extract_start.elapsed()
        );

        // 8. Training feedback
        self.record_egraph_training_trace(
            expr, sat.actual_iterations, sat.termination_reason,
            sat.egraph_nodes, sat.runner_elapsed, sat.continuation_gate.as_ref(),
        );

        // 9. Cache + return
        self.insert_into_cache(fingerprint.as_ref(), &result);
        Ok(result)
    }

    /// Attempt Skip or `LeftDeep` fast routes. Returns `Some(plan)` if
    /// handled, `None` if we should continue to e-graph.
    fn try_fast_route(
        &self,
        expr: &RelExpr,
        route_prediction: &mut crate::speculative_router::RoutePrediction,
        fingerprint: &Option<QueryFingerprint>,
        total_start: std::time::Instant,
    ) -> Result<Option<RelExpr>, EGraphError> {
        use tracing::{debug, info};

        match route_prediction.route {
            OptRoute::Skip => {
                debug!(
                    "Speculative route: SKIP (conf={:.2})",
                    route_prediction.confidence
                );
                self.record_fast_path_trace(expr, "speculative_skip", 0.0);
                return Ok(Some(expr.clone()));
            }
            OptRoute::LeftDeep => {
                debug!(
                    "Speculative route: LEFT_DEEP (conf={:.2})",
                    route_prediction.confidence
                );
                if crate::left_deep::can_use_left_deep(expr) {
                    let stats_provider = Arc::new(TableStatsProvider {
                        stats: Arc::clone(&self.table_stats),
                    });
                    let builder =
                        crate::left_deep::LeftDeepBuilder::new(stats_provider);
                    match builder.build(expr) {
                        Ok(optimized) => {
                            info!(
                                "Left-deep optimization completed in {:?}",
                                total_start.elapsed()
                            );
                            self.record_fast_path_trace(
                                expr,
                                "left_deep_success",
                                total_start.elapsed().as_secs_f64() * 1000.0,
                            );
                            self.insert_into_cache(fingerprint.as_ref(), &optimized);
                            return Ok(Some(optimized));
                        }
                        Err(e) => {
                            debug!(
                                "Left-deep failed ({}), falling back to EGraphLow", e
                            );
                            route_prediction.route = OptRoute::EGraphLow;
                            route_prediction.confidence = 0.7;
                        }
                    }
                } else {
                    debug!("Left-deep not eligible, falling back to EGraphLow");
                    route_prediction.route = OptRoute::EGraphLow;
                    route_prediction.confidence = 0.7;
                }
            }
            OptRoute::EGraphLow | OptRoute::EGraphMedium | OptRoute::EGraphHigh => {
                debug!(
                    "Speculative route: {:?} (conf={:.2}, predicted_iters={})",
                    route_prediction.route,
                    route_prediction.confidence,
                    route_prediction.predicted_iterations_needed,
                );
            }
        }
        Ok(None)
    }

    /// Attempt large-join optimization. Returns `Some(plan)` if the
    /// large-join optimizer handled it, `None` otherwise.
    fn try_large_join(
        &self,
        expr: &RelExpr,
        table_count: usize,
        fingerprint: &Option<QueryFingerprint>,
    ) -> Result<Option<RelExpr>, EGraphError> {
        if table_count < self.config.large_join_threshold {
            return Ok(None);
        }
        if let crate::large_join::LargeJoinStrategy::EGraph =
            &self.config.large_join_strategy
        {
            return Ok(None);
        }

        let cost_model: Arc<dyn ra_core::cost::CostModel> =
            Arc::new(ra_hardware::HardwareCostModel::new(self.hardware_profile()));
        let stats_provider = Arc::new(TableStatsProvider {
            stats: Arc::clone(&self.table_stats),
        });

        let large_optimizer = crate::large_join::LargeJoinOptimizer::new(
            self.config.large_join_strategy.clone(),
            cost_model,
            stats_provider,
        );

        let joins = crate::large_join::LargeJoinOptimizer::extract_joins(expr);
        if joins.is_empty() {
            return Ok(None);
        }
        let result = match large_optimizer.optimize(joins) {
            Ok(r) => r,
            Err(e) => {
                use tracing::info;
                info!(
                    "Large-join optimizer unavailable ({e}), falling back to e-graph"
                );
                return Ok(None);
            }
        };
        self.insert_into_cache(fingerprint.as_ref(), &result);
        Ok(Some(result))
    }

    /// Compute iteration limit and timeout for e-graph saturation.
    fn compute_egraph_limits(
        &self,
        route_prediction: &crate::speculative_router::RoutePrediction,
        table_count: usize,
    ) -> (usize, u64) {
        let base_iter_limit = match route_prediction.route {
            OptRoute::EGraphLow | OptRoute::EGraphMedium | OptRoute::EGraphHigh
                if route_prediction.confidence >= 0.5 =>
            {
                route_prediction.route.iter_limit()
            }
            _ => {
                if self.config.use_adaptive_limits {
                    default_iter_limit_for_tables(table_count)
                } else {
                    self.config.iter_limit
                }
            }
        };

        let iter_limit =
            match self.resource_budget.as_ref().and_then(|b| b.max_iterations) {
                Some(budget_cap) => base_iter_limit.min(budget_cap),
                None => base_iter_limit,
            };

        let timeout_ms = match route_prediction.route {
            OptRoute::EGraphLow | OptRoute::EGraphMedium | OptRoute::EGraphHigh
                if route_prediction.confidence >= 0.5 =>
            {
                route_prediction.route.timeout_ms()
            }
            _ => {
                if self.config.use_adaptive_limits {
                    default_timeout_ms_for_tables(table_count)
                } else {
                    self.config.time_limit_secs * 1000
                }
            }
        };

        (iter_limit, timeout_ms)
    }

    /// Run the e-graph equality saturation loop.
    fn run_egraph_saturation(
        &self,
        expr: &RelExpr,
        opt_features: &OptimizationFeatures,
        route_prediction: &crate::speculative_router::RoutePrediction,
        iter_limit: usize,
        timeout_ms: u64,
        table_count: usize,
    ) -> Result<SaturationResult, EGraphError> {
        use std::time::Instant;
        use tracing::debug;

        let stats_cache =
            crate::stats_cache::StatsCache::from_arc(Arc::clone(&self.table_stats));

        // Pre-optimization: decorrelate subqueries
        let decorrelated;
        let effective_expr =
            if crate::subquery_decorrelation::tree_contains_subquery(expr) {
                debug!("Decorrelating subqueries before e-graph conversion");
                decorrelated = crate::subquery_decorrelation::decorrelate(expr);
                &decorrelated
            } else {
                expr
            };

        // Post-decorrelation budget adjustment.
        // When decorrelation converts EXISTS/IN subqueries into semi-joins,
        // the subquery-driven routing (EGraphMedium = 8 iters) is no longer
        // appropriate. Recompute the budget based on the actual table count
        // of the decorrelated expression.
        let (effective_iter_limit, effective_table_count) = {
            let post_table_count =
                crate::large_join::LargeJoinOptimizer::count_tables(
                    effective_expr,
                );
            let table_based_limit =
                default_iter_limit_for_tables(post_table_count);
            if !std::ptr::eq(effective_expr, expr)
                && table_based_limit < iter_limit
            {
                debug!(
                    "Post-decorrelation budget adjustment: \
                     tables {}->{}, iters {}->{}",
                    table_count, post_table_count, iter_limit, table_based_limit
                );
                (table_based_limit, post_table_count)
            } else {
                (iter_limit, table_count)
            }
        };

        let rec_expr = to_rec_expr(effective_expr)?;
        let runner_start = Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);
        let rules = self.load_rules(expr);

        let convergence_behavior = self
            .resource_budget
            .as_ref()
            .map_or(ConvergenceBehavior::Adaptive, |b| b.convergence);

        let mut egraph: EGraph<RelLang, RelAnalysis> = EGraph::default();
        let root = egraph.add_expr(&rec_expr);

        let mut continuation_gate = match route_prediction.route {
            OptRoute::EGraphLow | OptRoute::EGraphMedium | OptRoute::EGraphHigh => {
                Some(ContinuationGate::new(
                    opt_features.clone(),
                    self.cost_model.clone(),
                ))
            }
            _ => None,
        };

        let (actual_iterations, termination_reason, egraph) =
            self.run_saturation_loop(
                egraph, root, &rules, effective_iter_limit, timeout,
                runner_start, convergence_behavior,
                effective_table_count, effective_expr,
                &mut continuation_gate,
            );

        let runner_elapsed = runner_start.elapsed();
        let egraph_nodes = egraph.total_size();

        Ok(SaturationResult {
            egraph,
            root,
            actual_iterations,
            termination_reason,
            runner_elapsed,
            egraph_nodes,
            continuation_gate,
            stats_cache,
        })
    }

    /// Core saturation iteration loop with all termination checks.
    /// Core saturation iteration loop with all termination checks.
    #[expect(clippy::too_many_arguments)]
    fn run_saturation_loop(
        &self,
        mut egraph: EGraph<RelLang, RelAnalysis>,
        root: Id,
        rules: &[Rewrite<RelLang, RelAnalysis>],
        iter_limit: usize,
        timeout: std::time::Duration,
        runner_start: std::time::Instant,
        convergence_behavior: ConvergenceBehavior,
        table_count: usize,
        expr: &RelExpr,
        continuation_gate: &mut Option<ContinuationGate>,
    ) -> (usize, &'static str, EGraph<RelLang, RelAnalysis>) {
        let mut ctx = self.build_loop_context(
            expr, continuation_gate, convergence_behavior,
        );

        let mut termination_reason: &'static str = "iteration_limit";
        let mut actual_iterations = 0;

        for iteration in 0..iter_limit {
            if runner_start.elapsed() >= timeout {
                termination_reason = "timeout";
                break;
            }

            let prev_classes = egraph.number_of_classes();

            let runner: Runner<RelLang, RelAnalysis> = Runner::default()
                .with_egraph(egraph)
                .with_node_limit(self.config.node_limit)
                .with_iter_limit(1)
                .with_time_limit(timeout.saturating_sub(runner_start.elapsed()))
                .run(rules);

            let stop_reason = runner.stop_reason.clone();
            egraph = runner.egraph;
            actual_iterations = iteration + 1;

            if let Some(reason) = Self::check_iteration_termination(
                &mut ctx, &egraph, root, iteration, actual_iterations,
                prev_classes, continuation_gate, convergence_behavior,
                table_count, stop_reason.as_ref(),
            ) {
                termination_reason = reason;
                break;
            }
        }

        Self::log_saturation_stats(
            &ctx, runner_start, actual_iterations,
            egraph.total_size(), egraph.number_of_classes(), termination_reason,
        );

        (actual_iterations, termination_reason, egraph)
    }

    /// Build the mutable context for the saturation loop.
    fn build_loop_context(
        &self,
        expr: &RelExpr,
        continuation_gate: &Option<ContinuationGate>,
        convergence_behavior: ConvergenceBehavior,
    ) -> LoopContext {
        use tracing::debug;

        let convergence_detector = crate::convergence::ConvergenceDetector::new(
            convergence_behavior.window_size(),
            convergence_behavior.min_growth_rate(),
        );

        let cost_pruner = if self.config.use_cost_pruning {
            Some(crate::cost_pruning::CostPruner::new(
                self.config.cost_pruning_threshold,
            ))
        } else {
            None
        };

        let beam_search_tracker = self
            .config
            .beam_search_config
            .as_ref()
            .map(|cfg| crate::beam_search::BeamSearchTracker::new(cfg.clone()));

        if self.config.use_join_graph_filtering {
            let join_graph = crate::join_graph::JoinGraph::from_expr(expr);
            let stats = join_graph.stats();
            if stats.table_count > 2 {
                debug!(
                    "Join graph: {} tables, {} edges, density={:.2}, \
                     estimated reduction={:.1}%",
                    stats.table_count,
                    stats.edge_count,
                    stats.density(),
                    stats.estimated_reduction_factor() * 100.0
                );
            }
        }

        let hardware_cached = if cost_pruner.is_some()
            || beam_search_tracker.is_some()
            || continuation_gate.is_some()
        {
            Some(self.hardware_profile())
        } else {
            None
        };

        LoopContext {
            convergence_detector,
            cost_pruner,
            beam_search_tracker,
            hardware_cached,
            best_cost: f64::INFINITY,
            cost_improvement_stalled: 0,
        }
    }

    /// Check all termination conditions for a single iteration.
    /// Returns `Some(reason)` if the loop should break.
    /// Check all termination conditions for one iteration.
    /// Returns `Some(reason)` if the loop should stop.
    fn check_iteration_termination(
        ctx: &mut LoopContext,
        egraph: &EGraph<RelLang, RelAnalysis>,
        root: Id,
        iteration: usize,
        actual_iterations: usize,
        prev_classes: usize,
        continuation_gate: &mut Option<ContinuationGate>,
        convergence_behavior: ConvergenceBehavior,
        table_count: usize,
        stop_reason: Option<&egg::StopReason>,
    ) -> Option<&'static str> {
        use tracing::debug;

        let curr_classes = egraph.number_of_classes();
        let unions = if iteration > 0 {
            prev_classes.saturating_sub(curr_classes)
        } else {
            curr_classes
        };
        ctx.convergence_detector.record(crate::convergence::IterationMetrics {
            iteration,
            unions,
            total_nodes: egraph.total_size(),
            total_classes: curr_classes,
        });

        let current_cost = ctx.hardware_cached.as_ref().map(|hardware| {
            let cost_fn = crate::extract::RelCostFn::new(hardware.clone());
            let extractor = egg::Extractor::new(egraph, cost_fn);
            extractor.find_best(root).0
        });

        if let Some(reason) = Self::check_cost_trackers(
            ctx, current_cost, root, iteration,
        ) {
            return Some(reason);
        }

        if let Some(reason) = Self::check_continuation(
            continuation_gate, current_cost, iteration, egraph.total_size(),
        ) {
            return Some(reason);
        }

        if ctx.convergence_detector.should_terminate()
            == crate::convergence::TerminationDecision::Converged
        {
            debug!(
                "Early termination: converged at iteration {} (stats: {:?})",
                iteration, ctx.convergence_detector.stats()
            );
            return Some("converged");
        }

        if let Some(reason) = Self::check_convergence_policy(
            convergence_behavior, actual_iterations, table_count,
        ) {
            return Some(reason);
        }

        if stop_reason.is_some_and(|r| matches!(r, egg::StopReason::Saturated)) {
            return Some("saturated");
        }

        None
    }

    /// Check cost pruning and beam search trackers.
    fn check_cost_trackers(
        ctx: &mut LoopContext,
        current_cost: Option<f64>,
        root: Id,
        iteration: usize,
    ) -> Option<&'static str> {
        use tracing::debug;

        if let Some(pruner) = ctx.cost_pruner.as_mut() {
            if let Some(cost) = current_cost {
                pruner.record_cost(root, cost);
                let threshold = 0.01;
                if cost < ctx.best_cost * (1.0 - threshold) {
                    ctx.best_cost = cost;
                    ctx.cost_improvement_stalled = 0;
                } else {
                    ctx.cost_improvement_stalled += 1;
                    if ctx.cost_improvement_stalled >= 3 {
                        debug!(
                            "Early termination: cost stagnant for 3 \
                             iterations (best: {:.2})",
                            ctx.best_cost
                        );
                        return Some("cost_stagnant");
                    }
                }
            }
        }

        if let Some(tracker) = ctx.beam_search_tracker.as_mut() {
            if let Some(cost) = current_cost {
                tracker.start_iteration(iteration);
                tracker.record_plan(root, cost);
                let pruned = tracker.prune();
                if pruned > 0 {
                    debug!(
                        "Beam search: pruned {} plans at iteration {} \
                         (kept top {})",
                        pruned, iteration, tracker.stats().plans_kept
                    );
                }
            }
        }

        None
    }

    /// Check the continuation gate for speculative early stopping.
    fn check_continuation(
        gate: &mut Option<ContinuationGate>,
        current_cost: Option<f64>,
        iteration: usize,
        total_size: usize,
    ) -> Option<&'static str> {
        use tracing::debug;

        let gate = gate.as_mut()?;
        let cost = current_cost?;
        match gate.should_continue(iteration, cost, total_size) {
            ContinuationDecision::StopCostStagnant => {
                debug!(
                    "Continuation gate: cost stagnant at iteration {}",
                    iteration
                );
                Some("speculative_cost_stagnant")
            }
            ContinuationDecision::StopModelPrediction => {
                debug!(
                    "Continuation gate: model predicts no improvement at iter {}",
                    iteration
                );
                Some("speculative_model_stop")
            }
            ContinuationDecision::Continue => None,
        }
    }

    /// Check convergence behavior policy for early termination.
    fn check_convergence_policy(
        behavior: ConvergenceBehavior,
        actual_iterations: usize,
        table_count: usize,
    ) -> Option<&'static str> {
        use tracing::debug;

        match behavior {
            ConvergenceBehavior::Immediate => {
                debug!(
                    "Immediate convergence: stopping after {} iteration(s)",
                    actual_iterations,
                );
                Some("convergence_immediate")
            }
            ConvergenceBehavior::Adaptive if actual_iterations >= 2 => {
                if is_simple_query(table_count) {
                    debug!(
                        "Adaptive convergence: simple query, \
                         stopping after {} iterations",
                        actual_iterations,
                    );
                    Some("convergence_adaptive_simple")
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Log pruning and saturation statistics after loop completion.
    fn log_saturation_stats(
        ctx: &LoopContext,
        runner_start: std::time::Instant,
        actual_iterations: usize,
        egraph_nodes: usize,
        egraph_classes: usize,
        termination_reason: &str,
    ) {
        use tracing::{debug, info};

        if let Some(pruner) = ctx.cost_pruner.as_ref() {
            let stats = pruner.stats();
            if stats.classes_evaluated > 0 {
                debug!(
                    "Cost pruning: best_cost={:.2}, evaluated={}, pruned={} \
                     ({:.1}% pruning rate)",
                    stats.global_best_cost.unwrap_or(f64::INFINITY),
                    stats.classes_evaluated,
                    stats.classes_pruned,
                    stats.pruning_rate()
                );
            }
        }
        if let Some(tracker) = ctx.beam_search_tracker.as_ref() {
            let stats = tracker.stats();
            if stats.is_active() && stats.plans_pruned > 0 {
                info!(
                    "Beam search: beam_width={}, total_plans={}, kept={}, \
                     pruned={} ({:.1}% reduction)",
                    stats.beam_width,
                    stats.total_plans,
                    stats.plans_kept,
                    stats.plans_pruned,
                    stats.pruning_rate()
                );
            }
        }

        info!(
            "E-graph saturation: {:?} ({} iterations, {} nodes, {} classes, \
             reason: {})",
            runner_start.elapsed(), actual_iterations, egraph_nodes,
            egraph_classes, termination_reason
        );
    }

    /// Record training trace for an e-graph optimization run.
    fn record_egraph_training_trace(
        &self,
        expr: &RelExpr,
        actual_iterations: usize,
        termination_reason: &str,
        egraph_nodes: usize,
        runner_elapsed: std::time::Duration,
        continuation_gate: Option<&ContinuationGate>,
    ) {
        let coordinator = match self.training_coordinator.as_ref() {
            Some(c) => c,
            None => return,
        };

        let cost_history: Vec<f64> = continuation_gate
            .map(|g| g.cost_history().to_vec())
            .unwrap_or_default();

        let final_improvement_pct = if cost_history.len() >= 2 {
            let first = cost_history[0];
            let last = cost_history[cost_history.len() - 1];
            if first > 0.0 { ((first - last) / first) * 100.0 } else { 0.0 }
        } else {
            0.0
        };

        let trace = OptimizationTrace {
            features: crate::cost_model::extract_features(expr),
            iterations_run: actual_iterations,
            cost_per_iteration: cost_history.clone(),
            termination_reason: termination_reason.to_string(),
            final_improvement_pct,
            optimal_stop_point: OptimizationTrace::compute_optimal_stop(
                &cost_history,
            ),
            egraph_nodes_final: egraph_nodes,
            optimization_time_ms: runner_elapsed.as_secs_f64() * 1000.0,
        };

        if let Ok(mut coord) = coordinator.lock() {
            coord.record_trace(trace);
        }
    }

    /// Insert a plan into the cache if caching is enabled.
    fn insert_into_cache(&self, fingerprint: Option<&QueryFingerprint>, plan: &RelExpr) {
        if let Some(fp) = fingerprint {
            if let Some(ref mutex) = self.plan_cache {
                let mut cache = mutex
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                cache.insert(fp.clone(), plan.clone());
            }
        }
    }

    /// Record a zero-iteration trace for fast-path queries
    /// (trivial/left-deep/skip).
    ///
    /// Feeds the training coordinator so the model learns which queries
    /// do NOT need e-graph optimization.
    fn record_fast_path_trace(&self, expr: &RelExpr, reason: &str, time_ms: f64) {
        if let Some(ref coordinator) = self.training_coordinator {
            let trace = OptimizationTrace {
                features: crate::cost_model::extract_features(expr),
                iterations_run: 0,
                cost_per_iteration: Vec::new(),
                termination_reason: reason.to_string(),
                final_improvement_pct: 0.0,
                optimal_stop_point: 0,
                egraph_nodes_final: 0,
                optimization_time_ms: time_ms,
            };
            if let Ok(mut coord) = coordinator.lock() {
                coord.record_trace(trace);
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
        _facts: &dyn ra_core::FactsProvider,
    ) -> Result<RelExpr, EGraphError> {
        // Delegate to the main optimize() pipeline which includes all fast
        // paths: trivial query detection, plan cache, speculative routing,
        // left-deep construction, and continuation gate.
        self.optimize(expr)
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
        let result = self.extract_with_hybrid_fallback(&runner.egraph, root, &*self.table_stats, &hardware)?;
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
    #[expect(clippy::too_many_lines, reason = "bounded optimization with convergence control")]
    pub fn optimize_bounded(&self, expr: &RelExpr) -> Result<OptimizationResult, EGraphError> {
        use tracing::debug;

        let budget = self.resource_budget.clone().unwrap_or_default();
        let convergence_behavior = budget.convergence;
        let mut tracker = ResourceTracker::start(budget);

        // Pre-optimization: decorrelate subqueries before e-graph conversion
        let decorrelated;
        let effective_expr =
            if crate::subquery_decorrelation::tree_contains_subquery(expr) {
                debug!("Decorrelating subqueries before e-graph conversion");
                decorrelated = crate::subquery_decorrelation::decorrelate(expr);
                &decorrelated
            } else {
                expr
            };

        let rec_expr = to_rec_expr(effective_expr)?;
        let hardware = self.hardware_profile();
        let rules = self.load_rules(effective_expr);

        let iter_limit = self.config.iter_limit;
        let node_limit = self.config.node_limit;
        let time_limit_secs = self.config.time_limit_secs;

        // Table count for Adaptive convergence decisions
        let table_count =
            crate::large_join::LargeJoinOptimizer::count_tables(effective_expr);

        let mut egraph: EGraph<RelLang, RelAnalysis> = EGraph::default();
        let root = egraph.add_expr(&rec_expr);

        let mut best_plan: Option<RelExpr> = None;
        let mut best_cost = f64::INFINITY;

        // Configure convergence detector from the budget
        let mut convergence_detector = crate::convergence::ConvergenceDetector::new(
            convergence_behavior.window_size(),
            convergence_behavior.min_growth_rate(),
        );

        // Extract initial plan (the original, unoptimized)
        if let Ok(plan) = self.extract_with_hybrid_fallback(&egraph, root, &*self.table_stats, &hardware) {
            best_plan = Some(plan);
            best_cost = estimate_plan_cost(&egraph, root, &hardware);
        }

        for iteration in 0..iter_limit {
            // Check budget before running an iteration
            let check = tracker.check();
            if !check.is_within_budget() {
                return handle_overflow(&tracker, expr, best_plan, best_cost);
            }

            let prev_classes = egraph.number_of_classes();

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

            // Record convergence metrics
            let curr_classes = egraph.number_of_classes();
            let unions = if iteration > 0 {
                prev_classes.saturating_sub(curr_classes)
            } else {
                curr_classes
            };
            convergence_detector.record(crate::convergence::IterationMetrics {
                iteration,
                unions,
                total_nodes: egraph.total_size(),
                total_classes: curr_classes,
            });

            // Try to extract the best plan from the current e-graph
            if let Ok(plan) = self.extract_with_hybrid_fallback(&egraph, root, &*self.table_stats, &hardware) {
                let cost = estimate_plan_cost(&egraph, root, &hardware);
                if cost < best_cost {
                    best_cost = cost;
                    best_plan = Some(plan);
                }
            }

            // Check convergence detector
            if convergence_detector.should_terminate()
                == crate::convergence::TerminationDecision::Converged
            {
                break;
            }

            // Convergence-behavior-aware early termination.
            // Same logic as optimize(): stop early for Immediate
            // and Adaptive-simple without waiting for the detector
            // to fill its full window.
            let iterations_done = iteration + 1;
            match convergence_behavior {
                ConvergenceBehavior::Immediate => {
                    debug!(
                        "Immediate convergence: stopping after {} iteration(s)",
                        iterations_done,
                    );
                    break;
                }
                ConvergenceBehavior::Adaptive if iterations_done >= 2
                    && is_simple_query(table_count) => {
                        debug!(
                            "Adaptive convergence: simple query, \
                             stopping after {} iterations",
                            iterations_done,
                        );
                        break;
                    }
                _ => {}
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
    #[expect(clippy::too_many_lines, reason = "optimization pipeline with verbose tracing")]
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
        if let Ok(plan) = self.extract_with_hybrid_fallback(&egraph, root, &*self.table_stats, &hardware) {
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
                    self.extract_with_hybrid_fallback(&egraph, root, &*self.table_stats, &hardware).ok()
                } else {
                    None
                };

                // Run this single rule once
                let runner: Runner<RelLang, RelAnalysis> = Runner::default()
                    .with_egraph(egraph)
                    .with_node_limit(node_limit)
                    .with_iter_limit(1)
                    .run(std::slice::from_ref(rule));

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
                    if let Ok(plan) = self.extract_with_hybrid_fallback(&egraph, root, &*self.table_stats, &hardware) {
                        if cost_after < best_cost {
                            best_cost = cost_after;
                            best_plan = Some(plan.clone());
                        }

                        // Capture intermediate step if verbose
                        if verbose {
                            if let Some(before) = plan_before {
                                step_number += 1;
                                let reason = if let Some(improvement) = cost_improvement {
                                    format!("Cost improvement: {improvement:.4}")
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
            #[expect(
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
        let result = self.extract_with_hybrid_fallback(&runner.egraph, root, &*self.table_stats, &hardware)?;

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
        let stats_map = Arc::make_mut(&mut self.table_stats);

        for delta in delta_set {
            match delta {
                ra_stats::delta::StatisticsDelta::TableRowCount { table, new, .. } => {
                    let stats = stats_map
                        .entry(table.clone())
                        .or_insert_with(|| ra_core::statistics::Statistics::new(*new as f64));
                    stats.row_count = *new as f64;
                    updated_tables.insert(table.clone());
                }
                ra_stats::delta::StatisticsDelta::ColumnNDV {
                    table, column, new, ..
                } => {
                    if let Some(stats) = stats_map.get_mut(table) {
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
                    if let Some(stats) = stats_map.get_mut(table) {
                        if let Some(col) = stats.columns.get_mut(column) {
                            col.null_fraction = *new;
                            updated_tables.insert(table.clone());
                        }
                    }
                }
                ra_stats::delta::StatisticsDelta::TableAdded { table, row_count } => {
                    stats_map.insert(
                        table.clone(),
                        ra_core::statistics::Statistics::new(*row_count as f64),
                    );
                    updated_tables.insert(table.clone());
                }
                ra_stats::delta::StatisticsDelta::TableRemoved { table, .. } => {
                    stats_map.remove(table);
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

impl Default for Optimizer {
    fn default() -> Self {
        Self::new()
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

/// Statistics provider for table statistics.
#[derive(Debug, Clone)]
struct TableStatsProvider {
    stats: Arc<HashMap<String, ra_core::statistics::Statistics>>,
}

impl ra_core::cost::StatisticsProvider for TableStatsProvider {
    fn get_statistics(&self, table: &str) -> Option<&ra_core::statistics::Statistics> {
        self.stats.get(table)
    }
}
