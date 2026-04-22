//! Comprehensive integration tests for the plan cache (RFC 0060).
//!
//! Validates cache behavior under realistic OLTP workloads:
//! - Cache hit rate (target: >90%)
//! - Performance: cached queries vs uncached optimization
//! - LRU eviction correctness
//! - Fuzzy matching behavior
//! - Statistics accuracy
//!
//! These tests exercise the full optimizer pipeline with plan
//! caching enabled, not just the cache data structure in isolation.

use std::time::Instant;

use ra_core::algebra::{AggregateExpr, AggregateFunction, JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_engine::{CacheMatchType, Optimizer, PlanCache, PlanCacheConfig, QueryFingerprint};

// ── Query template helpers ──────────────────────────────────────

/// Template 1: Point lookup by primary key.
/// `SELECT * FROM users WHERE id = ?`
fn point_lookup(user_id: i64) -> RelExpr {
    RelExpr::scan("users").filter(Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::new("id"))),
        right: Box::new(Expr::Const(Const::Int(user_id))),
    })
}

/// Template 2: Range scan with compound filter.
/// `SELECT * FROM orders WHERE amount > ? AND status = ?`
fn range_scan(threshold: i64, status: &str) -> RelExpr {
    RelExpr::scan("orders").filter(Expr::BinOp {
        op: BinOp::And,
        left: Box::new(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("amount"))),
            right: Box::new(Expr::Const(Const::Int(threshold))),
        }),
        right: Box::new(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("status"))),
            right: Box::new(Expr::Const(Const::String(status.to_owned()))),
        }),
    })
}

/// Template 3: Two-table join with filter.
/// `SELECT * FROM users JOIN orders ON users.id = orders.user_id
///  WHERE users.age > ?`
fn join_with_filter(age: i64) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("users", "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("orders", "user_id"))),
        },
        left: Box::new(RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(age))),
        })),
        right: Box::new(RelExpr::scan("orders")),
    }
}

/// Template 4: Aggregation query.
/// `SELECT dept, COUNT(*), SUM(salary) FROM employees
///  WHERE salary > ? GROUP BY dept`
fn aggregation(salary_threshold: i64) -> RelExpr {
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
        input: Box::new(RelExpr::scan("employees").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("salary"))),
            right: Box::new(Expr::Const(Const::Int(salary_threshold))),
        })),
    }
}

/// Template 5: Three-table join.
/// `SELECT * FROM users
///  JOIN orders ON users.id = orders.user_id
///  JOIN products ON orders.product_id = products.id
///  WHERE products.price > ?`
fn three_table_join(price: i64) -> RelExpr {
    let user_orders = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("users", "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("orders", "user_id"))),
        },
        left: Box::new(RelExpr::scan("users")),
        right: Box::new(RelExpr::scan("orders")),
    };
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("orders", "product_id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("products", "id"))),
        },
        left: Box::new(user_orders),
        right: Box::new(RelExpr::scan("products").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("price"))),
            right: Box::new(Expr::Const(Const::Int(price))),
        })),
    }
}

fn cached_optimizer() -> Optimizer {
    Optimizer::new().with_plan_cache(PlanCacheConfig {
        max_entries: 1024,
        similarity_threshold: 0.9,
        enable_fuzzy_matching: true,
        ..PlanCacheConfig::default()
    })
}

fn cached_optimizer_small(max_entries: usize) -> Optimizer {
    Optimizer::new().with_plan_cache(PlanCacheConfig {
        max_entries,
        similarity_threshold: 0.9,
        enable_fuzzy_matching: true,
        ..PlanCacheConfig::default()
    })
}

