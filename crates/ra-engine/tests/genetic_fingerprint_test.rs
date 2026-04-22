//! Integration tests for genetic query fingerprinting and plan
//! cache (RFC 0060).
//!
//! Validates fingerprint generation, similarity detection, and
//! cache hit rates across representative workload patterns.

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering, RelExpr, SortDirection, SortKey,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_engine::genetic_fingerprint::QueryFingerprint;
use ra_engine::plan_cache::{CacheMatchType, PlanCache, PlanCacheConfig};

// ── Helpers ──────────────────────────────────────────────────────

fn eq_filter(table: &str, col: &str, value: i64) -> RelExpr {
    RelExpr::scan(table).filter(Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::new(col))),
        right: Box::new(Expr::Const(Const::Int(value))),
    })
}

fn range_filter(table: &str, col: &str, lo: i64, hi: i64) -> RelExpr {
    RelExpr::scan(table).filter(Expr::BinOp {
        op: BinOp::And,
        left: Box::new(Expr::BinOp {
            op: BinOp::Ge,
            left: Box::new(Expr::Column(ColumnRef::new(col))),
            right: Box::new(Expr::Const(Const::Int(lo))),
        }),
        right: Box::new(Expr::BinOp {
            op: BinOp::Le,
            left: Box::new(Expr::Column(ColumnRef::new(col))),
            right: Box::new(Expr::Const(Const::Int(hi))),
        }),
    })
}

fn two_table_join(left_val: i64, right_val: i64) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("users", "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("orders", "user_id"))),
        },
        left: Box::new(eq_filter("users", "age", left_val)),
        right: Box::new(eq_filter("orders", "total", right_val)),
    }
}

fn agg_query(threshold: i64) -> RelExpr {
    RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("dept"))],
        aggregates: vec![
            AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: None,
            },
            AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(Expr::Column(ColumnRef::new("salary"))),
                distinct: false,
                alias: None,
            },
        ],
        input: Box::new(eq_filter("employees", "salary", threshold)),
    }
}

// ── Fingerprint: parameter variations ────────────────────────────

#[test]
fn point_lookups_with_different_ids_match() {
    let fps: Vec<QueryFingerprint> = (0..100)
        .map(|i| QueryFingerprint::from_rel_expr(&eq_filter("users", "id", i)))
        .collect();

    for fp in &fps[1..] {
        assert!(
            fps[0].is_exact_match(fp),
            "Point lookups with different IDs should match"
        );
    }
}

#[test]
fn range_queries_with_different_bounds_match() {
    let fp1 = QueryFingerprint::from_rel_expr(&range_filter("sales", "amount", 100, 500));
    let fp2 = QueryFingerprint::from_rel_expr(&range_filter("sales", "amount", 1000, 9999));
    assert!(fp1.is_exact_match(&fp2));
}

#[test]
fn join_queries_with_different_params_match() {
    let fp1 = QueryFingerprint::from_rel_expr(&two_table_join(25, 100));
    let fp2 = QueryFingerprint::from_rel_expr(&two_table_join(60, 5000));
    assert!(fp1.is_exact_match(&fp2));
    assert!((fp1.similarity(&fp2) - 1.0).abs() < f64::EPSILON);
}

#[test]
fn aggregate_with_different_thresholds_match() {
    let fp1 = QueryFingerprint::from_rel_expr(&agg_query(50000));
    let fp2 = QueryFingerprint::from_rel_expr(&agg_query(80000));
    assert!(fp1.is_exact_match(&fp2));
}

// ── Fingerprint: structural differences ──────────────────────────

#[test]
fn different_join_types_different_fingerprint() {
    let inner = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("a"))),
            right: Box::new(Expr::Column(ColumnRef::new("b"))),
        },
        left: Box::new(RelExpr::scan("t1")),
        right: Box::new(RelExpr::scan("t2")),
    };
    let left_outer = RelExpr::Join {
        join_type: JoinType::LeftOuter,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("a"))),
            right: Box::new(Expr::Column(ColumnRef::new("b"))),
        },
        left: Box::new(RelExpr::scan("t1")),
        right: Box::new(RelExpr::scan("t2")),
    };
    let fp_inner = QueryFingerprint::from_rel_expr(&inner);
    let fp_left = QueryFingerprint::from_rel_expr(&left_outer);
    assert!(!fp_inner.is_exact_match(&fp_left));
}

