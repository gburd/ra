//! Integration tests for distributed aggregation strategies.

use ra_core::algebra::{AggregateExpr, AggregateFunction, RelExpr};
use ra_core::distributed_agg::{
    all_decomposable, decompose_all, is_two_phase_worthwhile,
    reduction_ratio, AggValue, AggregationStrategy,
    DistributedAggConfig,
};
use ra_core::expr::{ColumnRef, Expr};

fn make_agg(func: AggregateFunction, alias: &str) -> AggregateExpr {
    AggregateExpr {
        function: func,
        arg: Some(Expr::Column(ColumnRef::new("col"))),
        distinct: false,
        alias: Some(alias.to_owned()),
    }
}

fn make_agg_no_arg(
    func: AggregateFunction,
    alias: &str,
) -> AggregateExpr {
    AggregateExpr {
        function: func,
        arg: None,
        distinct: false,
        alias: Some(alias.to_owned()),
    }
}

fn default_config() -> DistributedAggConfig {
    DistributedAggConfig::default()
}

// --- Strategy selection integration tests ---

#[test]
fn sum_on_large_dataset_chooses_two_phase() {
    let config = default_config();
    let s = AggregationStrategy::choose_strategy(
        AggregateFunction::Sum,
        10_000_000,
        1000,
        false,
        &config,
    );
    assert!(matches!(s, AggregationStrategy::TwoPhase { .. }));
}

#[test]
fn count_on_large_dataset_chooses_two_phase() {
    let config = default_config();
    let s = AggregationStrategy::choose_strategy(
        AggregateFunction::Count,
        5_000_000,
        50,
        false,
        &config,
    );
    assert!(matches!(s, AggregationStrategy::TwoPhase { .. }));
}

#[test]
fn min_on_large_dataset_chooses_two_phase() {
    let config = default_config();
    let s = AggregationStrategy::choose_strategy(
        AggregateFunction::Min,
        2_000_000,
        200,
        false,
        &config,
    );
    assert!(matches!(s, AggregationStrategy::TwoPhase { .. }));
}

#[test]
fn max_on_large_dataset_chooses_two_phase() {
    let config = default_config();
    let s = AggregationStrategy::choose_strategy(
        AggregateFunction::Max,
        2_000_000,
        200,
        false,
        &config,
    );
    assert!(matches!(s, AggregationStrategy::TwoPhase { .. }));
}

#[test]
fn avg_on_large_dataset_chooses_two_phase() {
    let config = default_config();
    let s = AggregationStrategy::choose_strategy(
        AggregateFunction::Avg,
        3_000_000,
        100,
        false,
        &config,
    );
    assert!(matches!(s, AggregationStrategy::TwoPhase { .. }));
}

#[test]
fn sum_on_small_dataset_chooses_single_phase() {
    let config = default_config();
    let s = AggregationStrategy::choose_strategy(
        AggregateFunction::Sum,
        50_000,
        100,
        false,
        &config,
    );
    assert_eq!(s, AggregationStrategy::SinglePhase);
}

#[test]
fn skewed_large_sum_chooses_three_phase() {
    let config = default_config();
    let s = AggregationStrategy::choose_strategy(
        AggregateFunction::Sum,
        100_000_000,
        10_000,
        true,
        &config,
    );
    assert!(matches!(s, AggregationStrategy::ThreePhase { .. }));
}

#[test]
fn skewed_but_small_stays_single_phase() {
    let config = default_config();
    let s = AggregationStrategy::choose_strategy(
        AggregateFunction::Sum,
        1000,
        10,
        true,
        &config,
    );
    assert_eq!(s, AggregationStrategy::SinglePhase);
}

#[test]
fn stddev_always_single_phase() {
    let config = default_config();
    let s = AggregationStrategy::choose_strategy(
        AggregateFunction::StdDev,
        100_000_000,
        100,
        false,
        &config,
    );
    assert_eq!(s, AggregationStrategy::SinglePhase);
}

#[test]
fn variance_always_single_phase() {
    let config = default_config();
    let s = AggregationStrategy::choose_strategy(
        AggregateFunction::Variance,
        100_000_000,
        100,
        false,
        &config,
    );
    assert_eq!(s, AggregationStrategy::SinglePhase);
}