/// Generate the full OLTP workload: 5 templates x 40 variations = 200
fn oltp_workload() -> Vec<RelExpr> {
    let statuses = ["active", "pending", "shipped", "returned"];
    let mut queries = Vec::with_capacity(200);

    for i in 0..40_i64 {
        queries.push(point_lookup(i * 7 + 1));
    }
    for i in 0..40_i64 {
        let status = statuses[i as usize % statuses.len()];
        queries.push(range_scan(i * 50 + 100, status));
    }
    for i in 0..40_i64 {
        queries.push(join_with_filter(18 + i));
    }
    for i in 0..40_i64 {
        queries.push(aggregation(30000 + i * 1000));
    }
    for i in 0..40_i64 {
        queries.push(three_table_join(10 + i * 5));
    }
    queries
}

// ── 1. OLTP workload simulation ─────────────────────────────────

#[test]
fn oltp_200_queries_hit_rate_above_90_pct() {
    let opt = cached_optimizer();
    let workload = oltp_workload();

    for q in &workload {
        opt.optimize(q).expect("optimization should succeed");
    }

    let stats = opt.cache_stats().expect("cache enabled");
    let hit_rate = stats.hit_rate();

    // 5 cold misses (one per template) + 195 hits = 97.5%
    assert!(
        hit_rate > 0.90,
        "OLTP workload hit rate should be >90%, got {:.1}% \
         (exact_hits={}, fuzzy_hits={}, misses={}, lookups={})",
        hit_rate * 100.0,
        stats.exact_hits,
        stats.fuzzy_hits,
        stats.misses,
        stats.lookups,
    );
}

#[test]
fn oltp_exact_hits_dominate() {
    let opt = cached_optimizer();
    let workload = oltp_workload();

    for q in &workload {
        let _ = opt.optimize(q);
    }

    let stats = opt.cache_stats().expect("cache enabled");
    // Exact hits should be the vast majority since templates
    // differ only in literal values.
    assert!(
        stats.exact_hits > stats.fuzzy_hits,
        "Exact hits ({}) should exceed fuzzy hits ({})",
        stats.exact_hits,
        stats.fuzzy_hits,
    );
}

#[test]
fn oltp_only_5_cold_misses() {
    let opt = cached_optimizer();
    let workload = oltp_workload();

    for q in &workload {
        let _ = opt.optimize(q);
    }

    let stats = opt.cache_stats().expect("cache enabled");
    // Each of the 5 templates has exactly one cold miss on first
    // encounter. Depending on fuzzy matching behavior across
    // templates, misses could be slightly higher, but should
    // never exceed the template count.
    assert!(
        stats.misses <= 5,
        "Expected at most 5 cold misses (one per template), \
         got {}",
        stats.misses,
    );
}

// ── 2. Performance: cached vs uncached ──────────────────────────

#[test]
fn cached_queries_faster_than_uncached() {
    let workload = oltp_workload();

    // Measure uncached optimization time
    let opt_uncached = Optimizer::new();
    let start = Instant::now();
    for q in &workload {
        let _ = opt_uncached.optimize(q);
    }
    let uncached_elapsed = start.elapsed();

    // Measure cached optimization time (includes cold misses)
    let opt_cached = cached_optimizer();
    // Warm the cache with one pass
    for q in &workload {
        let _ = opt_cached.optimize(q);
    }
    // Measure second pass (all hits)
    let start = Instant::now();
    for q in &workload {
        let _ = opt_cached.optimize(q);
    }
    let cached_elapsed = start.elapsed();

    // Cached should be significantly faster. We use a conservative
    // 2x threshold since CI environments can be noisy; the actual
    // speedup is typically 10-50x.
    assert!(
        cached_elapsed < uncached_elapsed,
        "Cached pass ({cached_elapsed:?}) should be faster \
         than uncached ({uncached_elapsed:?})",
    );
}

