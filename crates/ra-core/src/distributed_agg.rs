//! Distributed aggregation strategies for query optimization.
//!
//! This module provides strategies for decomposing aggregation operations
//! across distributed nodes. The key insight is that many aggregate functions
//! can be split into local (partial) and global (final) phases, reducing
//! the amount of data shuffled across the network.
//!
//! # Strategies
//!
//! - **Two-Phase**: Local pre-aggregation followed by global finalization.
//!   Works for decomposable functions like SUM, COUNT, MIN, MAX.
//! - **Three-Phase**: Adds an intermediate shuffle phase between local
//!   dedup and global aggregation, useful for skewed keys.
//! - **Map-Reduce**: Classic map/reduce decomposition for general aggregates.
//! - **Skew-Aware**: Splits processing for hot keys and normal keys,
//!   routing hot keys through a different strategy to avoid stragglers.
//! - **Single-Phase**: Centralized aggregation for small datasets where
//!   the overhead of distribution exceeds the benefit.

use serde::{Deserialize, Serialize};

use crate::algebra::{AggregateExpr, AggregateFunction};
use crate::expr::Expr;

/// A value used for identifying hot keys in skew-aware strategies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AggValue {
    /// A null value.
    Null,
    /// An integer value.
    Int(i64),
    /// A floating-point value.
    Float(f64),
    /// A string value.
    String(String),
}

impl std::fmt::Display for AggValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => write!(f, "NULL"),
            Self::Int(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::String(v) => write!(f, "'{v}'"),
        }
    }
}

/// Strategy for executing an aggregation across distributed nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AggregationStrategy {
    /// Two-phase: local pre-aggregation + global finalization.
    ///
    /// The local phase runs on each node to produce partial aggregates,
    /// and the global phase merges them into the final result.
    TwoPhase {
        /// Aggregate expression for the local (partial) phase.
        local_agg: AggregateExpr,
        /// Aggregate expression for the global (final) phase.
        global_agg: AggregateExpr,
    },

    /// Three-phase: local + shuffle + global (for skewed keys).
    ///
    /// Adds a shuffle step between local dedup and global aggregation
    /// to handle data skew by redistributing intermediate results.
    ThreePhase {
        /// Aggregate expression for the local phase.
        local_agg: AggregateExpr,
        /// Keys used to shuffle intermediate results.
        shuffle_keys: Vec<Expr>,
        /// Aggregate expression for the global phase.
        global_agg: AggregateExpr,
    },

    /// Map-reduce style aggregation.
    ///
    /// A generalization where `map_fn` produces intermediate values
    /// and `reduce_fn` combines them.
    MapReduce {
        /// The map function (produces intermediate aggregates).
        map_fn: AggregateExpr,
        /// The reduce function (combines intermediate aggregates).
        reduce_fn: AggregateExpr,
    },

    /// Skew-aware: handle hot keys separately from normal keys.
    ///
    /// Hot keys are routed to a dedicated strategy (often replicated
    /// across multiple nodes) while normal keys use standard two-phase.
    SkewAware {
        /// Values identified as hot keys.
        hot_keys: Vec<AggValue>,
        /// Strategy for processing hot keys.
        hot_key_strategy: Box<AggregationStrategy>,
        /// Strategy for processing normal (non-hot) keys.
        normal_strategy: Box<AggregationStrategy>,
    },

    /// Single-phase centralized aggregation.
    ///
    /// All data is gathered to a single node for aggregation.
    /// Only suitable for small datasets.
    SinglePhase,
}

/// Result of decomposing an aggregate function into local and global phases.
#[derive(Debug, Clone, PartialEq)]
pub struct AggDecomposition {
    /// The function to run in the local (partial) phase.
    pub local_function: AggregateFunction,
    /// The function to run in the global (final) phase.
    pub global_function: AggregateFunction,
}

/// Result of decomposing AVG into its constituent parts.
#[derive(Debug, Clone, PartialEq)]
pub struct AvgDecomposition {
    /// Local phase produces SUM.
    pub local_sum: AggregateFunction,
    /// Local phase also produces COUNT.
    pub local_count: AggregateFunction,
    /// Global phase sums the partial sums.
    pub global_sum: AggregateFunction,
    /// Global phase sums the partial counts.
    pub global_count: AggregateFunction,
}

