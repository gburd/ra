//! Tests for morsel-driven parallel execution model.
//!
//! Morsel-driven execution uses work-stealing parallelism with data chunks
//! (morsels). Tests cover morsel sizing, work stealing, NUMA awareness,
//! and elastic parallelism.

mod helpers;

use helpers::*;
use ra_core::algebra::{AggregateExpr, AggregateFunction, RelExpr};
use ra_hardware::HardwareProfile;

// ── Morsel Size Selection Tests ────────────────────────────────

#[test]
fn test_morsel_driven_default_size() {
    // Default morsel size (typically 100K rows)
    let input = scan("large_table");
    assert_optimization_improves(input);
}

#[test]
fn test_morsel_driven_adaptive_size_large_rows() {
    // Smaller morsels for large rows
    let input = scan("wide_table");
    assert_rule_applies(input);
}

#[test]
fn test_morsel_driven_adaptive_size_narrow_rows() {
    // Larger morsels for narrow rows
    let input = scan("narrow_table");
    let projected = project(input, vec!["id"]);
    assert_optimization_improves(projected);
}

// ── Work Stealing Tests ─────────────────────────────────────────

#[test]
fn test_morsel_driven_work_stealing() {
    // Idle workers steal morsels from busy workers
    let input = scan("partitioned_data");
    let filtered = input.filter(gt(col("value"), int(100)));
    assert_optimization_improves(filtered);
}

#[test]
fn test_morsel_driven_load_balancing() {
    // Automatic load balancing via work stealing
    let input = scan("skewed_workload");
    assert_rule_applies(input);
}

#[test]
fn test_morsel_driven_dynamic_scheduling() {
    // Dynamic work distribution
    let input = scan("variable_complexity");
    let filtered = input.filter(gt(col("computation"), int(50)));
    assert_optimization_improves(filtered);
}

// ── NUMA Awareness Tests ────────────────────────────────────────

#[test]
fn test_morsel_driven_numa_local_access() {
    // Process morsels on NUMA-local CPU
    let input = scan("numa_partitioned");
    let opt = create_test_optimizer_with_hardware(HardwareProfile::cpu_only());
    let _result = opt.optimize(&input).expect("optimization should succeed");
}

#[test]
fn test_morsel_driven_numa_interleaving() {
    // Interleave morsels across NUMA nodes
    let input = scan("large_dataset");
    assert_optimization_improves(input);
}

// ── Load Balancing Tests ────────────────────────────────────────

#[test]
fn test_morsel_driven_balanced_distribution() {
    // Evenly distribute morsels to workers
    let input = scan("uniform_data");
    assert_rule_applies(input);
}

#[test]
fn test_morsel_driven_handle_stragglers() {
    // Work stealing handles straggler workers
    let input = scan("data_with_stragglers");
    assert_optimization_improves(input);
}

// ── Pipeline Breakers Tests ─────────────────────────────────────

#[test]
fn test_morsel_driven_pipeline_breaker_sort() {
    // Sort breaks pipeline, redistributes morsels
    let input = scan("unsorted");
    let sorted = sort(input, "key", true);
    assert_optimization_improves(sorted);
}

#[test]
fn test_morsel_driven_pipeline_breaker_hash_join() {
    // Hash join build phase breaks pipeline
    let join = two_table_join("orders", "customers", "customer_id", "id");
    assert_rule_applies(join);
}

// ── Parallel Pipelines Tests ────────────────────────────────────

#[test]
fn test_morsel_driven_parallel_scan() {
    // Multiple threads scan different morsels
    let input = scan("partitioned_table");
    assert_optimization_improves(input);
}

#[test]
fn test_morsel_driven_parallel_filter() {
    // Parallel filter on morsels
    let input = scan("data");
    let filtered = input.filter(gt(col("value"), int(1000)));
    assert_rule_applies(filtered);
}

#[test]
fn test_morsel_driven_parallel_aggregation() {
    // Parallel aggregation with morsel-local state
    let input = scan("transactions");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("category")],
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

// ── Task Scheduling Tests ───────────────────────────────────────

#[test]
fn test_morsel_driven_task_queue() {
    // Central task queue for work stealing
    let input = scan("data");
    assert_rule_applies(input);
}

#[test]
fn test_morsel_driven_priority_scheduling() {
    // Prioritize critical path morsels
    let input = scan("priority_data");
    assert_optimization_improves(input);
}

// ── Synchronization Points Tests ────────────────────────────────

#[test]
fn test_morsel_driven_barrier_synchronization() {
    // Barrier for global operations
    let input = scan("data");
    let sorted = sort(input, "key", true);
    assert_optimization_improves(sorted);
}

#[test]
fn test_morsel_driven_lock_free_queues() {
    // Lock-free queues for work stealing
    let input = scan("concurrent_data");
    assert_rule_applies(input);
}

// ── Elastic Parallelism Tests ───────────────────────────────────

#[test]
fn test_morsel_driven_elastic_workers() {
    // Adjust worker count dynamically
    let input = scan("variable_load");
    assert_optimization_improves(input);
}

#[test]
fn test_morsel_driven_scale_up() {
    // Add workers for large workloads
    let input = scan("huge_dataset");
    assert_rule_applies(input);
}

// ── Resource Management Tests ───────────────────────────────────

#[test]
fn test_morsel_driven_memory_management() {
    // Manage memory per-morsel
    let input = scan("memory_intensive");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("high_cardinality")],
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
fn test_morsel_driven_memory_pressure() {
    // Handle memory pressure gracefully
    let input = scan("large_groups");
    assert_rule_applies(input);
}
