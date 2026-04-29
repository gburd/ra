#![expect(clippy::float_cmp, reason = "test code")]
//! Comprehensive tests for RFC 0059 v2: event-driven plan cache
//! invalidation using differential dataflow.
//!
//! Tests verify:
//! - Single-dimension invalidation (row count, NDV, index, histogram)
//! - Multi-table dependency tracking
//! - Unaffected plans stay cached
//! - Soft vs hard invalidation behavior
//! - Edge cases (zero rows, missing stats, empty changes)
//! - End-to-end flow: detect changes -> compute affected -> invalidate

#![expect(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{HashMap, HashSet};

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::cost::StatisticsProvider;
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::statistics::{
    ColumnStats, EquiWidthHistogram, Histogram, HistogramBucket, IndexStats, Statistics,
};
use ra_engine::differential::{
    ChangeSource, HistogramDigest, IndexChange, PlanDependencies, ResourceId, StalenessThresholds,
    StatisticsChange,
};
use ra_engine::genetic_fingerprint::QueryFingerprint;
use ra_engine::plan_cache::{CacheMatchType, PlanCache, PlanCacheConfig};
use ra_engine::{change_ratio, IncrementalOptimizer};

// ── Helpers ─────────────────────────────────────────────────────

fn scan(table: &str) -> RelExpr {
    RelExpr::scan(table)
}

fn scan_filter(table: &str, col: &str, val: i64) -> RelExpr {
    RelExpr::scan(table).filter(Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(Expr::Column(ColumnRef::new(col))),
        right: Box::new(Expr::Const(Const::Int(val))),
    })
}

fn join_query(left_table: &str, right_table: &str) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified(left_table, "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified(right_table, "fk_id"))),
        },
        left: Box::new(scan(left_table)),
        right: Box::new(scan(right_table)),
    }
}

fn make_deps(tables: &[(&str, f64)]) -> PlanDependencies {
    PlanDependencies {
        table_cardinalities: tables.iter().map(|(t, r)| ((*t).to_string(), *r)).collect(),
        indexes: HashSet::new(),
        distinct_counts: HashMap::new(),
        histogram_digests: HashMap::new(),
        facts: HashSet::new(),
    }
}

fn make_deps_with_ndv(tables: &[(&str, f64)], columns: &[(&str, &str, f64)]) -> PlanDependencies {
    let mut deps = make_deps(tables);
    for (table, col, ndv) in columns {
        deps.distinct_counts
            .insert(((*table).to_string(), (*col).to_string()), *ndv);
    }
    deps
}

fn make_deps_with_index(tables: &[(&str, f64)], indexes: &[(&str, &str)]) -> PlanDependencies {
    let mut deps = make_deps(tables);
    for (table, idx) in indexes {
        deps.indexes
            .insert(((*table).to_string(), (*idx).to_string()));
    }
    deps
}

// ── 1. Row count change triggers invalidation ───────────────────

#[test]
fn row_count_change_invalidates_plan() {
    let mut opt = IncrementalOptimizer::new();
    let plan = scan("users");
    let fp = QueryFingerprint::from_rel_expr(&plan);
    let deps = make_deps(&[("users", 1000.0)]);

    opt.register_plan_dependencies(&fp, &deps);

    let changes = vec![ChangeSource::Statistics(StatisticsChange::RowCount {
        table: "users".into(),
        old_value: 1000.0,
        new_value: 100_000.0,
        ratio: 100.0,
    })];

    let affected = opt
        .compute_affected_plans(&changes)
        .expect("should succeed");
    assert_eq!(affected.len(), 1);
    assert_eq!(affected[0], fp);
}

// ── 2. NDV change triggers invalidation ─────────────────────────