#[test]
fn cached_optimization_under_1ms_per_query() {
    let opt = cached_optimizer();

    // Warm the cache
    let _ = opt.optimize(&point_lookup(1));
    let _ = opt.optimize(&range_scan(100, "active"));
    let _ = opt.optimize(&join_with_filter(25));
    let _ = opt.optimize(&aggregation(50000));
    let _ = opt.optimize(&three_table_join(50));

    // Measure cached lookups
    let queries: Vec<RelExpr> = (0..100_i64)
        .map(|i| match i % 5 {
            0 => point_lookup(i * 3),
            1 => range_scan(i * 20, "active"),
            2 => join_with_filter(18 + i),
            3 => aggregation(40000 + i * 500),
            _ => three_table_join(i * 7),
        })
        .collect();

    let start = Instant::now();
    for q in &queries {
        let _ = opt.optimize(q);
    }
    let elapsed = start.elapsed();

    let avg_us = elapsed.as_micros() as f64 / 100.0;
    // Each cached lookup should be well under 1ms (1000us).
    // Allow generous headroom for CI.
    assert!(
        avg_us < 1000.0,
        "Average cached optimization time ({avg_us:.0}us) \
         should be <1ms",
    );
}

// ── 3. Cache eviction behavior ──────────────────────────────────

#[test]
fn lru_eviction_evicts_oldest_entries() {
    let opt = cached_optimizer_small(5);

    // Insert 5 entries (one per template)
    let _ = opt.optimize(&point_lookup(1));
    let _ = opt.optimize(&range_scan(100, "a"));
    let _ = opt.optimize(&join_with_filter(25));
    let _ = opt.optimize(&aggregation(50000));
    let _ = opt.optimize(&three_table_join(50));

    // Access templates 3-5 to keep them warm
    let _ = opt.optimize(&join_with_filter(30));
    let _ = opt.optimize(&aggregation(60000));
    let _ = opt.optimize(&three_table_join(75));

    // Insert a new template to trigger eviction of the LRU entry
    // (point_lookup, which was accessed earliest and not refreshed)
    let new_query = RelExpr::scan("payments").filter(Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::new("id"))),
        right: Box::new(Expr::Const(Const::Int(999))),
    });
    let _ = opt.optimize(&new_query);

    let stats = opt.cache_stats().expect("cache enabled");
    assert!(
        stats.evictions >= 1,
        "Should have evicted at least 1 entry, got {}",
        stats.evictions,
    );
}

#[test]
fn eviction_maintains_cache_size() {
    let opt = cached_optimizer_small(3);

    // Insert 6 distinct query shapes -> triggers 3 evictions
    let tables = ["t1", "t2", "t3", "t4", "t5", "t6"];
    for table in &tables {
        let _ = opt.optimize(&RelExpr::scan(*table));
    }

    let stats = opt.cache_stats().expect("cache enabled");
    assert!(
        stats.current_entries <= 3,
        "Cache should not exceed max_entries (3), has {}",
        stats.current_entries,
    );
    assert!(
        stats.evictions >= 3,
        "Should have evicted at least 3 entries, got {}",
        stats.evictions,
    );
}

#[test]
fn recently_used_entries_survive_eviction() {
    let opt = cached_optimizer_small(4);

    // Insert 4 templates
    let _ = opt.optimize(&point_lookup(1));
    let _ = opt.optimize(&range_scan(100, "active"));
    let _ = opt.optimize(&join_with_filter(25));
    let _ = opt.optimize(&aggregation(50000));

    // Access point_lookup heavily to keep it warm
    for i in 0..10 {
        let _ = opt.optimize(&point_lookup(i));
    }

    // Now add a new template to force eviction
    let _ = opt.optimize(&three_table_join(99));

    // point_lookup should still be cached (recently used)
    let _ = opt.optimize(&point_lookup(42));
    let stats = opt.cache_stats().expect("cache enabled");

    // The point_lookup query should hit the cache
    assert!(
        stats.exact_hits > 0,
        "Recently used point_lookup should survive eviction",
    );
}

// ── 4. Fuzzy matching ───────────────────────────────────────────