/// Result of decomposing STDDEV/VARIANCE into partial statistics.
#[derive(Debug, Clone, PartialEq)]
pub struct VarianceDecomposition {
    /// Local COUNT for each partition.
    pub local_count: AggregateFunction,
    /// Local SUM for each partition.
    pub local_sum: AggregateFunction,
    /// Local SUM of squares for each partition.
    pub local_sum_squares: AggregateFunction,
}

/// Configuration for the distributed aggregation optimizer.
#[derive(Debug, Clone)]
pub struct DistributedAggConfig {
    /// Minimum input rows before two-phase is worthwhile.
    pub min_rows_for_two_phase: u64,
    /// Minimum input rows before three-phase is worthwhile.
    pub min_rows_for_three_phase: u64,
    /// Maximum dataset size (rows) for single-phase aggregation.
    pub max_rows_for_single_phase: u64,
    /// Number of nodes in the cluster.
    pub num_nodes: u32,
    /// Network bandwidth in bytes per second.
    pub network_bandwidth_bps: f64,
    /// Average row size in bytes.
    pub avg_row_bytes: f64,
}

impl Default for DistributedAggConfig {
    fn default() -> Self {
        Self {
            min_rows_for_two_phase: 1_000_000,
            min_rows_for_three_phase: 10_000_000,
            max_rows_for_single_phase: 100_000,
            num_nodes: 8,
            network_bandwidth_bps: 1_250_000_000.0, // 10 Gbps
            avg_row_bytes: 100.0,
        }
    }
}

impl AggregationStrategy {
    /// Check whether an aggregate function is decomposable into
    /// local and global phases.
    #[must_use]
    pub fn is_decomposable(func: AggregateFunction) -> bool {
        matches!(
            func,
            AggregateFunction::Count
                | AggregateFunction::Sum
                | AggregateFunction::Min
                | AggregateFunction::Max
                | AggregateFunction::Avg
        )
    }

    /// Decompose an aggregate function into local (partial) and
    /// global (final) phases.
    ///
    /// Returns `None` for non-decomposable functions like `StdDev`
    /// or `StringAgg`.
    #[must_use]
    pub fn decompose_aggregate(
        func: AggregateFunction,
    ) -> Option<AggDecomposition> {
        match func {
            AggregateFunction::Count => Some(AggDecomposition {
                local_function: AggregateFunction::Count,
                global_function: AggregateFunction::Sum,
            }),
            // AVG decomposes the same as SUM here; the full
            // AVG decomposition (SUM + COUNT) is in decompose_avg().
            AggregateFunction::Sum | AggregateFunction::Avg => {
                Some(AggDecomposition {
                    local_function: AggregateFunction::Sum,
                    global_function: AggregateFunction::Sum,
                })
            }
            AggregateFunction::Min => Some(AggDecomposition {
                local_function: AggregateFunction::Min,
                global_function: AggregateFunction::Min,
            }),
            AggregateFunction::Max => Some(AggDecomposition {
                local_function: AggregateFunction::Max,
                global_function: AggregateFunction::Max,
            }),
            AggregateFunction::StdDev
            | AggregateFunction::Variance
            | AggregateFunction::StringAgg
            | AggregateFunction::ArrayAgg => None,
        }
    }

    /// Decompose AVG into SUM and COUNT for two-phase execution.
    ///
    /// AVG cannot be directly decomposed as a single local/global pair.
    /// Instead, the local phase produces both SUM(x) and COUNT(x),
    /// and the global phase computes SUM(sums) / SUM(counts).
    #[must_use]
    pub fn decompose_avg() -> AvgDecomposition {
        AvgDecomposition {
            local_sum: AggregateFunction::Sum,
            local_count: AggregateFunction::Count,
            global_sum: AggregateFunction::Sum,
            global_count: AggregateFunction::Sum,
        }
    }