#[test]
fn ndv_change_invalidates_plan() {
    let mut opt = IncrementalOptimizer::new();
    let plan = scan_filter("users", "city", 1);
    let fp = QueryFingerprint::from_rel_expr(&plan);
    let deps = make_deps_with_ndv(&[("users", 1000.0)], &[("users", "city", 100.0)]);

    opt.register_plan_dependencies(&fp, &deps);

    let changes = vec![ChangeSource::Statistics(StatisticsChange::DistinctCount {
        table: "users".into(),
        column: "city".into(),
        old_value: 100.0,
        new_value: 500.0,
        ratio: 5.0,
    })];

    let affected = opt
        .compute_affected_plans(&changes)
        .expect("should succeed");
    assert_eq!(affected.len(), 1);
}

// ── 3. Index drop triggers invalidation ─────────────────────────

#[test]
fn index_drop_invalidates_plan() {
    let mut opt = IncrementalOptimizer::new();
    let plan = scan("orders");
    let fp = QueryFingerprint::from_rel_expr(&plan);
    let deps = make_deps_with_index(&[("orders", 5000.0)], &[("orders", "idx_date")]);

    opt.register_plan_dependencies(&fp, &deps);

    let changes = vec![ChangeSource::Index(IndexChange::Dropped {
        table: "orders".into(),
        index_name: "idx_date".into(),
    })];

    let affected = opt
        .compute_affected_plans(&changes)
        .expect("should succeed");
    assert_eq!(affected.len(), 1);
}

// ── 4. Unaffected queries stay cached ───────────────────────────

#[test]
fn unaffected_query_stays_cached() {
    let mut opt = IncrementalOptimizer::new();

    let fp_users = QueryFingerprint::from_rel_expr(&scan("users"));
    let fp_orders = QueryFingerprint::from_rel_expr(&scan("orders"));

    opt.register_plan_dependencies(&fp_users, &make_deps(&[("users", 1000.0)]));
    opt.register_plan_dependencies(&fp_orders, &make_deps(&[("orders", 5000.0)]));

    let changes = vec![ChangeSource::Statistics(StatisticsChange::RowCount {
        table: "orders".into(),
        old_value: 5000.0,
        new_value: 50_000.0,
        ratio: 10.0,
    })];

    let affected = opt
        .compute_affected_plans(&changes)
        .expect("should succeed");
    assert_eq!(affected.len(), 1);
    assert_eq!(affected[0], fp_orders);
}

// ── 5. Multi-table dependency invalidation ──────────────────────

#[test]
fn multi_table_join_affected_by_any_table_change() {
    let mut opt = IncrementalOptimizer::new();
    let plan = join_query("users", "orders");
    let fp = QueryFingerprint::from_rel_expr(&plan);
    let deps = make_deps(&[("users", 1000.0), ("orders", 5000.0)]);

    opt.register_plan_dependencies(&fp, &deps);

    // Change only orders
    let changes = vec![ChangeSource::Statistics(StatisticsChange::RowCount {
        table: "orders".into(),
        old_value: 5000.0,
        new_value: 50_000.0,
        ratio: 10.0,
    })];

    let affected = opt
        .compute_affected_plans(&changes)
        .expect("should succeed");
    assert_eq!(affected.len(), 1);
    assert_eq!(affected[0], fp);
}

// ── 6. Edge case: zero row count ────────────────────────────────

#[test]
fn zero_to_nonzero_row_count_detected() {
    let ratio = change_ratio(0.0, 1000.0);
    assert_eq!(ratio, f64::MAX);
}

#[test]
fn zero_to_zero_row_count_no_change() {
    let ratio = change_ratio(0.0, 0.0);
    assert!((ratio - 1.0).abs() < 1e-10);
}

// ── 7. Empty changes produce no invalidation ────────────────────

#[test]
fn empty_changes_no_invalidation() {
    let mut opt = IncrementalOptimizer::new();
    let fp = QueryFingerprint::from_rel_expr(&scan("t"));
    opt.register_plan_dependencies(&fp, &make_deps(&[("t", 100.0)]));

    let affected = opt.compute_affected_plans(&[]).expect("should succeed");
    assert!(affected.is_empty());
}