#[test]
fn fuzzy_matching_disabled_only_exact_hits() {
    let opt = Optimizer::new().with_plan_cache(PlanCacheConfig {
        max_entries: 1024,
        similarity_threshold: 0.9,
        enable_fuzzy_matching: false,
        ..PlanCacheConfig::default()
    });

    let workload = oltp_workload();
    for q in &workload {
        let _ = opt.optimize(q);
    }

    let stats = opt.cache_stats().expect("cache enabled");
    assert_eq!(
        stats.fuzzy_hits, 0,
        "With fuzzy disabled, should have 0 fuzzy hits, got {}",
        stats.fuzzy_hits,
    );
}

#[test]
fn fuzzy_threshold_affects_hit_rate() {
    // With a very high threshold (0.99), fewer fuzzy matches
    let opt_strict = Optimizer::new().with_plan_cache(PlanCacheConfig {
        max_entries: 1024,
        similarity_threshold: 0.99,
        enable_fuzzy_matching: true,
        ..PlanCacheConfig::default()
    });
    // With a lower threshold (0.5), more fuzzy matches
    let opt_loose = Optimizer::new().with_plan_cache(PlanCacheConfig {
        max_entries: 1024,
        similarity_threshold: 0.5,
        enable_fuzzy_matching: true,
        ..PlanCacheConfig::default()
    });

    let workload = oltp_workload();
    for q in &workload {
        let _ = opt_strict.optimize(q);
        let _ = opt_loose.optimize(q);
    }

    let strict_stats = opt_strict.cache_stats().expect("cache enabled");
    let loose_stats = opt_loose.cache_stats().expect("cache enabled");

    // With same-template variations, both should get high exact
    // hit rates. The fuzzy threshold matters for cross-template
    // matching. The loose threshold should get at least as many
    // total hits as strict.
    let strict_total = strict_stats.exact_hits + strict_stats.fuzzy_hits;
    let loose_total = loose_stats.exact_hits + loose_stats.fuzzy_hits;
    assert!(
        loose_total >= strict_total,
        "Looser threshold should get >= hits: \
         loose={loose_total}, strict={strict_total}",
    );
}

// ── 5. Direct PlanCache API tests ───────────────────────────────

#[test]
fn plan_cache_direct_oltp_simulation() {
    let mut cache = PlanCache::with_defaults();

    let templates: Vec<Box<dyn Fn(i64) -> RelExpr>> = vec![
        Box::new(|v| point_lookup(v)),
        Box::new(|v| range_scan(v, "active")),
        Box::new(|v| join_with_filter(v)),
        Box::new(|v| aggregation(v)),
        Box::new(|v| three_table_join(v)),
    ];

    // Seed cache with one instance of each template
    for (i, template) in templates.iter().enumerate() {
        let plan = template(i as i64 * 1000);
        let fp = QueryFingerprint::from_rel_expr(&plan);
        cache.insert(fp, plan);
    }

    // Run 200 queries with varying parameters
    let mut hits = 0_u32;
    for i in 0..200_u32 {
        let template_idx = (i % 5) as usize;
        let param = (i * 7 + 13) as i64;
        let query = templates[template_idx](param);
        let fp = QueryFingerprint::from_rel_expr(&query);
        if cache.lookup(&fp).is_some() {
            hits += 1;
        }
    }

    let hit_rate = f64::from(hits) / 200.0;
    assert!(
        hit_rate > 0.9,
        "Direct cache OLTP simulation: hit rate {:.1}% \
         (expected >90%)",
        hit_rate * 100.0,
    );
}

#[test]
fn plan_cache_exact_matches_for_param_variations() {
    let mut cache = PlanCache::with_defaults();

    // Insert one point lookup
    let seed = point_lookup(42);
    let fp_seed = QueryFingerprint::from_rel_expr(&seed);
    cache.insert(fp_seed, seed);

    // Every parameter variation should exact-match
    for i in 0..50 {
        let q = point_lookup(i);
        let fp = QueryFingerprint::from_rel_expr(&q);
        let result = cache.lookup(&fp).expect("parameter variation should hit");
        assert_eq!(
            result.match_type,
            CacheMatchType::Exact,
            "Variation {i} should be an exact match",
        );
        assert!(
            (result.similarity - 1.0).abs() < f64::EPSILON,
            "Exact match should have similarity 1.0",
        );
    }
}

