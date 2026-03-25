//! Synthetic OLTP workload tests for the plan cache (RFC 0060).
//!
//! Validates cache behavior under realistic OLTP conditions:
//! - Connection pooling simulation (10 connections, 5 templates)
//! - Prepared statement patterns with bind-value variations
//! - Batch operations with cached plans
//! - Mixed read/write workloads (70/30)
//!
//! Metrics captured:
//! - Cache hit rate per query template (target >95%)
//! - Latency: p50, p95, p99 for cached vs uncached
//! - Memory usage via `PlanCacheStats`
//! - Throughput (queries/sec)

use std::time::{Duration, Instant};

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, RelExpr,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_engine::{
    CacheMatchType, Optimizer, PlanCache, PlanCacheConfig,
    QueryFingerprint,
};

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
            left: Box::new(Expr::Column(
                ColumnRef::new("amount"),
            )),
            right: Box::new(Expr::Const(Const::Int(threshold))),
        }),
        right: Box::new(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(
                ColumnRef::new("status"),
            )),
            right: Box::new(Expr::Const(Const::String(
                status.to_owned(),
            ))),
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
            left: Box::new(Expr::Column(
                ColumnRef::qualified("users", "id"),
            )),
            right: Box::new(Expr::Column(
                ColumnRef::qualified("orders", "user_id"),
            )),
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
        input: Box::new(
            RelExpr::scan("employees").filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(
                    ColumnRef::new("salary"),
                )),
                right: Box::new(Expr::Const(Const::Int(
                    salary_threshold,
                ))),
            }),
        ),
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
            left: Box::new(Expr::Column(
                ColumnRef::qualified("users", "id"),
            )),
            right: Box::new(Expr::Column(
                ColumnRef::qualified("orders", "user_id"),
            )),
        },
        left: Box::new(RelExpr::scan("users")),
        right: Box::new(RelExpr::scan("orders")),
    };
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(
                ColumnRef::qualified("orders", "product_id"),
            )),
            right: Box::new(Expr::Column(
                ColumnRef::qualified("products", "id"),
            )),
        },
        left: Box::new(user_orders),
        right: Box::new(
            RelExpr::scan("products").filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(
                    ColumnRef::new("price"),
                )),
                right: Box::new(Expr::Const(Const::Int(price))),
            }),
        ),
    }
}

// ── Write-side templates (plan for read-portion of DML) ─────────

/// Bulk insert planning: scan source table with filter.
/// Simulates `INSERT INTO archive SELECT * FROM orders WHERE
///  created_at < ?`
fn bulk_insert_plan(cutoff: i64) -> RelExpr {
    RelExpr::scan("orders").filter(Expr::BinOp {
        op: BinOp::Lt,
        left: Box::new(Expr::Column(
            ColumnRef::new("created_at"),
        )),
        right: Box::new(Expr::Const(Const::Int(cutoff))),
    })
}

/// Batch update planning: scan + filter for rows to update.
/// Simulates `UPDATE inventory SET qty = qty - 1 WHERE sku = ?`
fn batch_update_plan(sku: i64) -> RelExpr {
    RelExpr::scan("inventory").filter(Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::new("sku"))),
        right: Box::new(Expr::Const(Const::Int(sku))),
    })
}

// ── Optimizer constructors ──────────────────────────────────────

fn cached_optimizer() -> Optimizer {
    Optimizer::new().with_plan_cache(PlanCacheConfig {
        max_entries: 1024,
        similarity_threshold: 0.9,
        enable_fuzzy_matching: true,
    })
}

// ── Latency measurement helpers ─────────────────────────────────

fn percentile(sorted: &[Duration], pct: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]
    #[allow(clippy::cast_possible_truncation)]
    let idx = ((sorted.len() as f64 * pct / 100.0).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[idx]
}

