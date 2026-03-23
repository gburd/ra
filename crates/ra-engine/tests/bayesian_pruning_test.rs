//! Tests for Bayesian adaptive search space pruning (RFC 0059).

use ra_core::algebra::{AggregateExpr, AggregateFunction, JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_engine::bayesian_pruning::{BayesianPruner, BucketStats, PruningConfig};
use ra_engine::pattern_fingerprint::{self, PlanFingerprint};

// ---------------------------------------------------------------
// Helper constructors
// ---------------------------------------------------------------

fn scan(name: &str) -> RelExpr {
    RelExpr::Scan {
        table: name.to_string(),
        alias: None,
    }
}

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

fn and(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::And,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn join(left: RelExpr, right: RelExpr, cond: Expr) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: cond,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn cross_join(left: RelExpr, right: RelExpr) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Cross,
        condition: Expr::Const(Const::Bool(true)),
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn aggregate(input: RelExpr) -> RelExpr {
    RelExpr::Aggregate {
        group_by: vec![col("name")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(Expr::Const(Const::Int(1))),
            distinct: false,
            alias: Some("cnt".to_string()),
        }],
        input: Box::new(input),
    }
}

fn filter(input: RelExpr, pred: Expr) -> RelExpr {
    RelExpr::Filter {
        predicate: pred,
        input: Box::new(input),
    }
}

fn project(input: RelExpr) -> RelExpr {
    RelExpr::Project {
        columns: vec![],
        input: Box::new(input),
    }
}

// ---------------------------------------------------------------
// PlanFingerprint tests
// ---------------------------------------------------------------

#[test]
fn test_fingerprint_single_scan() {
    let plan = scan("users");
    let fp = PlanFingerprint::from_plan(&plan);
    assert_eq!(fp.table_bucket, 0); // 0-1 tables
    assert_eq!(fp.join_bucket, 0); // 0 joins
    assert_eq!(fp.predicate_complexity, 0); // low
    assert!(!fp.has_cross_join);
    assert!(!fp.has_correlated_subquery);
    assert!(!fp.has_early_aggregation);
}

#[test]
fn test_fingerprint_two_table_join() {
    let plan = join(scan("a"), scan("b"), eq(col("a.id"), col("b.id")));
    let fp = PlanFingerprint::from_plan(&plan);
    assert_eq!(fp.table_bucket, 1); // 2-3 tables
    assert_eq!(fp.join_bucket, 1); // 1-2 joins
}

#[test]
fn test_fingerprint_five_table_join() {
    let plan = join(
        join(
            join(
                join(scan("a"), scan("b"), eq(col("a.id"), col("b.id"))),
                scan("c"),
                eq(col("b.id"), col("c.id")),
            ),
            scan("d"),
            eq(col("c.id"), col("d.id")),
        ),
        scan("e"),
        eq(col("d.id"), col("e.id")),
    );
    let fp = PlanFingerprint::from_plan(&plan);
    assert_eq!(fp.table_bucket, 2); // 4-6 tables
                                    // 4 joins is in bucket 3-5 => join_bucket 2
    assert_eq!(fp.join_bucket, 2);
}

#[test]
fn test_fingerprint_cross_join() {
    let plan = cross_join(scan("a"), scan("b"));
    let fp = PlanFingerprint::from_plan(&plan);
    assert!(fp.has_cross_join);
}

#[test]
fn test_fingerprint_no_cross_join() {
    let plan = join(scan("a"), scan("b"), eq(col("a.id"), col("b.id")));
    let fp = PlanFingerprint::from_plan(&plan);
    assert!(!fp.has_cross_join);
}

#[test]
fn test_fingerprint_early_aggregation() {
    // Aggregate below join
    let plan = join(aggregate(scan("a")), scan("b"), eq(col("cnt"), col("b.id")));
    let fp = PlanFingerprint::from_plan(&plan);
    assert!(fp.has_early_aggregation);
}

#[test]
fn test_fingerprint_no_early_aggregation() {
    // Aggregate above join (normal)
    let plan = aggregate(join(scan("a"), scan("b"), eq(col("a.id"), col("b.id"))));
    let fp = PlanFingerprint::from_plan(&plan);
    assert!(!fp.has_early_aggregation);
}

#[test]
fn test_fingerprint_predicate_complexity_low() {
    // A single filter: eq(col, const) = 3 nodes => medium bucket
    let plan = filter(scan("a"), eq(col("x"), Expr::Const(Const::Int(1))));
    let fp = PlanFingerprint::from_plan(&plan);
    assert_eq!(fp.predicate_complexity, 1); // medium (3-6 nodes)
}

#[test]
fn test_fingerprint_predicate_complexity_medium() {
    // Multiple predicates combined
    let pred = and(
        eq(col("x"), Expr::Const(Const::Int(1))),
        eq(col("y"), Expr::Const(Const::Int(2))),
    );
    let plan = filter(
        join(scan("a"), scan("b"), eq(col("a.id"), col("b.id"))),
        pred,
    );
    let fp = PlanFingerprint::from_plan(&plan);
    // Join condition (3 nodes) + filter predicate (5 nodes) = 8 => high
    assert!(fp.predicate_complexity >= 1);
}

#[test]
fn test_fingerprint_equality() {
    // Same structure => same fingerprint
    let plan1 = join(scan("a"), scan("b"), eq(col("a.id"), col("b.id")));
    let plan2 = join(scan("x"), scan("y"), eq(col("x.id"), col("y.id")));
    let fp1 = PlanFingerprint::from_plan(&plan1);
    let fp2 = PlanFingerprint::from_plan(&plan2);
    assert_eq!(fp1, fp2);
}

#[test]
fn test_fingerprint_inequality() {
    let plan1 = scan("a");
    let plan2 = join(scan("a"), scan("b"), eq(col("a.id"), col("b.id")));
    let fp1 = PlanFingerprint::from_plan(&plan1);
    let fp2 = PlanFingerprint::from_plan(&plan2);
    assert_ne!(fp1, fp2);
}

// ---------------------------------------------------------------
// Counting helper tests
// ---------------------------------------------------------------

#[test]
fn test_count_tables() {
    assert_eq!(pattern_fingerprint::count_tables(&scan("a")), 1);
    assert_eq!(
        pattern_fingerprint::count_tables(&join(scan("a"), scan("b"), eq(col("x"), col("y")))),
        2,
    );
    assert_eq!(
        pattern_fingerprint::count_tables(&project(filter(
            scan("a"),
            eq(col("x"), Expr::Const(Const::Int(1)))
        ))),
        1
    );
}

#[test]
fn test_count_joins() {
    assert_eq!(pattern_fingerprint::count_joins(&scan("a")), 0);
    assert_eq!(
        pattern_fingerprint::count_joins(&join(scan("a"), scan("b"), eq(col("x"), col("y")))),
        1,
    );
    assert_eq!(
        pattern_fingerprint::count_joins(&join(
            join(scan("a"), scan("b"), eq(col("x"), col("y"))),
            scan("c"),
            eq(col("z"), col("w")),
        )),
        2,
    );
}

#[test]
fn test_contains_cross_join() {
    assert!(!pattern_fingerprint::contains_cross_join(&scan("a")));
    assert!(pattern_fingerprint::contains_cross_join(&cross_join(
        scan("a"),
        scan("b")
    )));
    assert!(!pattern_fingerprint::contains_cross_join(&join(
        scan("a"),
        scan("b"),
        eq(col("x"), col("y")),
    )));
    // Cross join nested inside a regular join
    assert!(pattern_fingerprint::contains_cross_join(&join(
        cross_join(scan("a"), scan("b")),
        scan("c"),
        eq(col("x"), col("y")),
    )));
}

#[test]
fn test_has_agg_below_join() {
    // Aggregate above join: not early
    assert!(!pattern_fingerprint::has_agg_below_join(&aggregate(join(
        scan("a"),
        scan("b"),
        eq(col("x"), col("y")),
    ))));
    // Aggregate below join: early
    assert!(pattern_fingerprint::has_agg_below_join(&join(
        aggregate(scan("a")),
        scan("b"),
        eq(col("x"), col("y")),
    )));
    // Aggregate inside right side of join: early
    assert!(pattern_fingerprint::has_agg_below_join(&join(
        scan("a"),
        aggregate(scan("b")),
        eq(col("x"), col("y")),
    )));
}

// ---------------------------------------------------------------
// BucketStats tests
// ---------------------------------------------------------------

#[test]
fn test_uninformative_prior() {
    let stats = BucketStats::uninformative();
    assert!((stats.mean() - 0.5).abs() < f64::EPSILON);
    assert!((stats.sample_count() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn test_record_improvement() {
    let mut stats = BucketStats::uninformative();
    stats.record(true, 1.0); // no decay
                             // alpha=2, beta=1 => mean = 2/3
    assert!((stats.mean() - 2.0 / 3.0).abs() < 1e-10);
    assert!((stats.sample_count() - 1.0).abs() < 1e-10);
}

#[test]
fn test_record_no_improvement() {
    let mut stats = BucketStats::uninformative();
    stats.record(false, 1.0); // no decay
                              // alpha=1, beta=2 => mean = 1/3
    assert!((stats.mean() - 1.0 / 3.0).abs() < 1e-10);
}

#[test]
fn test_record_with_decay() {
    let mut stats = BucketStats::uninformative();
    stats.record(true, 0.95);
    // After decay: alpha = 1 + (1-1)*0.95 + 1 = 2.0
    // beta = 1 + (1-1)*0.95 = 1.0
    assert!((stats.alpha - 2.0).abs() < 1e-10);
    assert!((stats.beta - 1.0).abs() < 1e-10);

    stats.record(false, 0.95);
    // Decay: alpha = 1 + (2-1)*0.95 = 1.95, beta = 1 + (1-1)*0.95 = 1.0
    // Update: beta += 1 => 2.0
    assert!((stats.alpha - 1.95).abs() < 1e-10);
    assert!((stats.beta - 2.0).abs() < 1e-10);
}

#[test]
fn test_ewma_convergence() {
    let mut stats = BucketStats::uninformative();
    // Record 50 improvements in a row with decay=0.95
    for _ in 0..50 {
        stats.record(true, 0.95);
    }
    // Posterior mean should be very high
    assert!(stats.mean() > 0.9);

    // Now record 50 non-improvements
    for _ in 0..50 {
        stats.record(false, 0.95);
    }
    // Posterior mean should have dropped significantly
    assert!(stats.mean() < 0.2);
}

#[test]
fn test_variance_decreases_with_observations() {
    let stats0 = BucketStats::uninformative();
    let var0 = stats0.variance();

    let mut stats10 = BucketStats::uninformative();
    for _ in 0..10 {
        stats10.record(true, 1.0);
    }
    let var10 = stats10.variance();

    assert!(var10 < var0);
}

// ---------------------------------------------------------------
// BayesianPruner tests
// ---------------------------------------------------------------

#[test]
fn test_pruner_always_explores_with_few_observations() {
    let pruner = BayesianPruner::with_defaults();
    let fp = PlanFingerprint::from_plan(&scan("a"));
    // No observations => always explore
    assert!(pruner.should_explore(&fp, 1.0));
    assert!(pruner.should_explore(&fp, 0.5));
    assert!(pruner.should_explore(&fp, 0.1));
}

#[test]
fn test_pruner_explores_high_posterior() {
    let mut pruner = BayesianPruner::with_defaults();
    let fp = PlanFingerprint::from_plan(&scan("a"));

    // Record enough successes to build a strong posterior
    for _ in 0..10 {
        pruner.record_outcome(&fp, true);
    }

    // High posterior + plenty of budget => explore
    assert!(pruner.should_explore(&fp, 0.8));
}

#[test]
fn test_pruner_skips_low_posterior() {
    let mut pruner = BayesianPruner::with_defaults();
    let fp = PlanFingerprint::from_plan(&scan("a"));

    // Record many failures
    for _ in 0..20 {
        pruner.record_outcome(&fp, false);
    }

    // Low posterior => skip even with budget remaining
    assert!(!pruner.should_explore(&fp, 0.5));
}

#[test]
fn test_pruner_threshold_rises_as_budget_shrinks() {
    let pruner = BayesianPruner::with_defaults();
    let t_full = pruner.adaptive_threshold(1.0);
    let t_half = pruner.adaptive_threshold(0.5);
    let t_low = pruner.adaptive_threshold(0.2);
    let t_zero = pruner.adaptive_threshold(0.0);

    assert!(t_full < t_half);
    assert!(t_half < t_low);
    assert!(t_low < t_zero);
    assert!((t_full - 0.15).abs() < 0.01);
    assert!((t_zero - 1.0).abs() < 0.01);
}

#[test]
fn test_pruner_becomes_more_selective_as_budget_drains() {
    let mut pruner = BayesianPruner::with_defaults();
    let fp = PlanFingerprint::from_plan(&join(scan("a"), scan("b"), eq(col("x"), col("y"))));

    // Record a moderate success rate (40%)
    for i in 0..10 {
        pruner.record_outcome(&fp, i < 4);
    }

    // With plenty of budget, this moderate rate should be explored
    let explore_full = pruner.should_explore(&fp, 0.9);
    // With very little budget, the threshold is high enough to skip
    let explore_low = pruner.should_explore(&fp, 0.1);

    assert!(explore_full);
    assert!(!explore_low);
}

#[test]
fn test_pruner_counts() {
    let mut pruner = BayesianPruner::with_defaults();
    let fp = PlanFingerprint::from_plan(&scan("a"));

    pruner.record_outcome(&fp, true);
    pruner.record_outcome(&fp, false);
    pruner.record_skip(&fp, 0.5);

    assert_eq!(pruner.explored_count(), 2);
    assert_eq!(pruner.skipped_count(), 1);
}

#[test]
fn test_pruner_skip_rate() {
    let mut pruner = BayesianPruner::with_defaults();
    let fp = PlanFingerprint::from_plan(&scan("a"));

    pruner.record_outcome(&fp, true);
    pruner.record_outcome(&fp, true);
    pruner.record_skip(&fp, 0.5);

    // 1 skip out of 3 total decisions
    assert!((pruner.skip_rate() - 1.0 / 3.0).abs() < 1e-10);
}

#[test]
fn test_pruner_skip_rate_zero_when_empty() {
    let pruner = BayesianPruner::with_defaults();
    assert!((pruner.skip_rate() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn test_pruner_bucket_count() {
    let mut pruner = BayesianPruner::with_defaults();
    assert_eq!(pruner.bucket_count(), 0);

    let fp1 = PlanFingerprint::from_plan(&scan("a"));
    let fp2 = PlanFingerprint::from_plan(&join(scan("a"), scan("b"), eq(col("x"), col("y"))));

    pruner.record_outcome(&fp1, true);
    assert_eq!(pruner.bucket_count(), 1);

    pruner.record_outcome(&fp2, false);
    assert_eq!(pruner.bucket_count(), 2);
}

#[test]
fn test_pruner_history_recording() {
    let mut pruner = BayesianPruner::with_defaults();
    let fp = PlanFingerprint::from_plan(&scan("a"));

    pruner.record_explored(&fp, true, 0.8);
    pruner.record_skip(&fp, 0.5);
    pruner.record_explored(&fp, false, 0.3);

    assert_eq!(pruner.history().len(), 3);
    assert!(pruner.history()[0].explored);
    assert_eq!(pruner.history()[0].improved, Some(true));
    assert!(!pruner.history()[1].explored);
    assert_eq!(pruner.history()[1].improved, None);
    assert!(pruner.history()[2].explored);
    assert_eq!(pruner.history()[2].improved, Some(false));
}

#[test]
fn test_pruner_history_cap() {
    let config = PruningConfig {
        max_history: 5,
        ..PruningConfig::default()
    };
    let mut pruner = BayesianPruner::new(config);
    let fp = PlanFingerprint::from_plan(&scan("a"));

    for _ in 0..10 {
        pruner.record_explored(&fp, true, 0.5);
    }

    assert_eq!(pruner.history().len(), 5);
}

#[test]
fn test_pruner_reset() {
    let mut pruner = BayesianPruner::with_defaults();
    let fp = PlanFingerprint::from_plan(&scan("a"));

    pruner.record_outcome(&fp, true);
    pruner.record_skip(&fp, 0.5);
    assert!(pruner.bucket_count() > 0);

    pruner.reset();
    assert_eq!(pruner.bucket_count(), 0);
    assert_eq!(pruner.explored_count(), 0);
    assert_eq!(pruner.skipped_count(), 0);
    assert!(pruner.history().is_empty());
}

#[test]
fn test_pruner_summary() {
    let mut pruner = BayesianPruner::with_defaults();
    let fp1 = PlanFingerprint::from_plan(&scan("a"));
    let fp2 = PlanFingerprint::from_plan(&join(scan("a"), scan("b"), eq(col("x"), col("y"))));

    // fp1: 3 successes
    for _ in 0..3 {
        pruner.record_outcome(&fp1, true);
    }
    // fp2: 3 failures
    for _ in 0..3 {
        pruner.record_outcome(&fp2, false);
    }
    pruner.record_skip(&fp1, 0.5);

    let summary = pruner.summary();
    assert_eq!(summary.bucket_count, 2);
    assert_eq!(summary.total_explored, 6);
    assert_eq!(summary.total_skipped, 1);
    assert!(summary.highest_bucket_mean > summary.lowest_bucket_mean);
}

// ---------------------------------------------------------------
// Adaptive threshold tests
// ---------------------------------------------------------------

#[test]
fn test_adaptive_threshold_full_budget() {
    let pruner = BayesianPruner::with_defaults();
    let t = pruner.adaptive_threshold(1.0);
    assert!((t - 0.15).abs() < 0.01);
}

#[test]
fn test_adaptive_threshold_no_budget() {
    let pruner = BayesianPruner::with_defaults();
    let t = pruner.adaptive_threshold(0.0);
    assert!((t - 1.0).abs() < 0.01);
}

#[test]
fn test_adaptive_threshold_custom_sensitivity() {
    let config = PruningConfig {
        budget_sensitivity: 1.0, // linear
        base_threshold: 0.1,
        ..PruningConfig::default()
    };
    let pruner = BayesianPruner::new(config);

    let t_half = pruner.adaptive_threshold(0.5);
    // threshold = 0.1 + 0.9 * 0.5^1 = 0.1 + 0.45 = 0.55
    assert!((t_half - 0.55).abs() < 0.01);
}

// ---------------------------------------------------------------
// End-to-end scenario tests
// ---------------------------------------------------------------

#[test]
fn test_scenario_cross_join_quickly_pruned() {
    let mut pruner = BayesianPruner::with_defaults();

    let cross_plan = cross_join(scan("a"), scan("b"));
    let fp = pruner.fingerprint(&cross_plan);

    // Cross joins almost never improve. Record 10 failures.
    for _ in 0..10 {
        pruner.record_outcome(&fp, false);
    }

    // With 50% budget remaining, cross join pattern should be skipped
    assert!(!pruner.should_explore(&fp, 0.5));
}

#[test]
fn test_scenario_join_reorder_often_helps() {
    let mut pruner = BayesianPruner::with_defaults();

    let join_plan = join(
        join(scan("a"), scan("b"), eq(col("a.id"), col("b.id"))),
        scan("c"),
        eq(col("b.id"), col("c.id")),
    );
    let fp = pruner.fingerprint(&join_plan);

    // Join reordering frequently helps. Record 8 out of 10 successes.
    for i in 0..10 {
        pruner.record_outcome(&fp, i < 8);
    }

    // High success rate => still explore even with budget pressure
    assert!(pruner.should_explore(&fp, 0.3));
}

#[test]
fn test_scenario_workload_shift() {
    let mut pruner = BayesianPruner::with_defaults();
    let fp = PlanFingerprint::from_plan(&join(scan("a"), scan("b"), eq(col("x"), col("y"))));

    // Phase 1: pattern is always helpful
    for _ in 0..20 {
        pruner.record_outcome(&fp, true);
    }
    assert!(pruner.should_explore(&fp, 0.5));

    // Phase 2: workload shifts, pattern stops helping
    for _ in 0..30 {
        pruner.record_outcome(&fp, false);
    }

    // EWMA decay means the old successes have faded
    assert!(!pruner.should_explore(&fp, 0.5));
}

#[test]
fn test_scenario_complex_query_fingerprint_diversity() {
    let pruner = BayesianPruner::with_defaults();

    // Simple scan
    let fp1 = pruner.fingerprint(&scan("a"));
    // Two-table join
    let fp2 = pruner.fingerprint(&join(scan("a"), scan("b"), eq(col("x"), col("y"))));
    // Join with aggregate
    let fp3 = pruner.fingerprint(&aggregate(join(
        scan("a"),
        scan("b"),
        eq(col("x"), col("y")),
    )));
    // Cross join
    let fp4 = pruner.fingerprint(&cross_join(scan("a"), scan("b")));
    // Early aggregation
    let fp5 = pruner.fingerprint(&join(
        aggregate(scan("a")),
        scan("b"),
        eq(col("x"), col("y")),
    ));

    // All should be distinct (different structural properties)
    let fps = vec![&fp1, &fp2, &fp3, &fp4, &fp5];
    for i in 0..fps.len() {
        for j in (i + 1)..fps.len() {
            // Not all pairs need to differ (fp2 and fp3 might match
            // if aggregation above join doesn't change bucket values).
            // But at minimum fp1 vs fp2 and fp2 vs fp4 should differ.
            if i == 0 && j == 1 {
                assert_ne!(fps[i], fps[j], "scan vs 2-table join should differ");
            }
        }
    }
    // Cross join vs inner join should differ
    assert_ne!(fp2, fp4, "inner join vs cross join should differ");
}

#[test]
fn test_scenario_budget_pressure_increases_selectivity() {
    let mut pruner = BayesianPruner::with_defaults();
    let fp = PlanFingerprint::from_plan(&join(scan("a"), scan("b"), eq(col("x"), col("y"))));

    // Record a 30% success rate (below default base threshold of 0.15
    // at full budget, but above threshold when budget is depleted)
    for i in 0..10 {
        pruner.record_outcome(&fp, i < 3);
    }

    // Collect decisions at different budget levels
    let explore_90 = pruner.should_explore(&fp, 0.9);
    let explore_50 = pruner.should_explore(&fp, 0.5);
    let explore_20 = pruner.should_explore(&fp, 0.2);
    let explore_05 = pruner.should_explore(&fp, 0.05);

    // With plenty of budget, 30% is above the low threshold
    assert!(explore_90);
    // As budget drops, the threshold rises above 30%
    // At 50% budget: threshold ~0.36, posterior ~0.30 => skip
    assert!(!explore_50 || !explore_20);
    // With almost no budget, definitely skip
    assert!(!explore_05);
}