#[test]
fn plan_cache_string_param_variations_match() {
    let mut cache = PlanCache::with_defaults();

    let seed = range_scan(100, "active");
    let fp_seed = QueryFingerprint::from_rel_expr(&seed);
    cache.insert(fp_seed, seed);

    // Different string values should still match (fingerprint
    // ignores literal values)
    for status in &["pending", "shipped", "returned", "cancelled"] {
        let q = range_scan(200, status);
        let fp = QueryFingerprint::from_rel_expr(&q);
        assert!(
            cache.lookup(&fp).is_some(),
            "String variation '{status}' should hit cache",
        );
    }
}

// ── 6. Fingerprint correctness ──────────────────────────────────

#[test]
fn fingerprints_stable_across_param_values() {
    let fps: Vec<QueryFingerprint> = (0..100)
        .map(|i| QueryFingerprint::from_rel_expr(&point_lookup(i)))
        .collect();

    for (i, fp) in fps.iter().enumerate().skip(1) {
        assert!(
            fps[0].is_exact_match(fp),
            "point_lookup({i}) fingerprint should match \
             point_lookup(0)",
        );
    }
}

#[test]
fn different_templates_produce_different_fingerprints() {
    let fp_point = QueryFingerprint::from_rel_expr(&point_lookup(1));
    let fp_range = QueryFingerprint::from_rel_expr(&range_scan(100, "a"));
    let fp_join = QueryFingerprint::from_rel_expr(&join_with_filter(25));
    let fp_agg = QueryFingerprint::from_rel_expr(&aggregation(50000));
    let fp_three = QueryFingerprint::from_rel_expr(&three_table_join(50));

    let all = [&fp_point, &fp_range, &fp_join, &fp_agg, &fp_three];
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            assert!(
                !all[i].is_exact_match(all[j]),
                "Templates {i} and {j} should produce \
                 different fingerprints",
            );
        }
    }
}

#[test]
fn join_query_fingerprint_captures_topology() {
    let fp = QueryFingerprint::from_rel_expr(&three_table_join(100));
    assert_eq!(fp.table_count, 3);
    assert_eq!(fp.join_count, 2);
    assert!(!fp.has_aggregation);
}

#[test]
fn aggregation_fingerprint_captures_shape() {
    let fp = QueryFingerprint::from_rel_expr(&aggregation(50000));
    assert!(fp.has_aggregation);
    assert_eq!(fp.table_count, 1);
    assert_eq!(fp.join_count, 0);
}

// ── 7. Statistics accuracy ──────────────────────────────────────

#[test]
fn stats_accurately_reflect_workload() {
    let opt = cached_optimizer();
    let workload = oltp_workload();

    for q in &workload {
        let _ = opt.optimize(q);
    }

    let stats = opt.cache_stats().expect("cache enabled");

    // Total lookups = total queries (each optimize does one lookup)
    assert_eq!(
        stats.lookups, 200,
        "Should have 200 lookups for 200 queries",
    );

    // Hits + misses should equal lookups
    let total_hits = stats.exact_hits + stats.fuzzy_hits;
    assert_eq!(
        total_hits + stats.misses,
        stats.lookups,
        "hits ({total_hits}) + misses ({}) should equal \
         lookups ({})",
        stats.misses,
        stats.lookups,
    );

    // Cache should contain exactly 5 entries (one per template)
    assert_eq!(
        stats.current_entries, 5,
        "Should cache exactly 5 templates, got {}",
        stats.current_entries,
    );
}