// ── 8. Missing stats produce no false positives ─────────────────

#[test]
fn no_registered_plans_no_invalidation() {
    let opt = IncrementalOptimizer::new();
    let changes = vec![ChangeSource::Statistics(StatisticsChange::RowCount {
        table: "phantom".into(),
        old_value: 1.0,
        new_value: 1000.0,
        ratio: 1000.0,
    })];

    let affected = opt
        .compute_affected_plans(&changes)
        .expect("should succeed");
    assert!(affected.is_empty());
}

// ── 9. Detect changes: cardinality below threshold ──────────────

#[test]
fn detect_changes_below_cardinality_threshold() {
    let opt = IncrementalOptimizer::new();
    let old = Statistics::new(1000.0);
    let new = Statistics::new(1500.0);
    let changes = opt.detect_changes("t", &old, &new);
    assert!(
        changes.is_empty(),
        "1.5x ratio should be below default 2.0 threshold"
    );
}

// ── 10. Detect changes: cardinality above threshold ─────────────

#[test]
fn detect_changes_above_cardinality_threshold() {
    let opt = IncrementalOptimizer::new();
    let old = Statistics::new(1000.0);
    let new = Statistics::new(3000.0);
    let changes = opt.detect_changes("t", &old, &new);
    assert_eq!(changes.len(), 1);
    assert!(matches!(
        &changes[0],
        ChangeSource::Statistics(StatisticsChange::RowCount {
            ratio,
            ..
        }) if *ratio >= 2.0
    ));
}

// ── 11. Detect NDV changes ──────────────────────────────────────

#[test]
fn detect_ndv_change() {
    let opt = IncrementalOptimizer::new();
    let mut old = Statistics::new(1000.0);
    old.columns.insert("city".into(), ColumnStats::new(100.0));
    let mut new = Statistics::new(1000.0);
    new.columns.insert("city".into(), ColumnStats::new(200.0));
    let changes = opt.detect_changes("t", &old, &new);
    assert!(changes.iter().any(|c| matches!(
        c,
        ChangeSource::Statistics(
            StatisticsChange::DistinctCount { column, .. }
        ) if column == "city"
    )));
}

// ── 12. Detect index addition ───────────────────────────────────

#[test]
fn detect_index_added() {
    let opt = IncrementalOptimizer::new();
    let old = Statistics::new(1000.0);
    let mut new = Statistics::new(1000.0);
    new.indexes.insert(
        "idx_new".into(),
        IndexStats::new(vec!["col".into()], ra_core::facts::IndexType::BTree),
    );
    let changes = opt.detect_changes("t", &old, &new);
    assert!(changes.iter().any(|c| matches!(
        c,
        ChangeSource::Index(IndexChange::Added {
            index_name,
            ..
        }) if index_name == "idx_new"
    )));
}

// ── 13. Detect index dropped ────────────────────────────────────

#[test]
fn detect_index_dropped() {
    let opt = IncrementalOptimizer::new();
    let mut old = Statistics::new(1000.0);
    old.indexes.insert(
        "idx_old".into(),
        IndexStats::new(vec!["col".into()], ra_core::facts::IndexType::BTree),
    );
    let new = Statistics::new(1000.0);
    let changes = opt.detect_changes("t", &old, &new);
    assert!(changes.iter().any(|c| matches!(
        c,
        ChangeSource::Index(IndexChange::Dropped {
            index_name,
            ..
        }) if index_name == "idx_old"
    )));
}

// ── 14. Histogram KL divergence detection ───────────────────────

#[test]
fn histogram_kl_divergence_above_threshold() {
    let d1 = HistogramDigest {
        bucket_count: 3,
        frequencies: vec![0.1, 0.1, 0.8],
        total_rows: 100.0,
    };
    let d2 = HistogramDigest {
        bucket_count: 3,
        frequencies: vec![0.8, 0.1, 0.1],
        total_rows: 100.0,
    };
    let kl = d1.kl_divergence(&d2);
    assert!(kl > 0.5, "Reversed distribution should have KL > 0.5");
}

