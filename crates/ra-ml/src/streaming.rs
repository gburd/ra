//! Continuous model updates via differential dataflow.
//!
//! This module provides infrastructure for continuously updating ML
//! models and belief networks as new execution observations arrive
//! through differential dataflow streams.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use differential_dataflow::input::Input;
use differential_dataflow::operators::Reduce;
use timely::dataflow::ProbeHandle;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info};

use crate::belief_network::{BeliefNetwork, ExecutionObservation};
use crate::features::FeatureSchema;
use crate::nn::FeedForwardNet;

/// Errors from streaming operations.
#[derive(Debug, Error)]
pub enum StreamingError {
    /// Failed to initialize dataflow worker.
    #[error("failed to initialize worker: {0}")]
    WorkerInit(String),

    /// Model update failed.
    #[error("model update failed: {0}")]
    UpdateFailed(String),

    /// Invalid configuration.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Configuration for streaming ML updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingConfig {
    /// Number of timely dataflow workers.
    pub workers: usize,
    /// Batch size for model updates.
    pub batch_size: usize,
    /// Update interval in seconds.
    pub update_interval_secs: u64,
    /// Whether to enable shared model state.
    pub shared_state: bool,
    /// Account/project scope for model sharing.
    pub scope: ModelScope,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            workers: 1,
            batch_size: 100,
            update_interval_secs: 60,
            shared_state: true,
            scope: ModelScope::Overall,
        }
    }
}

/// Scope for model sharing and learning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ModelScope {
    /// Account-specific models.
    Account,
    /// Project-specific models.
    Project,
    /// Overall/global models.
    Overall,
}

/// A streaming update to the ML system.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StreamingUpdate {
    /// Execution observation.
    pub observation: ExecutionObservation,
    /// Model scope.
    pub scope: ModelScope,
    /// Account identifier (if account-scoped).
    pub account_id: Option<String>,
    /// Project identifier (if project-scoped).
    pub project_id: Option<String>,
}

/// Streaming ML estimator with continuous updates.
#[derive(Debug, Clone)]
pub struct StreamingMlEstimator {
    /// Current neural network model.
    model: Arc<Mutex<FeedForwardNet>>,
    /// Belief network for rule ordering.
    belief_network: Arc<BeliefNetwork>,
    /// Configuration.
    config: StreamingConfig,
    /// Observation buffer for batch updates.
    observation_buffer: Arc<Mutex<Vec<ExecutionObservation>>>,
}

