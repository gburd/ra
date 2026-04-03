//! Bayesian belief network for rule ordering and pruning.
//!
//! Implements a dynamic belief network that learns conditional
//! probabilities from execution observations to order and prune
//! optimization rules based on their expected effectiveness.
//!
//! The network constantly evolves via differential dataflow as new
//! execution observations arrive, updating conditional probability
//! tables and rule priority rankings.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use abomonation_derive::Abomonation;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info};

/// Errors from belief network operations.
#[derive(Debug, Error)]
pub enum BeliefNetworkError {
    /// No observations available for the given rule.
    #[error("no observations for rule {rule_id}")]
    NoObservations {
        /// Rule identifier
        rule_id: String
    },

    /// Invalid probability value.
    #[error("invalid probability: {0}")]
    InvalidProbability(f64),

    /// Network is not trained.
    #[error("network has not been trained")]
    Untrained,
}

/// An execution observation recording rule effectiveness.
#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize, Abomonation)]
pub struct ExecutionObservation {
    /// Rule that was applied.
    pub rule_id: String,
    /// Estimated plan execution time before rule application.
    pub estimated_time_before: f64,
    /// Estimated plan execution time after rule application.
    pub estimated_time_after: f64,
    /// Actual measured execution time (if available).
    pub actual_time: Option<f64>,
    /// Whether the rule improved the plan.
    pub improved: bool,
    /// Context features (table sizes, join types, etc.).
    pub context: Vec<f64>,
    /// Timestamp of observation.
    pub timestamp: i64,
}

impl Eq for ExecutionObservation {}

impl Ord for ExecutionObservation {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rule_id.cmp(&other.rule_id)
            .then_with(|| self.timestamp.cmp(&other.timestamp))
            .then_with(|| self.estimated_time_before.partial_cmp(&other.estimated_time_before).unwrap_or(std::cmp::Ordering::Equal))
    }
}

impl ExecutionObservation {
    /// Compute the improvement ratio from this observation.
    #[must_use]
    pub fn improvement_ratio(&self) -> f64 {
        if self.estimated_time_before > 0.0 {
            (self.estimated_time_before - self.estimated_time_after) / self.estimated_time_before
        } else {
            0.0
        }
    }

    /// Compute q-error between estimated and actual time.
    #[must_use]
    pub fn q_error(&self) -> Option<f64> {
        self.actual_time.map(|actual| {
            let est = self.estimated_time_after.max(1.0);
            let act = actual.max(1.0);
            (est / act).max(act / est)
        })
    }
}

/// Conditional probability table for a rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionalProbabilityTable {
    /// Rule identifier.
    pub rule_id: String,
    /// Prior probability of improvement.
    pub prior: f64,
    /// Context-conditioned probabilities.
    /// Maps context feature hash to probability of improvement.
    pub conditional: HashMap<u64, f64>,
    /// Number of observations used for training.
    pub observation_count: usize,
    /// Mean improvement ratio when rule succeeds.
    pub mean_improvement: f64,
    /// Standard deviation of improvement.
    pub std_improvement: f64,
}

impl ConditionalProbabilityTable {
    /// Create an empty CPT with uniform prior.
    #[must_use]
    pub fn new(rule_id: String) -> Self {
        Self {
            rule_id,
            prior: 0.5,
            conditional: HashMap::new(),
            observation_count: 0,
            mean_improvement: 0.0,
            std_improvement: 0.0,
        }
    }

