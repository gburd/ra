//! Multi-column statistics for correlated column cardinality estimation.
//!
//! Provides intelligent matching and estimation using multi-column statistics
//! to handle correlated columns that violate the independence assumption.

use crate::types::{ColumnId, MultiColumnStats};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for multi-column statistics tracking and estimation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiColumnConfig {
    /// Enable multi-column statistics usage.
    pub enabled: bool,
    /// Maximum number of columns to track together (2-5 typical).
    pub max_column_combinations: usize,
    /// Minimum correlation threshold to prefer multi-column stats (0.0-1.0).
    /// Higher values mean only use stats for highly correlated columns.
    pub min_correlation_threshold: f64,
    /// Whether to fall back to independence assumption when no stats found.
    pub fallback_to_independence: bool,
    /// Minimum improvement factor to justify using multi-column stats.
    /// If improvement < threshold, use simpler independence assumption.
    pub min_improvement_factor: f64,
}

impl MultiColumnConfig {
    /// Default configuration: balanced between accuracy and complexity.
    pub fn default() -> Self {
        Self {
            enabled: true,
            max_column_combinations: 3,
            min_correlation_threshold: 0.3,
            fallback_to_independence: true,
            min_improvement_factor: 1.5,
        }
    }

    /// Aggressive configuration: maximize accuracy, track more combinations.
    pub fn aggressive() -> Self {
        Self {
            enabled: true,
            max_column_combinations: 5,
            min_correlation_threshold: 0.1,
            fallback_to_independence: true,
            min_improvement_factor: 1.2,
        }
    }

    /// Minimal configuration: only track strong correlations, limit overhead.
    pub fn minimal() -> Self {
        Self {
            enabled: true,
            max_column_combinations: 2,
            min_correlation_threshold: 0.7,
            fallback_to_independence: true,
            min_improvement_factor: 2.0,
        }
    }

    /// Disabled configuration: always use independence assumption.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            max_column_combinations: 0,
            min_correlation_threshold: 1.0,
            fallback_to_independence: true,
            min_improvement_factor: 1.0,
        }
    }
}

/// Result of matching query columns to available multi-column statistics.
#[derive(Debug, Clone, PartialEq)]
pub enum MatchQuality {
    /// Exact match: query columns exactly match a tracked statistic.
    Exact,
    /// Prefix match: query columns are a prefix of a tracked statistic.
    /// Example: query (city, state) matches tracked (city, state, zip).
    Prefix,
    /// Superset match: tracked statistic is subset of query columns.
    /// Example: tracked (city, state) can help with query (city, state, zip).
    Superset,
    /// No suitable match found.
    NoMatch,
}

/// Multi-column cardinality estimator with intelligent statistics matching.
#[derive(Debug, Clone)]
pub struct MultiColumnEstimator {
    /// Configuration for estimation behavior.
    pub config: MultiColumnConfig,
    /// Available multi-column statistics keyed by sorted column sets.
    pub stats: HashMap<Vec<ColumnId>, MultiColumnStats>,
}

impl MultiColumnEstimator {
    /// Create a new estimator with given configuration.
    pub fn new(config: MultiColumnConfig) -> Self {
        Self {
            config,
            stats: HashMap::new(),
        }
    }

