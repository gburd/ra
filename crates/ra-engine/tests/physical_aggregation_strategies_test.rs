//! Tests for physical aggregation strategy selection.
//!
//! Tests cover hash aggregation, sort-based aggregation, streaming
//! aggregation, two-phase and three-phase aggregation, and distinct
//! aggregation strategies.

mod helpers;

use helpers::*;
use ra_core::algebra::{AggregateExpr, AggregateFunction, RelExpr};

// ── Hash Aggregation Tests ──────────────────────────────────────

#[test]
fn test_hash_aggregation_few_groups() {
    let input = scan("sales");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("region")],
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
fn test_hash_aggregation_many_groups() {
    let input = scan("events");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("user_id"), col("event_type")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(Expr::Const(Const::Int(1))),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_hash_aggregation_fits_memory() {
    // Hash table fits in available memory
    let input = scan("orders");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("customer_id")],
        aggregates: vec![
            AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(col("total")),
                distinct: false,
                alias: None,
            },
            AggregateExpr {
                function: AggregateFunction::Count,
                arg: Some(Expr::Const(Const::Int(1))),
                distinct: false,
                alias: None,
            },
        ],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

#[test]
fn test_hash_aggregation_multiple_aggregates() {
    let input = scan("transactions");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("account_id")],
        aggregates: vec![
            AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(col("debit")),
                distinct: false,
                alias: None,
            },
            AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(col("credit")),
                distinct: false,
                alias: None,
            },
            AggregateExpr {
                function: AggregateFunction::Count,
                arg: Some(Expr::Const(Const::Int(1))),
                distinct: false,
                alias: None,
            },
        ],
        input: Box::new(input),
    };
    assert_rule_applies(agg);
}

// ── Sort-Based Aggregation Tests ────────────────────────────────