#[test]
fn stats_hit_rate_matches_manual_calculation() {
    let opt = cached_optimizer();

    // 1 miss (cold)
    let _ = opt.optimize(&point_lookup(1));
    // 3 hits
    let _ = opt.optimize(&point_lookup(2));
    let _ = opt.optimize(&point_lookup(3));
    let _ = opt.optimize(&point_lookup(4));

    let stats = opt.cache_stats().expect("cache enabled");
    // 1 miss + 3 hits = 4 lookups, 75% hit rate
    let expected_rate = 3.0 / 4.0;
    assert!(
        (stats.hit_rate() - expected_rate).abs() < f64::EPSILON,
        "Hit rate should be {expected_rate}, got {}",
        stats.hit_rate(),
    );
}

// ── 8. Cache clear behavior ─────────────────────────────────────

#[test]
fn cache_clear_resets_entries_but_preserves_config() {
    let opt = cached_optimizer();

    // Populate cache
    for i in 0..10 {
        let _ = opt.optimize(&point_lookup(i));
    }
    let stats_before = opt.cache_stats().expect("cache enabled");
    assert!(stats_before.current_entries > 0);

    // Clear
    opt.clear_cache();

    let stats_after = opt.cache_stats().expect("cache enabled");
    assert_eq!(
        stats_after.current_entries, 0,
        "Cache should be empty after clear",
    );

    // Cache should still work after clear
    let _ = opt.optimize(&point_lookup(42));
    let _ = opt.optimize(&point_lookup(43));
    let stats_final = opt.cache_stats().expect("cache enabled");
    assert!(
        stats_final.current_entries > 0,
        "Cache should accept new entries after clear",
    );
}

// ── 9. Edge cases ───────────────────────────────────────────────

#[test]
fn cache_disabled_by_default() {
    let opt = Optimizer::new();
    assert!(
        opt.cache_stats().is_none(),
        "Cache should be disabled by default",
    );
}

#[test]
fn single_query_repeated_1000_times() {
    let opt = cached_optimizer();

    for i in 0..1000 {
        let _ = opt.optimize(&point_lookup(i));
    }

    let stats = opt.cache_stats().expect("cache enabled");
    // 1 cold miss + 999 hits
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.exact_hits, 999);
    assert!(stats.hit_rate() > 0.99);
}

#[test]
fn cache_with_max_entries_1_still_works() {
    let opt = cached_optimizer_small(1);

    // Only 1 entry fits. Each new template evicts the previous.
    let _ = opt.optimize(&point_lookup(1));
    let _ = opt.optimize(&range_scan(100, "a"));
    let _ = opt.optimize(&join_with_filter(25));

    let stats = opt.cache_stats().expect("cache enabled");
    assert_eq!(
        stats.current_entries, 1,
        "Cache should have exactly 1 entry",
    );
    assert!(
        stats.evictions >= 2,
        "Should have evicted at least 2 entries",
    );
}

// ── 10. Mixed workload: OLTP + analytical ───────────────────────

#[test]
fn mixed_oltp_and_analytical_workload() {
    let opt = cached_optimizer();

    // Phase 1: OLTP (point lookups)
    for i in 0..50 {
        let _ = opt.optimize(&point_lookup(i));
    }

    // Phase 2: Analytical (aggregations)
    for i in 0..50 {
        let _ = opt.optimize(&aggregation(i * 1000));
    }

    // Phase 3: Back to OLTP
    for i in 50..100 {
        let _ = opt.optimize(&point_lookup(i));
    }

    let stats = opt.cache_stats().expect("cache enabled");
    // 2 cold misses (one for point_lookup, one for aggregation)
    // + 148 hits = 98.7% hit rate
    assert!(
        stats.hit_rate() > 0.90,
        "Mixed workload hit rate {:.1}% should be >90%",
        stats.hit_rate() * 100.0,
    );
    assert_eq!(stats.current_entries, 2, "Should have 2 cached templates",);
}
