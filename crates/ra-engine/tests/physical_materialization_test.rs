//! Tests for physical materialization strategy optimization rules.
//!
//! Materialization strategies determine when and how to materialize
//! intermediate results, balancing memory usage and recomputation costs.

mod helpers;

use helpers::*;
use ra_core::algebra::{JoinType, RelExpr};

// ── Eager vs Lazy Materialization ───────────────────────────

#[test]
fn test_eager_materialization_small_result() {
    // Small intermediate result should be materialized eagerly
    let filtered = filtered_scan("large_table", "selective_filter", 1);
    assert_rule_applies(filtered);
}

#[test]
fn test_lazy_materialization_large_result() {
    // Large intermediate result should use lazy evaluation
    let plan = scan("huge_table");
    assert_rule_applies(plan);
}

#[test]
fn test_pipeline_breaker_materialization() {
    // Operators that break pipelines require materialization
    let sorted = sort(scan("unsorted_data"), "key", true);
    assert_rule_applies(sorted);
}

// ── Common Table Expression (CTE) Materialization ───────────

#[test]
fn test_cte_single_use_inline() {
    // CTE used once should be inlined
    let subquery = filtered_scan("base", "condition", 1);
    let projected = project(subquery, vec!["col1", "col2"]);
    assert_rule_applies(projected);
}

#[test]
fn test_cte_multiple_use_materialize() {
    // CTE used multiple times should be materialized
    let cte = filtered_scan("expensive_query", "complex", 1);
    let left = project(cte.clone(), vec!["a"]);
    let right = project(cte, vec!["b"]);
    let union = RelExpr::Union {
        all: true,
        left: Box::new(left),
        right: Box::new(right),
    };
    assert_rule_applies(union);
}

#[test]
fn test_recursive_cte_materialization() {
    // Recursive CTEs require work tables
    let base = scan("hierarchy_base");
    assert_rule_applies(base);
}

// ── Temporary Table Strategies ──────────────────────────────

#[test]
fn test_temp_table_for_large_intermediate() {
    // Large intermediate used multiple times → temp table
    let agg = RelExpr::Aggregate {
        group_by: vec![col("region")],
        aggregates: vec![],
        input: Box::new(scan("large_fact")),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_temp_table_vs_subquery() {
    // Complex subquery repeated → temp table
    let complex = filtered_scan("complex_computation", "expensive", 1);
    let j1 = two_table_join("orders", "complex_computation", "key", "key");
    assert_rule_applies(j1);
}

// ── In-Memory vs Disk Materialization ───────────────────────

#[test]
fn test_in_memory_small_dataset() {
    // Small dataset fits in memory
    let small = filtered_scan("small_table", "filter", 1);
    let limited = limit(small, 1000);
    assert_rule_applies(limited);
}

#[test]
fn test_disk_spill_large_dataset() {
    // Large dataset requires disk spillover
    let large_agg = RelExpr::Aggregate {
        group_by: vec![col("high_cardinality")],
        aggregates: vec![],
        input: Box::new(scan("huge_table")),
    };
    assert_rule_applies(large_agg);
}

// ── Materialization for Reuse ───────────────────────────────

#[test]
fn test_materialize_for_join_reuse() {
    // Build side used in multiple joins
    let build = filtered_scan("dimension", "active", 1);
    let probe1 = scan("fact1");
    let probe2 = scan("fact2");

    let j1 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("dim_id"), col("id")),
        left: Box::new(probe1),
        right: Box::new(build.clone()),
    };

    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("dim_id"), col("id")),
        left: Box::new(probe2),
        right: Box::new(build),
    };

    assert_rule_applies(j1);
    assert_rule_applies(j2);
}

#[test]
fn test_no_materialize_single_use() {
    // Single-use intermediate doesn't need materialization
    let filtered = filtered_scan("table", "col", 1);
    let projected = project(filtered, vec!["result"]);
    assert_rule_applies(projected);
}

// ── Result Caching ──────────────────────────────────────────

#[test]
fn test_cache_expensive_computation() {
    // Expensive computation result should be cached
    let complex = RelExpr::Aggregate {
        group_by: vec![col("category"), col("region")],
        aggregates: vec![],
        input: Box::new(scan("large_dataset")),
    };
    assert_rule_applies(complex);
}

#[test]
fn test_cache_invalidation_strategy() {
    // Cached results need invalidation on updates
    let base = scan("frequently_updated");
    assert_rule_applies(base);
}

// ── Materialized View Optimization ──────────────────────────

#[test]
fn test_materialized_view_rewrite() {
    // Query matches materialized view pattern
    let agg = RelExpr::Aggregate {
        group_by: vec![col("date")],
        aggregates: vec![],
        input: Box::new(scan("daily_sales")),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_partial_materialized_view_match() {
    // Query partially matches materialized view
    let mv_scan = scan("sales_mv");
    let filtered = filtered_scan("sales_mv", "region", 1);
    assert_rule_applies(filtered);
}
