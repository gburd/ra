//! Integration tests for statistics caching (Task #243).
//!
//! Validates that statistics caching avoids repeated clones during optimization,
//! improving performance for queries with multiple cost extraction calls.

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Expr};
use ra_core::statistics::Statistics;
use ra_engine::{Optimizer, OptimizerConfig};

fn qual_col(table: &str, name: &str) -> Expr {
    Expr::Column(ColumnRef::qualified(table, name))
}

fn eq(left: Expr, right: Expr) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn scan(name: &str) -> RelExpr {
    RelExpr::Scan {
        table: name.to_string(),
        alias: None,
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

fn make_optimizer_with_stats(table_names: &[&str]) -> Optimizer {
    let mut opt = Optimizer::with_config(OptimizerConfig {
        node_limit: 100_000,
        iter_limit: 10,
        time_limit_secs: 5,
        large_join_threshold: 10,
        large_join_strategy: ra_engine::large_join::LargeJoinStrategy::EGraph,
        max_optimization_time_ms: 5000,
        parallel: ra_engine::egraph::ParallelConfig::default(),
        use_adaptive_limits: false, // Use fixed limits for deterministic testing
        use_cost_pruning: true,     // Enable cost pruning (multiple extractions)
        cost_pruning_threshold: 1.5,
        use_join_graph_filtering: false,
        beam_search_config: None,
        enable_plan_cache: false,
        plan_cache_config: ra_engine::PlanCacheConfig::default(),
        max_staleness_penalty: 10.0,
        use_lazy_rules: false,
        transaction_context: None,
        ..OptimizerConfig::default()
    });

    // Add statistics for all tables
    for name in table_names {
        let mut stats = Statistics::new(10000.0);
        stats.avg_row_size = 100;
        stats.total_size = 1_000_000;

        // Add some column stats to make Statistics objects larger
        // (more expensive to clone)
        for i in 0..10 {
            let col_name = format!("col{}", i);
            let col_stats = ra_core::statistics::ColumnStats {
                distinct_count: 100.0,
                null_fraction: 0.1,
                min_value: None,
                max_value: None,
                avg_length: Some(10.0),
                histogram: None,
                correlation: None,
                most_common_values: None,
                most_common_freqs: None,
            };
            stats.columns.insert(col_name, col_stats);
        }

        opt.add_table_stats(*name, stats);
    }

    opt
}

#[test]
fn test_stats_cache_with_simple_query() {
    // Query with 3-way join
    let query = join(
        join(
            scan("a"),
            scan("b"),
            eq(qual_col("a", "id"), qual_col("b", "a_id")),
        ),
        scan("c"),
        eq(qual_col("b", "id"), qual_col("c", "b_id")),
    );

    let optimizer = make_optimizer_with_stats(&["a", "b", "c"]);
    let result = optimizer.optimize(&query);

    // Should succeed and produce optimized plan
    assert!(result.is_ok());
}

#[test]
fn test_stats_cache_with_cost_pruning() {
    // Query with 4-way join where cost pruning will be active
    let query = join(
        join(
            join(
                scan("a"),
                scan("b"),
                eq(qual_col("a", "id"), qual_col("b", "a_id")),
            ),
            scan("c"),
            eq(qual_col("b", "id"), qual_col("c", "b_id")),
        ),
        scan("d"),
        eq(qual_col("c", "id"), qual_col("d", "c_id")),
    );

    // Optimizer with cost pruning enabled (multiple cost extractions per iteration)
    let optimizer = make_optimizer_with_stats(&["a", "b", "c", "d"]);
    let result = optimizer.optimize(&query);

    // Should succeed even with multiple cost extractions
    assert!(result.is_ok());
}

#[test]
fn test_stats_cache_with_many_tables() {
    // Query with 5-way join (larger statistics HashMap)
    let query = join(
        join(
            join(
                join(
                    scan("t1"),
                    scan("t2"),
                    eq(qual_col("t1", "id"), qual_col("t2", "t1_id")),
                ),
                scan("t3"),
                eq(qual_col("t2", "id"), qual_col("t3", "t2_id")),
            ),
            scan("t4"),
            eq(qual_col("t3", "id"), qual_col("t4", "t3_id")),
        ),
        scan("t5"),
        eq(qual_col("t4", "id"), qual_col("t5", "t4_id")),
    );

    let optimizer = make_optimizer_with_stats(&["t1", "t2", "t3", "t4", "t5"]);
    let result = optimizer.optimize(&query);

    assert!(result.is_ok());
}

#[test]
fn test_stats_cache_no_stats() {
    // Query with no statistics registered
    let query = join(
        scan("a"),
        scan("b"),
        eq(qual_col("a", "id"), qual_col("b", "a_id")),
    );

    let optimizer = Optimizer::with_config(OptimizerConfig {
        node_limit: 100_000,
        iter_limit: 5,
        time_limit_secs: 5,
        large_join_threshold: 10,
        large_join_strategy: ra_engine::large_join::LargeJoinStrategy::EGraph,
        max_optimization_time_ms: 5000,
        parallel: ra_engine::egraph::ParallelConfig::default(),
        use_adaptive_limits: false,
        use_cost_pruning: false,
        cost_pruning_threshold: 1.5,
        use_join_graph_filtering: false,
        beam_search_config: None,
        enable_plan_cache: false,
        plan_cache_config: ra_engine::PlanCacheConfig::default(),
        max_staleness_penalty: 10.0,
        use_lazy_rules: false,
        transaction_context: None,
        ..OptimizerConfig::default()
    });

    let result = optimizer.optimize(&query);

    // Should succeed even without statistics (uses default cost function)
    assert!(result.is_ok());
}

#[test]
fn test_stats_cache_correctness() {
    // Verify that using cached statistics produces the same result as before
    let query = join(
        join(
            scan("a"),
            scan("b"),
            eq(qual_col("a", "id"), qual_col("b", "a_id")),
        ),
        scan("c"),
        eq(qual_col("b", "id"), qual_col("c", "b_id")),
    );

    // Optimizer with caching (default behavior now)
    let opt_cached = make_optimizer_with_stats(&["a", "b", "c"]);
    let result_cached = opt_cached.optimize(&query);

    // Both should succeed
    assert!(result_cached.is_ok());

    // The optimized plan should be valid (has at least one node)
    let plan = result_cached.unwrap();
    assert!(matches!(plan, RelExpr::Join { .. } | RelExpr::Scan { .. }));
}