    /// Decompose STDDEV/VARIANCE into partial statistics.
    ///
    /// Requires COUNT, SUM, and SUM of squares in the local phase.
    /// The global phase combines these using the parallel variance
    /// algorithm (Chan et al.).
    #[must_use]
    pub fn decompose_variance() -> VarianceDecomposition {
        VarianceDecomposition {
            local_count: AggregateFunction::Count,
            local_sum: AggregateFunction::Sum,
            local_sum_squares: AggregateFunction::Sum,
        }
    }

    /// Choose the best aggregation strategy based on data characteristics.
    ///
    /// Considers input size, number of groups, skew, and cluster config.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn choose_strategy(
        func: AggregateFunction,
        input_rows: u64,
        _distinct_groups: u64,
        has_skew: bool,
        config: &DistributedAggConfig,
    ) -> Self {
        if input_rows <= config.max_rows_for_single_phase {
            return Self::SinglePhase;
        }

        if !Self::is_decomposable(func) {
            return Self::SinglePhase;
        }

        if has_skew && input_rows >= config.min_rows_for_three_phase {
            return Self::default_three_phase(func);
        }

        if input_rows >= config.min_rows_for_two_phase {
            return Self::default_two_phase(func);
        }

        Self::SinglePhase
    }

    /// Estimate the network cost reduction from using two-phase
    /// aggregation compared to single-phase.
    ///
    /// Returns a fraction from 0.0 (no benefit) to 1.0 (maximum benefit).
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn estimated_benefit(
        input_rows: u64,
        distinct_groups: u64,
        config: &DistributedAggConfig,
    ) -> f64 {
        if config.num_nodes <= 1 || input_rows == 0 {
            return 0.0;
        }

        let num_nodes = f64::from(config.num_nodes);
        let shuffle_fraction = (num_nodes - 1.0) / num_nodes;

        let cost_single =
            input_rows as f64 * config.avg_row_bytes * shuffle_fraction
                / config.network_bandwidth_bps;

        let partial_rows = distinct_groups as f64 * num_nodes;
        let cost_two_phase =
            partial_rows * config.avg_row_bytes * shuffle_fraction
                / config.network_bandwidth_bps;

        if cost_single <= 0.0 {
            return 0.0;
        }

        ((cost_single - cost_two_phase) / cost_single).clamp(0.0, 1.0)
    }

    /// Estimate the cost of three-phase aggregation for distinct counts.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn three_phase_cost(
        input_rows: u64,
        distinct_values: u64,
        groups: u64,
        config: &DistributedAggConfig,
    ) -> f64 {
        if config.num_nodes <= 1 {
            return 0.0;
        }

        let num_nodes = f64::from(config.num_nodes);
        let shuffle_fraction = (num_nodes - 1.0) / num_nodes;

        let dedup_rows_per_node =
            (distinct_values as f64).min(input_rows as f64 / num_nodes);
        let dedup_rows = dedup_rows_per_node * num_nodes;

        let shuffle_1 = dedup_rows * config.avg_row_bytes
            * shuffle_fraction
            / config.network_bandwidth_bps;

        let key_bytes = 16.0;
        let shuffle_2 = groups as f64 * num_nodes * key_bytes
            * shuffle_fraction
            / config.network_bandwidth_bps;

        shuffle_1 + shuffle_2
    }

    /// Create a default two-phase strategy for a given aggregate function.
    fn default_two_phase(func: AggregateFunction) -> Self {
        if let Some(decomp) = Self::decompose_aggregate(func) {
            Self::TwoPhase {
                local_agg: AggregateExpr {
                    function: decomp.local_function,
                    arg: None,
                    distinct: false,
                    alias: Some("partial".to_owned()),
                },
                global_agg: AggregateExpr {
                    function: decomp.global_function,
                    arg: None,
                    distinct: false,
                    alias: Some("final".to_owned()),
                },
            }
        } else {
            Self::SinglePhase
        }
    }

    /// Create a default three-phase strategy for a given aggregate function.
    fn default_three_phase(func: AggregateFunction) -> Self {
        if let Some(decomp) = Self::decompose_aggregate(func) {
            Self::ThreePhase {
                local_agg: AggregateExpr {
                    function: decomp.local_function,
                    arg: None,
                    distinct: false,
                    alias: Some("partial".to_owned()),
                },
                shuffle_keys: Vec::new(),
                global_agg: AggregateExpr {
                    function: decomp.global_function,
                    arg: None,
                    distinct: false,
                    alias: Some("final".to_owned()),
                },
            }
        } else {
            Self::SinglePhase
        }
    }
}