#[test]
fn histogram_kl_divergence_identical() {
    let d = HistogramDigest {
        bucket_count: 4,
        frequencies: vec![0.25, 0.25, 0.25, 0.25],
        total_rows: 1000.0,
    };
    let kl = d.kl_divergence(&d);
    assert!(kl < 1e-6, "Identical distributions should have KL ~ 0");
}

#[test]
fn histogram_kl_divergence_different_bucket_count() {
    let d1 = HistogramDigest {
        bucket_count: 2,
        frequencies: vec![0.5, 0.5],
        total_rows: 100.0,
    };
    let d2 = HistogramDigest {
        bucket_count: 3,
        frequencies: vec![0.33, 0.33, 0.34],
        total_rows: 100.0,
    };
    let kl = d1.kl_divergence(&d2);
    assert_eq!(kl, f64::MAX);
}

// ── 15. End-to-end: detect -> compute -> invalidate ─────────────

#[test]
fn end_to_end_invalidation_flow() {
    let mut opt = IncrementalOptimizer::new();
    let mut cache = PlanCache::with_defaults();

    // Register 3 plans
    let plans: Vec<(RelExpr, &str, f64)> = vec![
        (scan("users"), "users", 1000.0),
        (scan("orders"), "orders", 5000.0),
        (scan("products"), "products", 200.0),
    ];

    for (plan, table, rows) in &plans {
        let fp = QueryFingerprint::from_rel_expr(plan);
        let deps = make_deps(&[(table, *rows)]);
        cache.insert_with_deps(fp.clone(), plan.clone(), deps.clone());
        opt.register_plan_dependencies(&fp, &deps);
    }
    assert_eq!(cache.len(), 3);
    assert_eq!(opt.plan_dependency_count(), 3);

    // Simulate ANALYZE on orders table: 5000 -> 50000
    let old_stats = Statistics::new(5000.0);
    let new_stats = Statistics::new(50_000.0);
    let changes = opt.detect_changes("orders", &old_stats, &new_stats);
    assert!(!changes.is_empty());

    // Compute affected plans
    let affected = opt
        .compute_affected_plans(&changes)
        .expect("should succeed");
    assert_eq!(affected.len(), 1);

    // Invalidate in the cache
    cache.invalidate(&affected);

    // Orders plan should be evicted (cold entry)
    let fp_orders = QueryFingerprint::from_rel_expr(&scan("orders"));
    assert!(
        cache.lookup(&fp_orders).is_none(),
        "orders plan should be evicted"
    );

    // Users and products plans should still be cached
    let fp_users = QueryFingerprint::from_rel_expr(&scan("users"));
    let fp_products = QueryFingerprint::from_rel_expr(&scan("products"));
    assert!(cache.lookup(&fp_users).is_some());
    assert!(cache.lookup(&fp_products).is_some());
}

// ── 16. Soft invalidation for hot entries ───────────────────────

#[test]
fn hot_entry_gets_soft_invalidation() {
    let config = PlanCacheConfig {
        soft_invalidation_hit_threshold: 5,
        ..PlanCacheConfig::default()
    };
    let mut cache = PlanCache::new(config);

    let plan = scan("users");
    let fp = QueryFingerprint::from_rel_expr(&plan);
    cache.insert(fp.clone(), plan);

    // Simulate heavy usage
    for _ in 0..10 {
        let _ = cache.lookup(&fp);
    }

    cache.invalidate(std::slice::from_ref(&fp));

    // Should still be in cache but marked stale
    let result = cache.lookup(&fp).expect("should hit");
    assert_eq!(result.match_type, CacheMatchType::Stale);
    assert_eq!(cache.stats().soft_invalidations, 1);
    assert_eq!(cache.stats().hard_invalidations, 0);
}