#[test]
fn string_agg_always_single_phase() {
    let config = default_config();
    let s = AggregationStrategy::choose_strategy(
        AggregateFunction::StringAgg,
        100_000_000,
        100,
        false,
        &config,
    );
    assert_eq!(s, AggregationStrategy::SinglePhase);
}

#[test]
fn array_agg_always_single_phase() {
    let config = default_config();
    let s = AggregationStrategy::choose_strategy(
        AggregateFunction::ArrayAgg,
        100_000_000,
        100,
        false,
        &config,
    );
    assert_eq!(s, AggregationStrategy::SinglePhase);
}

// --- Decomposition integration tests ---

#[test]
fn decompose_select_region_sum_amount() {
    let aggs = vec![make_agg(AggregateFunction::Sum, "total_amount")];
    let result = decompose_all(&aggs).expect("should decompose");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0.function, AggregateFunction::Sum);
    assert_eq!(result[0].1.function, AggregateFunction::Sum);
}

#[test]
fn decompose_select_region_count_star() {
    let aggs =
        vec![make_agg_no_arg(AggregateFunction::Count, "num_orders")];
    let result = decompose_all(&aggs).expect("should decompose");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0.function, AggregateFunction::Count);
    assert_eq!(result[0].1.function, AggregateFunction::Sum);
}

#[test]
fn decompose_select_region_avg_amount() {
    let aggs = vec![make_agg(AggregateFunction::Avg, "avg_amount")];
    let result = decompose_all(&aggs).expect("should decompose");
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].0.function, AggregateFunction::Sum);
    assert_eq!(result[1].0.function, AggregateFunction::Count);
}

#[test]
fn decompose_mixed_sum_count_avg() {
    let aggs = vec![
        make_agg(AggregateFunction::Sum, "total"),
        make_agg_no_arg(AggregateFunction::Count, "cnt"),
        make_agg(AggregateFunction::Avg, "avg_val"),
    ];
    let result = decompose_all(&aggs).expect("should decompose");
    // SUM -> 1 pair, COUNT -> 1 pair, AVG -> 2 pairs
    assert_eq!(result.len(), 4);
}

#[test]
fn decompose_fails_with_stddev() {
    let aggs = vec![
        make_agg(AggregateFunction::Sum, "total"),
        make_agg(AggregateFunction::StdDev, "stddev_val"),
    ];
    assert!(decompose_all(&aggs).is_none());
}

#[test]
fn decompose_sum_count_min_max() {
    let aggs = vec![
        make_agg(AggregateFunction::Sum, "s"),
        make_agg_no_arg(AggregateFunction::Count, "c"),
        make_agg(AggregateFunction::Min, "lo"),
        make_agg(AggregateFunction::Max, "hi"),
    ];
    let result = decompose_all(&aggs).expect("should decompose");
    assert_eq!(result.len(), 4);
    assert_eq!(result[2].0.function, AggregateFunction::Min);
    assert_eq!(result[2].1.function, AggregateFunction::Min);
    assert_eq!(result[3].0.function, AggregateFunction::Max);
    assert_eq!(result[3].1.function, AggregateFunction::Max);
}

// --- all_decomposable integration tests ---

#[test]
fn all_decomposable_basic_aggs() {
    let aggs = vec![
        make_agg(AggregateFunction::Sum, "s"),
        make_agg_no_arg(AggregateFunction::Count, "c"),
        make_agg(AggregateFunction::Min, "lo"),
        make_agg(AggregateFunction::Max, "hi"),
        make_agg(AggregateFunction::Avg, "a"),
    ];
    assert!(all_decomposable(&aggs));
}

#[test]
fn not_all_decomposable_with_array_agg() {
    let aggs = vec![
        make_agg(AggregateFunction::Sum, "s"),
        make_agg(AggregateFunction::ArrayAgg, "arr"),
    ];
    assert!(!all_decomposable(&aggs));
}

// --- Benefit estimation integration tests ---

#[test]
fn benefit_1b_rows_10k_groups_100_nodes() {
    let config = DistributedAggConfig {
        num_nodes: 100,
        ..default_config()
    };
    let benefit = AggregationStrategy::estimated_benefit(
        1_000_000_000,
        10_000,
        &config,
    );
    assert!(benefit > 0.99);
}