/// Check whether a set of aggregate expressions are all decomposable.
#[must_use]
pub fn all_decomposable(aggs: &[AggregateExpr]) -> bool {
    aggs.iter()
        .all(|a| AggregationStrategy::is_decomposable(a.function))
}

/// Decompose multiple aggregate expressions for two-phase execution.
///
/// Returns pairs of (`local_agg`, `global_agg`) for each input aggregate,
/// or `None` if any aggregate is not decomposable.
#[must_use]
pub fn decompose_all(
    aggs: &[AggregateExpr],
) -> Option<Vec<(AggregateExpr, AggregateExpr)>> {
    let mut result = Vec::with_capacity(aggs.len());

    for agg in aggs {
        if agg.function == AggregateFunction::Avg {
            let avg = AggregationStrategy::decompose_avg();
            result.push((
                AggregateExpr {
                    function: avg.local_sum,
                    arg: agg.arg.clone(),
                    distinct: false,
                    alias: Some(format!(
                        "partial_sum_{}",
                        agg.alias.as_deref().unwrap_or("col")
                    )),
                },
                AggregateExpr {
                    function: avg.global_sum,
                    arg: None,
                    distinct: false,
                    alias: agg.alias.clone(),
                },
            ));
            result.push((
                AggregateExpr {
                    function: avg.local_count,
                    arg: agg.arg.clone(),
                    distinct: false,
                    alias: Some(format!(
                        "partial_count_{}",
                        agg.alias.as_deref().unwrap_or("col")
                    )),
                },
                AggregateExpr {
                    function: avg.global_count,
                    arg: None,
                    distinct: false,
                    alias: None,
                },
            ));
        } else {
            let decomp =
                AggregationStrategy::decompose_aggregate(agg.function)?;
            result.push((
                AggregateExpr {
                    function: decomp.local_function,
                    arg: agg.arg.clone(),
                    distinct: false,
                    alias: Some(format!(
                        "partial_{}",
                        agg.alias.as_deref().unwrap_or("col")
                    )),
                },
                AggregateExpr {
                    function: decomp.global_function,
                    arg: None,
                    distinct: false,
                    alias: agg.alias.clone(),
                },
            ));
        }
    }

    Some(result)
}

/// Estimate reduction ratio for two-phase aggregation.
///
/// Returns the fraction of data that does NOT need to be shuffled
/// (0.0 = no reduction, 1.0 = maximum reduction).
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn reduction_ratio(input_rows: u64, distinct_groups: u64) -> f64 {
    if input_rows == 0 {
        return 0.0;
    }
    let ratio = 1.0 - (distinct_groups as f64 / input_rows as f64);
    ratio.clamp(0.0, 1.0)
}

/// Determine whether two-phase aggregation is worthwhile given
/// the reduction ratio and cluster characteristics.
#[must_use]
pub fn is_two_phase_worthwhile(
    input_rows: u64,
    distinct_groups: u64,
    config: &DistributedAggConfig,
) -> bool {
    if input_rows < config.min_rows_for_two_phase {
        return false;
    }
    let ratio = reduction_ratio(input_rows, distinct_groups);
    ratio > 0.5
}

#[cfg(test)]
#[expect(clippy::float_cmp, reason = "exact float equality needed for deterministic cost model tests")]
mod tests {
    use super::*;

    #[test]
    fn decomposable_functions() {
        assert!(AggregationStrategy::is_decomposable(
            AggregateFunction::Count
        ));
        assert!(AggregationStrategy::is_decomposable(
            AggregateFunction::Sum
        ));
        assert!(AggregationStrategy::is_decomposable(
            AggregateFunction::Min
        ));
        assert!(AggregationStrategy::is_decomposable(
            AggregateFunction::Max
        ));
        assert!(AggregationStrategy::is_decomposable(
            AggregateFunction::Avg
        ));
    }

