use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egg::{EGraph, Id, Rewrite, Runner};
use ra_core::algebra::RelExpr;
#[cfg(feature = "timeline")]
use ra_stats::delta::DeltaSet;
use tracing::warn;

use crate::analysis::RelAnalysis;
use crate::extract::extract_best;
use crate::genetic_fingerprint::QueryFingerprint;
use crate::plan_cache::{PlanCache, PlanCacheConfig, PlanCacheStats};
use crate::resource_budget::{
    ConvergenceBehavior, OverflowStrategy, ResourceBudget, ResourceTracker,
};
use crate::rewrite::all_rules;

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
    #[expect(clippy::too_many_lines, reason = "core optimization pipeline")]
    pub fn optimize(&self, expr: &RelExpr) -> Result<RelExpr, EGraphError> {
        use std::time::Instant;
        use tracing::{debug, info};

        let total_start = Instant::now();

        // Plan cache fast path: check if we have a cached plan for
        // a structurally equivalent query.
        let fingerprint = if self.plan_cache.is_some() {
            let fp = QueryFingerprint::from_rel_expr(expr);
            if let Some(ref mutex) = self.plan_cache {
                let mut cache = mutex
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
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
                    self.insert_into_cache(fingerprint.as_ref(), &optimized);
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
            if let crate::large_join::LargeJoinStrategy::EGraph = &self.config.large_join_strategy {
                // Continue with standard e-graph optimization
            } else {
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
                    self.insert_into_cache(fingerprint.as_ref(), &result);
                    return Ok(result);
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

        let base_iter_limit = if self.config.use_adaptive_limits {
            complexity.default_iter_limit()
        } else {
            self.config.iter_limit
        };

        // Cap iteration limit by the resource budget when set.
        // This ensures the budget's max_iterations is respected
        // even in the non-bounded optimize() path.
        let iter_limit = match self.resource_budget.as_ref().and_then(|b| b.max_iterations) {
            Some(budget_cap) => base_iter_limit.min(budget_cap),
            None => base_iter_limit,
        };

        let timeout_ms = if self.config.use_adaptive_limits {
            complexity.default_timeout_ms()
        } else {
            self.config.time_limit_secs * 1000
        };

        // Fast path: Trivial single-table queries with no joins need no optimization.
        // Skip e-graph construction entirely to avoid the ~20ms overhead on simple queries
        // like "SELECT * FROM orders" or "SELECT COUNT(*) FROM users WHERE status = 'active'".
        if matches!(complexity, crate::query_complexity::QueryComplexity::Trivial)
            && crate::query_complexity::count_joins(expr) == 0
        {
            debug!("Trivial single-table query: skipping e-graph optimization");
            self.insert_into_cache(fingerprint.as_ref(), expr);
            return Ok(expr.clone());
        }

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

        // Create convergence detector tuned to the budget's behavior.
        // When a resource budget is set, its ConvergenceBehavior controls
        // how aggressively we detect convergence. Without a budget we
        // fall back to the Adaptive defaults (window=3, growth=5%).
        let convergence_behavior = self
            .resource_budget
            .as_ref()
            .map_or(ConvergenceBehavior::Adaptive, |b| b.convergence);
        let mut convergence_detector = crate::convergence::ConvergenceDetector::new(
            convergence_behavior.window_size(),
            convergence_behavior.min_growth_rate(),
        );

        // Create cost pruner (if enabled)
        let mut cost_pruner = if self.config.use_cost_pruning {
            Some(crate::cost_pruning::CostPruner::new(
                self.config.cost_pruning_threshold,
            ))
        } else {
            None
        };

        // Create beam search tracker (if enabled)
        let mut beam_search_tracker = self
            .config
            .beam_search_config
            .as_ref()
            .map(|beam_config| crate::beam_search::BeamSearchTracker::new(beam_config.clone()));

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

            // Check for convergence (detector-based)
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

            // Convergence-behavior-aware early termination.
            //
            // The detector needs `window_size` data points before
            // it can declare convergence. For Immediate and Adaptive
            // modes we apply policy-level checks that don't depend
            // on filling the full window:
            //
            //   Immediate  -- stop after 1 iteration (OLTP fast path)
            //   Adaptive   -- stop after 2 iterations for simple
            //                  queries (Trivial/Simple complexity)
            //   Thorough / Complete -- rely on the detector above
            match convergence_behavior {
                ConvergenceBehavior::Immediate => {
                    termination_reason = "convergence_immediate";
                    debug!(
                        "Immediate convergence: stopping after {} iteration(s)",
                        actual_iterations,
                    );
                    break;
                }
                ConvergenceBehavior::Adaptive if actual_iterations >= 2 => {
                    let is_simple = matches!(
                        complexity,
                        crate::query_complexity::QueryComplexity::Trivial
                            | crate::query_complexity::QueryComplexity::Simple
                    );
                    if is_simple {
                        termination_reason = "convergence_adaptive_simple";
                        debug!(
                            "Adaptive convergence: simple query, \
                             stopping after {} iterations",
                            actual_iterations,
                        );
                        break;
                    }
                }
                _ => {}
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

        self.insert_into_cache(fingerprint.as_ref(), &result);
        Ok(result)
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
    #[expect(clippy::too_many_lines, reason = "bounded optimization with convergence control")]
    pub fn optimize_bounded(&self, expr: &RelExpr) -> Result<OptimizationResult, EGraphError> {
        use tracing::debug;

        let budget = self.resource_budget.clone().unwrap_or_default();
        let convergence_behavior = budget.convergence;
        let mut tracker = ResourceTracker::start(budget);

        let rec_expr = to_rec_expr(expr)?;
        let hardware = self.hardware_profile();
        let rules = self.load_rules(expr);

        let iter_limit = self.config.iter_limit;
        let node_limit = self.config.node_limit;
        let time_limit_secs = self.config.time_limit_secs;

        // Compute query complexity for Adaptive convergence decisions
        let complexity = if self.config.use_adaptive_limits {
            crate::query_complexity::QueryComplexity::from_expr(expr)
        } else {
            let table_count = crate::large_join::LargeJoinOptimizer::count_tables(expr);
            match table_count {
                0..=1 => crate::query_complexity::QueryComplexity::Trivial,
                2..=4 => crate::query_complexity::QueryComplexity::Simple,
                5..=7 => crate::query_complexity::QueryComplexity::Medium,
                8..=9 => crate::query_complexity::QueryComplexity::Complex,
                _ => crate::query_complexity::QueryComplexity::VeryComplex,
            }
        };

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
        if let Ok(plan) = extract_best(&egraph, root, &self.table_stats, &hardware) {
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
            if let Ok(plan) = extract_best(&egraph, root, &self.table_stats, &hardware) {
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
                ConvergenceBehavior::Adaptive if iterations_done >= 2 => {
                    let is_simple = matches!(
                        complexity,
                        crate::query_complexity::QueryComplexity::Trivial
                            | crate::query_complexity::QueryComplexity::Simple
                    );
                    if is_simple {
                        debug!(
                            "Adaptive convergence: simple query, \
                             stopping after {} iterations",
                            iterations_done,
                        );
                        break;
                    }
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
    stats: HashMap<String, ra_core::statistics::Statistics>,
}

impl ra_core::cost::StatisticsProvider for TableStatsProvider {
    fn get_statistics(&self, table: &str) -> Option<&ra_core::statistics::Statistics> {
        self.stats.get(table)
    }
}