#[test]
fn benefit_1m_rows_1m_groups_no_reduction() {
    let config = DistributedAggConfig {
        num_nodes: 10,
        ..default_config()
    };
    let benefit = AggregationStrategy::estimated_benefit(
        1_000_000,
        1_000_000,
        &config,
    );
    assert!(benefit < 0.01);
}

#[test]
fn benefit_moderate_reduction() {
    let config = DistributedAggConfig {
        num_nodes: 8,
        ..default_config()
    };
    let benefit = AggregationStrategy::estimated_benefit(
        10_000_000,
        100_000,
        &config,
    );
    assert!(benefit > 0.9);
}

// --- Reduction ratio integration tests ---

#[test]
fn reduction_1b_rows_10k_groups() {
    let ratio = reduction_ratio(1_000_000_000, 10_000);
    assert!(ratio > 0.999);
}

#[test]
fn reduction_100k_rows_100k_groups() {
    let ratio = reduction_ratio(100_000, 100_000);
    assert_eq!(ratio, 0.0);
}

#[test]
fn reduction_10m_rows_1m_groups() {
    let ratio = reduction_ratio(10_000_000, 1_000_000);
    assert!((ratio - 0.9).abs() < 0.01);
}

// --- is_two_phase_worthwhile integration tests ---

#[test]
fn two_phase_worthwhile_high_reduction() {
    let config = default_config();
    assert!(is_two_phase_worthwhile(10_000_000, 1000, &config));
}

#[test]
fn two_phase_not_worthwhile_equal_groups() {
    let config = default_config();
    assert!(!is_two_phase_worthwhile(5_000_000, 4_000_000, &config));
}

#[test]
fn two_phase_not_worthwhile_below_threshold() {
    let config = default_config();
    assert!(!is_two_phase_worthwhile(500_000, 100, &config));
}

// --- Three-phase cost estimation integration tests ---

#[test]
fn three_phase_cost_decreases_with_dedup() {
    let config = DistributedAggConfig {
        num_nodes: 10,
        ..default_config()
    };
    // High duplication: cost should be lower
    let cost_high_dup = AggregationStrategy::three_phase_cost(
        100_000_000,
        100_000,
        50,
        &config,
    );
    // Low duplication: cost should be higher
    let cost_low_dup = AggregationStrategy::three_phase_cost(
        100_000_000,
        50_000_000,
        50,
        &config,
    );
    assert!(
        cost_high_dup < cost_low_dup,
        "More duplication should mean lower three-phase cost"
    );
}

// --- Strategy with RelExpr integration tests ---

#[test]
fn two_phase_strategy_for_aggregate_relexpr() {
    let plan = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("region"))],
        aggregates: vec![make_agg(AggregateFunction::Sum, "total")],
        input: Box::new(RelExpr::scan("orders")),
    };

    if let RelExpr::Aggregate { aggregates, .. } = &plan {
        assert!(all_decomposable(aggregates));
        let decomposed =
            decompose_all(aggregates).expect("should decompose");
        assert_eq!(decomposed.len(), 1);
    }
}

#[test]
fn multi_agg_relexpr_decomposition() {
    let plan = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("country"))],
        aggregates: vec![
            make_agg_no_arg(AggregateFunction::Count, "cnt"),
            make_agg(AggregateFunction::Avg, "avg_age"),
        ],
        input: Box::new(RelExpr::scan("users")),
    };

    if let RelExpr::Aggregate { aggregates, .. } = &plan {
        let decomposed =
            decompose_all(aggregates).expect("should decompose");
        // COUNT -> 1 pair, AVG -> 2 pairs
        assert_eq!(decomposed.len(), 3);
    }
}

#[test]
fn non_decomposable_agg_relexpr() {
    let plan = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("region"))],
        aggregates: vec![make_agg(
            AggregateFunction::StringAgg,
            "names",
        )],
        input: Box::new(RelExpr::scan("users")),
    };

    if let RelExpr::Aggregate { aggregates, .. } = &plan {
        assert!(!all_decomposable(aggregates));
        assert!(decompose_all(aggregates).is_none());
    }
}