    #[test]
    fn non_decomposable_functions() {
        assert!(!AggregationStrategy::is_decomposable(
            AggregateFunction::StdDev
        ));
        assert!(!AggregationStrategy::is_decomposable(
            AggregateFunction::Variance
        ));
        assert!(!AggregationStrategy::is_decomposable(
            AggregateFunction::StringAgg
        ));
        assert!(!AggregationStrategy::is_decomposable(
            AggregateFunction::ArrayAgg
        ));
    }

    #[test]
    fn decompose_count() {
        let d = AggregationStrategy::decompose_aggregate(
            AggregateFunction::Count,
        )
        .expect("COUNT should be decomposable");
        assert_eq!(d.local_function, AggregateFunction::Count);
        assert_eq!(d.global_function, AggregateFunction::Sum);
    }

    #[test]
    fn decompose_sum() {
        let d = AggregationStrategy::decompose_aggregate(
            AggregateFunction::Sum,
        )
        .expect("SUM should be decomposable");
        assert_eq!(d.local_function, AggregateFunction::Sum);
        assert_eq!(d.global_function, AggregateFunction::Sum);
    }

    #[test]
    fn decompose_min() {
        let d = AggregationStrategy::decompose_aggregate(
            AggregateFunction::Min,
        )
        .expect("MIN should be decomposable");
        assert_eq!(d.local_function, AggregateFunction::Min);
        assert_eq!(d.global_function, AggregateFunction::Min);
    }

    #[test]
    fn decompose_max() {
        let d = AggregationStrategy::decompose_aggregate(
            AggregateFunction::Max,
        )
        .expect("MAX should be decomposable");
        assert_eq!(d.local_function, AggregateFunction::Max);
        assert_eq!(d.global_function, AggregateFunction::Max);
    }

    #[test]
    fn decompose_avg_returns_sum() {
        let d = AggregationStrategy::decompose_aggregate(
            AggregateFunction::Avg,
        )
        .expect("AVG should be decomposable (via SUM)");
        assert_eq!(d.local_function, AggregateFunction::Sum);
        assert_eq!(d.global_function, AggregateFunction::Sum);
    }

    #[test]
    fn decompose_stddev_returns_none() {
        assert!(AggregationStrategy::decompose_aggregate(
            AggregateFunction::StdDev
        )
        .is_none());
    }

    #[test]
    fn decompose_variance_returns_none() {
        assert!(AggregationStrategy::decompose_aggregate(
            AggregateFunction::Variance
        )
        .is_none());
    }

    #[test]
    fn decompose_string_agg_returns_none() {
        assert!(AggregationStrategy::decompose_aggregate(
            AggregateFunction::StringAgg
        )
        .is_none());
    }

    #[test]
    fn avg_decomposition_parts() {
        let d = AggregationStrategy::decompose_avg();
        assert_eq!(d.local_sum, AggregateFunction::Sum);
        assert_eq!(d.local_count, AggregateFunction::Count);
        assert_eq!(d.global_sum, AggregateFunction::Sum);
        assert_eq!(d.global_count, AggregateFunction::Sum);
    }

    #[test]
    fn variance_decomposition_parts() {
        let d = AggregationStrategy::decompose_variance();
        assert_eq!(d.local_count, AggregateFunction::Count);
        assert_eq!(d.local_sum, AggregateFunction::Sum);
        assert_eq!(d.local_sum_squares, AggregateFunction::Sum);
    }

    #[test]
    fn choose_single_phase_small_data() {
        let config = DistributedAggConfig::default();
        let strategy = AggregationStrategy::choose_strategy(
            AggregateFunction::Sum,
            1000,
            10,
            false,
            &config,
        );
        assert_eq!(strategy, AggregationStrategy::SinglePhase);
    }