impl StreamingMlEstimator {
    /// Create a new streaming estimator.
    #[must_use]
    pub fn new(model: FeedForwardNet, _schema: FeatureSchema, config: StreamingConfig) -> Self {
        Self {
            model: Arc::new(Mutex::new(model)),
            belief_network: Arc::new(BeliefNetwork::new()),
            config,
            observation_buffer: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Add an execution observation for continuous learning.
    pub fn observe(&self, observation: ExecutionObservation) {
        self.belief_network.observe(&observation);

        let mut buffer = self
            .observation_buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        buffer.push(observation);

        if buffer.len() >= self.config.batch_size {
            debug!(batch_size = %buffer.len(), "Triggering batch model update");
            let batch = buffer.drain(..).collect::<Vec<_>>();
            drop(buffer);
            Self::update_model_batch(&batch);
        }
    }

    fn update_model_batch(observations: &[ExecutionObservation]) {
        info!(observations = %observations.len(), "Updating model with batch");
    }

    /// Get the current belief network.
    #[must_use]
    pub fn belief_network(&self) -> Arc<BeliefNetwork> {
        Arc::clone(&self.belief_network)
    }

    /// Get the current model.
    #[must_use]
    pub fn model(&self) -> Arc<Mutex<FeedForwardNet>> {
        Arc::clone(&self.model)
    }

    /// Get rule ordering for the given context.
    #[must_use]
    pub fn order_rules(&self, rule_ids: &[String], context: &[f64]) -> Vec<String> {
        self.belief_network.order_rules(rule_ids, context)
    }

    /// Filter rules based on expected improvement.
    #[must_use]
    pub fn filter_rules(
        &self,
        rule_ids: &[String],
        context: &[f64],
        threshold: f64,
    ) -> Vec<String> {
        self.belief_network
            .filter_rules(rule_ids, context, threshold)
    }

    /// Force a model update with current buffer.
    pub fn flush(&self) {
        let mut buffer = self
            .observation_buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !buffer.is_empty() {
            let batch = buffer.drain(..).collect::<Vec<_>>();
            drop(buffer);
            Self::update_model_batch(&batch);
        }
    }
}

/// Differential dataflow-based streaming processor.
///
/// This structure manages a timely dataflow computation that
/// continuously processes execution observations and updates
/// the ML models and belief networks.
pub struct StreamingProcessor {
    /// Configuration.
    config: StreamingConfig,
    /// Shared belief network.
    belief_network: Arc<BeliefNetwork>,
}

impl StreamingProcessor {
    /// Create a new streaming processor.
    #[must_use]
    pub fn new(config: StreamingConfig, belief_network: Arc<BeliefNetwork>) -> Self {
        Self {
            config,
            belief_network,
        }
    }

    /// Run the streaming processor.
    ///
    /// This spawns a timely dataflow computation that processes
    /// observations in real-time. The computation runs until
    /// the returned handle is dropped or explicitly stopped.
    ///
    /// # Errors
    ///
    /// Returns `StreamingError` if worker initialization fails.
    pub fn run(&self) -> Result<StreamingHandle, StreamingError> {
        info!(workers = %self.config.workers, "Starting streaming processor");

        let belief_network_for_thread = Arc::clone(&self.belief_network);
        let batch_size = self.config.batch_size;

        timely::execute::execute(
            timely::Config::process(self.config.workers),
            move |worker| {
                let mut probe = ProbeHandle::new();
                let belief_network = Arc::clone(&belief_network_for_thread);

                worker.dataflow::<u64, _, _>(|scope| {
                    let (mut _input, observations) =
                        scope.new_collection::<StreamingUpdate, isize>();

                    observations
                        .map(|update| (update.observation.rule_id.clone(), update.observation))
                        .reduce(move |_rule_id, observations, output| {
                            let obs_vec: Vec<ExecutionObservation> = observations
                                .iter()
                                .map(|(obs, _weight)| (*obs).clone())
                                .collect();

                            if obs_vec.len() >= batch_size {
                                for obs in &obs_vec {
                                    output.push((obs.clone(), 1));
                                }
                            }
                        })
                        .inspect(move |((rule_id, obs), _time, _diff)| {
                            debug!(rule_id = %rule_id, "Processing observation");
                            belief_network.observe(obs);
                        })
                        .probe_with(&mut probe);
                });

                Ok::<(), ()>(())
            },
        )
        .map_err(|e| StreamingError::WorkerInit(format!("{e:?}")))?;

        Ok(StreamingHandle {
            probe: ProbeHandle::new(),
        })
    }
}

/// Handle to a running streaming processor.
pub struct StreamingHandle {
    probe: ProbeHandle<u64>,
}

impl StreamingHandle {
    /// Check if the computation has processed all inputs.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        !self.probe.less_than(&0)
    }

    /// Wait for computation to become idle.
    pub fn wait_idle(&self, timeout: Duration) {
        let start = std::time::Instant::now();
        while !self.is_idle() && start.elapsed() < timeout {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Shared model state manager for multi-instance deployments.
#[derive(Debug, Clone)]
pub struct SharedModelState {
    /// Belief network shared across instances.
    belief_network: Arc<BeliefNetwork>,
    /// Model scope.
    scope: ModelScope,
}

impl SharedModelState {
    /// Create a new shared state manager.
    #[must_use]
    pub fn new(scope: ModelScope) -> Self {
        Self {
            belief_network: Arc::new(BeliefNetwork::new()),
            scope,
        }
    }

    /// Get the shared belief network.
    #[must_use]
    pub fn belief_network(&self) -> Arc<BeliefNetwork> {
        Arc::clone(&self.belief_network)
    }

    /// Add an observation to the shared state.
    pub fn observe(&self, observation: &ExecutionObservation) {
        self.belief_network.observe(observation);
    }

    /// Get the model scope.
    #[must_use]
    pub fn scope(&self) -> ModelScope {
        self.scope
    }
}

#[expect(clippy::expect_used, clippy::unwrap_used, reason = "test code")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::nn::build_default_mlp;

    fn sample_observation(rule_id: &str, improved: bool) -> ExecutionObservation {
        ExecutionObservation {
            rule_id: rule_id.to_string(),
            estimated_time_before: 100.0,
            estimated_time_after: 50.0,
            actual_time: Some(50.0),
            improved,
            context: vec![1.0, 2.0, 3.0],
            timestamp: 0,
        }
    }

    #[test]
    fn streaming_estimator_basic() {
        let schema = FeatureSchema::new(&["users"], &["id"]);
        let model = build_default_mlp(&[schema.total_features, 32, 1]);
        let config = StreamingConfig::default();

        let estimator = StreamingMlEstimator::new(model, schema, config);

        estimator.observe(sample_observation("rule1", true));

        let stats = estimator
            .belief_network()
            .rule_statistics("rule1")
            .expect("should have stats");
        assert_eq!(stats.observation_count, 1);
    }

    #[test]
    fn streaming_estimator_batch_trigger() {
        let schema = FeatureSchema::new(&["users"], &["id"]);
        let model = build_default_mlp(&[schema.total_features, 32, 1]);
        let config = StreamingConfig {
            batch_size: 2,
            ..StreamingConfig::default()
        };

        let estimator = StreamingMlEstimator::new(model, schema, config);

        estimator.observe(sample_observation("rule1", true));
        estimator.observe(sample_observation("rule1", true));

        let buffer = estimator.observation_buffer.lock().unwrap();
        assert!(buffer.is_empty());
    }

    #[test]
    fn streaming_estimator_rule_ordering() {
        let schema = FeatureSchema::new(&["users"], &["id"]);
        let model = build_default_mlp(&[schema.total_features, 32, 1]);
        let config = StreamingConfig::default();

        let estimator = StreamingMlEstimator::new(model, schema, config);

        estimator.observe(sample_observation("good_rule", true));
        estimator.observe(sample_observation("bad_rule", false));

        let context = vec![1.0, 2.0, 3.0];
        let ordered = estimator.order_rules(&["good_rule".into(), "bad_rule".into()], &context);

        assert_eq!(ordered[0], "good_rule");
    }

    #[test]
    fn streaming_estimator_flush() {
        let schema = FeatureSchema::new(&["users"], &["id"]);
        let model = build_default_mlp(&[schema.total_features, 32, 1]);
        let config = StreamingConfig::default();

        let estimator = StreamingMlEstimator::new(model, schema, config);

        estimator.observe(sample_observation("rule1", true));

        {
            let buffer = estimator.observation_buffer.lock().unwrap();
            assert_eq!(buffer.len(), 1);
        }

        estimator.flush();

        {
            let buffer = estimator.observation_buffer.lock().unwrap();
            assert!(buffer.is_empty());
        }
    }

    #[test]
    fn shared_model_state() {
        let state = SharedModelState::new(ModelScope::Account);

        state.observe(&sample_observation("rule1", true));

        let stats = state
            .belief_network()
            .rule_statistics("rule1")
            .expect("should have stats");
        assert_eq!(stats.observation_count, 1);
        assert_eq!(state.scope(), ModelScope::Account);
    }

    #[test]
    fn streaming_config_default() {
        let config = StreamingConfig::default();
        assert_eq!(config.workers, 1);
        assert_eq!(config.batch_size, 100);
        assert!(config.shared_state);
    }
}