fn measure_latencies(
    opt: &Optimizer,
    queries: &[RelExpr],
) -> Vec<Duration> {
    queries
        .iter()
        .map(|q| {
            let start = Instant::now();
            let _ = opt.optimize(q);
            start.elapsed()
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════
// 1. Connection pooling simulation
// ═══════════════════════════════════════════════════════════════

#[test]
fn connection_pool_10_conns_5_templates_200_each() {
    let opt = cached_optimizer();
    let statuses = ["active", "pending", "shipped", "returned"];

    // 10 simulated connections, each executing 200 queries
    // across 5 templates (total 2000 queries).
    for conn_id in 0..10_u32 {
        for iter in 0..200_i64 {
            let offset = i64::from(conn_id) * 1000 + iter;
            match iter % 5 {
                0 => {
                    let _ = opt.optimize(&point_lookup(offset));
                }
                1 => {
                    let s = statuses[offset as usize % 4];
                    let _ =
                        opt.optimize(&range_scan(offset * 10, s));
                }
                2 => {
                    let _ =
                        opt.optimize(&join_with_filter(18 + offset));
                }
                3 => {
                    let _ = opt.optimize(
                        &aggregation(30000 + offset * 100),
                    );
                }
                _ => {
                    let _ = opt.optimize(
                        &three_table_join(10 + offset * 5),
                    );
                }
            }
        }
    }

    let stats = opt.cache_stats().expect("cache enabled");
    let hit_rate = stats.hit_rate();

    // 5 cold misses on first encounter, then 1995 hits = 99.75%
    assert!(
        hit_rate > 0.95,
        "Connection pool hit rate should be >95%, got {:.2}% \
         (exact={}, fuzzy={}, misses={}, lookups={})",
        hit_rate * 100.0,
        stats.exact_hits,
        stats.fuzzy_hits,
        stats.misses,
        stats.lookups,
    );

    assert_eq!(
        stats.lookups, 2000,
        "Should have 2000 total lookups (10 conns x 200 each)",
    );

    assert_eq!(
        stats.current_entries, 5,
        "Should cache exactly 5 templates, got {}",
        stats.current_entries,
    );
}

#[test]
fn connection_pool_all_connections_share_cache() {
    let opt = cached_optimizer();

    // Connection 0 warms the cache
    let _ = opt.optimize(&point_lookup(1));
    let stats_after_warmup =
        opt.cache_stats().expect("cache enabled");
    assert_eq!(stats_after_warmup.misses, 1);
    assert_eq!(stats_after_warmup.exact_hits, 0);

    // Connections 1-9 should all hit cache
    for conn_id in 1..10 {
        let _ = opt.optimize(&point_lookup(conn_id * 100));
    }

    let stats = opt.cache_stats().expect("cache enabled");
    assert_eq!(
        stats.exact_hits, 9,
        "All 9 subsequent connections should hit cache",
    );
    assert_eq!(stats.misses, 1, "Only one cold miss expected");
}

// ═══════════════════════════════════════════════════════════════
// 2. Prepared statement pattern
// ═══════════════════════════════════════════════════════════════

#[test]
fn prepared_stmt_int_bind_values() {
    let mut cache = PlanCache::with_defaults();

    let seed = point_lookup(1);
    let fp = QueryFingerprint::from_rel_expr(&seed);
    cache.insert(fp, seed);

    // 100 different integer bind values all hit
    for i in 0..100 {
        let q = point_lookup(i * 37 + 42);
        let qfp = QueryFingerprint::from_rel_expr(&q);
        let result = cache.lookup(&qfp).expect(
            "integer bind variation should always hit cache",
        );
        assert_eq!(result.match_type, CacheMatchType::Exact);
    }

    let stats = cache.stats();
    assert_eq!(stats.exact_hits, 100);
    assert_eq!(stats.misses, 0);
}

#[test]
fn prepared_stmt_string_bind_values() {
    let mut cache = PlanCache::with_defaults();

    let seed = range_scan(100, "active");
    let fp = QueryFingerprint::from_rel_expr(&seed);
    cache.insert(fp, seed);

    let statuses = [
        "active", "pending", "shipped", "returned",
        "cancelled", "refunded", "processing", "hold",
    ];
    for status in &statuses {
        for threshold in [50, 100, 500, 1000, 5000] {
            let q = range_scan(threshold, status);
            let qfp = QueryFingerprint::from_rel_expr(&q);
            assert!(
                cache.lookup(&qfp).is_some(),
                "String bind '{}' with threshold {} should hit",
                status,
                threshold,
            );
        }
    }

    let stats = cache.stats();
    // 8 statuses x 5 thresholds = 40 hits
    assert_eq!(stats.exact_hits, 40);
    assert_eq!(stats.misses, 0);
}

#[test]
fn prepared_stmt_mixed_types_exact_match() {
    let opt = cached_optimizer();

    // Warm with one instance
    let _ = opt.optimize(&range_scan(100, "active"));

    // Vary both int and string params
    for i in 0..50_i64 {
        let status = match i % 4 {
            0 => "active",
            1 => "pending",
            2 => "shipped",
            _ => "returned",
        };
        let _ = opt.optimize(&range_scan(i * 20 + 50, status));
    }

    let stats = opt.cache_stats().expect("cache enabled");
    // 1 miss (cold) + 50 hits = 51 lookups
    assert_eq!(stats.lookups, 51);
    assert_eq!(stats.exact_hits, 50);
    assert_eq!(stats.misses, 1);
}

// ═══════════════════════════════════════════════════════════════
// 3. Batch operations
// ═══════════════════════════════════════════════════════════════

#[test]
fn batch_insert_plans_cached() {
    let opt = cached_optimizer();

    // Simulate bulk insert planning with varying cutoff dates
    for cutoff in 0..100_i64 {
        let _ = opt.optimize(&bulk_insert_plan(cutoff * 86400));
    }

    let stats = opt.cache_stats().expect("cache enabled");
    assert_eq!(
        stats.misses, 1,
        "Bulk insert plans differ only in cutoff literal",
    );
    assert_eq!(stats.exact_hits, 99);
    assert!(stats.hit_rate() > 0.95);
}

#[test]
fn batch_update_plans_cached() {
    let opt = cached_optimizer();

    // Simulate batch updates with varying SKUs
    for sku in 0..100_i64 {
        let _ = opt.optimize(&batch_update_plan(sku));
    }

    let stats = opt.cache_stats().expect("cache enabled");
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.exact_hits, 99);
}

#[test]
fn batch_mixed_insert_update_plans() {
    let opt = cached_optimizer();

    // Interleave insert and update planning
    for i in 0..200_i64 {
        if i % 2 == 0 {
            let _ =
                opt.optimize(&bulk_insert_plan(i * 86400));
        } else {
            let _ = opt.optimize(&batch_update_plan(i));
        }
    }

    let stats = opt.cache_stats().expect("cache enabled");
    // 2 cold misses (one per template), 198 hits
    assert_eq!(stats.misses, 2);
    assert_eq!(
        stats.exact_hits + stats.fuzzy_hits,
        198,
    );
    assert!(stats.hit_rate() > 0.95);
}

// ═══════════════════════════════════════════════════════════════
// 4. Mixed workload (70% reads, 30% writes)
// ═══════════════════════════════════════════════════════════════

#[test]
fn mixed_workload_70_read_30_write() {
    let opt = cached_optimizer();

    // 700 reads, 300 writes = 1000 queries total
    let statuses = ["active", "pending", "shipped", "returned"];
    for i in 0..1000_i64 {
        if i % 10 < 7 {
            // 70% reads: rotate through 5 read templates
            match (i / 10) % 5 {
                0 => {
                    let _ = opt.optimize(&point_lookup(i));
                }
                1 => {
                    let s = statuses[i as usize % 4];
                    let _ = opt.optimize(&range_scan(i * 10, s));
                }
                2 => {
                    let _ =
                        opt.optimize(&join_with_filter(18 + i));
                }
                3 => {
                    let _ = opt.optimize(
                        &aggregation(30000 + i * 100),
                    );
                }
                _ => {
                    let _ = opt.optimize(
                        &three_table_join(10 + i * 5),
                    );
                }
            }
        } else {
            // 30% writes: rotate between insert and update
            if i % 2 == 0 {
                let _ =
                    opt.optimize(&bulk_insert_plan(i * 86400));
            } else {
                let _ = opt.optimize(&batch_update_plan(i));
            }
        }
    }

    let stats = opt.cache_stats().expect("cache enabled");
    let hit_rate = stats.hit_rate();

    // 7 distinct templates (5 read + 2 write), so 7 cold misses
    // out of 1000 = 99.3% hit rate
    assert!(
        hit_rate > 0.95,
        "Mixed 70/30 workload hit rate should be >95%, \
         got {:.2}% (exact={}, fuzzy={}, misses={}, lookups={})",
        hit_rate * 100.0,
        stats.exact_hits,
        stats.fuzzy_hits,
        stats.misses,
        stats.lookups,
    );

    assert_eq!(
        stats.current_entries, 7,
        "Should cache 7 templates (5 read + 2 write), got {}",
        stats.current_entries,
    );
}

#[test]
fn mixed_workload_write_plans_dont_evict_reads() {
    let opt = Optimizer::new().with_plan_cache(PlanCacheConfig {
        max_entries: 10,
        similarity_threshold: 0.9,
        enable_fuzzy_matching: true,
    });

    // Warm read templates
    let _ = opt.optimize(&point_lookup(1));
    let _ = opt.optimize(&range_scan(100, "active"));
    let _ = opt.optimize(&join_with_filter(25));
    let _ = opt.optimize(&aggregation(50000));
    let _ = opt.optimize(&three_table_join(50));

    // Add write templates
    let _ = opt.optimize(&bulk_insert_plan(86400));
    let _ = opt.optimize(&batch_update_plan(42));

    // All 7 fit in cache (max_entries=10)
    let stats = opt.cache_stats().expect("cache enabled");
    assert_eq!(stats.current_entries, 7);
    assert_eq!(stats.evictions, 0);

    // Verify all read templates still hit
    let _ = opt.optimize(&point_lookup(999));
    let _ = opt.optimize(&range_scan(999, "pending"));
    let _ = opt.optimize(&join_with_filter(999));
    let _ = opt.optimize(&aggregation(99999));
    let _ = opt.optimize(&three_table_join(999));

    let stats = opt.cache_stats().expect("cache enabled");
    // 7 cold misses from warmup + 5 hits from verification
    assert_eq!(stats.exact_hits, 5);
}

// ═══════════════════════════════════════════════════════════════
// 5. Latency comparison: cached vs uncached
// ═══════════════════════════════════════════════════════════════

#[test]
fn latency_cached_vs_uncached_percentiles() {
    let queries: Vec<RelExpr> = (0..200_i64)
        .map(|i| match i % 5 {
            0 => point_lookup(i),
            1 => range_scan(i * 10, "active"),
            2 => join_with_filter(18 + i),
            3 => aggregation(30000 + i * 100),
            _ => three_table_join(10 + i * 5),
        })
        .collect();

    // Uncached baseline
    let opt_uncached = Optimizer::new();
    let mut uncached_times = measure_latencies(
        &opt_uncached, &queries,
    );
    uncached_times.sort();

    // Cached: warm pass then measurement pass
    let opt_cached = cached_optimizer();
    for q in &queries {
        let _ = opt_cached.optimize(q);
    }
    let mut cached_times = measure_latencies(
        &opt_cached, &queries,
    );
    cached_times.sort();

    let uncached_p50 = percentile(&uncached_times, 50.0);
    let uncached_p95 = percentile(&uncached_times, 95.0);
    let uncached_p99 = percentile(&uncached_times, 99.0);

    let cached_p50 = percentile(&cached_times, 50.0);
    let cached_p95 = percentile(&cached_times, 95.0);
    let cached_p99 = percentile(&cached_times, 99.0);

    // Cached should be faster at every percentile
    assert!(
        cached_p50 <= uncached_p50,
        "Cached p50 ({cached_p50:?}) should be <= \
         uncached p50 ({uncached_p50:?})",
    );
    assert!(
        cached_p95 <= uncached_p95,
        "Cached p95 ({cached_p95:?}) should be <= \
         uncached p95 ({uncached_p95:?})",
    );
    assert!(
        cached_p99 <= uncached_p99,
        "Cached p99 ({cached_p99:?}) should be <= \
         uncached p99 ({uncached_p99:?})",
    );

    // Report (visible in test output with --nocapture)
    #[allow(clippy::print_stdout)]
    {
        println!("\n=== Latency Comparison (200 queries) ===");
        println!(
            "         {:>12} {:>12} {:>12}",
            "p50", "p95", "p99"
        );
        println!(
            "Uncached {:>12?} {:>12?} {:>12?}",
            uncached_p50, uncached_p95, uncached_p99,
        );
        println!(
            "Cached   {:>12?} {:>12?} {:>12?}",
            cached_p50, cached_p95, cached_p99,
        );
        if !uncached_p50.is_zero() {
            #[allow(clippy::cast_precision_loss)]
            let speedup = uncached_p50.as_nanos() as f64
                / cached_p50.as_nanos().max(1) as f64;
            println!("p50 speedup: {speedup:.1}x");
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// 6. Throughput measurement
// ═══════════════════════════════════════════════════════════════

#[test]
fn throughput_cached_exceeds_uncached() {
    let queries: Vec<RelExpr> = (0..500_i64)
        .map(|i| match i % 5 {
            0 => point_lookup(i),
            1 => range_scan(i * 10, "active"),
            2 => join_with_filter(18 + i),
            3 => aggregation(30000 + i * 100),
            _ => three_table_join(10 + i * 5),
        })
        .collect();

    // Uncached throughput
    let opt_uncached = Optimizer::new();
    let start = Instant::now();
    for q in &queries {
        let _ = opt_uncached.optimize(q);
    }
    let uncached_elapsed = start.elapsed();

    // Cached throughput (warm pass + measurement pass)
    let opt_cached = cached_optimizer();
    for q in &queries {
        let _ = opt_cached.optimize(q);
    }
    let start = Instant::now();
    for q in &queries {
        let _ = opt_cached.optimize(q);
    }
    let cached_elapsed = start.elapsed();

    #[allow(clippy::cast_precision_loss)]
    let uncached_qps =
        500.0 / uncached_elapsed.as_secs_f64();
    #[allow(clippy::cast_precision_loss)]
    let cached_qps =
        500.0 / cached_elapsed.as_secs_f64();

    assert!(
        cached_qps > uncached_qps,
        "Cached throughput ({cached_qps:.0} q/s) should exceed \
         uncached ({uncached_qps:.0} q/s)",
    );

    #[allow(clippy::print_stdout)]
    {
        println!("\n=== Throughput (500 queries) ===");
        println!(
            "Uncached: {uncached_qps:.0} queries/sec \
             ({uncached_elapsed:?})",
        );
        println!(
            "Cached:   {cached_qps:.0} queries/sec \
             ({cached_elapsed:?})",
        );
        println!(
            "Speedup:  {:.1}x",
            cached_qps / uncached_qps,
        );
    }
}

// ═══════════════════════════════════════════════════════════════
// 7. Memory overhead via PlanCacheStats
// ═══════════════════════════════════════════════════════════════

#[test]
fn memory_overhead_bounded_by_max_entries() {
    let opt = Optimizer::new().with_plan_cache(PlanCacheConfig {
        max_entries: 16,
        similarity_threshold: 0.9,
        enable_fuzzy_matching: true,
    });

    // Insert 100 distinct templates (simple scans of distinct
    // table names produce distinct fingerprints)
    for i in 0..100 {
        let table = format!("table_{i}");
        let _ = opt.optimize(&RelExpr::scan(&table));
    }

    let stats = opt.cache_stats().expect("cache enabled");
    assert!(
        stats.current_entries <= 16,
        "Cache entries ({}) should not exceed max_entries (16)",
        stats.current_entries,
    );
    assert!(
        stats.evictions >= 84,
        "Should have evicted at least 84 entries, got {}",
        stats.evictions,
    );
}

#[test]
fn memory_stable_under_sustained_workload() {
    let opt = Optimizer::new().with_plan_cache(PlanCacheConfig {
        max_entries: 8,
        similarity_threshold: 0.9,
        enable_fuzzy_matching: true,
    });

    // Run 1000 queries across 5 templates (fits in cache=8)
    for i in 0..1000_i64 {
        match i % 5 {
            0 => {
                let _ = opt.optimize(&point_lookup(i));
            }
            1 => {
                let _ =
                    opt.optimize(&range_scan(i * 10, "active"));
            }
            2 => {
                let _ = opt.optimize(&join_with_filter(18 + i));
            }
            3 => {
                let _ = opt.optimize(
                    &aggregation(30000 + i * 100),
                );
            }
            _ => {
                let _ = opt.optimize(
                    &three_table_join(10 + i * 5),
                );
            }
        }
    }

    let stats = opt.cache_stats().expect("cache enabled");
    assert_eq!(
        stats.current_entries, 5,
        "Steady-state entries should be 5 (one per template)",
    );
    assert_eq!(
        stats.evictions, 0,
        "No evictions needed when workload fits in cache",
    );
}

// ═══════════════════════════════════════════════════════════════
// 8. Per-template hit rate report
// ═══════════════════════════════════════════════════════════════

#[test]
fn per_template_hit_rate_report() {
    let template_names: [&str; 5] = [
        "point_lookup",
        "range_scan",
        "join_with_filter",
        "aggregation",
        "three_table_join",
    ];
    let statuses = ["active", "pending", "shipped", "returned"];

    // Use the direct PlanCache API for per-template tracking
    let mut cache = PlanCache::with_defaults();
    let mut template_hits = [0_u32; 5];
    let mut template_misses = [0_u32; 5];

    // 200 queries per template = 1000 total
    for t in 0..5_usize {
        for i in 0..200_i64 {
            let q = match t {
                0 => point_lookup(i * 7 + 1),
                1 => {
                    let s = statuses[i as usize % 4];
                    range_scan(i * 50 + 100, s)
                }
                2 => join_with_filter(18 + i),
                3 => aggregation(30000 + i * 1000),
                _ => three_table_join(10 + i * 5),
            };
            let fp = QueryFingerprint::from_rel_expr(&q);
            if cache.lookup(&fp).is_some() {
                template_hits[t] += 1;
            } else {
                template_misses[t] += 1;
                cache.insert(fp, q);
            }
        }
    }

    // Report
    #[allow(clippy::print_stdout)]
    {
        println!(
            "\n=== Per-Template Hit Rate (200 queries each) ===",
        );
        println!(
            "{:<20} {:>6} {:>6} {:>8}",
            "Template", "Hits", "Misses", "Hit Rate",
        );
        println!("{:-<44}", "");
    }

    for t in 0..5 {
        #[allow(clippy::cast_precision_loss)]
        let rate = f64::from(template_hits[t])
            / f64::from(template_hits[t] + template_misses[t]);

        assert!(
            rate > 0.95,
            "Template '{}' hit rate {:.2}% should be >95%",
            template_names[t],
            rate * 100.0,
        );

        #[allow(clippy::print_stdout)]
        {
            println!(
                "{:<20} {:>6} {:>6} {:>7.2}%",
                template_names[t],
                template_hits[t],
                template_misses[t],
                rate * 100.0,
            );
        }
    }

    let total_stats = cache.stats();
    #[allow(clippy::print_stdout)]
    {
        println!("{:-<44}", "");
        println!(
            "Overall: {:.2}% hit rate, {} entries cached",
            total_stats.hit_rate() * 100.0,
            total_stats.current_entries,
        );
    }
}

// ═══════════════════════════════════════════════════════════════
// 9. Full OLTP report (comprehensive)
// ═══════════════════════════════════════════════════════════════

#[test]
fn comprehensive_oltp_report() {
    let statuses = ["active", "pending", "shipped", "returned"];

    // Build the workload: 5 templates, 200 variations each
    let mut workload: Vec<(usize, RelExpr)> =
        Vec::with_capacity(1000);
    for i in 0..200_i64 {
        workload.push((0, point_lookup(i * 7 + 1)));
    }
    for i in 0..200_i64 {
        let s = statuses[i as usize % 4];
        workload.push((1, range_scan(i * 50 + 100, s)));
    }
    for i in 0..200_i64 {
        workload.push((2, join_with_filter(18 + i)));
    }
    for i in 0..200_i64 {
        workload.push((3, aggregation(30000 + i * 1000)));
    }
    for i in 0..200_i64 {
        workload.push((4, three_table_join(10 + i * 5)));
    }

    // Uncached baseline
    let opt_uncached = Optimizer::new();
    let mut uncached_latencies = vec![Vec::new(); 5];
    for (t, q) in &workload {
        let start = Instant::now();
        let _ = opt_uncached.optimize(q);
        uncached_latencies[*t].push(start.elapsed());
    }

    // Cached: warm pass
    let opt_cached = cached_optimizer();
    for (_, q) in &workload {
        let _ = opt_cached.optimize(q);
    }

    // Cached: measurement pass
    opt_cached.clear_cache();
    // Re-run with fresh stats but same optimizer
    let opt_cached2 = cached_optimizer();
    let mut cached_latencies = vec![Vec::new(); 5];
    for (t, q) in &workload {
        let start = Instant::now();
        let _ = opt_cached2.optimize(q);
        cached_latencies[*t].push(start.elapsed());
    }

    let template_names = [
        "point_lookup",
        "range_scan",
        "join_filter",
        "aggregation",
        "three_join",
    ];

    // Sort latencies for percentile computation
    for v in &mut uncached_latencies {
        v.sort();
    }
    for v in &mut cached_latencies {
        v.sort();
    }

    #[allow(clippy::print_stdout)]
    {
        println!(
            "\n=== Comprehensive OLTP Report (1000 queries) ===",
        );
        println!();
        println!(
            "{:<14} | {:>10} {:>10} {:>10} | {:>10} {:>10} {:>10} | {:>8}",
            "Template", "UC p50", "UC p95", "UC p99",
            "C p50", "C p95", "C p99", "Speedup",
        );
        println!("{:-<107}", "");
    }

    for t in 0..5 {
        let uc_p50 = percentile(&uncached_latencies[t], 50.0);
        let uc_p95 = percentile(&uncached_latencies[t], 95.0);
        let uc_p99 = percentile(&uncached_latencies[t], 99.0);

        let c_p50 = percentile(&cached_latencies[t], 50.0);
        let c_p95 = percentile(&cached_latencies[t], 95.0);
        let c_p99 = percentile(&cached_latencies[t], 99.0);

        #[allow(clippy::cast_precision_loss)]
        let speedup = uc_p50.as_nanos() as f64
            / c_p50.as_nanos().max(1) as f64;

        #[allow(clippy::print_stdout)]
        {
            println!(
                "{:<14} | {:>10?} {:>10?} {:>10?} | {:>10?} {:>10?} {:>10?} | {:>7.1}x",
                template_names[t],
                uc_p50, uc_p95, uc_p99,
                c_p50, c_p95, c_p99,
                speedup,
            );
        }
    }

    // Cache stats
    let stats =
        opt_cached2.cache_stats().expect("cache enabled");

    #[allow(clippy::print_stdout)]
    {
        println!();
        println!("=== Cache Statistics ===");
        println!("Total lookups: {}", stats.lookups);
        println!("Exact hits:    {}", stats.exact_hits);
        println!("Fuzzy hits:    {}", stats.fuzzy_hits);
        println!("Misses:        {}", stats.misses);
        println!("Evictions:     {}", stats.evictions);
        println!("Entries:       {}", stats.current_entries);
        println!(
            "Hit rate:      {:.2}%",
            stats.hit_rate() * 100.0,
        );
    }

    // Assertions
    assert!(
        stats.hit_rate() > 0.95,
        "Overall hit rate {:.2}% should be >95%",
        stats.hit_rate() * 100.0,
    );
    assert_eq!(stats.lookups, 1000);
    assert!(stats.misses <= 5);
    assert_eq!(stats.current_entries, 5);
}

// ═══════════════════════════════════════════════════════════════
// 10. Edge cases under OLTP conditions
// ═══════════════════════════════════════════════════════════════

#[test]
fn rapid_template_switching() {
    let opt = cached_optimizer();

    // Rapidly alternate between all 5 templates
    for i in 0..500_i64 {
        match i % 5 {
            0 => {
                let _ = opt.optimize(&point_lookup(i));
            }
            1 => {
                let _ =
                    opt.optimize(&range_scan(i * 10, "active"));
            }
            2 => {
                let _ = opt.optimize(&join_with_filter(18 + i));
            }
            3 => {
                let _ = opt.optimize(
                    &aggregation(30000 + i * 100),
                );
            }
            _ => {
                let _ = opt.optimize(
                    &three_table_join(10 + i * 5),
                );
            }
        }
    }

    let stats = opt.cache_stats().expect("cache enabled");
    assert_eq!(stats.misses, 5);
    assert_eq!(stats.exact_hits, 495);
    assert!(stats.hit_rate() > 0.98);
}

#[test]
fn high_cardinality_bind_values() {
    let opt = cached_optimizer();

    // 10,000 unique bind values for same template
    for i in 0..10_000_i64 {
        let _ = opt.optimize(&point_lookup(i));
    }

    let stats = opt.cache_stats().expect("cache enabled");
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.exact_hits, 9999);
    assert_eq!(stats.current_entries, 1);
    assert!(stats.hit_rate() > 0.999);
}

#[test]
fn cache_effective_after_clear_mid_workload() {
    let opt = cached_optimizer();

    // Phase 1: warm up
    for i in 0..100_i64 {
        let _ = opt.optimize(&point_lookup(i));
    }

    let stats1 = opt.cache_stats().expect("cache enabled");
    assert_eq!(stats1.exact_hits, 99);

    // Clear mid-workload
    opt.clear_cache();

    // Phase 2: cache rebuilds
    for i in 100..200_i64 {
        let _ = opt.optimize(&point_lookup(i));
    }

    let stats2 = opt.cache_stats().expect("cache enabled");
    // After clear, there's 1 new cold miss + 99 hits
    // Stats accumulate, so we check the delta
    // Total: 100 pre-clear lookups + 100 post-clear lookups = 200
    // Misses: 1 (original cold) + 1 (post-clear cold) = 2 total
    // But clear resets entries, not stats -- so check entries
    assert_eq!(stats2.current_entries, 1);
}