    #[test]
    fn choose_two_phase_large_data() {
        let config = DistributedAggConfig::default();
        let strategy = AggregationStrategy::choose_strategy(
            AggregateFunction::Sum,
            5_000_000,
            100,
            false,
            &config,
        );
        assert!(matches!(strategy, AggregationStrategy::TwoPhase { .. }));
    }

    #[test]
    fn choose_three_phase_skewed() {
        let config = DistributedAggConfig::default();
        let strategy = AggregationStrategy::choose_strategy(
            AggregateFunction::Sum,
            50_000_000,
            1000,
            true,
            &config,
        );
        assert!(matches!(
            strategy,
            AggregationStrategy::ThreePhase { .. }
        ));
    }

    #[test]
    fn choose_single_phase_non_decomposable() {
        let config = DistributedAggConfig::default();
        let strategy = AggregationStrategy::choose_strategy(
            AggregateFunction::StdDev,
            50_000_000,
            1000,
            false,
            &config,
        );
        assert_eq!(strategy, AggregationStrategy::SinglePhase);
    }

    #[test]
    fn estimated_benefit_high_reduction() {
        let config = DistributedAggConfig {
            num_nodes: 100,
            ..DistributedAggConfig::default()
        };
        let benefit = AggregationStrategy::estimated_benefit(
            1_000_000_000,
            10_000,
            &config,
        );
        assert!(benefit > 0.99, "Expected >99% benefit, got {benefit}");
    }

    #[test]
    fn estimated_benefit_no_reduction() {
        let config = DistributedAggConfig {
            num_nodes: 100,
            ..DistributedAggConfig::default()
        };
        let benefit = AggregationStrategy::estimated_benefit(
            1000,
            1000,
            &config,
        );
        assert!(
            benefit < 0.1,
            "Expected low benefit with no reduction, got {benefit}"
        );
    }

    #[test]
    fn estimated_benefit_single_node() {
        let config = DistributedAggConfig {
            num_nodes: 1,
            ..DistributedAggConfig::default()
        };
        let benefit = AggregationStrategy::estimated_benefit(
            1_000_000,
            100,
            &config,
        );
        assert_eq!(benefit, 0.0);
    }

    #[test]
    fn estimated_benefit_zero_rows() {
        let config = DistributedAggConfig::default();
        let benefit =
            AggregationStrategy::estimated_benefit(0, 0, &config);
        assert_eq!(benefit, 0.0);
    }

    #[test]
    fn three_phase_cost_basic() {
        let config = DistributedAggConfig {
            num_nodes: 10,
            ..DistributedAggConfig::default()
        };
        let cost = AggregationStrategy::three_phase_cost(
            1_000_000_000,
            1_000_000,
            100,
            &config,
        );
        assert!(cost > 0.0);
    }

    #[test]
    fn three_phase_cost_single_node() {
        let config = DistributedAggConfig {
            num_nodes: 1,
            ..DistributedAggConfig::default()
        };
        let cost = AggregationStrategy::three_phase_cost(
            1_000_000, 10_000, 100, &config,
        );
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn all_decomposable_empty() {
        assert!(all_decomposable(&[]));
    }

    #[test]
    fn all_decomposable_sum_count() {
        let aggs = vec![
            AggregateExpr {
                function: AggregateFunction::Sum,
                arg: None,
                distinct: false,
                alias: None,
            },
            AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: None,
            },
        ];
        assert!(all_decomposable(&aggs));
    }

    #[test]
    fn all_decomposable_with_stddev() {
        let aggs = vec![
            AggregateExpr {
                function: AggregateFunction::Sum,
                arg: None,
                distinct: false,
                alias: None,
            },
            AggregateExpr {
                function: AggregateFunction::StdDev,
                arg: None,
                distinct: false,
                alias: None,
            },
        ];
        assert!(!all_decomposable(&aggs));
    }

