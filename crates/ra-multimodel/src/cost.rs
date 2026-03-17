//! Multi-model cost estimation.
//!
//! Provides a unified cost model that selects the appropriate
//! model-specific estimator based on the data model of the target
//! database. This module bridges graph, document, and time-series
//! cost functions into the core [`CostModel`] trait.

use serde::{Deserialize, Serialize};

use ra_core::cost::Cost;

/// The data model of a target database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataModel {
    /// Relational (SQL) model.
    Relational,
    /// Property graph model (Neo4j, `JanusGraph`).
    Graph,
    /// Document model (`MongoDB`, Couchbase).
    Document,
    /// Time-series model (`InfluxDB`, `TimescaleDB`).
    TimeSeries,
}

/// Configuration for multi-model cost estimation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiModelCostConfig {
    /// The primary data model of the target.
    pub data_model: DataModel,
    /// Weight for traversal operations (graph-specific).
    pub traversal_weight: f64,
    /// Weight for document deserialization cost.
    pub deserialization_weight: f64,
    /// Weight for time-series compression overhead.
    pub compression_weight: f64,
    /// Network cost multiplier for distributed queries.
    pub network_multiplier: f64,
}

impl Default for MultiModelCostConfig {
    fn default() -> Self {
        Self {
            data_model: DataModel::Relational,
            traversal_weight: 1.0,
            deserialization_weight: 1.0,
            compression_weight: 1.0,
            network_multiplier: 1.0,
        }
    }
}

/// Adjust a base cost estimate for a specific data model.
///
/// Applies model-specific weights to the cost components based on
/// the physical characteristics of the target database.
#[must_use]
pub fn adjust_cost_for_model(base: &Cost, config: &MultiModelCostConfig) -> Cost {
    match config.data_model {
        DataModel::Relational => base.clone(),
        DataModel::Graph => Cost::new(
            base.cpu * config.traversal_weight,
            base.io * 0.5, // graph traversal is adjacency-local
            base.network * config.network_multiplier,
            base.memory,
        ),
        DataModel::Document => Cost::new(
            base.cpu * config.deserialization_weight,
            base.io * 1.2, // document reads are typically larger
            base.network * config.network_multiplier,
            base.memory,
        ),
        DataModel::TimeSeries => Cost::new(
            base.cpu * config.compression_weight,
            base.io * 0.3, // columnar compression reduces IO
            base.network * config.network_multiplier,
            base.memory,
        ),
    }
}

/// Estimate the benefit of converting a relational join to a
/// graph traversal.
#[must_use]
pub fn join_vs_traversal_benefit(left_card: f64, right_card: f64, avg_degree: f64) -> f64 {
    let join_cost = left_card * right_card.ln().max(1.0);
    let traversal_cost = left_card * avg_degree;
    if join_cost <= 0.0 {
        return 0.0;
    }
    ((join_cost - traversal_cost) / join_cost).clamp(0.0, 1.0)
}

/// Estimate the benefit of using a continuous aggregate vs. raw scan.
#[must_use]
pub fn cagg_vs_raw_benefit(raw_rows: f64, bucket_count: f64) -> f64 {
    if raw_rows <= 0.0 {
        return 0.0;
    }
    ((raw_rows - bucket_count) / raw_rows).clamp(0.0, 1.0)
}

/// Estimate the benefit of an index-only scan vs. collection scan.
#[must_use]
pub fn covered_query_benefit(avg_doc_size: f64, avg_index_entry_size: f64) -> f64 {
    if avg_doc_size <= 0.0 {
        return 0.0;
    }
    ((avg_doc_size - avg_index_entry_size) / avg_doc_size).clamp(0.0, 1.0)
}

impl std::fmt::Display for DataModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Relational => write!(f, "Relational"),
            Self::Graph => write!(f, "Graph"),
            Self::Document => write!(f, "Document"),
            Self::TimeSeries => write!(f, "TimeSeries"),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn data_model_display() {
        assert_eq!(DataModel::Relational.to_string(), "Relational");
        assert_eq!(DataModel::Graph.to_string(), "Graph");
        assert_eq!(DataModel::Document.to_string(), "Document");
        assert_eq!(DataModel::TimeSeries.to_string(), "TimeSeries");
    }

    #[test]
    fn default_config() {
        let config = MultiModelCostConfig::default();
        assert_eq!(config.data_model, DataModel::Relational);
        assert!((config.traversal_weight - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn config_roundtrip() {
        let config = MultiModelCostConfig {
            data_model: DataModel::Graph,
            traversal_weight: 0.8,
            deserialization_weight: 1.2,
            compression_weight: 0.5,
            network_multiplier: 2.0,
        };
        let json = serde_json::to_string(&config).expect("serialization should succeed");
        let deserialized: MultiModelCostConfig =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(config, deserialized);
    }

    #[test]
    fn adjust_relational_identity() {
        let base = Cost::new(10.0, 20.0, 5.0, 1024);
        let config = MultiModelCostConfig::default();
        let adjusted = adjust_cost_for_model(&base, &config);
        assert_eq!(adjusted, base);
    }

    #[test]
    fn adjust_graph_reduces_io() {
        let base = Cost::new(10.0, 20.0, 0.0, 1024);
        let config = MultiModelCostConfig {
            data_model: DataModel::Graph,
            ..MultiModelCostConfig::default()
        };
        let adjusted = adjust_cost_for_model(&base, &config);
        assert!(adjusted.io < base.io);
    }

    #[test]
    fn adjust_document_increases_io() {
        let base = Cost::new(10.0, 20.0, 0.0, 1024);
        let config = MultiModelCostConfig {
            data_model: DataModel::Document,
            ..MultiModelCostConfig::default()
        };
        let adjusted = adjust_cost_for_model(&base, &config);
        assert!(adjusted.io > base.io);
    }

    #[test]
    fn adjust_timeseries_reduces_io() {
        let base = Cost::new(10.0, 20.0, 0.0, 1024);
        let config = MultiModelCostConfig {
            data_model: DataModel::TimeSeries,
            ..MultiModelCostConfig::default()
        };
        let adjusted = adjust_cost_for_model(&base, &config);
        assert!(adjusted.io < base.io);
    }

    #[test]
    fn join_vs_traversal_high_degree() {
        let benefit = join_vs_traversal_benefit(1000.0, 1_000_000.0, 5.0);
        assert!(benefit > 0.5);
    }

    #[test]
    fn join_vs_traversal_low_card() {
        let benefit = join_vs_traversal_benefit(10.0, 100.0, 50.0);
        // When traversal is more expensive, benefit is low or zero
        assert!(benefit >= 0.0);
        assert!(benefit <= 1.0);
    }

    #[test]
    fn join_vs_traversal_zero_left() {
        let benefit = join_vs_traversal_benefit(0.0, 100.0, 5.0);
        assert!((benefit - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cagg_benefit_large_table() {
        let benefit = cagg_vs_raw_benefit(100_000_000.0, 8760.0);
        assert!(benefit > 0.99);
    }

    #[test]
    fn cagg_benefit_zero_rows() {
        let benefit = cagg_vs_raw_benefit(0.0, 100.0);
        assert!((benefit - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn covered_query_benefit_large_docs() {
        let benefit = covered_query_benefit(2048.0, 64.0);
        assert!(benefit > 0.9);
    }

    #[test]
    fn covered_query_benefit_same_size() {
        let benefit = covered_query_benefit(100.0, 100.0);
        assert!((benefit - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn covered_query_benefit_zero_doc() {
        let benefit = covered_query_benefit(0.0, 64.0);
        assert!((benefit - 0.0).abs() < f64::EPSILON);
    }
}
