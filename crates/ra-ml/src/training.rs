//! Training data generation and model evaluation.
//!
//! Provides utilities to generate training samples from query plans
//! with known cardinalities, and to evaluate model accuracy using
//! q-error metrics.

use std::collections::HashMap;

use ra_core::algebra::RelExpr;
use ra_core::statistics::Statistics;
use serde::{Deserialize, Serialize};

use crate::estimator::{q_error, CardinalityEstimator, SimpleStatsProvider};
use crate::features::{log_scale, FeatureSchema};

/// A single training sample: features paired with the true
/// log-scaled row count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingSample {
    /// Input feature vector.
    pub features: Vec<f64>,
    /// Target: `log2(1 + actual_rows)`.
    pub target: f64,
    /// Original query description (for debugging).
    pub description: String,
}

/// A training dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingDataset {
    /// The samples.
    pub samples: Vec<TrainingSample>,
    /// The feature schema used to generate features.
    pub schema: FeatureSchema,
}

impl TrainingDataset {
    /// Create an empty dataset with the given schema.
    #[must_use]
    pub fn new(schema: FeatureSchema) -> Self {
        Self {
            samples: Vec::new(),
            schema,
        }
    }

    /// Add a training sample from a query plan and its actual
    /// result cardinality.
    pub fn add_sample(
        &mut self,
        expr: &RelExpr,
        actual_rows: f64,
        description: &str,
        stats: &HashMap<String, Statistics>,
    ) {
        let features = self.schema.extract(expr, stats);
        self.samples.push(TrainingSample {
            features,
            target: log_scale(actual_rows),
            description: description.to_string(),
        });
    }

    /// Serialize the dataset to JSON bytes for external training.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if the data cannot be
    /// encoded.
    pub fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec_pretty(self)
    }

    /// Deserialize a dataset from JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns a deserialization error on invalid input.
    pub fn from_json(json: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(json)
    }

    /// Return the number of samples.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Return true if the dataset has no samples.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// Evaluation results from running an estimator against known
/// cardinalities.
#[derive(Debug, Clone)]
pub struct EvaluationResult {
    /// Per-query q-errors.
    pub q_errors: Vec<f64>,
    /// Per-query estimated cardinalities.
    pub estimated: Vec<f64>,
    /// Per-query actual cardinalities.
    pub actual: Vec<f64>,
}

impl EvaluationResult {
    /// Compute the median q-error.
    #[must_use]
    pub fn median_q_error(&self) -> f64 {
        if self.q_errors.is_empty() {
            return 0.0;
        }
        let mut sorted = self.q_errors.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = sorted.len() / 2;
        if sorted.len().is_multiple_of(2) {
            f64::midpoint(sorted[mid - 1], sorted[mid])
        } else {
            sorted[mid]
        }
    }

    /// Compute the mean q-error.
    #[must_use]
    pub fn mean_q_error(&self) -> f64 {
        if self.q_errors.is_empty() {
            return 0.0;
        }
        self.q_errors.iter().sum::<f64>() / self.q_errors.len() as f64
    }
}

/// Evaluate an estimator against a set of queries with known
/// cardinalities.
pub fn evaluate_estimator(
    estimator: &dyn CardinalityEstimator,
    queries: &[(RelExpr, f64)],
    stats_provider: &SimpleStatsProvider,
) -> EvaluationResult {
    let mut q_errors = Vec::with_capacity(queries.len());
    let mut estimated = Vec::with_capacity(queries.len());
    let mut actual = Vec::with_capacity(queries.len());

    for (expr, actual_rows) in queries {
        let card = estimator.estimate(expr, stats_provider);
        let err = q_error(card.rows, *actual_rows);
        q_errors.push(err);
        estimated.push(card.rows);
        actual.push(*actual_rows);
    }

    EvaluationResult {
        q_errors,
        estimated,
        actual,
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "Test code appropriately uses expect for deterministic training operations"
)]
mod tests {
    use super::*;
    use crate::estimator::HeuristicEstimator;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp as ExprBinOp, ColumnRef, Const, Expr};

    fn test_schema() -> FeatureSchema {
        FeatureSchema::new(&["users", "orders"], &["id", "amount"])
    }

    fn test_stats() -> HashMap<String, Statistics> {
        let mut map = HashMap::new();
        map.insert("users".to_string(), Statistics::new(1000.0));
        map.insert("orders".to_string(), Statistics::new(5000.0));
        map
    }

    #[test]
    fn dataset_add_and_serialize() {
        let schema = test_schema();
        let mut dataset = TrainingDataset::new(schema);
        let stats = test_stats();

        let expr = RelExpr::scan("users");
        dataset.add_sample(&expr, 1000.0, "scan users", &stats);

        assert_eq!(dataset.len(), 1);
        assert!(!dataset.is_empty());

        let json = dataset.to_json().expect("serialize");
        let restored = TrainingDataset::from_json(&json).expect("deserialize");
        assert_eq!(restored.len(), 1);
    }

    #[test]
    fn dataset_empty() {
        let schema = test_schema();
        let dataset = TrainingDataset::new(schema);
        assert!(dataset.is_empty());
        assert_eq!(dataset.len(), 0);
    }

    #[test]
    fn evaluate_heuristic() {
        let est = HeuristicEstimator;
        let mut provider = SimpleStatsProvider::new();
        provider.add("users", Statistics::new(1000.0));

        let queries = vec![
            (RelExpr::scan("users"), 1000.0),
            (
                RelExpr::scan("users").filter(Expr::BinOp {
                    op: ExprBinOp::Eq,
                    left: Box::new(Expr::Column(ColumnRef::new("id"))),
                    right: Box::new(Expr::Const(Const::Int(1))),
                }),
                5.0,
            ),
        ];

        let result = evaluate_estimator(&est, &queries, &provider);
        assert_eq!(result.q_errors.len(), 2);
        assert!((result.q_errors[0] - 1.0).abs() < f64::EPSILON);
        assert!(result.q_errors[1] > 1.0);
    }

    #[test]
    fn evaluation_result_stats() {
        let result = EvaluationResult {
            q_errors: vec![1.0, 2.0, 3.0, 4.0, 5.0],
            estimated: vec![100.0, 200.0, 300.0, 400.0, 500.0],
            actual: vec![100.0, 100.0, 100.0, 100.0, 100.0],
        };
        assert!((result.median_q_error() - 3.0).abs() < f64::EPSILON);
        assert!((result.mean_q_error() - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn evaluation_result_empty() {
        let result = EvaluationResult {
            q_errors: vec![],
            estimated: vec![],
            actual: vec![],
        };
        assert!((result.median_q_error() - 0.0).abs() < f64::EPSILON);
        assert!((result.mean_q_error() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn evaluation_result_even_count() {
        let result = EvaluationResult {
            q_errors: vec![1.0, 3.0],
            estimated: vec![100.0, 300.0],
            actual: vec![100.0, 100.0],
        };
        assert!((result.median_q_error() - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn training_sample_target_encoding() {
        let schema = test_schema();
        let mut dataset = TrainingDataset::new(schema);
        let stats = test_stats();

        dataset.add_sample(&RelExpr::scan("users"), 100.0, "test", &stats);

        let target = dataset.samples[0].target;
        let expected = log_scale(100.0);
        assert!((target - expected).abs() < f64::EPSILON);
    }
}