    #[test]
    fn decompose_all_sum_only() {
        let aggs = vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: None,
            distinct: false,
            alias: Some("total".to_owned()),
        }];
        let result = decompose_all(&aggs).expect("should decompose");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.function, AggregateFunction::Sum);
        assert_eq!(result[0].1.function, AggregateFunction::Sum);
    }

    #[test]
    fn decompose_all_avg_expands() {
        let aggs = vec![AggregateExpr {
            function: AggregateFunction::Avg,
            arg: None,
            distinct: false,
            alias: Some("avg_val".to_owned()),
        }];
        let result = decompose_all(&aggs).expect("should decompose");
        assert_eq!(result.len(), 2, "AVG should expand to SUM + COUNT");
        assert_eq!(result[0].0.function, AggregateFunction::Sum);
        assert_eq!(result[1].0.function, AggregateFunction::Count);
    }

    #[test]
    fn decompose_all_with_stddev_fails() {
        let aggs = vec![AggregateExpr {
            function: AggregateFunction::StdDev,
            arg: None,
            distinct: false,
            alias: None,
        }];
        assert!(decompose_all(&aggs).is_none());
    }

    #[test]
    fn reduction_ratio_high() {
        let ratio = reduction_ratio(1_000_000, 100);
        assert!(
            (ratio - 0.9999).abs() < 0.001,
            "Expected ~1.0, got {ratio}"
        );
    }

    #[test]
    fn reduction_ratio_no_reduction() {
        let ratio = reduction_ratio(1000, 1000);
        assert_eq!(ratio, 0.0);
    }

    #[test]
    fn reduction_ratio_zero_rows() {
        assert_eq!(reduction_ratio(0, 0), 0.0);
    }

    #[test]
    fn reduction_ratio_more_groups_than_rows() {
        let ratio = reduction_ratio(100, 200);
        assert_eq!(ratio, 0.0, "Should clamp to 0.0");
    }

    #[test]
    fn two_phase_worthwhile_large_reduction() {
        let config = DistributedAggConfig::default();
        assert!(is_two_phase_worthwhile(5_000_000, 1000, &config));
    }

    #[test]
    fn two_phase_not_worthwhile_small_data() {
        let config = DistributedAggConfig::default();
        assert!(!is_two_phase_worthwhile(1000, 10, &config));
    }

    #[test]
    fn two_phase_not_worthwhile_low_reduction() {
        let config = DistributedAggConfig::default();
        assert!(!is_two_phase_worthwhile(
            2_000_000, 1_500_000, &config
        ));
    }

    #[test]
    fn agg_value_display_null() {
        assert_eq!(AggValue::Null.to_string(), "NULL");
    }

    #[test]
    fn agg_value_display_int() {
        assert_eq!(AggValue::Int(42).to_string(), "42");
    }

    #[test]
    #[expect(clippy::approx_constant, reason = "3.14 is test data, not mathematical constant")]
    fn agg_value_display_float() {
        assert_eq!(AggValue::Float(3.14).to_string(), "3.14");
    }

    #[test]
    fn agg_value_display_string() {
        assert_eq!(
            AggValue::String("hello".to_owned()).to_string(),
            "'hello'"
        );
    }

    #[test]
    fn strategy_serialize_roundtrip_single() {
        let s = AggregationStrategy::SinglePhase;
        let json = serde_json::to_string(&s)
            .expect("serialize should succeed");
        let d: AggregationStrategy = serde_json::from_str(&json)
            .expect("deserialize should succeed");
        assert_eq!(s, d);
    }

    #[test]
    fn strategy_serialize_roundtrip_two_phase() {
        let s = AggregationStrategy::TwoPhase {
            local_agg: AggregateExpr {
                function: AggregateFunction::Sum,
                arg: None,
                distinct: false,
                alias: Some("partial".to_owned()),
            },
            global_agg: AggregateExpr {
                function: AggregateFunction::Sum,
                arg: None,
                distinct: false,
                alias: Some("final".to_owned()),
            },
        };
        let json = serde_json::to_string(&s)
            .expect("serialize should succeed");
        let d: AggregationStrategy = serde_json::from_str(&json)
            .expect("deserialize should succeed");
        assert_eq!(s, d);
    }

    #[test]
    fn strategy_serialize_roundtrip_skew_aware() {
        let s = AggregationStrategy::SkewAware {
            hot_keys: vec![
                AggValue::String("UNKNOWN".to_owned()),
                AggValue::Null,
            ],
            hot_key_strategy: Box::new(
                AggregationStrategy::SinglePhase,
            ),
            normal_strategy: Box::new(AggregationStrategy::TwoPhase {
                local_agg: AggregateExpr {
                    function: AggregateFunction::Sum,
                    arg: None,
                    distinct: false,
                    alias: None,
                },
                global_agg: AggregateExpr {
                    function: AggregateFunction::Sum,
                    arg: None,
                    distinct: false,
                    alias: None,
                },
            }),
        };
        let json = serde_json::to_string(&s)
            .expect("serialize should succeed");
        let d: AggregationStrategy = serde_json::from_str(&json)
            .expect("deserialize should succeed");
        assert_eq!(s, d);
    }

    #[test]
    fn default_config_values() {
        let config = DistributedAggConfig::default();
        assert_eq!(config.min_rows_for_two_phase, 1_000_000);
        assert_eq!(config.min_rows_for_three_phase, 10_000_000);
        assert_eq!(config.max_rows_for_single_phase, 100_000);
        assert_eq!(config.num_nodes, 8);
        assert_eq!(config.avg_row_bytes, 100.0);
    }

    #[test]
    fn decompose_all_mixed_sum_and_count() {
        let aggs = vec![
            AggregateExpr {
                function: AggregateFunction::Sum,
                arg: None,
                distinct: false,
                alias: Some("total_sales".to_owned()),
            },
            AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("num_orders".to_owned()),
            },
        ];
        let result = decompose_all(&aggs).expect("should decompose");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0.function, AggregateFunction::Sum);
        assert_eq!(result[0].1.function, AggregateFunction::Sum);
        assert_eq!(result[1].0.function, AggregateFunction::Count);
        assert_eq!(result[1].1.function, AggregateFunction::Sum);
    }

    #[test]
    fn decompose_all_min_max() {
        let aggs = vec![
            AggregateExpr {
                function: AggregateFunction::Min,
                arg: None,
                distinct: false,
                alias: Some("lowest".to_owned()),
            },
            AggregateExpr {
                function: AggregateFunction::Max,
                arg: None,
                distinct: false,
                alias: Some("highest".to_owned()),
            },
        ];
        let result = decompose_all(&aggs).expect("should decompose");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0.function, AggregateFunction::Min);
        assert_eq!(result[0].1.function, AggregateFunction::Min);
        assert_eq!(result[1].0.function, AggregateFunction::Max);
        assert_eq!(result[1].1.function, AggregateFunction::Max);
    }

    #[test]
    fn choose_strategy_boundary_two_phase() {
        let config = DistributedAggConfig {
            min_rows_for_two_phase: 1_000_000,
            max_rows_for_single_phase: 100_000,
            ..DistributedAggConfig::default()
        };
        let strategy = AggregationStrategy::choose_strategy(
            AggregateFunction::Sum,
            1_000_000,
            100,
            false,
            &config,
        );
        assert!(matches!(strategy, AggregationStrategy::TwoPhase { .. }));
    }

    #[test]
    fn choose_strategy_between_single_and_two_phase() {
        let config = DistributedAggConfig {
            min_rows_for_two_phase: 1_000_000,
            max_rows_for_single_phase: 100_000,
            ..DistributedAggConfig::default()
        };
        let strategy = AggregationStrategy::choose_strategy(
            AggregateFunction::Sum,
            500_000,
            100,
            false,
            &config,
        );
        assert_eq!(strategy, AggregationStrategy::SinglePhase);
    }

    #[test]
    fn benefit_increases_with_more_groups() {
        let config = DistributedAggConfig {
            num_nodes: 10,
            ..DistributedAggConfig::default()
        };
        let benefit_few = AggregationStrategy::estimated_benefit(
            1_000_000,
            100,
            &config,
        );
        let benefit_many = AggregationStrategy::estimated_benefit(
            1_000_000,
            500_000,
            &config,
        );
        assert!(
            benefit_few >= benefit_many,
            "Fewer groups => higher benefit: few={benefit_few}, many={benefit_many}"
        );
    }
}