    /// Update the CPT with new observations.
    pub fn update(&mut self, observations: &[ExecutionObservation]) {
        if observations.is_empty() {
            return;
        }

        let improvements: Vec<f64> = observations
            .iter()
            .map(ExecutionObservation::improvement_ratio)
            .collect();

        self.prior = observations.iter().filter(|o| o.improved).count() as f64
            / observations.len() as f64;

        self.observation_count = observations.len();

        self.mean_improvement = improvements.iter().sum::<f64>() / improvements.len() as f64;
        self.std_improvement = if improvements.len() > 1 {
            let variance = improvements
                .iter()
                .map(|x| {
                    let diff = x - self.mean_improvement;
                    diff * diff
                })
                .sum::<f64>()
                / (improvements.len() - 1) as f64;
            variance.sqrt()
        } else {
            0.0
        };

        for obs in observations {
            let context_hash = hash_context(&obs.context);
            let current = self.conditional.entry(context_hash).or_insert(0.5);
            let alpha = 0.1;
            *current = (1.0 - alpha) * *current + alpha * if obs.improved { 1.0 } else { 0.0 };
        }

        debug!(
            rule_id = %self.rule_id,
            prior = %self.prior,
            observations = %self.observation_count,
            mean_improvement = %self.mean_improvement,
            "Updated CPT"
        );
    }

    /// Predict probability of improvement given context features.
    #[must_use]
    pub fn predict(&self, context: &[f64]) -> f64 {
        let context_hash = hash_context(context);
        self.conditional.get(&context_hash).copied().unwrap_or(self.prior)
    }

    /// Compute expected value of applying this rule.
    #[must_use]
    pub fn expected_value(&self, context: &[f64]) -> f64 {
        let prob = self.predict(context);
        prob * self.mean_improvement
    }
}