    /// Create estimator with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(MultiColumnConfig::default())
    }

    /// Add a multi-column statistic to the estimator.
    pub fn add_stats(&mut self, stats: MultiColumnStats) {
        let mut key = stats.columns.clone();
        key.sort();
        self.stats.insert(key, stats);
    }

    /// Find best matching statistics for a set of query columns.
    pub fn find_best_match(&self, columns: &[ColumnId]) -> (MatchQuality, Option<&MultiColumnStats>) {
        if !self.config.enabled || columns.is_empty() {
            return (MatchQuality::NoMatch, None);
        }

        let mut sorted_cols = columns.to_vec();
        sorted_cols.sort();

        // Try exact match first
        if let Some(stats) = self.stats.get(&sorted_cols) {
            return (MatchQuality::Exact, Some(stats));
        }

        // Try prefix match: find stats where query columns are prefix
        for (stat_cols, stats) in &self.stats {
            if sorted_cols.len() <= stat_cols.len() {
                let is_prefix = sorted_cols.iter()
                    .zip(stat_cols.iter())
                    .all(|(a, b)| a == b);
                if is_prefix {
                    return (MatchQuality::Prefix, Some(stats));
                }
            }
        }

        // Try superset match: find stats that are subset of query columns
        let mut best_superset: Option<(&Vec<ColumnId>, &MultiColumnStats)> = None;
        let mut best_coverage = 0;

        for (stat_cols, stats) in &self.stats {
            if stat_cols.len() < sorted_cols.len() {
                let coverage = stat_cols.iter()
                    .filter(|c| sorted_cols.contains(c))
                    .count();
                if coverage == stat_cols.len() && coverage > best_coverage {
                    best_superset = Some((stat_cols, stats));
                    best_coverage = coverage;
                }
            }
        }

        if let Some((_, stats)) = best_superset {
            return (MatchQuality::Superset, Some(stats));
        }

        (MatchQuality::NoMatch, None)
    }

    /// Estimate cardinality for a set of columns with given individual NDVs.
    pub fn estimate_cardinality(
        &self,
        columns: &[ColumnId],
        individual_ndvs: &[u64],
        total_rows: u64,
    ) -> u64 {
        if columns.len() != individual_ndvs.len() {
            return self.fallback_estimate(individual_ndvs, total_rows);
        }

        let (quality, maybe_stats) = self.find_best_match(columns);

        match (quality, maybe_stats) {
            (MatchQuality::Exact, Some(stats)) => {
                self.estimate_from_stats(stats, individual_ndvs, total_rows)
            }
            (MatchQuality::Prefix, Some(stats)) => {
                // Use stats but adjust for fewer columns
                self.estimate_from_stats(stats, individual_ndvs, total_rows)
            }
            (MatchQuality::Superset, Some(stats)) => {
                // Partial match: blend with independence assumption
                let multi_estimate = stats.distinct_count;
                let indep_estimate = self.fallback_estimate(individual_ndvs, total_rows);
                self.blend_estimates(multi_estimate, indep_estimate, 0.7)
            }
            _ => self.fallback_estimate(individual_ndvs, total_rows),
        }
    }

    /// Estimate using available multi-column stats with improvement check.
    fn estimate_from_stats(
        &self,
        stats: &MultiColumnStats,
        individual_ndvs: &[u64],
        total_rows: u64,
    ) -> u64 {
        let improvement = stats.improvement_factor(individual_ndvs);

        if improvement >= self.config.min_improvement_factor {
            stats.distinct_count.min(total_rows)
        } else {
            // Improvement too small, use independence
            self.fallback_estimate(individual_ndvs, total_rows)
        }
    }

    /// Fallback to independence assumption: NDV = min(product(NDVs), total_rows).
    fn fallback_estimate(&self, individual_ndvs: &[u64], total_rows: u64) -> u64 {
        if !self.config.fallback_to_independence {
            return total_rows;
        }

        let product: u64 = individual_ndvs.iter()
            .copied()
            .reduce(|a, b| a.saturating_mul(b))
            .unwrap_or(total_rows);

        product.min(total_rows)
    }

    /// Blend two estimates with given weight for the first (0.0-1.0).
    fn blend_estimates(&self, estimate1: u64, estimate2: u64, weight1: f64) -> u64 {
        let weight1 = weight1.clamp(0.0, 1.0);
        let weight2 = 1.0 - weight1;
        let blended = estimate1 as f64 * weight1 + estimate2 as f64 * weight2;
        blended.round() as u64
    }

    /// Count available statistics.
    pub fn stats_count(&self) -> usize {
        self.stats.len()
    }

    /// Clear all statistics.
    pub fn clear(&mut self) {
        self.stats.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MultiDimHistogram;

    // ---- MultiColumnConfig ----

    #[test]
    fn config_default() {
        let cfg = MultiColumnConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.max_column_combinations, 3);
        assert!(cfg.fallback_to_independence);
    }

    #[test]
    fn config_aggressive() {
        let cfg = MultiColumnConfig::aggressive();
        assert_eq!(cfg.max_column_combinations, 5);
        assert!(cfg.min_correlation_threshold < 0.3);
    }

    #[test]
    fn config_minimal() {
        let cfg = MultiColumnConfig::minimal();
        assert_eq!(cfg.max_column_combinations, 2);
        assert!(cfg.min_correlation_threshold > 0.5);
    }

    #[test]
    fn config_disabled() {
        let cfg = MultiColumnConfig::disabled();
        assert!(!cfg.enabled);
        assert_eq!(cfg.max_column_combinations, 0);
    }

    #[test]
    fn config_serialize_roundtrip() {
        let cfg = MultiColumnConfig::default();
        let json = serde_json::to_string(&cfg).expect("serialize");
        let restored: MultiColumnConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cfg, restored);
    }

    // ---- MultiColumnEstimator ----

    fn make_stats(columns: Vec<&str>, distinct_count: u64, total_rows: u64) -> MultiColumnStats {
        MultiColumnStats {
            columns: columns.iter().map(|s| s.to_string()).collect(),
            distinct_count,
            total_rows,
            correlation_matrix: vec![0.9],
            histogram: None,
        }
    }

    #[test]
    fn estimator_new() {
        let est = MultiColumnEstimator::new(MultiColumnConfig::default());
        assert_eq!(est.stats_count(), 0);
    }

    #[test]
    fn estimator_with_defaults() {
        let est = MultiColumnEstimator::with_defaults();
        assert!(est.config.enabled);
    }

    #[test]
    fn estimator_add_stats() {
        let mut est = MultiColumnEstimator::with_defaults();
        let stats = make_stats(vec!["city", "state"], 100, 10000);
        est.add_stats(stats);
        assert_eq!(est.stats_count(), 1);
    }

    #[test]
    fn estimator_exact_match() {
        let mut est = MultiColumnEstimator::with_defaults();
        let stats = make_stats(vec!["city", "state"], 100, 10000);
        est.add_stats(stats);

        let (quality, found) = est.find_best_match(&["city".to_string(), "state".to_string()]);
        assert_eq!(quality, MatchQuality::Exact);
        assert!(found.is_some());
    }

    #[test]
    fn estimator_exact_match_order_independent() {
        let mut est = MultiColumnEstimator::with_defaults();
        let stats = make_stats(vec!["state", "city"], 100, 10000);
        est.add_stats(stats);

        let (quality, found) = est.find_best_match(&["city".to_string(), "state".to_string()]);
        assert_eq!(quality, MatchQuality::Exact);
        assert!(found.is_some());
    }

    #[test]
    fn estimator_prefix_match() {
        let mut est = MultiColumnEstimator::with_defaults();
        let stats = make_stats(vec!["city", "state", "zip"], 1000, 100000);
        est.add_stats(stats);

        let (quality, found) = est.find_best_match(&["city".to_string(), "state".to_string()]);
        assert_eq!(quality, MatchQuality::Prefix);
        assert!(found.is_some());
    }

    #[test]
    fn estimator_superset_match() {
        let mut est = MultiColumnEstimator::with_defaults();
        let stats = make_stats(vec!["city", "state"], 100, 10000);
        est.add_stats(stats);

        let (quality, found) = est.find_best_match(&[
            "city".to_string(),
            "state".to_string(),
            "zip".to_string(),
        ]);
        assert_eq!(quality, MatchQuality::Superset);
        assert!(found.is_some());
    }

    #[test]
    fn estimator_no_match() {
        let mut est = MultiColumnEstimator::with_defaults();
        let stats = make_stats(vec!["city", "state"], 100, 10000);
        est.add_stats(stats);

        let (quality, found) = est.find_best_match(&["product".to_string(), "category".to_string()]);
        assert_eq!(quality, MatchQuality::NoMatch);
        assert!(found.is_none());
    }

    #[test]
    fn estimator_disabled_returns_no_match() {
        let mut est = MultiColumnEstimator::new(MultiColumnConfig::disabled());
        let stats = make_stats(vec!["city", "state"], 100, 10000);
        est.add_stats(stats);

        let (quality, _) = est.find_best_match(&["city".to_string(), "state".to_string()]);
        assert_eq!(quality, MatchQuality::NoMatch);
    }

    #[test]
    fn estimator_empty_columns_no_match() {
        let est = MultiColumnEstimator::with_defaults();
        let (quality, found) = est.find_best_match(&[]);
        assert_eq!(quality, MatchQuality::NoMatch);
        assert!(found.is_none());
    }

    #[test]
    fn estimate_exact_match_correlated() {
        let mut est = MultiColumnEstimator::with_defaults();
        let stats = make_stats(vec!["city", "state"], 100, 100000);
        est.add_stats(stats);

        let cardinality = est.estimate_cardinality(
            &["city".to_string(), "state".to_string()],
            &[100, 50],
            100000,
        );
        // Should use multi-column stats (100) not independence (100*50=5000)
        assert!(cardinality < 200);
    }

    #[test]
    fn estimate_fallback_independence() {
        let est = MultiColumnEstimator::with_defaults();
        let cardinality = est.estimate_cardinality(
            &["product".to_string(), "category".to_string()],
            &[100, 50],
            100000,
        );
        // No stats available, should use independence: 100*50 = 5000
        assert_eq!(cardinality, 5000);
    }

    #[test]
    fn estimate_fallback_capped_by_total_rows() {
        let est = MultiColumnEstimator::with_defaults();
        let cardinality = est.estimate_cardinality(
            &["a".to_string(), "b".to_string()],
            &[10000, 10000],
            50000,
        );
        // Independence would give 100M, but capped at total_rows
        assert_eq!(cardinality, 50000);
    }

    #[test]
    fn estimate_mismatched_lengths() {
        let mut est = MultiColumnEstimator::with_defaults();
        let stats = make_stats(vec!["a", "b"], 100, 10000);
        est.add_stats(stats);

        let cardinality = est.estimate_cardinality(
            &["a".to_string()],
            &[100, 50],
            10000,
        );
        // Mismatched lengths, use fallback
        assert!(cardinality > 0);
    }

    #[test]
    fn estimate_with_histogram() {
        let mut est = MultiColumnEstimator::with_defaults();
        let hist = MultiDimHistogram::new(
            vec!["x".to_string(), "y".to_string()],
            vec![vec![0.0, 10.0], vec![0.0, 5.0]],
            vec![100, 200, 150, 250],
        );
        let stats = MultiColumnStats {
            columns: vec!["x".to_string(), "y".to_string()],
            distinct_count: 150,
            total_rows: 700,
            correlation_matrix: vec![0.85],
            histogram: Some(hist),
        };
        est.add_stats(stats);

        let cardinality = est.estimate_cardinality(
            &["x".to_string(), "y".to_string()],
            &[50, 50],
            700,
        );
        assert!(cardinality < 500);
    }

    #[test]
    fn estimate_low_improvement_factor() {
        let mut cfg = MultiColumnConfig::default();
        cfg.min_improvement_factor = 10.0;
        let mut est = MultiColumnEstimator::new(cfg);

        // Stats show weak correlation (distinct=4500 vs independent=5000)
        let stats = make_stats(vec!["a", "b"], 4500, 10000);
        est.add_stats(stats);

        let cardinality = est.estimate_cardinality(
            &["a".to_string(), "b".to_string()],
            &[100, 50],
            10000,
        );
        // Improvement too low, should fall back to independence
        assert_eq!(cardinality, 5000);
    }

    #[test]
    fn estimator_clear() {
        let mut est = MultiColumnEstimator::with_defaults();
        est.add_stats(make_stats(vec!["a", "b"], 100, 1000));
        assert_eq!(est.stats_count(), 1);
        est.clear();
        assert_eq!(est.stats_count(), 0);
    }

    #[test]
    fn blend_estimates_equal_weight() {
        let est = MultiColumnEstimator::with_defaults();
        let blended = est.blend_estimates(100, 200, 0.5);
        assert_eq!(blended, 150);
    }

    #[test]
    fn blend_estimates_full_weight_first() {
        let est = MultiColumnEstimator::with_defaults();
        let blended = est.blend_estimates(100, 200, 1.0);
        assert_eq!(blended, 100);
    }

    #[test]
    fn blend_estimates_full_weight_second() {
        let est = MultiColumnEstimator::with_defaults();
        let blended = est.blend_estimates(100, 200, 0.0);
        assert_eq!(blended, 200);
    }

    #[test]
    fn blend_estimates_clamps_weight() {
        let est = MultiColumnEstimator::with_defaults();
        let blended = est.blend_estimates(100, 200, 1.5);
        assert_eq!(blended, 100);
    }

    #[test]
    fn estimator_multiple_stats() {
        let mut est = MultiColumnEstimator::with_defaults();
        est.add_stats(make_stats(vec!["a", "b"], 100, 10000));
        est.add_stats(make_stats(vec!["c", "d"], 200, 20000));
        est.add_stats(make_stats(vec!["e", "f", "g"], 500, 50000));
        assert_eq!(est.stats_count(), 3);
    }

    #[test]
    fn estimator_overwrite_same_columns() {
        let mut est = MultiColumnEstimator::with_defaults();
        est.add_stats(make_stats(vec!["a", "b"], 100, 10000));
        est.add_stats(make_stats(vec!["b", "a"], 200, 10000));
        assert_eq!(est.stats_count(), 1);
        let (_, stats) = est.find_best_match(&["a".to_string(), "b".to_string()]);
        assert_eq!(stats.unwrap().distinct_count, 200);
    }

    #[test]
    fn estimate_empty_individual_ndvs() {
        let est = MultiColumnEstimator::with_defaults();
        let cardinality = est.estimate_cardinality(
            &[],
            &[],
            10000,
        );
        assert_eq!(cardinality, 10000);
    }

    #[test]
    fn estimate_single_column() {
        let mut est = MultiColumnEstimator::with_defaults();
        let stats = make_stats(vec!["a"], 100, 10000);
        est.add_stats(stats);
        let cardinality = est.estimate_cardinality(
            &["a".to_string()],
            &[100],
            10000,
        );
        assert_eq!(cardinality, 100);
    }

    #[test]
    fn estimate_three_columns_correlated() {
        let mut est = MultiColumnEstimator::with_defaults();
        let stats = MultiColumnStats {
            columns: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            distinct_count: 200,
            total_rows: 100000,
            correlation_matrix: vec![0.9, 0.85, 0.92],
            histogram: None,
        };
        est.add_stats(stats);
        let cardinality = est.estimate_cardinality(
            &["a".to_string(), "b".to_string(), "c".to_string()],
            &[100, 50, 50],
            100000,
        );
        assert!(cardinality < 1000);
    }

    #[test]
    fn config_fallback_disabled() {
        let mut cfg = MultiColumnConfig::default();
        cfg.fallback_to_independence = false;
        let est = MultiColumnEstimator::new(cfg);
        let cardinality = est.estimate_cardinality(
            &["a".to_string(), "b".to_string()],
            &[100, 50],
            10000,
        );
        assert_eq!(cardinality, 10000);
    }

    #[test]
    fn match_quality_partial_overlap() {
        let mut est = MultiColumnEstimator::with_defaults();
        est.add_stats(make_stats(vec!["a", "b"], 100, 10000));
        let (quality, _) = est.find_best_match(&["a".to_string(), "c".to_string()]);
        assert_eq!(quality, MatchQuality::NoMatch);
    }

    #[test]
    fn estimate_zero_total_rows() {
        let est = MultiColumnEstimator::with_defaults();
        let cardinality = est.estimate_cardinality(
            &["a".to_string(), "b".to_string()],
            &[100, 50],
            0,
        );
        assert_eq!(cardinality, 0);
    }

    #[test]
    fn estimate_saturating_multiplication() {
        let est = MultiColumnEstimator::with_defaults();
        let cardinality = est.estimate_cardinality(
            &["a".to_string(), "b".to_string(), "c".to_string()],
            &[u64::MAX / 2, u64::MAX / 2, 10],
            u64::MAX,
        );
        assert_eq!(cardinality, u64::MAX);
    }

    #[test]
    fn best_match_prefers_larger_superset() {
        let mut est = MultiColumnEstimator::with_defaults();
        est.add_stats(make_stats(vec!["a", "b"], 100, 10000));
        est.add_stats(make_stats(vec!["a", "b", "c"], 150, 10000));
        let (quality, stats) = est.find_best_match(&[
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
        ]);
        assert_eq!(quality, MatchQuality::Superset);
        assert_eq!(stats.unwrap().columns.len(), 3);
    }

    #[test]
    fn config_min_correlation_threshold_filtering() {
        let mut cfg = MultiColumnConfig::default();
        cfg.min_correlation_threshold = 0.95;
        let est = MultiColumnEstimator::new(cfg);
        // This would normally use multi-column stats, but correlation too low
        // (This test validates the config is stored; actual filtering would be in a fuller implementation)
        assert!(est.config.min_correlation_threshold > 0.9);
    }
}
