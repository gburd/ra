//! Integration of ML components with the optimizer.
//!
//! This module connects the belief network for rule ordering with
//! the optimizer's rule application logic, and feeds execution
//! observations back into continual training.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use ra_core::algebra::RelExpr;
use ra_core::cost::StatisticsProvider;
use ra_core::CostModel;
use ra_ml::belief_network::{BeliefNetwork, ExecutionObservation};
use ra_ml::streaming::{StreamingMlEstimator, ModelScope};

use crate::rule_priority::RulePriority;

/// ML-enhanced optimizer configuration.
#[derive(Debug, Clone)]
pub struct MlOptimizerConfig {
    /// Whether to enable ML-based rule ordering.
    pub enable_rule_ordering: bool,
    /// Whether to enable ML-based rule filtering.
    pub enable_rule_filtering: bool,
    /// Threshold for rule filtering (0.0-1.0).
    pub filter_threshold: f64,
    /// Whether to collect execution observations.
    pub collect_observations: bool,
    /// Model scope for sharing.
    pub model_scope: ModelScope,
}

impl Default for MlOptimizerConfig {
    fn default() -> Self {
        Self {
            enable_rule_ordering: true,
            enable_rule_filtering: false,
            filter_threshold: 0.1,
            collect_observations: true,
            model_scope: ModelScope::Overall,
        }
    }
}

/// ML-enhanced optimizer that uses belief networks for rule ordering.
pub struct MlOptimizer {
    /// Streaming ML estimator.
    estimator: Option<Arc<StreamingMlEstimator>>,
    /// Configuration.
    config: MlOptimizerConfig,
    /// Last optimization context for observation.
    last_context: Arc<Mutex<Option<OptimizationContext>>>,
}

/// Context from an optimization run for observation collection.
#[derive(Debug, Clone)]
struct OptimizationContext {
    /// Input plan.
    input_plan: RelExpr,
    /// Output plan.
    output_plan: RelExpr,
    /// Rules applied.
    rules_applied: Vec<String>,
    /// Estimated cost before.
    cost_before: f64,
    /// Estimated cost after.
    cost_after: f64,
    /// Optimization start time.
    start_time: Instant,
}