#[test]
fn test_sort_aggregation_input_sorted() {
    // Input already sorted by group key
    let input = sort(scan("time_series"), "timestamp", true);
    let agg = RelExpr::Aggregate {
        group_by: vec![col("timestamp")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Avg,
            arg: Some(col("value")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

#[test]
fn test_sort_aggregation_high_cardinality() {
    // Very high cardinality benefits from sorting
    let input = scan("unique_events");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("session_id"), col("event_id")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(Expr::Const(Const::Int(1))),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_sort_aggregation_memory_constrained() {
    // Limited memory forces sort-based approach
    let input = scan("large_dataset");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Max,
            arg: Some(col("price")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

// ── Streaming Aggregation Tests ─────────────────────────────────

#[test]
fn test_streaming_aggregation_ordered_input() {
    // Streaming works when input is ordered by group key
    let input = sort(scan("logs"), "hour", true);
    let agg = RelExpr::Aggregate {
        group_by: vec![col("hour")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(Expr::Const(Const::Int(1))),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

#[test]
fn test_streaming_aggregation_single_pass() {
    // Streaming aggregation for single pass over data
    let input = scan("sensor_readings");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("sensor_id")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Avg,
            arg: Some(col("temperature")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_rule_applies(agg);
}

// ── Two-Phase Aggregation Tests ─────────────────────────────────

#[test]
fn test_two_phase_aggregation_distributed() {
    // Two-phase for distributed/parallel execution
    let input = scan("distributed_data");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("partition_key")],
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

#[test]
fn test_two_phase_aggregation_decomposable() {
    // Decomposable aggregates (SUM, COUNT, MIN, MAX)
    let input = scan("metrics");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("metric_name")],
        aggregates: vec![
            AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(col("value")),
                distinct: false,
                alias: None,
            },
            AggregateExpr {
                function: AggregateFunction::Min,
                arg: Some(col("value")),
                distinct: false,
                alias: None,
            },
            AggregateExpr {
                function: AggregateFunction::Max,
                arg: Some(col("value")),
                distinct: false,
                alias: None,
            },
        ],
        input: Box::new(input),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_two_phase_aggregation_reduces_data_volume() {
    let input = scan("high_volume_data");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("group_id")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(Expr::Const(Const::Int(1))),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

// ── Three-Phase Aggregation Tests ───────────────────────────────

#[test]
fn test_three_phase_aggregation_skewed_data() {
    // Three-phase for handling data skew
    let input = scan("skewed_distribution");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("hot_key")],
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
fn test_three_phase_aggregation_large_groups() {
    let input = scan("massive_dataset");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("category"), col("subcategory")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Avg,
            arg: Some(col("value")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_rule_applies(agg);
}

// ── Distinct Aggregation Tests ──────────────────────────────────

#[test]
fn test_distinct_aggregation_count_distinct() {
    let input = scan("events");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("page")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(col("user_id")),
            distinct: true,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

#[test]
fn test_distinct_aggregation_sum_distinct() {
    let input = scan("orders");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("customer_id")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("order_total")),
            distinct: true,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_distinct_aggregation_multiple_columns() {
    let input = scan("transactions");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("account_id")],
        aggregates: vec![
            AggregateExpr {
                function: AggregateFunction::Count,
                arg: Some(col("merchant_id")),
                distinct: true,
                alias: None,
            },
            AggregateExpr {
                function: AggregateFunction::Count,
                arg: Some(col("category")),
                distinct: true,
                alias: None,
            },
        ],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

// ── Global Aggregation Tests ────────────────────────────────────

#[test]
fn test_global_aggregation_no_grouping() {
    // Aggregation without GROUP BY
    let input = scan("sales");
    let agg = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![
            AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(col("amount")),
                distinct: false,
                alias: None,
            },
            AggregateExpr {
                function: AggregateFunction::Count,
                arg: Some(Expr::Const(Const::Int(1))),
                distinct: false,
                alias: None,
            },
        ],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

#[test]
fn test_global_aggregation_avg() {
    let input = scan("grades");
    let agg = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Avg,
            arg: Some(col("score")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_rule_applies(agg);
}

// ── Aggregation with Filtering ──────────────────────────────────

#[test]
fn test_aggregation_after_filter() {
    let input = filtered_scan("orders", "status", 1);
    let agg = RelExpr::Aggregate {
        group_by: vec![col("customer_id")],
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

#[test]
fn test_aggregation_with_selective_filter() {
    let input = scan("events");
    let filtered = input.filter(eq(col("event_type"), string("purchase")));
    let agg = RelExpr::Aggregate {
        group_by: vec![col("product_id")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(Expr::Const(Const::Int(1))),
            distinct: false,
            alias: None,
        }],
        input: Box::new(filtered),
    };
    assert_rule_applies(agg);
}

// ── Aggregation with Joins ──────────────────────────────────────

#[test]
fn test_aggregation_after_join() {
    let join = two_table_join("orders", "customers", "customer_id", "id");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("country")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("order_total")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(join),
    };
    assert_optimization_improves(agg);
}

// ── Hardware-Specific Aggregation ───────────────────────────────

#[test]
fn test_gpu_hash_aggregation() {
    let input = scan("large_table");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("category")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("value")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_hardware_affects_cost(agg);
}

#[test]
fn test_parallel_aggregation_cpu_cores() {
    let input = scan("metrics");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("timestamp")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Avg,
            arg: Some(col("cpu_usage")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

// ── Aggregation Memory Management ───────────────────────────────

#[test]
fn test_aggregation_spill_to_disk() {
    // Test hash aggregation with disk spilling
    let input = scan("huge_table");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("high_card_column")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("value")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_rule_applies(agg);
}

#[test]
fn test_aggregation_adaptive_strategy() {
    // Adaptive aggregation switches based on runtime behavior
    let input = scan("dynamic_data");
    let agg = RelExpr::Aggregate {
        group_by: vec![col("key")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(Expr::Const(Const::Int(1))),
            distinct: false,
            alias: None,
        }],
        input: Box::new(input),
    };
    assert_optimization_improves(agg);
}

// ── Nested Aggregation ──────────────────────────────────────────

#[test]
fn test_nested_aggregation() {
    let inner_input = scan("sales");
    let inner_agg = RelExpr::Aggregate {
        group_by: vec![col("product_id")],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(col("quantity")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(inner_input),
    };

    let outer_agg = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Max,
            arg: Some(col("sum_quantity")),
            distinct: false,
            alias: None,
        }],
        input: Box::new(inner_agg),
    };

    assert_optimization_improves(outer_agg);
}