// ── 17. Custom threshold configuration ──────────────────────────

#[test]
fn custom_thresholds_affect_detection() {
    let thresholds = StalenessThresholds {
        cardinality_ratio: 10.0,
        ndistinct_ratio: 5.0,
        index_changes_trigger: false,
        histogram_kl_threshold: 1.0,
        max_age: None,
    };
    let opt = IncrementalOptimizer::with_thresholds(
        ra_engine::OptimizerConfig::default(),
        ra_engine::TimelyConfig::default(),
        thresholds,
    );

    // 3x change should be below the 10x threshold
    let old = Statistics::new(1000.0);
    let new = Statistics::new(3000.0);
    let changes = opt.detect_changes("t", &old, &new);
    assert!(changes.is_empty(), "3x should be below 10x threshold");

    // Index change should be ignored
    let mut new_with_idx = Statistics::new(1000.0);
    new_with_idx.indexes.insert(
        "idx".into(),
        IndexStats::new(vec!["col".into()], ra_core::facts::IndexType::BTree),
    );
    let changes = opt.detect_changes("t", &old, &new_with_idx);
    assert!(
        changes.is_empty(),
        "index_changes_trigger=false should suppress"
    );
}

// ── 18. Multiple changes invalidate multiple plans ──────────────

#[test]
fn multiple_changes_invalidate_multiple_plans() {
    let mut opt = IncrementalOptimizer::new();
    let fp_a = QueryFingerprint::from_rel_expr(&scan("a"));
    let fp_b = QueryFingerprint::from_rel_expr(&scan("b"));
    let fp_c = QueryFingerprint::from_rel_expr(&scan("c"));

    opt.register_plan_dependencies(&fp_a, &make_deps(&[("a", 100.0)]));
    opt.register_plan_dependencies(&fp_b, &make_deps(&[("b", 200.0)]));
    opt.register_plan_dependencies(&fp_c, &make_deps(&[("c", 300.0)]));

    let changes = vec![
        ChangeSource::Statistics(StatisticsChange::RowCount {
            table: "a".into(),
            old_value: 100.0,
            new_value: 10_000.0,
            ratio: 100.0,
        }),
        ChangeSource::Statistics(StatisticsChange::RowCount {
            table: "c".into(),
            old_value: 300.0,
            new_value: 30_000.0,
            ratio: 100.0,
        }),
    ];

    let affected = opt
        .compute_affected_plans(&changes)
        .expect("should succeed");
    assert_eq!(affected.len(), 2);
    assert!(affected.contains(&fp_a));
    assert!(affected.contains(&fp_c));
    assert!(!affected.contains(&fp_b));
}

// ── 19. Resource ID key format ──────────────────────────────────

#[test]
fn resource_id_key_format() {
    assert_eq!(
        ResourceId::RowCount("users".into()).key(),
        "users.row_count"
    );
    assert_eq!(
        ResourceId::NDistinct("users".into(), "city".into()).key(),
        "users.city.ndistinct"
    );
    assert_eq!(
        ResourceId::Index("orders".into(), "idx_date".into()).key(),
        "orders.idx_date"
    );
    assert_eq!(
        ResourceId::Histogram("t".into(), "col".into()).key(),
        "t.col.histogram"
    );
    assert_eq!(ResourceId::Fact("pk_users".into()).key(), "pk_users");
}

// ── 20. PlanDependencies from plan and stats ────────────────────