#[test]
fn added_sort_changes_fingerprint() {
    let base = eq_filter("users", "age", 25);
    let sorted = RelExpr::Sort {
        keys: vec![SortKey {
            expr: Expr::Column(ColumnRef::new("name")),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }],
        input: Box::new(eq_filter("users", "age", 25)),
    };
    let fp_base = QueryFingerprint::from_rel_expr(&base);
    let fp_sorted = QueryFingerprint::from_rel_expr(&sorted);
    assert_ne!(fp_base.has_sort, fp_sorted.has_sort);
}

#[test]
fn three_table_vs_two_table_different() {
    let two = two_table_join(1, 1);
    let three = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("x"))),
            right: Box::new(Expr::Column(ColumnRef::new("y"))),
        },
        left: Box::new(two_table_join(1, 1)),
        right: Box::new(RelExpr::scan("items")),
    };
    let fp2 = QueryFingerprint::from_rel_expr(&two);
    let fp3 = QueryFingerprint::from_rel_expr(&three);
    assert_ne!(fp2.table_count, fp3.table_count);
    assert!(fp2.similarity(&fp3) < 0.9);
}

// ── Similarity scoring ───────────────────────────────────────────

#[test]
fn similarity_is_symmetric() {
    let q1 = eq_filter("users", "id", 1);
    let q2 = agg_query(50000);
    let fp1 = QueryFingerprint::from_rel_expr(&q1);
    let fp2 = QueryFingerprint::from_rel_expr(&q2);
    assert!((fp1.similarity(&fp2) - fp2.similarity(&fp1)).abs() < f64::EPSILON);
}

#[test]
fn similarity_range_0_to_1() {
    let queries = vec![
        RelExpr::scan("a"),
        eq_filter("b", "c", 1),
        two_table_join(1, 1),
        agg_query(1),
    ];
    let fps: Vec<QueryFingerprint> = queries
        .iter()
        .map(QueryFingerprint::from_rel_expr)
        .collect();
    for i in 0..fps.len() {
        for j in 0..fps.len() {
            let sim = fps[i].similarity(&fps[j]);
            assert!(
                (0.0..=1.0).contains(&sim),
                "Similarity {sim} out of range for ({i}, {j})"
            );
        }
    }
}

// ── Plan cache: OLTP workload simulation ─────────────────────────

#[test]
fn oltp_workload_high_cache_hit_rate() {
    let mut cache = PlanCache::with_defaults();

    // 5 query templates, each exercised with varying parameters
    let templates: Vec<Box<dyn Fn(i64) -> RelExpr>> = vec![
        Box::new(|v| eq_filter("users", "id", v)),
        Box::new(|v| eq_filter("orders", "id", v)),
        Box::new(|v| two_table_join(v, v * 10)),
        Box::new(|v| range_filter("products", "price", v, v + 100)),
        Box::new(|v| agg_query(v)),
    ];

    // Seed cache with one instance of each template
    for (i, template) in templates.iter().enumerate() {
        let plan = template(i as i64);
        let fp = QueryFingerprint::from_rel_expr(&plan);
        cache.insert(fp, plan);
    }

    // Run 200 queries with random parameter values
    let mut hits = 0_u32;
    for i in 0..200 {
        let template_idx = i % 5;
        let param = (i * 7 + 13) % 10000;
        let query = templates[template_idx](param as i64);
        let fp = QueryFingerprint::from_rel_expr(&query);
        if cache.lookup(&fp).is_some() {
            hits += 1;
        }
    }

    let hit_rate = f64::from(hits) / 200.0;
    assert!(
        hit_rate > 0.9,
        "OLTP workload should achieve >90% hit rate, got {hit_rate:.1}%"
    );
}

// ── Plan cache: exact vs fuzzy matching ──────────────────────────

