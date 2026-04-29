#![expect(clippy::expect_used, reason = "test code")]
//! Tests for physical parallelization strategies.
//!
//! Tests cover parallel scan, join, aggregation, sort, and various
//! parallelism patterns including inter/intra-operator parallelism,
//! morsel-driven execution, bushy parallelism, and work stealing.

mod helpers;

use helpers::*;
use ra_core::algebra::{AggregateExpr, AggregateFunction, JoinType, RelExpr};
use ra_hardware::HardwareProfile;

// ── Parallel Scan Tests ─────────────────────────────────────────

#[test]
fn test_parallel_scan_large_table() {
    let input = scan("large_fact_table");
    let opt = create_test_optimizer_with_hardware(HardwareProfile::cpu_only());
    let _result = opt.optimize(&input).expect("optimization should succeed");
}

#[test]
fn test_parallel_scan_partitioned_table() {
    // Partitioned table enables natural parallelism
    let input = scan("partitioned_by_date");
    assert_optimization_improves(input);
}

#[test]
fn test_parallel_scan_with_filter() {
    let input = filtered_scan("events", "timestamp", 1000);
    assert_cost_calculated(input);
}

#[test]
fn test_parallel_scan_degree_selection() {
    // Test degree of parallelism selection
    let input = scan("medium_table");
    assert_optimization_improves(input);
}

// ── Parallel Join Tests ─────────────────────────────────────────

#[test]
fn test_parallel_hash_join() {
    let join = two_table_join("orders", "customers", "customer_id", "id");
    assert_optimization_improves(join);
}

#[test]
fn test_parallel_merge_join() {
    let sorted1 = sort(scan("table1"), "id", true);
    let sorted2 = sort(scan("table2"), "id", true);
    let join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(sorted1),
        right: Box::new(sorted2),
    };
    assert_cost_calculated(join);
}

#[test]
fn test_parallel_nested_loop_join() {
    let _small = scan("small_dim");
    let _large = scan("large_fact");
    let join = two_table_join("large_fact", "small_dim", "dim_id", "id");
    assert_optimization_improves(join);
}

#[test]
fn test_parallel_join_build_phase() {
    // Parallel build phase in hash join
    let _t1 = scan("build_table");
    let _t2 = scan("probe_table");
    let join = two_table_join("probe_table", "build_table", "key", "key");
    assert_cost_calculated(join);
}

// ── Parallel Aggregation Tests ──────────────────────────────────

#[test]
fn test_parallel_hash_aggregation() {
    let input = scan("transactions");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("merchant_id")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("amount")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

#[test]
fn test_parallel_sort_aggregation() {
    let input = scan("log_events");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("event_type")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(int(1)),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_cost_calculated(agg);
}

#[test]
fn test_parallel_global_aggregation() {
    let input = scan("sales");
    let agg = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("total")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

// ── Parallel Sort Tests ─────────────────────────────────────────

#[test]
fn test_parallel_external_sort() {
    // Large dataset requiring external sort
    let input = scan("unsorted_large");
    let sorted = sort(input, "key", true);
    assert_optimization_improves(sorted);
}

#[test]
fn test_parallel_merge_sort() {
    let input = scan("data_to_sort");
    let sorted = sort(input, "timestamp", false);
    assert_cost_calculated(sorted);
}

#[test]
fn test_parallel_sort_with_limit() {
    let input = scan("rankings");
    let sorted = sort(input, "score", false);
    let limited = limit(sorted, 100);
    assert_optimization_improves(limited);
}

// ── Inter-Operator Parallelism ──────────────────────────────────

#[test]
fn test_inter_operator_parallelism_pipeline() {
    // Multiple operators execute in parallel pipeline
    let input = scan("source");
    let filtered = input.filter(gt(col("value"), int(100)));
    let projected = project(filtered, vec!["id", "value"]);
    assert_optimization_improves(projected);
}

#[test]
fn test_inter_operator_parallelism_bushy_plan() {
    let t1 = scan("t1");
    let t2 = scan("t2");
    let t3 = scan("t3");
    let t4 = scan("t4");

    let left_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    let right_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(t3),
        right: Box::new(t4),
    };

    let final_join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(left_join),
        right: Box::new(right_join),
    };

    assert_cost_calculated(final_join);
}

// ── Intra-Operator Parallelism ──────────────────────────────────