// --- AggValue integration tests ---

#[test]
#[expect(clippy::approx_constant, reason = "3.14 is test data, not mathematical constant")]
fn agg_value_types() {
    let null = AggValue::Null;
    let int = AggValue::Int(42);
    let float = AggValue::Float(3.14);
    let string = AggValue::String("test".to_owned());

    assert_eq!(null.to_string(), "NULL");
    assert_eq!(int.to_string(), "42");
    assert_eq!(float.to_string(), "3.14");
    assert_eq!(string.to_string(), "'test'");
}

#[test]
fn skew_aware_strategy_construction() {
    let strategy = AggregationStrategy::SkewAware {
        hot_keys: vec![AggValue::Null, AggValue::String("UNKNOWN".to_owned())],
        hot_key_strategy: Box::new(AggregationStrategy::SinglePhase),
        normal_strategy: Box::new(AggregationStrategy::TwoPhase {
            local_agg: make_agg(AggregateFunction::Sum, "partial"),
            global_agg: make_agg(AggregateFunction::Sum, "final"),
        }),
    };

    if let AggregationStrategy::SkewAware {
        hot_keys,
        hot_key_strategy,
        normal_strategy,
    } = &strategy
    {
        assert_eq!(hot_keys.len(), 2);
        assert_eq!(*hot_key_strategy.as_ref(), AggregationStrategy::SinglePhase);
        assert!(matches!(
            normal_strategy.as_ref(),
            AggregationStrategy::TwoPhase { .. }
        ));
    }
}

// --- Config variation tests ---

#[test]
fn custom_config_thresholds() {
    let config = DistributedAggConfig {
        min_rows_for_two_phase: 100,
        max_rows_for_single_phase: 50,
        num_nodes: 4,
        ..default_config()
    };

    // 200 rows exceeds both single-phase max and two-phase min
    let s = AggregationStrategy::choose_strategy(
        AggregateFunction::Sum,
        200,
        10,
        false,
        &config,
    );
    assert!(matches!(s, AggregationStrategy::TwoPhase { .. }));
}

#[test]
fn single_node_cluster_always_single_phase_benefit() {
    let config = DistributedAggConfig {
        num_nodes: 1,
        ..default_config()
    };
    let benefit =
        AggregationStrategy::estimated_benefit(1_000_000_000, 100, &config);
    assert_eq!(benefit, 0.0);
}

#[test]
fn large_cluster_high_benefit() {
    let config = DistributedAggConfig {
        num_nodes: 1000,
        ..default_config()
    };
    let benefit =
        AggregationStrategy::estimated_benefit(1_000_000_000, 100, &config);
    assert!(benefit > 0.99);
}

// --- Serialization integration tests ---

#[test]
fn strategy_json_roundtrip_all_variants() {
    let variants: Vec<AggregationStrategy> = vec![
        AggregationStrategy::SinglePhase,
        AggregationStrategy::TwoPhase {
            local_agg: make_agg(AggregateFunction::Sum, "partial"),
            global_agg: make_agg(AggregateFunction::Sum, "final"),
        },
        AggregationStrategy::ThreePhase {
            local_agg: make_agg(AggregateFunction::Count, "partial"),
            shuffle_keys: vec![Expr::Column(ColumnRef::new("region"))],
            global_agg: make_agg(AggregateFunction::Sum, "final"),
        },
        AggregationStrategy::MapReduce {
            map_fn: make_agg(AggregateFunction::Sum, "map"),
            reduce_fn: make_agg(AggregateFunction::Sum, "reduce"),
        },
        AggregationStrategy::SkewAware {
            hot_keys: vec![AggValue::Null],
            hot_key_strategy: Box::new(AggregationStrategy::SinglePhase),
            normal_strategy: Box::new(AggregationStrategy::SinglePhase),
        },
    ];

    for strategy in &variants {
        let json = serde_json::to_string(strategy)
            .expect("serialize should succeed");
        let deserialized: AggregationStrategy =
            serde_json::from_str(&json)
                .expect("deserialize should succeed");
        assert_eq!(*strategy, deserialized);
    }
}