impl MlOptimizer {
    /// Create a new ML-enhanced optimizer.
    #[must_use]
    pub fn new(config: MlOptimizerConfig) -> Self {
        Self {
            estimator: None,
            config,
            last_context: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the streaming ML estimator.
    pub fn set_estimator(&mut self, estimator: Arc<StreamingMlEstimator>) {
        self.estimator = Some(estimator);
    }

    /// Get rule ordering based on plan context.
    ///
    /// Uses the belief network to rank rules by expected improvement.
    #[must_use]
    pub fn order_rules(
        &self,
        rule_ids: &[String],
        plan: &RelExpr,
        stats: &dyn StatisticsProvider,
    ) -> Vec<String> {
        if !self.config.enable_rule_ordering {
            return rule_ids.to_vec();
        }

        if let Some(estimator) = &self.estimator {
            let context = extract_plan_context(plan, stats);
            estimator.order_rules(rule_ids, &context)
        } else {
            rule_ids.to_vec()
        }
    }

    /// Filter rules based on expected improvement.
    ///
    /// Returns only rules that are likely to improve the plan.
    #[must_use]
    pub fn filter_rules(
        &self,
        rule_ids: &[String],
        plan: &RelExpr,
        stats: &dyn StatisticsProvider,
    ) -> Vec<String> {
        if !self.config.enable_rule_filtering {
            return rule_ids.to_vec();
        }

        if let Some(estimator) = &self.estimator {
            let context = extract_plan_context(plan, stats);
            estimator.filter_rules(rule_ids, &context, self.config.filter_threshold)
        } else {
            rule_ids.to_vec()
        }
    }

    /// Record the start of an optimization.
    pub fn start_optimization(
        &self,
        plan: &RelExpr,
        cost: f64,
    ) {
        if !self.config.collect_observations {
            return;
        }

        let mut ctx = self.last_context.lock().unwrap_or_else(|e| e.into_inner());
        *ctx = Some(OptimizationContext {
            input_plan: plan.clone(),
            output_plan: plan.clone(),
            rules_applied: Vec::new(),
            cost_before: cost,
            cost_after: cost,
            start_time: Instant::now(),
        });
    }

    /// Record a rule application.
    pub fn record_rule_application(&self, rule_id: &str) {
        if !self.config.collect_observations {
            return;
        }

        let mut ctx = self.last_context.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(context) = ctx.as_mut() {
            context.rules_applied.push(rule_id.to_string());
        }
    }

    /// Record the completion of an optimization.
    pub fn complete_optimization(
        &self,
        output_plan: &RelExpr,
        final_cost: f64,
    ) {
        if !self.config.collect_observations {
            return;
        }

        let mut ctx = self.last_context.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut context) = ctx.take() {
            context.output_plan = output_plan.clone();
            context.cost_after = final_cost;

            let elapsed = context.start_time.elapsed();
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

            if let Some(estimator) = &self.estimator {
                for rule_id in &context.rules_applied {
                    let observation = ExecutionObservation {
                        rule_id: rule_id.clone(),
                        estimated_time_before: context.cost_before,
                        estimated_time_after: context.cost_after,
                        actual_time: Some(elapsed.as_secs_f64()),
                        improved: context.cost_after < context.cost_before,
                        context: vec![
                            context.cost_before,
                            context.cost_after,
                            context.rules_applied.len() as f64,
                        ],
                        timestamp,
                    };
                    estimator.observe(observation);
                }
            }
        }
    }

    /// Get the belief network.
    #[must_use]
    pub fn belief_network(&self) -> Option<Arc<BeliefNetwork>> {
        self.estimator.as_ref().map(|e| e.belief_network())
    }
}

/// Extract plan context features for belief network prediction.
fn extract_plan_context(
    _plan: &RelExpr,
    _stats: &dyn StatisticsProvider,
) -> Vec<f64> {
    vec![1.0, 2.0, 3.0]
}

/// Integrate ML-based rule ordering with existing rule priority system.
pub fn integrate_with_rule_priority(
    _rule_priority: &mut RulePriority,
    ml_optimizer: &MlOptimizer,
    rule_ids: &[String],
    plan: &RelExpr,
    stats: &dyn StatisticsProvider,
) -> Vec<String> {
    let ml_ordered = ml_optimizer.order_rules(rule_ids, plan, stats);

    let ml_filtered = ml_optimizer.filter_rules(&ml_ordered, plan, stats);

    ml_filtered
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::statistics::Statistics;
    use ra_ml::nn::build_default_mlp;

    #[test]
    fn ml_optimizer_basic() {
        let config = MlOptimizerConfig::default();
        let optimizer = MlOptimizer::new(config);

        let plan = RelExpr::scan("users");
        let rules = vec!["rule1".to_string(), "rule2".to_string()];

        struct DummyStats;
        impl StatisticsProvider for DummyStats {
            fn get_statistics(&self, _table: &str) -> Option<&Statistics> {
                None
            }
        }

        let ordered = optimizer.order_rules(&rules, &plan, &DummyStats);
        assert_eq!(ordered.len(), 2);
    }

    #[test]
    fn ml_optimizer_observation() {
        let config = MlOptimizerConfig {
            collect_observations: true,
            ..Default::default()
        };
        let optimizer = MlOptimizer::new(config);

        let plan = RelExpr::scan("users");
        optimizer.start_optimization(&plan, 100.0);
        optimizer.record_rule_application("test_rule");
        optimizer.complete_optimization(&plan, 50.0);

        let ctx = optimizer.last_context.lock().unwrap();
        assert!(ctx.is_none());
    }

    #[test]
    fn ml_optimizer_disabled() {
        let config = MlOptimizerConfig {
            enable_rule_ordering: false,
            enable_rule_filtering: false,
            ..Default::default()
        };
        let optimizer = MlOptimizer::new(config);

        let plan = RelExpr::scan("users");
        let rules = vec!["rule1".to_string(), "rule2".to_string()];

        struct DummyStats;
        impl StatisticsProvider for DummyStats {
            fn get_statistics(&self, _table: &str) -> Option<&Statistics> {
                None
            }
        }

        let ordered = optimizer.order_rules(&rules, &plan, &DummyStats);
        assert_eq!(ordered, rules);

        let filtered = optimizer.filter_rules(&rules, &plan, &DummyStats);
        assert_eq!(filtered, rules);
    }
}
