//! End-to-end discovery pipeline.
//!
//! Orchestrates the full cycle: collect logs → mine patterns →
//! synthesize candidate rules → validate → output accepted rules.
//! The pipeline can run iteratively, discovering new rules as more
//! execution data accumulates.

use tracing::{info, warn};

use crate::fingerprint::Fingerprint;
use crate::log::LogStore;
use crate::mining::{discover_pattern_pairs, mine_frequent_patterns, MiningConfig, PatternPair};
use crate::synthesis::{synthesize_rules, CandidateRule, SynthesisConfig};
use crate::validation::{validate_rules, CostEstimator, ValidationConfig, ValidationResult};

/// Configuration for the full discovery pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Mining configuration.
    pub mining: MiningConfig,
    /// Synthesis configuration.
    pub synthesis: SynthesisConfig,
    /// Validation configuration.
    pub validation: ValidationConfig,
    /// Fraction of logs to use for training (rest for validation).
    pub train_fraction: f64,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            mining: MiningConfig::default(),
            synthesis: SynthesisConfig::default(),
            validation: ValidationConfig::default(),
            train_fraction: 0.8,
        }
    }
}

/// Output of a single discovery run.
#[derive(Debug)]
pub struct DiscoveryOutput {
    /// Candidate rules that passed validation.
    pub accepted_rules: Vec<(CandidateRule, ValidationResult)>,
    /// All candidate rules (including rejected ones).
    pub all_candidates: Vec<CandidateRule>,
    /// Pattern pairs discovered during mining.
    pub pattern_pairs: Vec<PatternPair>,
    /// Number of training logs used.
    pub training_log_count: usize,
    /// Number of validation logs used.
    pub validation_log_count: usize,
}

/// Run the full discovery pipeline on the given log store.
///
/// Splits logs into training and validation sets, mines patterns
/// from the training set, synthesizes candidate rules, and
/// validates them against the held-out set.
#[must_use]
pub fn run_discovery(
    log_store: &LogStore,
    cost_estimator: &CostEstimator,
    config: &PipelineConfig,
) -> DiscoveryOutput {
    let (train_logs, val_logs) = log_store.split(config.train_fraction);

    info!(
        train = train_logs.len(),
        val = val_logs.len(),
        "splitting logs for discovery"
    );

    let original_fps: Vec<Fingerprint> = train_logs
        .iter()
        .map(|log| Fingerprint::of(&log.original_plan))
        .collect();
    let optimized_fps: Vec<Fingerprint> = train_logs
        .iter()
        .map(|log| Fingerprint::of(&log.optimized_plan))
        .collect();

    let freq = mine_frequent_patterns(&original_fps, &config.mining);
    info!(count = freq.len(), "mined frequent patterns");

    let pattern_pairs = discover_pattern_pairs(&original_fps, &optimized_fps, &config.mining);
    info!(count = pattern_pairs.len(), "discovered pattern pairs");

    let all_candidates = synthesize_rules(&pattern_pairs, &config.synthesis);
    info!(count = all_candidates.len(), "synthesized candidate rules");

    let accepted_rules = validate_rules(
        &all_candidates,
        val_logs,
        cost_estimator,
        &config.validation,
    );

    if accepted_rules.is_empty() {
        warn!("no candidate rules passed validation");
    } else {
        info!(count = accepted_rules.len(), "rules passed validation");
    }

    DiscoveryOutput {
        accepted_rules,
        all_candidates,
        pattern_pairs,
        training_log_count: train_logs.len(),
        validation_log_count: val_logs.len(),
    }
}

/// Incrementally run discovery with new logs appended to the store.
///
/// Designed for continuous learning: as the system executes more
/// queries, new patterns may emerge.
#[must_use]
pub fn run_incremental_discovery(
    log_store: &LogStore,
    cost_estimator: &CostEstimator,
    config: &PipelineConfig,
    min_new_logs: usize,
) -> Option<DiscoveryOutput> {
    if log_store.len() < min_new_logs {
        info!(
            current = log_store.len(),
            required = min_new_logs,
            "not enough logs for incremental discovery"
        );
        return None;
    }

    Some(run_discovery(log_store, cost_estimator, config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::ExecutionLog;
    use ra_core::algebra::RelExpr;
    use ra_core::cost::Cost;
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
    use std::collections::HashMap;
    use std::time::Duration;

    fn make_log_entry(original: RelExpr, optimized: RelExpr) -> ExecutionLog {
        ExecutionLog {
            id: 0,
            original_plan: original,
            optimized_plan: optimized,
            estimated_cost: Cost::new(10.0, 5.0, 0.0, 1024),
            execution_time: Duration::from_millis(50),
            actual_cardinalities: HashMap::new(),
            estimated_cardinalities: HashMap::new(),
            tags: vec![],
        }
    }

    fn build_log_store() -> LogStore {
        let mut store = LogStore::new();

        for _ in 0..20 {
            let original = RelExpr::scan("orders").filter(Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("status"))),
                right: Box::new(Expr::Const(Const::String("active".into()))),
            });
            let optimized = RelExpr::scan("orders");
            store.record(make_log_entry(original, optimized));
        }

        store
    }

    fn simple_cost_estimator() -> CostEstimator {
        CostEstimator::new(|plan| match plan {
            RelExpr::Scan { .. } => Cost::new(1.0, 1.0, 0.0, 100),
            RelExpr::Filter { .. } => Cost::new(5.0, 3.0, 0.0, 200),
            _ => Cost::new(10.0, 10.0, 0.0, 500),
        })
    }

    #[test]
    fn pipeline_runs_without_panic() {
        let store = build_log_store();
        let estimator = simple_cost_estimator();
        let config = PipelineConfig::default();

        let output = run_discovery(&store, &estimator, &config);

        assert_eq!(output.training_log_count, 16);
        assert_eq!(output.validation_log_count, 4);
    }

    #[test]
    fn pipeline_empty_store() {
        let store = LogStore::new();
        let estimator = simple_cost_estimator();
        let config = PipelineConfig::default();

        let output = run_discovery(&store, &estimator, &config);
        assert!(output.accepted_rules.is_empty());
        assert!(output.all_candidates.is_empty());
    }

    #[test]
    fn incremental_requires_min_logs() {
        let store = LogStore::new();
        let estimator = simple_cost_estimator();
        let config = PipelineConfig::default();

        let output = run_incremental_discovery(&store, &estimator, &config, 10);
        assert!(output.is_none());
    }

    #[test]
    fn incremental_runs_with_enough_logs() {
        let store = build_log_store();
        let estimator = simple_cost_estimator();
        let config = PipelineConfig::default();

        let output = run_incremental_discovery(&store, &estimator, &config, 10);
        assert!(output.is_some());
    }

    #[test]
    fn pipeline_config_defaults_sane() {
        let config = PipelineConfig::default();
        assert!(config.train_fraction > 0.0);
        assert!(config.train_fraction < 1.0);
        assert!(config.mining.min_support > 0);
        assert!(config.synthesis.min_confidence > 0.0);
    }
}
