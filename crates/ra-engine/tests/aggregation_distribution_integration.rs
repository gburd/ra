#![expect(clippy::panic, clippy::expect_used, reason = "test assertions")]
//! Integration tests for distributed aggregation + distribution
//! strategy integration.
//!
//! Validates that the `DistributedOptimizer` correctly selects
//! two-phase, three-phase, or skew-aware aggregation strategies
//! based on table statistics, histogram skew, and cluster topology.

use ra_core::algebra::{AggregateExpr, AggregateFunction, RelExpr};
use ra_core::distributed_agg::{
    all_decomposable, decompose_all, is_two_phase_worthwhile, AggregationStrategy,
    DistributedAggConfig,
};
use ra_core::distribution::{DataDistribution, NodeId};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::statistics::Statistics;
use ra_engine::distributed_optimizer::{
    ClusterTopology, DistributedOptimizer, DistributedOptimizerConfig,
};
use ra_stats::skew::{
    generate_uniform_histogram, generate_zipf_histogram, FrequencyBucket, FrequencyHistogram,
    SkewDetector,
};

fn col(name: &str) -> Expr {
    Expr::Column(ColumnRef::new(name))
}

fn eq(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// Build an optimizer with a large table and registered histogram.
fn optimizer_with_skewed_table(
    num_nodes: u32,
    histogram: FrequencyHistogram,
) -> DistributedOptimizer {
    let config = DistributedOptimizerConfig::default();
    let mut topology = ClusterTopology::uniform(num_nodes);
    topology.register_table("sales", NodeId(0), DataDistribution::Arbitrary);
    let mut stats = Statistics::new(50_000_000.0);
    stats.avg_row_size = 128;
    stats.total_size = 6_400_000_000;
    let mut opt = DistributedOptimizer::new(config, topology);
    opt.register_stats("sales", stats);
    opt.register_histogram("sales", "region", histogram);
    opt
}

/// Build an optimizer with a large table (no histograms).
fn optimizer_large_table(num_nodes: u32) -> DistributedOptimizer {
    let config = DistributedOptimizerConfig::default();
    let mut topology = ClusterTopology::uniform(num_nodes);
    topology.register_table("orders", NodeId(0), DataDistribution::Arbitrary);
    let mut stats = Statistics::new(100_000_000.0);
    stats.avg_row_size = 256;
    stats.total_size = 25_600_000_000;
    let mut opt = DistributedOptimizer::new(config, topology);
    opt.register_stats("orders", stats);
    opt
}

/// Build an optimizer with a small table.
fn optimizer_small_table(num_nodes: u32) -> DistributedOptimizer {
    let config = DistributedOptimizerConfig::default();
    let mut topology = ClusterTopology::uniform(num_nodes);
    topology.register_table("lookup", NodeId(0), DataDistribution::Arbitrary);
    let mut stats = Statistics::new(500.0);
    stats.avg_row_size = 64;
    stats.total_size = 32_000;
    let mut opt = DistributedOptimizer::new(config, topology);
    opt.register_stats("lookup", stats);
    opt
}

fn make_agg(func: AggregateFunction, alias: &str) -> AggregateExpr {
    AggregateExpr {
        function: func,
        arg: Some(col("amount")),
        distinct: false,
        alias: Some(alias.to_owned()),
    }
}

fn make_count(alias: &str) -> AggregateExpr {
    AggregateExpr {
        function: AggregateFunction::Count,
        arg: None,
        distinct: false,
        alias: Some(alias.to_owned()),
    }
}

// ===============================================================
// Two-phase aggregation with uniform data
// ===============================================================

#[test]
fn two_phase_count_uniform_distribution() {
    let opt = optimizer_large_table(8);
    let plan = RelExpr::Aggregate {
        group_by: vec![col("country_code")],
        aggregates: vec![make_count("cnt")],
        input: Box::new(RelExpr::scan("orders")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    let strategy = dre.input_strategy.as_ref().expect("should have strategy");
    assert_eq!(strategy.label(), "Shuffle");
}

#[test]
fn two_phase_sum_uniform_distribution() {
    let opt = optimizer_large_table(8);
    let plan = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![make_agg(AggregateFunction::Sum, "total")],
        input: Box::new(RelExpr::scan("orders")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    assert!(
        dre.input_strategy.is_some(),
        "grouped aggregate on large table should have strategy"
    );
}

#[test]
fn two_phase_min_max_uniform() {
    let opt = optimizer_large_table(4);
    let plan = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![
            make_agg(AggregateFunction::Min, "min_price"),
            make_agg(AggregateFunction::Max, "max_price"),
        ],
        input: Box::new(RelExpr::scan("orders")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    assert!(dre.input_strategy.is_some());
}

#[test]
fn two_phase_avg_desugaring() {
    // AVG decomposes to SUM + COUNT.
    let aggs = vec![AggregateExpr {
        function: AggregateFunction::Avg,
        arg: Some(col("price")),
        distinct: false,
        alias: Some("avg_price".to_owned()),
    }];
    let decomposed = decompose_all(&aggs).expect("AVG is decomposable");
    assert_eq!(
        decomposed.len(),
        2,
        "AVG should decompose into 2 partial aggregates"
    );
    assert_eq!(decomposed[0].0.function, AggregateFunction::Sum);
    assert_eq!(decomposed[1].0.function, AggregateFunction::Count);
}

#[test]
fn two_phase_multiple_aggs_decompose() {
    let aggs = vec![
        make_agg(AggregateFunction::Sum, "total"),
        make_count("cnt"),
        make_agg(AggregateFunction::Min, "low"),
        make_agg(AggregateFunction::Max, "high"),
    ];
    assert!(all_decomposable(&aggs));
    let decomposed = decompose_all(&aggs).expect("all decomposable");
    assert_eq!(decomposed.len(), 4);
}

// ===============================================================
// Non-decomposable aggregates fall back to single-phase
// ===============================================================

#[test]
fn stddev_falls_back_to_single_phase() {
    let opt = optimizer_large_table(8);
    let plan = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::StdDev,
            arg: Some(col("amount")),
            distinct: false,
            alias: Some("sd".to_owned()),
        }],
        input: Box::new(RelExpr::scan("orders")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    // StdDev is not decomposable, so it should still shuffle
    // (single-phase with group-by still requires shuffle).
    assert!(dre.input_strategy.is_some());
}

#[test]
fn string_agg_not_decomposable() {
    let aggs = vec![AggregateExpr {
        function: AggregateFunction::StringAgg,
        arg: Some(col("name")),
        distinct: false,
        alias: Some("names".to_owned()),
    }];
    assert!(!all_decomposable(&aggs));
    assert!(decompose_all(&aggs).is_none());
}

#[test]
fn mixed_decomposable_and_non() {
    let aggs = vec![
        make_agg(AggregateFunction::Sum, "total"),
        AggregateExpr {
            function: AggregateFunction::ArrayAgg,
            arg: Some(col("id")),
            distinct: false,
            alias: Some("ids".to_owned()),
        },
    ];
    assert!(!all_decomposable(&aggs));
    assert!(decompose_all(&aggs).is_none());
}

// ===============================================================
// Skew detection integration
// ===============================================================

#[test]
fn zipf_histogram_detects_skew() {
    let histogram = generate_zipf_histogram(100, 10_000_000, 1.5);
    let opt = optimizer_with_skewed_table(8, histogram);
    let has_skew = opt.analyze_group_key_skew(&RelExpr::scan("sales"), &[col("region")]);
    assert!(
        has_skew.is_some(),
        "Zipf 1.5 histogram should be detected as skewed"
    );
}

#[test]
fn uniform_histogram_no_skew() {
    let histogram = generate_uniform_histogram(100, 10_000_000);
    let opt = optimizer_with_skewed_table(8, histogram);
    let has_skew = opt.analyze_group_key_skew(&RelExpr::scan("sales"), &[col("region")]);
    assert!(
        has_skew.is_none(),
        "Uniform histogram should not detect skew"
    );
}

#[test]
fn skew_aware_plan_with_extreme_zipf() {
    let histogram = generate_zipf_histogram(50, 10_000_000, 2.0);
    let opt = optimizer_with_skewed_table(8, histogram);
    let plan = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![make_agg(AggregateFunction::Sum, "total")],
        input: Box::new(RelExpr::scan("sales")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    // With extreme skew detected, should still produce a valid plan.
    assert!(
        dre.input_strategy.is_some(),
        "skewed aggregate should have a distribution strategy"
    );
}

#[test]
fn hot_key_detection_with_manual_histogram() {
    let mut buckets = vec![FrequencyBucket {
        value: "US".to_owned(),
        count: 10_000_000,
    }];
    for i in 0..200 {
        buckets.push(FrequencyBucket {
            value: format!("country_{i}"),
            count: 100,
        });
    }
    let histogram = FrequencyHistogram::new(buckets);
    let detector = SkewDetector::default();
    let analysis = detector.analyze("region", &histogram);
    assert!(!analysis.hot_keys.is_empty());
    assert_eq!(analysis.hot_keys[0].value, "US");
}

#[test]
fn skew_detector_threshold_sensitivity() {
    let histogram = generate_zipf_histogram(100, 1_000_000, 1.0);
    let strict = SkewDetector::new(50.0);
    let lenient = SkewDetector::new(2.0);
    let strict_hot = strict.detect_hot_keys(&histogram);
    let lenient_hot = lenient.detect_hot_keys(&histogram);
    assert!(
        lenient_hot.len() >= strict_hot.len(),
        "lenient threshold should find >= hot keys"
    );
}

// ===============================================================
// Global aggregates (no GROUP BY)
// ===============================================================

#[test]
fn global_count_single_partition() {
    let opt = optimizer_large_table(8);
    let plan = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![make_count("total")],
        input: Box::new(RelExpr::scan("orders")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    assert!(
        dre.is_single_partition(),
        "global aggregate should gather to single node"
    );
}

#[test]
fn global_sum_single_partition() {
    let opt = optimizer_large_table(4);
    let plan = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![make_agg(AggregateFunction::Sum, "total")],
        input: Box::new(RelExpr::scan("orders")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    assert!(dre.is_single_partition());
}

#[test]
fn global_avg_single_partition() {
    let opt = optimizer_large_table(4);
    let plan = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![make_agg(AggregateFunction::Avg, "mean")],
        input: Box::new(RelExpr::scan("orders")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    assert!(dre.is_single_partition());
}

// ===============================================================
// Co-located aggregation (partition-wise)
// ===============================================================

#[test]
fn colocated_aggregate_partition_wise() {
    let config = DistributedOptimizerConfig::default();
    let mut topology = ClusterTopology::uniform(4);
    topology.register_table(
        "orders",
        NodeId(0),
        DataDistribution::HashPartitioned {
            keys: vec![col("region")],
            partition_count: 4,
        },
    );
    let mut stats = Statistics::new(100_000_000.0);
    stats.avg_row_size = 256;
    stats.total_size = 25_600_000_000;
    let mut opt = DistributedOptimizer::new(config, topology);
    opt.register_stats("orders", stats);

    let plan = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![make_count("cnt")],
        input: Box::new(RelExpr::scan("orders")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    let strategy = dre.input_strategy.as_ref().expect("should have strategy");
    assert_eq!(
        strategy.label(),
        "PartitionWise",
        "already partitioned on group-by key should be partition-wise"
    );
}

// ===============================================================
// Small table falls back to simpler strategies
// ===============================================================

#[test]
fn small_table_aggregate_has_strategy() {
    let opt = optimizer_small_table(4);
    let plan = RelExpr::Aggregate {
        group_by: vec![col("type")],
        aggregates: vec![make_count("cnt")],
        input: Box::new(RelExpr::scan("lookup")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    // Small table: 500 rows, should still produce a plan.
    // The strategy selection should handle this gracefully.
    assert!(dre.input_strategy.is_some());
}

// ===============================================================
// Strategy selection logic
// ===============================================================

#[test]
fn choose_strategy_respects_thresholds() {
    let config = DistributedAggConfig {
        min_rows_for_two_phase: 1_000_000,
        max_rows_for_single_phase: 100_000,
        min_rows_for_three_phase: 10_000_000,
        num_nodes: 8,
        ..DistributedAggConfig::default()
    };
    let small =
        AggregationStrategy::choose_strategy(AggregateFunction::Sum, 50_000, 100, false, &config);
    assert_eq!(small, AggregationStrategy::SinglePhase);

    let medium = AggregationStrategy::choose_strategy(
        AggregateFunction::Sum,
        5_000_000,
        100,
        false,
        &config,
    );
    assert!(matches!(medium, AggregationStrategy::TwoPhase { .. }));

    let large_skewed = AggregationStrategy::choose_strategy(
        AggregateFunction::Sum,
        50_000_000,
        1000,
        true,
        &config,
    );
    assert!(matches!(
        large_skewed,
        AggregationStrategy::ThreePhase { .. }
    ));
}

#[test]
fn two_phase_worthwhile_high_reduction() {
    let config = DistributedAggConfig {
        num_nodes: 8,
        min_rows_for_two_phase: 1_000_000,
        ..DistributedAggConfig::default()
    };
    // 10M rows, 100 groups => 99.999% reduction.
    assert!(is_two_phase_worthwhile(10_000_000, 100, &config));
}

#[test]
fn two_phase_not_worthwhile_many_groups() {
    let config = DistributedAggConfig {
        num_nodes: 8,
        min_rows_for_two_phase: 1_000_000,
        ..DistributedAggConfig::default()
    };
    // 2M rows, 1.8M groups => only 10% reduction.
    assert!(!is_two_phase_worthwhile(2_000_000, 1_800_000, &config));
}

// ===============================================================
// Aggregation over filtered input
// ===============================================================

#[test]
fn aggregate_over_filtered_input() {
    let opt = optimizer_large_table(8);
    let plan = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![make_agg(AggregateFunction::Sum, "total")],
        input: Box::new(RelExpr::scan("orders").filter(eq(
            col("status"),
            Expr::Const(Const::String("active".into())),
        ))),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    assert!(dre.input_strategy.is_some());
}

// ===============================================================
// Multi-node cluster variations
// ===============================================================

#[test]
fn single_node_aggregate() {
    let opt = optimizer_large_table(1);
    let plan = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![make_count("cnt")],
        input: Box::new(RelExpr::scan("orders")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    // Even single-node should produce a valid plan.
    assert!(dre.input_strategy.is_some());
}

#[test]
fn large_cluster_aggregate() {
    let config = DistributedOptimizerConfig::default();
    let mut topology = ClusterTopology::uniform(64);
    topology.register_table("events", NodeId(0), DataDistribution::Arbitrary);
    let mut stats = Statistics::new(1_000_000_000.0);
    stats.avg_row_size = 200;
    stats.total_size = 200_000_000_000;
    let mut opt = DistributedOptimizer::new(config, topology);
    opt.register_stats("events", stats);

    let plan = RelExpr::Aggregate {
        group_by: vec![col("user_id")],
        aggregates: vec![make_count("event_count")],
        input: Box::new(RelExpr::scan("events")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    assert!(dre.input_strategy.is_some());
    if let DataDistribution::HashPartitioned {
        partition_count, ..
    } = &dre.distribution
    {
        assert_eq!(*partition_count, 64);
    }
}

// ===============================================================
// Output distribution validation
// ===============================================================

#[test]
fn grouped_aggregate_output_is_hash_partitioned() {
    let opt = optimizer_large_table(8);
    let plan = RelExpr::Aggregate {
        group_by: vec![col("country_code")],
        aggregates: vec![make_count("cnt")],
        input: Box::new(RelExpr::scan("orders")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    if let DataDistribution::HashPartitioned {
        keys,
        partition_count,
    } = &dre.distribution
    {
        assert_eq!(keys, &[col("country_code")]);
        assert_eq!(*partition_count, 8);
    } else {
        panic!(
            "grouped aggregate should produce HashPartitioned, got {:?}",
            dre.distribution
        );
    }
}

#[test]
fn global_aggregate_output_is_single_partition() {
    let opt = optimizer_large_table(8);
    let plan = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![make_count("total")],
        input: Box::new(RelExpr::scan("orders")),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    assert!(dre.is_single_partition());
    assert!(dre.node_assignment.is_some());
}

// ===============================================================
// Histogram registration and analysis
// ===============================================================

#[test]
fn register_multiple_histograms() {
    let config = DistributedOptimizerConfig::default();
    let topology = ClusterTopology::uniform(4);
    let mut opt = DistributedOptimizer::new(config, topology);

    let h1 = generate_zipf_histogram(50, 1_000_000, 1.5);
    let h2 = generate_uniform_histogram(50, 1_000_000);
    opt.register_histogram("sales", "region", h1);
    opt.register_histogram("sales", "status", h2);

    // region should have skew, status should not.
    let skew_region = opt.analyze_group_key_skew(&RelExpr::scan("sales"), &[col("region")]);
    let skew_status = opt.analyze_group_key_skew(&RelExpr::scan("sales"), &[col("status")]);
    assert!(skew_region.is_some());
    assert!(skew_status.is_none());
}

#[test]
fn analyze_skew_returns_hot_key_values() {
    let mut buckets = vec![FrequencyBucket {
        value: "hot_key".to_owned(),
        count: 5_000_000,
    }];
    for i in 0..100 {
        buckets.push(FrequencyBucket {
            value: format!("val_{i}"),
            count: 50,
        });
    }
    let histogram = FrequencyHistogram::new(buckets);
    let opt = optimizer_with_skewed_table(8, histogram);
    let result = opt
        .analyze_group_key_skew(&RelExpr::scan("sales"), &[col("region")])
        .expect("should detect skew");
    let (_strategy, hot_values) = result;
    assert!(!hot_values.is_empty());
}

// ===============================================================
// End-to-end: aggregate over join
// ===============================================================

#[test]
fn aggregate_over_join() {
    let config = DistributedOptimizerConfig::default();
    let mut topology = ClusterTopology::uniform(4);
    topology.register_table("orders", NodeId(0), DataDistribution::Arbitrary);
    topology.register_table("customers", NodeId(1), DataDistribution::Arbitrary);
    let mut orders_stats = Statistics::new(100_000_000.0);
    orders_stats.avg_row_size = 128;
    orders_stats.total_size = 12_800_000_000;
    let mut cust_stats = Statistics::new(1_000_000.0);
    cust_stats.avg_row_size = 256;
    cust_stats.total_size = 256_000_000;

    let mut opt = DistributedOptimizer::new(config, topology);
    opt.register_stats("orders", orders_stats);
    opt.register_stats("customers", cust_stats);

    let join = RelExpr::Join {
        join_type: ra_core::algebra::JoinType::Inner,
        condition: eq(col("customer_id"), col("id")),
        left: Box::new(RelExpr::scan("orders")),
        right: Box::new(RelExpr::scan("customers")),
    };
    let plan = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![
            make_agg(AggregateFunction::Sum, "total_amount"),
            make_count("order_count"),
        ],
        input: Box::new(join),
    };
    let dre = opt.optimize_distribution(&plan).expect("should succeed");
    assert!(dre.input_strategy.is_some());
}

// ===============================================================
// Decomposition verification
// ===============================================================

#[test]
fn decompose_count_correctly() {
    let aggs = vec![make_count("cnt")];
    let decomposed = decompose_all(&aggs).expect("decomposable");
    assert_eq!(decomposed.len(), 1);
    // COUNT local: COUNT, global: SUM.
    assert_eq!(decomposed[0].0.function, AggregateFunction::Count);
    assert_eq!(decomposed[0].1.function, AggregateFunction::Sum);
}

#[test]
fn decompose_sum_correctly() {
    let aggs = vec![make_agg(AggregateFunction::Sum, "total")];
    let decomposed = decompose_all(&aggs).expect("decomposable");
    assert_eq!(decomposed.len(), 1);
    assert_eq!(decomposed[0].0.function, AggregateFunction::Sum);
    assert_eq!(decomposed[0].1.function, AggregateFunction::Sum);
}

#[test]
fn decompose_min_correctly() {
    let aggs = vec![make_agg(AggregateFunction::Min, "low")];
    let decomposed = decompose_all(&aggs).expect("decomposable");
    assert_eq!(decomposed[0].0.function, AggregateFunction::Min);
    assert_eq!(decomposed[0].1.function, AggregateFunction::Min);
}

#[test]
fn decompose_max_correctly() {
    let aggs = vec![make_agg(AggregateFunction::Max, "high")];
    let decomposed = decompose_all(&aggs).expect("decomposable");
    assert_eq!(decomposed[0].0.function, AggregateFunction::Max);
    assert_eq!(decomposed[0].1.function, AggregateFunction::Max);
}

#[test]
fn decompose_avg_to_sum_and_count() {
    let aggs = vec![make_agg(AggregateFunction::Avg, "avg_price")];
    let decomposed = decompose_all(&aggs).expect("decomposable");
    assert_eq!(decomposed.len(), 2);
    // First pair: SUM partial.
    assert_eq!(decomposed[0].0.function, AggregateFunction::Sum);
    assert_eq!(decomposed[0].1.function, AggregateFunction::Sum);
    // Second pair: COUNT partial.
    assert_eq!(decomposed[1].0.function, AggregateFunction::Count);
    assert_eq!(decomposed[1].1.function, AggregateFunction::Sum);
}

#[test]
fn decompose_preserves_aliases() {
    let aggs = vec![make_agg(AggregateFunction::Sum, "revenue")];
    let decomposed = decompose_all(&aggs).expect("decomposable");
    assert_eq!(decomposed[0].0.alias.as_deref(), Some("partial_revenue"));
    assert_eq!(decomposed[0].1.alias.as_deref(), Some("revenue"));
}