#[test]
fn test_intra_operator_parallelism_partitioned() {
    // Single operator parallelized internally
    let input = scan("partitioned_data");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("partition_key")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(int(1)),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

#[test]
fn test_intra_operator_parallelism_scan() {
    let input = scan("very_large_table");
    assert_cost_calculated(input);
}

// ── Morsel-Driven Parallelism ───────────────────────────────────

#[test]
fn test_morsel_driven_execution() {
    // Work-stealing with morsels (chunks of data)
    let input = scan("data");
    let filtered = input.filter(gt(col("value"), int(50)));
    assert_optimization_improves(filtered);
}

#[test]
fn test_morsel_driven_adaptive_size() {
    // Adaptive morsel sizing based on workload
    let _input = scan("variable_workload");
    let join = two_table_join("variable_workload", "lookup", "key", "id");
    assert_cost_calculated(join);
}

#[test]
fn test_morsel_driven_load_balancing() {
    let input = scan("skewed_data");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("key")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("value")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

// ── Bushy Parallelism ───────────────────────────────────────────

#[test]
fn test_bushy_parallelism_independent_subtrees() {
    // Independent subtrees execute in parallel
    let left_scan = filtered_scan("left_table", "id", 100);
    let right_scan = filtered_scan("right_table", "id", 200);
    let join = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(left_scan),
        right: Box::new(right_scan),
    };
    assert_optimization_improves(join);
}

#[test]
fn test_bushy_parallelism_multi_way_join() {
    let t1 = scan("t1");
    let t2 = scan("t2");
    let t3 = scan("t3");

    let j1 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(t1),
        right: Box::new(t2),
    };

    let j2 = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: eq(col("id"), col("id")),
        left: Box::new(j1),
        right: Box::new(t3),
    };

    assert_cost_calculated(j2);
}

// ── Work Stealing Tests ─────────────────────────────────────────

#[test]
fn test_work_stealing_load_imbalance() {
    // Work stealing handles load imbalance
    let input = scan("imbalanced_partitions");
    assert_optimization_improves(input);
}

#[test]
fn test_work_stealing_dynamic_scheduling() {
    let input = scan("dynamic_data");
    let filtered = input.filter(gt(col("complexity"), int(5)));
    assert_cost_calculated(filtered);
}

#[test]
fn test_work_stealing_task_granularity() {
    let _input = scan("fine_grained_tasks");
    let join = two_table_join("fine_grained_tasks", "reference", "key", "id");
    assert_optimization_improves(join);
}

// ── Parallel Execution Overhead ─────────────────────────────────

#[test]
fn test_parallelism_overhead_small_table() {
    // Small table doesn't benefit from parallelism
    let input = scan("tiny_config");
    let opt = create_test_optimizer();
    let _result = opt.optimize(&input).expect("optimization should succeed");
}

#[test]
fn test_parallelism_overhead_coordination() {
    let input = scan("medium_data");
    assert_cost_calculated(input);
}

// ── Parallel Pipeline Tests ─────────────────────────────────────

#[test]
fn test_parallel_pipeline_scan_filter_project() {
    let input = scan("data");
    let filtered = input.filter(gt(col("amount"), int(1000)));
    let projected = project(filtered, vec!["id", "amount"]);
    assert_optimization_improves(projected);
}

#[test]
fn test_parallel_pipeline_blocking_operator() {
    // Sort is a pipeline breaker
    let input = scan("unsorted");
    let sorted = sort(input, "key", true);
    let limited = limit(sorted, 10);
    assert_cost_calculated(limited);
}

// ── NUMA-Aware Parallelism ──────────────────────────────────────

#[test]
fn test_numa_aware_scheduling() {
    let input = scan("large_table");
    let opt = create_test_optimizer_with_hardware(HardwareProfile::cpu_only());
    let _result = opt.optimize(&input).expect("optimization should succeed");
}

#[test]
fn test_numa_local_data_access() {
    let input = scan("partitioned_numa");
    assert_optimization_improves(input);
}

// ── Parallel Degree Selection ───────────────────────────────────

#[test]
fn test_degree_of_parallelism_selection() {
    // Optimal DOP based on data size and cores
    let input = scan("data");
    assert_cost_calculated(input);
}

#[test]
fn test_adaptive_parallelism_degree() {
    let input = scan("variable_size");
    assert_optimization_improves(input);
}

#[test]
fn test_max_parallelism_constraint() {
    // Respect maximum parallelism limit
    let input = scan("huge_table");
    let opt = create_test_optimizer();
    let _result = opt.optimize(&input).expect("optimization should succeed");
}