fn hash_context(context: &[f64]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    for val in context {
        val.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

/// Bayesian belief network for rule ordering.
#[derive(Debug, Clone)]
pub struct BeliefNetwork {
    /// Conditional probability tables for each rule.
    cpts: Arc<Mutex<HashMap<String, ConditionalProbabilityTable>>>,
    /// Observation history.
    observations: Arc<Mutex<Vec<ExecutionObservation>>>,
    /// Maximum observations to retain in memory.
    max_observations: usize,
}

impl BeliefNetwork {
    /// Create a new belief network.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cpts: Arc::new(Mutex::new(HashMap::new())),
            observations: Arc::new(Mutex::new(Vec::new())),
            max_observations: 10_000,
        }
    }

    /// Create a belief network with custom observation limit.
    #[must_use]
    pub fn with_max_observations(max_observations: usize) -> Self {
        Self {
            cpts: Arc::new(Mutex::new(HashMap::new())),
            observations: Arc::new(Mutex::new(Vec::new())),
            max_observations,
        }
    }

    /// Add an execution observation.
    pub fn observe(&self, observation: ExecutionObservation) {
        let mut obs = self.observations.lock().unwrap_or_else(|e| e.into_inner());
        obs.push(observation.clone());

        if obs.len() > self.max_observations {
            let excess = obs.len() - self.max_observations;
            obs.drain(..excess);
        }

        let rule_id = observation.rule_id.clone();
        let rule_obs: Vec<ExecutionObservation> = obs
            .iter()
            .filter(|o| o.rule_id == rule_id)
            .cloned()
            .collect();

        drop(obs);

        let mut cpts = self.cpts.lock().unwrap_or_else(|e| e.into_inner());
        let cpt = cpts.entry(rule_id.clone()).or_insert_with(|| ConditionalProbabilityTable::new(rule_id));
        cpt.update(&rule_obs);
    }

    /// Batch add multiple observations.
    pub fn observe_batch(&self, observations: Vec<ExecutionObservation>) {
        for obs in observations {
            self.observe(obs);
        }
    }

    /// Get rule ordering based on expected value in the given context.
    ///
    /// Returns rule IDs sorted by descending expected improvement.
    #[must_use]
    pub fn order_rules(&self, rule_ids: &[String], context: &[f64]) -> Vec<String> {
        let cpts = self.cpts.lock().unwrap_or_else(|e| e.into_inner());

        let mut scored: Vec<(String, f64)> = rule_ids
            .iter()
            .map(|id| {
                let score = cpts.get(id).map_or(0.5, |cpt| cpt.expected_value(context));
                (id.clone(), score)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        scored.into_iter().map(|(id, _)| id).collect()
    }

    /// Filter rules that are unlikely to improve the plan.
    ///
    /// Returns rule IDs with expected improvement above threshold.
    #[must_use]
    pub fn filter_rules(&self, rule_ids: &[String], context: &[f64], threshold: f64) -> Vec<String> {
        let cpts = self.cpts.lock().unwrap_or_else(|e| e.into_inner());

        rule_ids
            .iter()
            .filter(|id| {
                cpts.get(*id)
                    .map_or(true, |cpt| cpt.expected_value(context) >= threshold)
            })
            .cloned()
            .collect()
    }

    /// Get statistics for a specific rule.
    ///
    /// # Errors
    ///
    /// Returns `BeliefNetworkError::NoObservations` if the rule has no data.
    pub fn rule_statistics(&self, rule_id: &str) -> Result<RuleStatistics, BeliefNetworkError> {
        let cpts = self.cpts.lock().unwrap_or_else(|e| e.into_inner());
        let cpt = cpts.get(rule_id).ok_or_else(|| BeliefNetworkError::NoObservations {
            rule_id: rule_id.to_string(),
        })?;

        let obs = self.observations.lock().unwrap_or_else(|e| e.into_inner());
        let rule_obs: Vec<&ExecutionObservation> = obs.iter().filter(|o| o.rule_id == rule_id).collect();

        let q_errors: Vec<f64> = rule_obs.iter().filter_map(|o| o.q_error()).collect();
        let mean_q_error = if q_errors.is_empty() {
            None
        } else {
            Some(q_errors.iter().sum::<f64>() / q_errors.len() as f64)
        };

        Ok(RuleStatistics {
            rule_id: rule_id.to_string(),
            observation_count: cpt.observation_count,
            prior_improvement_prob: cpt.prior,
            mean_improvement: cpt.mean_improvement,
            std_improvement: cpt.std_improvement,
            mean_q_error,
        })
    }

    /// Get statistics for all rules.
    #[must_use]
    pub fn all_statistics(&self) -> Vec<RuleStatistics> {
        let cpts = self.cpts.lock().unwrap_or_else(|e| e.into_inner());
        cpts.keys()
            .filter_map(|id| self.rule_statistics(id).ok())
            .collect()
    }

    /// Clear all observations and reset CPTs.
    pub fn reset(&self) {
        let mut obs = self.observations.lock().unwrap_or_else(|e| e.into_inner());
        obs.clear();
        let mut cpts = self.cpts.lock().unwrap_or_else(|e| e.into_inner());
        cpts.clear();
        info!("Belief network reset");
    }

    /// Export the network state for serialization.
    #[must_use]
    pub fn export(&self) -> BeliefNetworkState {
        let cpts = self.cpts.lock().unwrap_or_else(|e| e.into_inner());
        let observations = self.observations.lock().unwrap_or_else(|e| e.into_inner());

        BeliefNetworkState {
            cpts: cpts.values().cloned().collect(),
            recent_observations: observations.iter().rev().take(1000).cloned().collect(),
        }
    }

    /// Import network state from serialized data.
    pub fn import(&self, state: BeliefNetworkState) {
        let mut cpts = self.cpts.lock().unwrap_or_else(|e| e.into_inner());
        cpts.clear();
        for cpt in state.cpts {
            cpts.insert(cpt.rule_id.clone(), cpt);
        }

        let mut obs = self.observations.lock().unwrap_or_else(|e| e.into_inner());
        obs.clear();
        obs.extend(state.recent_observations);

        info!(cpts = %cpts.len(), observations = %obs.len(), "Imported belief network state");
    }
}

impl Default for BeliefNetwork {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics for a single rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleStatistics {
    /// Rule identifier.
    pub rule_id: String,
    /// Number of observations.
    pub observation_count: usize,
    /// Prior probability of improvement.
    pub prior_improvement_prob: f64,
    /// Mean improvement when successful.
    pub mean_improvement: f64,
    /// Standard deviation of improvement.
    pub std_improvement: f64,
    /// Mean q-error of cost estimates.
    pub mean_q_error: Option<f64>,
}

/// Serializable state of the belief network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeliefNetworkState {
    /// All CPTs.
    pub cpts: Vec<ConditionalProbabilityTable>,
    /// Recent observations for warm-up.
    pub recent_observations: Vec<ExecutionObservation>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_observation(rule_id: &str, improved: bool, time_before: f64, time_after: f64) -> ExecutionObservation {
        ExecutionObservation {
            rule_id: rule_id.to_string(),
            estimated_time_before: time_before,
            estimated_time_after: time_after,
            actual_time: Some(time_after),
            improved,
            context: vec![1.0, 2.0, 3.0],
            timestamp: 0,
        }
    }

    #[test]
    fn belief_network_basic() {
        let network = BeliefNetwork::new();
        let obs = sample_observation("rule1", true, 100.0, 50.0);
        network.observe(obs);

        let stats = network.rule_statistics("rule1").expect("should have stats");
        assert_eq!(stats.observation_count, 1);
        assert!((stats.prior_improvement_prob - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn belief_network_ordering() {
        let network = BeliefNetwork::new();

        network.observe(sample_observation("rule1", true, 100.0, 50.0));
        network.observe(sample_observation("rule1", true, 100.0, 40.0));

        network.observe(sample_observation("rule2", true, 100.0, 90.0));
        network.observe(sample_observation("rule2", false, 100.0, 110.0));

        let context = vec![1.0, 2.0, 3.0];
        let ordered = network.order_rules(&["rule1".into(), "rule2".into()], &context);

        assert_eq!(ordered[0], "rule1");
    }

    #[test]
    fn belief_network_filtering() {
        let network = BeliefNetwork::new();

        network.observe(sample_observation("good_rule", true, 100.0, 20.0));
        network.observe(sample_observation("bad_rule", false, 100.0, 120.0));

        let context = vec![1.0, 2.0, 3.0];
        let filtered = network.filter_rules(&["good_rule".into(), "bad_rule".into()], &context, 0.3);

        assert!(filtered.contains(&"good_rule".to_string()));
    }

    #[test]
    fn cpt_update() {
        let mut cpt = ConditionalProbabilityTable::new("test_rule".into());
        let obs = vec![
            sample_observation("test_rule", true, 100.0, 50.0),
            sample_observation("test_rule", true, 100.0, 60.0),
            sample_observation("test_rule", false, 100.0, 110.0),
        ];

        cpt.update(&obs);

        assert_eq!(cpt.observation_count, 3);
        assert!((cpt.prior - 0.666_666_666_666_666_6).abs() < 0.01);
        assert!(cpt.mean_improvement > 0.0);
    }

    #[test]
    fn observation_improvement_ratio() {
        let obs = sample_observation("rule", true, 100.0, 50.0);
        assert!((obs.improvement_ratio() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn observation_q_error() {
        let obs = ExecutionObservation {
            rule_id: "rule".to_string(),
            estimated_time_before: 100.0,
            estimated_time_after: 50.0,
            actual_time: Some(100.0),
            improved: true,
            context: vec![],
            timestamp: 0,
        };
        let q_err = obs.q_error().expect("should have q-error");
        assert!((q_err - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn belief_network_export_import() {
        let network = BeliefNetwork::new();
        network.observe(sample_observation("rule1", true, 100.0, 50.0));

        let state = network.export();
        assert_eq!(state.cpts.len(), 1);
        assert_eq!(state.recent_observations.len(), 1);

        let new_network = BeliefNetwork::new();
        new_network.import(state);

        let stats = new_network.rule_statistics("rule1").expect("should have stats");
        assert_eq!(stats.observation_count, 1);
    }

    #[test]
    fn belief_network_reset() {
        let network = BeliefNetwork::new();
        network.observe(sample_observation("rule1", true, 100.0, 50.0));

        network.reset();

        assert!(network.rule_statistics("rule1").is_err());
    }

    #[test]
    fn belief_network_max_observations() {
        let network = BeliefNetwork::with_max_observations(2);

        network.observe(sample_observation("rule1", true, 100.0, 50.0));
        network.observe(sample_observation("rule1", true, 100.0, 50.0));
        network.observe(sample_observation("rule1", true, 100.0, 50.0));

        let obs = network.observations.lock().unwrap();
        assert_eq!(obs.len(), 2);
    }
}