#[test]
fn exact_match_preferred_over_fuzzy() {
    let mut cache = PlanCache::with_defaults();

    let plan = eq_filter("users", "id", 42);
    let fp = QueryFingerprint::from_rel_expr(&plan);
    cache.insert(fp.clone(), plan);

    // Same fingerprint -> exact match
    let result = cache.lookup(&fp);
    assert!(result.is_some());
    assert_eq!(
        result.expect("should hit").match_type,
        CacheMatchType::Exact
    );
}

#[test]
fn cache_miss_when_no_similar_entry() {
    let config = PlanCacheConfig {
        enable_fuzzy_matching: true,
        similarity_threshold: 0.9,
        ..PlanCacheConfig::default()
    };
    let mut cache = PlanCache::new(config);

    let plan = eq_filter("users", "id", 1);
    let fp = QueryFingerprint::from_rel_expr(&plan);
    cache.insert(fp, plan);

    // Completely different query -> miss
    let different = agg_query(1);
    let dfp = QueryFingerprint::from_rel_expr(&different);
    assert!(cache.lookup(&dfp).is_none());
}

#[test]
fn fuzzy_matching_can_be_disabled() {
    let config = PlanCacheConfig {
        enable_fuzzy_matching: false,
        ..PlanCacheConfig::default()
    };
    let mut cache = PlanCache::new(config);

    let plan = eq_filter("users", "id", 1);
    let fp = QueryFingerprint::from_rel_expr(&plan);
    cache.insert(fp, plan);

    // Different query that might fuzzy-match -> miss when disabled
    let other = eq_filter("users", "name", 1);
    let ofp = QueryFingerprint::from_rel_expr(&other);
    // This won't exact-match (different column name)
    // and fuzzy is disabled
    let result = cache.lookup(&ofp);
    assert!(result.is_none());
}

// ── Plan cache: eviction behavior ────────────────────────────────

#[test]
fn eviction_preserves_recently_used_entries() {
    let config = PlanCacheConfig {
        max_entries: 5,
        ..PlanCacheConfig::default()
    };
    let mut cache = PlanCache::new(config);

    // Insert 5 entries for tables t1..t5
    for i in 1..=5 {
        let table = format!("t{i}");
        let plan = RelExpr::scan(&table);
        let fp = QueryFingerprint::from_rel_expr(&plan);
        cache.insert(fp, plan);
    }

    // Access t3, t4, t5 (making t1, t2 the LRU candidates)
    for i in 3..=5 {
        let table = format!("t{i}");
        let plan = RelExpr::scan(&table);
        let fp = QueryFingerprint::from_rel_expr(&plan);
        let _ = cache.lookup(&fp);
    }

    // Insert 2 more entries -> evicts t1, t2
    for i in 6..=7 {
        let table = format!("t{i}");
        let plan = RelExpr::scan(&table);
        let fp = QueryFingerprint::from_rel_expr(&plan);
        cache.insert(fp, plan);
    }

    assert_eq!(cache.len(), 5);

    // t3, t4, t5 should still be present
    for i in 3..=5 {
        let table = format!("t{i}");
        let plan = RelExpr::scan(&table);
        let fp = QueryFingerprint::from_rel_expr(&plan);
        assert!(
            cache.lookup(&fp).is_some(),
            "Recently used t{i} should survive eviction"
        );
    }
}

// ── Plan cache: statistics ───────────────────────────────────────

#[test]
fn cache_stats_track_correctly() {
    let mut cache = PlanCache::with_defaults();

    // Miss
    let plan1 = eq_filter("users", "id", 1);
    let fp1 = QueryFingerprint::from_rel_expr(&plan1);
    assert!(cache.lookup(&fp1).is_none());

    // Insert + exact hit
    cache.insert(fp1.clone(), plan1);
    assert!(cache.lookup(&fp1).is_some());

    // Another miss (different query)
    let plan2 = agg_query(100);
    let fp2 = QueryFingerprint::from_rel_expr(&plan2);
    assert!(cache.lookup(&fp2).is_none());

    let stats = cache.stats();
    assert_eq!(stats.lookups, 3);
    assert_eq!(stats.exact_hits, 1);
    assert_eq!(stats.misses, 2);
    assert_eq!(stats.current_entries, 1);
}