#[test]
fn plan_dependencies_from_plan_and_stats() {
    #[derive(Debug)]
    struct TestProvider {
        tables: HashMap<String, Statistics>,
    }
    impl StatisticsProvider for TestProvider {
        fn get_statistics(&self, table: &str) -> Option<&Statistics> {
            self.tables.get(table)
        }
    }

    let mut stats = Statistics::new(1000.0);
    stats.columns.insert("city".into(), ColumnStats::new(50.0));
    stats.indexes.insert(
        "pk_users".into(),
        IndexStats::new(vec!["id".into()], ra_core::facts::IndexType::BTree),
    );

    let provider = TestProvider {
        tables: [("users".into(), stats)].into_iter().collect(),
    };

    let plan = scan("users");
    let deps = PlanDependencies::from_plan_and_stats(&plan, &provider);

    assert!(deps.table_cardinalities.contains_key("users"));
    assert!(deps
        .distinct_counts
        .contains_key(&("users".into(), "city".into())));
    assert!(deps.indexes.contains(&("users".into(), "pk_users".into())));
}

// ── 21. Invalidate for table ────────────────────────────────────

#[test]
fn invalidate_for_table_works() {
    let mut cache = PlanCache::with_defaults();

    let p1 = scan("users");
    let fp1 = QueryFingerprint::from_rel_expr(&p1);
    cache.insert_with_deps(fp1.clone(), p1, make_deps(&[("users", 1000.0)]));

    let p2 = scan("orders");
    let fp2 = QueryFingerprint::from_rel_expr(&p2);
    cache.insert_with_deps(fp2.clone(), p2, make_deps(&[("orders", 5000.0)]));

    cache.invalidate_for_table("users");
    assert!(cache.lookup(&fp1).is_none());
    assert!(cache.lookup(&fp2).is_some());
}

// ── 22. Mark fresh after soft invalidation ──────────────────────

#[test]
fn mark_fresh_restores_exact_match() {
    let config = PlanCacheConfig {
        soft_invalidation_hit_threshold: 1,
        ..PlanCacheConfig::default()
    };
    let mut cache = PlanCache::new(config);

    let plan = scan("users");
    let fp = QueryFingerprint::from_rel_expr(&plan);
    cache.insert(fp.clone(), plan);
    let _ = cache.lookup(&fp); // hit_count >= threshold

    cache.invalidate(std::slice::from_ref(&fp));
    assert_eq!(cache.lookup(&fp).unwrap().match_type, CacheMatchType::Stale);

    cache.mark_fresh(&fp);
    assert_eq!(cache.lookup(&fp).unwrap().match_type, CacheMatchType::Exact);
}

// ── 23. Detect histogram drift via detect_changes ───────────────

#[test]
fn detect_histogram_drift_via_detect_changes() {
    let thresholds = StalenessThresholds {
        histogram_kl_threshold: 0.1,
        ..StalenessThresholds::default()
    };

    let opt = IncrementalOptimizer::with_thresholds(
        ra_engine::OptimizerConfig::default(),
        ra_engine::TimelyConfig::default(),
        thresholds,
    );

    let mut old = Statistics::new(1000.0);
    let mut old_col = ColumnStats::new(100.0);
    old_col.histogram = Some(Histogram::EquiWidth(EquiWidthHistogram {
        buckets: vec![
            HistogramBucket {
                upper_bound: "50".into(),
                row_count: 500.0,
                distinct_count: 50.0,
            },
            HistogramBucket {
                upper_bound: "100".into(),
                row_count: 500.0,
                distinct_count: 50.0,
            },
        ],
    }));
    old.columns.insert("age".into(), old_col);

    let mut new = Statistics::new(1000.0);
    let mut new_col = ColumnStats::new(100.0);
    new_col.histogram = Some(Histogram::EquiWidth(EquiWidthHistogram {
        buckets: vec![
            HistogramBucket {
                upper_bound: "50".into(),
                row_count: 100.0,
                distinct_count: 10.0,
            },
            HistogramBucket {
                upper_bound: "100".into(),
                row_count: 900.0,
                distinct_count: 90.0,
            },
        ],
    }));
    new.columns.insert("age".into(), new_col);

    let changes = opt.detect_changes("t", &old, &new);
    assert!(changes.iter().any(|c| matches!(
        c,
        ChangeSource::Statistics(StatisticsChange::HistogramDrift { .. })
    )));
}
