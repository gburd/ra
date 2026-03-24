//! Tests for progressive re-optimization and plan stitching (RFC 0052).

use std::collections::HashMap;
use std::time::Instant;

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering, RelExpr, SortDirection, SortKey,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::statistics::Statistics;
use ra_engine::plan_stitch::{self, OperatorState};
use ra_engine::progressive_reopt::{
    self, JoinImplKind, ReoptConfig, ReoptError, ReoptimizeFn,
    StitchPointKind, StitchTransferKind,
};

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

fn join(left: RelExpr, right: RelExpr, cond: Expr) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: cond,
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

fn sort(input: RelExpr) -> RelExpr {
    RelExpr::Sort {
        keys: vec![SortKey {
            expr: col("id"),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
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
// Divergence detection tests
// ---------------------------------------------------------------

#[test]
fn test_no_divergence_when_equal() {
    assert!(!progressive_reopt::should_reoptimize(1000, 1000, 2.0));
}

#[test]
fn test_divergence_when_underestimated() {
    // Estimated 100, actual 500 => ratio 5.0 > threshold 2.0
    assert!(progressive_reopt::should_reoptimize(100, 500, 2.0));
}

#[test]
fn test_divergence_when_overestimated() {
    // Estimated 1000, actual 100 => ratio 0.1 < 1/2.0 = 0.5
    assert!(progressive_reopt::should_reoptimize(1000, 100, 2.0));
}

#[test]
fn test_no_divergence_within_threshold() {
    // Estimated 100, actual 150 => ratio 1.5, within 2.0 threshold
    assert!(!progressive_reopt::should_reoptimize(100, 150, 2.0));
}

#[test]
fn test_zero_estimated_zero_actual() {
    assert!(!progressive_reopt::should_reoptimize(0, 0, 2.0));
}

#[test]
fn test_zero_estimated_nonzero_actual() {
    assert!(progressive_reopt::should_reoptimize(0, 100, 2.0));
}

#[test]
fn test_divergence_factor_normal() {
    let factor = progressive_reopt::divergence_factor(100, 500);
    assert!((factor - 5.0).abs() < f64::EPSILON);
}

#[test]
fn test_divergence_factor_zero_estimated() {
    let factor = progressive_reopt::divergence_factor(0, 100);
    assert_eq!(factor, f64::MAX);
}

#[test]
fn test_divergence_factor_both_zero() {
    let factor = progressive_reopt::divergence_factor(0, 0);
    assert!((factor - 1.0).abs() < f64::EPSILON);
}

// ---------------------------------------------------------------
// Stitch cost estimation tests
// ---------------------------------------------------------------

#[test]
fn test_stitch_cost_copy() {
    let cost = progressive_reopt::estimate_stitch_cost(1000, StitchTransferKind::Copy);
    assert!(cost > 0.0);
    assert!((cost - 10.0).abs() < f64::EPSILON);
}

#[test]
fn test_stitch_cost_hash_build() {
    let cost = progressive_reopt::estimate_stitch_cost(1000, StitchTransferKind::HashBuild);
    assert!(cost > 0.0);
    assert!((cost - 50.0).abs() < f64::EPSILON);
}

#[test]
fn test_stitch_cost_sort() {
    let cost = progressive_reopt::estimate_stitch_cost(1000, StitchTransferKind::Sort);
    assert!(cost > 0.0);
    assert!((cost - 100.0).abs() < f64::EPSILON);
}

#[test]
fn test_stitch_cost_discard() {
    let cost = progressive_reopt::estimate_stitch_cost(1000, StitchTransferKind::Discard);
    assert!((cost - 0.0).abs() < f64::EPSILON);
}

// ---------------------------------------------------------------
// Join transfer kind tests
// ---------------------------------------------------------------

#[test]
fn test_hash_to_merge_needs_sort() {
    assert_eq!(
        progressive_reopt::join_transfer_kind(JoinImplKind::Hash, JoinImplKind::Merge,),
        StitchTransferKind::Sort,
    );
}

#[test]
fn test_nested_loop_to_hash_needs_hash_build() {
    assert_eq!(
        progressive_reopt::join_transfer_kind(JoinImplKind::NestedLoop, JoinImplKind::Hash,),
        StitchTransferKind::HashBuild,
    );
}

#[test]
fn test_hash_to_nested_loop_discards() {
    assert_eq!(
        progressive_reopt::join_transfer_kind(JoinImplKind::Hash, JoinImplKind::NestedLoop,),
        StitchTransferKind::Discard,
    );
}

#[test]
fn test_same_impl_is_copy() {
    assert_eq!(
        progressive_reopt::join_transfer_kind(JoinImplKind::Hash, JoinImplKind::Hash,),
        StitchTransferKind::Copy,
    );
}

// ---------------------------------------------------------------
// Switch decision tests
// ---------------------------------------------------------------

#[test]
fn test_switch_worthwhile_with_large_savings() {
    // Current remaining: 1000, alternative: 500, overhead: 50
    // Total alt: 550 < 1000 * 0.8 = 800 => switch
    assert!(progressive_reopt::is_switch_worthwhile(
        1000.0, 500.0, 50.0, 0.8,
    ));
}

#[test]
fn test_switch_not_worthwhile_small_savings() {
    // Current remaining: 1000, alternative: 900, overhead: 50
    // Total alt: 950 > 1000 * 0.8 = 800 => don't switch
    assert!(!progressive_reopt::is_switch_worthwhile(
        1000.0, 900.0, 50.0, 0.8,
    ));
}

#[test]
fn test_switch_not_worthwhile_high_overhead() {
    // Current remaining: 1000, alternative: 100, overhead: 900
    // Total alt: 1000 > 1000 * 0.8 = 800 => overhead kills it
    assert!(!progressive_reopt::is_switch_worthwhile(
        1000.0, 100.0, 900.0, 0.8,
    ));
}

// ---------------------------------------------------------------
// Remaining cost estimation tests
// ---------------------------------------------------------------

#[test]
fn test_remaining_cost_at_start() {
    let remaining = progressive_reopt::estimate_remaining_cost(1000.0, 0.0);
    assert!((remaining - 1000.0).abs() < f64::EPSILON);
}

#[test]
fn test_remaining_cost_halfway() {
    let remaining = progressive_reopt::estimate_remaining_cost(1000.0, 0.5);
    assert!((remaining - 500.0).abs() < f64::EPSILON);
}

#[test]
fn test_remaining_cost_complete() {
    let remaining = progressive_reopt::estimate_remaining_cost(1000.0, 1.0);
    assert!(remaining.abs() < f64::EPSILON);
}

#[test]
fn test_remaining_cost_clamps_above_one() {
    let remaining = progressive_reopt::estimate_remaining_cost(1000.0, 1.5);
    assert!(remaining.abs() < f64::EPSILON);
}

// ---------------------------------------------------------------
// Stitch point insertion tests
// ---------------------------------------------------------------

#[test]
fn test_insert_stitch_points_single_join() {
    let plan = join(
        scan("orders"),
        scan("customers"),
        eq(col("orders.cid"), col("customers.id")),
    );

    let (_annotated, metas) = progressive_reopt::insert_stitch_points(&plan);

    assert_eq!(metas.len(), 1);
    assert_eq!(metas[0].kind, StitchPointKind::JoinBuildComplete,);
}

#[test]
fn test_insert_stitch_points_nested_joins() {
    let inner_join = join(scan("a"), scan("b"), eq(col("a.id"), col("b.aid")));
    let outer_join = join(inner_join, scan("c"), eq(col("b.id"), col("c.bid")));

    let (_annotated, metas) = progressive_reopt::insert_stitch_points(&outer_join);

    // Two joins => two stitch points
    assert_eq!(metas.len(), 2);
    assert!(metas
        .iter()
        .all(|m| m.kind == StitchPointKind::JoinBuildComplete));
}

#[test]
fn test_insert_stitch_points_join_and_aggregate() {
    let plan = aggregate(join(
        scan("orders"),
        scan("customers"),
        eq(col("orders.cid"), col("customers.id")),
    ));

    let (_annotated, metas) = progressive_reopt::insert_stitch_points(&plan);

    // One join + one aggregate = 2 stitch points
    assert_eq!(metas.len(), 2);
    assert!(metas
        .iter()
        .any(|m| m.kind == StitchPointKind::JoinBuildComplete));
    assert!(metas
        .iter()
        .any(|m| m.kind == StitchPointKind::AggregateInput));
}

#[test]
fn test_insert_stitch_points_sort() {
    let plan = sort(scan("orders"));

    let (_annotated, metas) = progressive_reopt::insert_stitch_points(&plan);

    assert_eq!(metas.len(), 1);
    assert_eq!(metas[0].kind, StitchPointKind::SortInput);
}

#[test]
fn test_insert_stitch_points_through_filter_project() {
    let plan = project(filter(
        join(scan("a"), scan("b"), eq(col("a.id"), col("b.aid"))),
        eq(col("a.x"), Expr::Const(Const::Int(42))),
    ));

    let (_annotated, metas) = progressive_reopt::insert_stitch_points(&plan);

    // Filter and project are transparent; the join stitch point
    // should still be found.
    assert_eq!(metas.len(), 1);
    assert_eq!(metas[0].kind, StitchPointKind::JoinBuildComplete,);
}

#[test]
fn test_insert_stitch_points_leaf_scan() {
    let plan = scan("orders");

    let (_annotated, metas) = progressive_reopt::insert_stitch_points(&plan);

    assert!(metas.is_empty());
}

// ---------------------------------------------------------------
// Full re-optimization decision tests
// ---------------------------------------------------------------

#[test]
fn test_evaluate_reopt_no_divergence() {
    let config = ReoptConfig::default();
    let decision = progressive_reopt::evaluate_reopt_decision(
        1000,  // estimated
        1500,  // actual (1.5x, within 2.0 threshold)
        500.0, // remaining cost
        200.0, // alt cost
        10.0,  // overhead
        &config,
    );
    assert!(!decision.should_switch);
}

#[test]
fn test_evaluate_reopt_divergence_with_savings() {
    let config = ReoptConfig::default();
    let decision = progressive_reopt::evaluate_reopt_decision(
        100,    // estimated
        1000,   // actual (10x, above 2.0 threshold)
        1000.0, // remaining cost
        200.0,  // alt cost
        50.0,   // overhead
        &config,
    );
    assert!(decision.should_switch);
    assert!(decision.divergence_factor > 2.0);
    assert!(decision.savings_fraction > 0.5);
}

#[test]
fn test_evaluate_reopt_divergence_without_savings() {
    let config = ReoptConfig::default();
    let decision = progressive_reopt::evaluate_reopt_decision(
        100,    // estimated
        1000,   // actual (10x divergence)
        1000.0, // remaining cost
        900.0,  // alt cost (barely better)
        200.0,  // high overhead
        &config,
    );
    // Divergence triggers evaluation, but cost doesn't justify
    assert!(!decision.should_switch);
}

#[test]
fn test_evaluate_reopt_custom_config() {
    let config = ReoptConfig {
        divergence_threshold: 5.0,
        switch_threshold: 0.5,
        max_reoptimizations: 1,
    };
    // 3x divergence is below 5.0 threshold
    let decision =
        progressive_reopt::evaluate_reopt_decision(100, 300, 1000.0, 200.0, 10.0, &config);
    assert!(!decision.should_switch);
}

// ---------------------------------------------------------------
// Plan stitching tests
// ---------------------------------------------------------------

#[test]
fn test_stitch_at_join() {
    let materialized = scan("materialized_orders");
    let reoptimized = join(
        scan("placeholder"),
        scan("customers"),
        eq(col("cid"), col("id")),
    );

    let result = plan_stitch::stitch_plans(
        &materialized,
        &reoptimized,
        StitchPointKind::JoinBuildComplete,
        &OperatorState::Join {
            build_side_complete: true,
            build_side_rows: 5000,
            probe_side_cursor: 0,
        },
    );

    // The stitched plan should replace the left child with
    // materialized input.
    assert!(result.stitch_points_applied == 1);
    assert!(result.stitch_overhead > 0.0);

    if let RelExpr::Join { left, right, .. } = &result.plan {
        if let RelExpr::Scan { table, .. } = left.as_ref() {
            assert_eq!(table, "materialized_orders");
        } else {
            panic!("Expected Scan as left child of stitched join");
        }
        if let RelExpr::Scan { table, .. } = right.as_ref() {
            assert_eq!(table, "customers");
        } else {
            panic!("Expected Scan as right child of stitched join");
        }
    } else {
        panic!("Expected Join at top of stitched plan");
    }
}

#[test]
fn test_stitch_at_aggregate() {
    let materialized = scan("partial_results");
    let reoptimized = aggregate(scan("placeholder"));

    let result = plan_stitch::stitch_plans(
        &materialized,
        &reoptimized,
        StitchPointKind::AggregateInput,
        &OperatorState::Aggregate {
            partial_group_count: 50,
            input_rows_consumed: 10000,
        },
    );

    assert!(result.stitch_points_applied == 1);
    if let RelExpr::Aggregate { input, .. } = &result.plan {
        if let RelExpr::Scan { table, .. } = input.as_ref() {
            assert_eq!(table, "partial_results");
        } else {
            panic!("Expected materialized input under aggregate");
        }
    } else {
        panic!("Expected Aggregate at top of stitched plan");
    }
}

#[test]
fn test_stitch_overhead_scales_with_rows() {
    let small_state = OperatorState::Join {
        build_side_complete: true,
        build_side_rows: 100,
        probe_side_cursor: 0,
    };
    let large_state = OperatorState::Join {
        build_side_complete: true,
        build_side_rows: 100_000,
        probe_side_cursor: 0,
    };

    let small_result = plan_stitch::stitch_plans(
        &scan("m"),
        &join(scan("p"), scan("r"), eq(col("a"), col("b"))),
        StitchPointKind::JoinBuildComplete,
        &small_state,
    );

    let large_result = plan_stitch::stitch_plans(
        &scan("m"),
        &join(scan("p"), scan("r"), eq(col("a"), col("b"))),
        StitchPointKind::JoinBuildComplete,
        &large_state,
    );

    assert!(large_result.stitch_overhead > small_result.stitch_overhead);
}

// ---------------------------------------------------------------
// Stitch point counting tests
// ---------------------------------------------------------------

#[test]
fn test_count_stitch_points_simple_scan() {
    assert_eq!(plan_stitch::count_stitch_points(&scan("t")), 0);
}

#[test]
fn test_count_stitch_points_single_join() {
    let plan = join(scan("a"), scan("b"), eq(col("a.id"), col("b.id")));
    assert_eq!(plan_stitch::count_stitch_points(&plan), 1);
}

#[test]
fn test_count_stitch_points_multi_join() {
    let plan = join(
        join(scan("a"), scan("b"), eq(col("a.id"), col("b.id"))),
        scan("c"),
        eq(col("b.id"), col("c.id")),
    );
    assert_eq!(plan_stitch::count_stitch_points(&plan), 2);
}

#[test]
fn test_count_stitch_points_with_aggregate() {
    let plan = aggregate(join(scan("a"), scan("b"), eq(col("a.id"), col("b.id"))));
    // 1 join + 1 aggregate = 2
    assert_eq!(plan_stitch::count_stitch_points(&plan), 2);
}

#[test]
fn test_count_stitch_points_with_sort() {
    let plan = sort(scan("a"));
    assert_eq!(plan_stitch::count_stitch_points(&plan), 1);
}

#[test]
fn test_count_stitch_points_filter_transparent() {
    let plan = filter(
        join(scan("a"), scan("b"), eq(col("a.id"), col("b.id"))),
        eq(col("x"), Expr::Const(Const::Int(1))),
    );
    // Filter doesn't add stitch points, only the join does
    assert_eq!(plan_stitch::count_stitch_points(&plan), 1);
}

// ---------------------------------------------------------------
// Find deepest join tests
// ---------------------------------------------------------------

#[test]
fn test_find_deepest_join_single() {
    let plan = join(scan("a"), scan("b"), eq(col("a.id"), col("b.id")));
    let deepest = plan_stitch::find_deepest_join(&plan);
    assert!(deepest.is_some());
}

#[test]
fn test_find_deepest_join_nested() {
    let inner = join(scan("a"), scan("b"), eq(col("a.id"), col("b.id")));
    let outer = join(inner, scan("c"), eq(col("b.id"), col("c.id")));
    let deepest = plan_stitch::find_deepest_join(&outer);
    assert!(deepest.is_some());
    // The deepest join should be the inner a-b join
    if let Some(RelExpr::Join { left, right, .. }) = deepest {
        if let RelExpr::Scan { table, .. } = left.as_ref() {
            assert_eq!(table, "a");
        }
        if let RelExpr::Scan { table, .. } = right.as_ref() {
            assert_eq!(table, "b");
        }
    }
}

#[test]
fn test_find_deepest_join_none_for_scan() {
    assert!(plan_stitch::find_deepest_join(&scan("t")).is_none());
}

#[test]
fn test_find_deepest_join_through_filter() {
    let plan = filter(
        join(scan("a"), scan("b"), eq(col("a.id"), col("b.id"))),
        eq(col("x"), Expr::Const(Const::Int(1))),
    );
    let deepest = plan_stitch::find_deepest_join(&plan);
    assert!(deepest.is_some());
}

// ---------------------------------------------------------------
// OperatorState row count tests
// ---------------------------------------------------------------

#[test]
fn test_operator_state_row_count_scan() {
    let state = OperatorState::Scan {
        cursor_position: 5000,
        buffered_row_count: 3000,
    };
    assert_eq!(state.row_count(), 3000);
}

#[test]
fn test_operator_state_row_count_join() {
    let state = OperatorState::Join {
        build_side_complete: true,
        build_side_rows: 7500,
        probe_side_cursor: 100,
    };
    assert_eq!(state.row_count(), 7500);
}

#[test]
fn test_operator_state_row_count_aggregate() {
    let state = OperatorState::Aggregate {
        partial_group_count: 50,
        input_rows_consumed: 10000,
    };
    assert_eq!(state.row_count(), 10000);
}

#[test]
fn test_operator_state_row_count_sort() {
    let state = OperatorState::Sort {
        sorted_run_count: 3,
        total_sorted_rows: 25000,
    };
    assert_eq!(state.row_count(), 25000);
}

// ---------------------------------------------------------------
// End-to-end scenario tests
// ---------------------------------------------------------------

#[test]
fn test_scenario_hash_join_cardinality_miss() {
    // Scenario: optimizer estimated 100 premium customers,
    // but there are actually 1M. We should re-optimize.

    let estimated = 100_u64;
    let actual = 1_000_000_u64;
    let config = ReoptConfig::default();

    // Step 1: detect divergence
    assert!(progressive_reopt::should_reoptimize(
        estimated,
        actual,
        config.divergence_threshold,
    ));

    // Step 2: evaluate costs
    let remaining_hash_cost = 50_000.0; // Hash join remaining cost
    let nl_join_cost = 1_000.0; // NL join with 10 orders
    let stitch_overhead = 500.0; // Transfer state

    let decision = progressive_reopt::evaluate_reopt_decision(
        estimated,
        actual,
        remaining_hash_cost,
        nl_join_cost,
        stitch_overhead,
        &config,
    );

    assert!(decision.should_switch);
    assert!(decision.savings_fraction > 0.9);

    // Step 3: stitch plans
    let materialized = scan("orders_partial");
    let new_plan = join(
        scan("orders_partial"),
        scan("customers"),
        eq(col("cid"), col("id")),
    );

    let result = plan_stitch::stitch_plans(
        &materialized,
        &new_plan,
        StitchPointKind::JoinBuildComplete,
        &OperatorState::Join {
            build_side_complete: true,
            build_side_rows: actual,
            probe_side_cursor: 0,
        },
    );

    assert_eq!(result.stitch_points_applied, 1);
    assert!(result.stitch_overhead > 0.0);
}

#[test]
fn test_scenario_aggregate_group_explosion() {
    // Scenario: expected 10 groups, got 100K groups. The
    // hash aggregate should switch to sort aggregate.

    let estimated = 10_u64;
    let actual = 100_000_u64;
    let config = ReoptConfig::default();

    assert!(progressive_reopt::should_reoptimize(
        estimated,
        actual,
        config.divergence_threshold,
    ));

    let decision = progressive_reopt::evaluate_reopt_decision(
        estimated, actual, 5000.0, // hash agg remaining cost (high memory)
        2000.0, // sort agg cost
        300.0,  // overhead
        &config,
    );

    assert!(decision.should_switch);
}

#[test]
fn test_scenario_no_reopt_within_tolerance() {
    // Estimated 1000, actual 1500 => 1.5x, within default 2.0
    let config = ReoptConfig::default();
    let decision =
        progressive_reopt::evaluate_reopt_decision(1000, 1500, 500.0, 200.0, 10.0, &config);
    assert!(!decision.should_switch);
}

#[test]
fn test_scenario_multiple_stitch_points_in_complex_plan() {
    // 3-way join with aggregate and sort
    let plan = sort(aggregate(join(
        join(
            scan("orders"),
            scan("customers"),
            eq(col("orders.cid"), col("customers.id")),
        ),
        scan("products"),
        eq(col("orders.pid"), col("products.id")),
    )));

    let (_, metas) = progressive_reopt::insert_stitch_points(&plan);

    // 2 joins + 1 aggregate + 1 sort = 4 stitch points
    assert_eq!(metas.len(), 4);

    let stitch_count = plan_stitch::count_stitch_points(&plan);
    assert_eq!(stitch_count, 4);
}

// ---------------------------------------------------------------
// Join order equivalence verification tests
// ---------------------------------------------------------------

#[test]
fn test_verify_equivalence_same_tables() {
    let plan_a = join(scan("a"), scan("b"), eq(col("a.id"), col("b.id")));
    let plan_b = join(scan("b"), scan("a"), eq(col("b.id"), col("a.id")));
    assert!(plan_stitch::verify_join_order_equivalence(&plan_a, &plan_b));
}

#[test]
fn test_verify_equivalence_different_tables() {
    let plan_a = join(scan("a"), scan("b"), eq(col("a.id"), col("b.id")));
    let plan_b = join(scan("a"), scan("c"), eq(col("a.id"), col("c.id")));
    assert!(!plan_stitch::verify_join_order_equivalence(&plan_a, &plan_b));
}

#[test]
fn test_verify_equivalence_nested_same_tables() {
    let plan_a = join(
        join(scan("a"), scan("b"), eq(col("a.id"), col("b.id"))),
        scan("c"),
        eq(col("b.id"), col("c.id")),
    );
    let plan_b = join(
        scan("a"),
        join(scan("c"), scan("b"), eq(col("c.id"), col("b.id"))),
        eq(col("a.id"), col("b.id")),
    );
    assert!(plan_stitch::verify_join_order_equivalence(&plan_a, &plan_b));
}

#[test]
fn test_verify_equivalence_extra_table() {
    let plan_a = join(scan("a"), scan("b"), eq(col("a.id"), col("b.id")));
    let plan_b = join(
        join(scan("a"), scan("b"), eq(col("a.id"), col("b.id"))),
        scan("c"),
        eq(col("b.id"), col("c.id")),
    );
    assert!(!plan_stitch::verify_join_order_equivalence(&plan_a, &plan_b));
}

#[test]
fn test_verify_equivalence_through_operators() {
    let plan_a = sort(aggregate(join(
        scan("a"),
        scan("b"),
        eq(col("a.id"), col("b.id")),
    )));
    let plan_b = aggregate(sort(join(
        scan("b"),
        scan("a"),
        eq(col("b.id"), col("a.id")),
    )));
    assert!(plan_stitch::verify_join_order_equivalence(&plan_a, &plan_b));
}

// ---------------------------------------------------------------
// Multi-point stitching tests
// ---------------------------------------------------------------

#[test]
fn test_stitch_multi_empty_points() {
    let plan = join(scan("a"), scan("b"), eq(col("a.id"), col("b.id")));
    let result = plan_stitch::stitch_multi(&plan, &[]);
    assert_eq!(result.stitch_points_applied, 0);
    assert!((result.stitch_overhead - 0.0).abs() < f64::EPSILON);
    assert_eq!(result.plan, plan);
}

#[test]
fn test_stitch_multi_single_point() {
    let reoptimized = join(
        scan("placeholder"),
        scan("customers"),
        eq(col("cid"), col("id")),
    );
    let points = vec![(
        scan("materialized_orders"),
        StitchPointKind::JoinBuildComplete,
        OperatorState::Join {
            build_side_complete: true,
            build_side_rows: 5000,
            probe_side_cursor: 0,
        },
    )];

    let result = plan_stitch::stitch_multi(&reoptimized, &points);
    assert_eq!(result.stitch_points_applied, 1);
    assert!(result.stitch_overhead > 0.0);
}

#[test]
fn test_stitch_multi_accumulates_overhead() {
    let reoptimized = aggregate(join(
        scan("placeholder"),
        scan("customers"),
        eq(col("cid"), col("id")),
    ));

    let points = vec![
        (
            scan("materialized_orders"),
            StitchPointKind::JoinBuildComplete,
            OperatorState::Join {
                build_side_complete: true,
                build_side_rows: 5000,
                probe_side_cursor: 0,
            },
        ),
        (
            scan("partial_agg"),
            StitchPointKind::AggregateInput,
            OperatorState::Aggregate {
                partial_group_count: 100,
                input_rows_consumed: 20000,
            },
        ),
    ];

    let result = plan_stitch::stitch_multi(&reoptimized, &points);
    assert_eq!(result.stitch_points_applied, 2);
    assert!(result.stitch_overhead > 0.0);
}

// ---------------------------------------------------------------
// Sort stitching tests
// ---------------------------------------------------------------

#[test]
fn test_stitch_at_sort() {
    let materialized = scan("partial_sorted");
    let reoptimized = sort(scan("placeholder"));

    let result = plan_stitch::stitch_plans(
        &materialized,
        &reoptimized,
        StitchPointKind::SortInput,
        &OperatorState::Sort {
            sorted_run_count: 3,
            total_sorted_rows: 10000,
        },
    );

    assert_eq!(result.stitch_points_applied, 1);
    assert!(result.stitch_overhead > 0.0);

    if let RelExpr::Sort { input, .. } = &result.plan {
        if let RelExpr::Scan { table, .. } = input.as_ref() {
            assert_eq!(table, "partial_sorted");
        } else {
            panic!("Expected Scan as input to stitched sort");
        }
    } else {
        panic!("Expected Sort at top of stitched plan");
    }
}

// ---------------------------------------------------------------
// Background re-optimization tests
// ---------------------------------------------------------------

/// A test optimizer that returns a "better" plan by wrapping the
/// input in a filter, simulating optimization improvement.
struct TestReoptimizer {
    improved: RelExpr,
}

impl ReoptimizeFn for TestReoptimizer {
    fn reoptimize(
        &self,
        _plan: &RelExpr,
        _stats: &HashMap<String, Statistics>,
    ) -> Result<RelExpr, ReoptError> {
        Ok(self.improved.clone())
    }
}

/// An optimizer that always fails.
struct FailingReoptimizer;

impl ReoptimizeFn for FailingReoptimizer {
    fn reoptimize(
        &self,
        _plan: &RelExpr,
        _stats: &HashMap<String, Statistics>,
    ) -> Result<RelExpr, ReoptError> {
        Err(ReoptError::OptimizerFailed("test failure".to_string()))
    }
}

/// An optimizer that returns the same plan (no improvement).
struct NoopReoptimizer;

impl ReoptimizeFn for NoopReoptimizer {
    fn reoptimize(
        &self,
        plan: &RelExpr,
        _stats: &HashMap<String, Statistics>,
    ) -> Result<RelExpr, ReoptError> {
        Ok(plan.clone())
    }
}

#[test]
fn test_quick_plan_returns_immediately() {
    let plan = join(
        scan("orders"),
        scan("customers"),
        eq(col("orders.cid"), col("customers.id")),
    );

    let improved_plan = filter(
        join(
            scan("orders"),
            scan("customers"),
            eq(col("orders.cid"), col("customers.id")),
        ),
        eq(col("orders.status"), Expr::Const(Const::Int(1))),
    );

    let start = Instant::now();
    let (quick, _handle) = progressive_reopt::progressive_optimize(
        plan.clone(),
        HashMap::new(),
        Box::new(TestReoptimizer {
            improved: improved_plan,
        }),
        ReoptConfig::default(),
    );
    let elapsed = start.elapsed();

    // Quick plan should return in <10ms (no optimization work).
    assert!(
        elapsed.as_millis() < 10,
        "Quick plan took {}ms, expected <10ms",
        elapsed.as_millis()
    );
    assert_eq!(quick, plan);
}

#[test]
fn test_background_improvement_completes() {
    let plan = join(
        scan("orders"),
        scan("customers"),
        eq(col("orders.cid"), col("customers.id")),
    );

    let improved_plan = filter(
        join(
            scan("orders"),
            scan("customers"),
            eq(col("orders.cid"), col("customers.id")),
        ),
        eq(col("orders.status"), Expr::Const(Const::Int(1))),
    );

    let (_quick, handle) = progressive_reopt::progressive_optimize(
        plan,
        HashMap::new(),
        Box::new(TestReoptimizer {
            improved: improved_plan.clone(),
        }),
        ReoptConfig::default(),
    );

    // Wait for the background thread to complete.
    let result = handle.recv();
    assert!(result.is_some());

    let result = result.expect("background thread should send result");
    assert!(result.completed);
    assert!(result.improved_plan.is_some());
    assert_eq!(result.improved_plan.as_ref(), Some(&improved_plan));
    assert!(result.attempts >= 1);
}

#[test]
fn test_stitched_plan_preserves_table_equivalence() {
    let original = join(
        scan("orders"),
        scan("customers"),
        eq(col("orders.cid"), col("customers.id")),
    );

    let reoptimized = join(
        scan("customers"),
        scan("orders"),
        eq(col("customers.id"), col("orders.cid")),
    );

    // Plans should reference the same tables.
    assert!(plan_stitch::verify_join_order_equivalence(
        &original,
        &reoptimized,
    ));

    // Stitched plan should also reference the same tables.
    let materialized = scan("materialized_orders");
    let result = plan_stitch::stitch_plans(
        &materialized,
        &reoptimized,
        StitchPointKind::JoinBuildComplete,
        &OperatorState::Join {
            build_side_complete: true,
            build_side_rows: 1000,
            probe_side_cursor: 0,
        },
    );

    // The stitched plan swaps left child to materialized_orders,
    // but the right child (orders) is preserved.
    assert_eq!(result.stitch_points_applied, 1);
}

#[test]
fn test_background_no_improvement_returns_none() {
    let plan = scan("simple_table");

    let (_quick, handle) = progressive_reopt::progressive_optimize(
        plan,
        HashMap::new(),
        Box::new(NoopReoptimizer),
        ReoptConfig::default(),
    );

    let result = handle.recv();
    assert!(result.is_some());

    let result = result.expect("background thread should send result");
    assert!(result.completed);
    assert!(result.improved_plan.is_none());
}

#[test]
fn test_background_optimizer_failure() {
    let plan = scan("simple_table");

    let (_quick, handle) = progressive_reopt::progressive_optimize(
        plan,
        HashMap::new(),
        Box::new(FailingReoptimizer),
        ReoptConfig::default(),
    );

    let result = handle.recv();
    assert!(result.is_some());

    let result = result.expect("background thread should send result");
    assert!(result.completed);
    assert!(result.improved_plan.is_none());
    assert_eq!(result.attempts, 1);
}

#[test]
fn test_background_cancel() {
    let plan = scan("simple_table");

    /// An optimizer that sleeps to simulate slow work.
    struct SlowReoptimizer;

    impl ReoptimizeFn for SlowReoptimizer {
        fn reoptimize(
            &self,
            plan: &RelExpr,
            _stats: &HashMap<String, Statistics>,
        ) -> Result<RelExpr, ReoptError> {
            std::thread::sleep(std::time::Duration::from_millis(100));
            Ok(plan.clone())
        }
    }

    let config = ReoptConfig {
        max_reoptimizations: 100,
        ..ReoptConfig::default()
    };

    let (_quick, mut handle) = progressive_reopt::progressive_optimize(
        plan,
        HashMap::new(),
        Box::new(SlowReoptimizer),
        config,
    );

    // Let it start, then cancel.
    std::thread::sleep(std::time::Duration::from_millis(50));
    handle.cancel_and_join();
    assert!(handle.is_finished());
}

#[test]
fn test_background_respects_max_reoptimizations() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let call_count = Arc::new(AtomicUsize::new(0));

    struct CountingReoptimizer {
        count: Arc<AtomicUsize>,
    }

    impl ReoptimizeFn for CountingReoptimizer {
        fn reoptimize(
            &self,
            _plan: &RelExpr,
            _stats: &HashMap<String, Statistics>,
        ) -> Result<RelExpr, ReoptError> {
            self.count.fetch_add(1, Ordering::SeqCst);
            // Return a different plan each time to keep iterating.
            let n = self.count.load(Ordering::SeqCst);
            Ok(RelExpr::Scan {
                table: format!("table_{n}"),
                alias: None,
            })
        }
    }

    let config = ReoptConfig {
        max_reoptimizations: 2,
        ..ReoptConfig::default()
    };

    let (_quick, handle) = progressive_reopt::progressive_optimize(
        scan("original"),
        HashMap::new(),
        Box::new(CountingReoptimizer {
            count: Arc::clone(&call_count),
        }),
        config,
    );

    let result = handle.recv();
    assert!(result.is_some());

    let result = result.expect("should receive result");
    assert!(result.completed);
    assert!(result.improved_plan.is_some());
    assert_eq!(result.attempts, 2);
    assert_eq!(call_count.load(Ordering::SeqCst), 2);
}

#[test]
fn test_progressive_optimize_corrected_stats_passed() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let stats_received = Arc::new(AtomicBool::new(false));

    struct StatsCheckReoptimizer {
        received: Arc<AtomicBool>,
    }

    impl ReoptimizeFn for StatsCheckReoptimizer {
        fn reoptimize(
            &self,
            plan: &RelExpr,
            stats: &HashMap<String, Statistics>,
        ) -> Result<RelExpr, ReoptError> {
            if stats.contains_key("orders") {
                let s = &stats["orders"];
                if (s.row_count - 50000.0).abs() < f64::EPSILON {
                    self.received.store(true, Ordering::SeqCst);
                }
            }
            Ok(plan.clone())
        }
    }

    let mut corrected = HashMap::new();
    corrected.insert("orders".to_string(), Statistics::new(50000.0));

    let (_quick, handle) = progressive_reopt::progressive_optimize(
        scan("orders"),
        corrected,
        Box::new(StatsCheckReoptimizer {
            received: Arc::clone(&stats_received),
        }),
        ReoptConfig::default(),
    );

    let _result = handle.recv();
    assert!(
        stats_received.load(Ordering::SeqCst),
        "corrected stats should have been passed to the optimizer"
    );
}

// ---------------------------------------------------------------
// End-to-end progressive re-optimization integration test
// ---------------------------------------------------------------

#[test]
fn test_end_to_end_progressive_reopt() {
    // Simulate the full progressive re-optimization flow:
    // 1. Quick plan returns immediately
    // 2. Background produces an improved plan
    // 3. Stitch the improved plan with materialized results
    // 4. Verify the stitched plan is table-equivalent

    let original = join(
        scan("orders"),
        scan("customers"),
        eq(col("orders.cid"), col("customers.id")),
    );

    // The "improved" plan reverses the join order (cheaper probe).
    let improved = join(
        scan("customers"),
        scan("orders"),
        eq(col("customers.id"), col("orders.cid")),
    );

    let start = Instant::now();
    let (quick, handle) = progressive_reopt::progressive_optimize(
        original.clone(),
        HashMap::new(),
        Box::new(TestReoptimizer {
            improved: improved.clone(),
        }),
        ReoptConfig::default(),
    );

    // Quick plan is immediate.
    assert!(start.elapsed().as_millis() < 10);
    assert_eq!(quick, original);

    // Background produces the improved plan.
    let bg_result = handle.recv().expect("should get background result");
    assert!(bg_result.completed);
    let bg_plan = bg_result.improved_plan.expect("should have improved plan");
    assert_eq!(bg_plan, improved);

    // Verify table equivalence between original and improved.
    assert!(plan_stitch::verify_join_order_equivalence(&original, &bg_plan));

    // Stitch the improved plan with materialized left child.
    let materialized = scan("materialized_customers");
    let stitched = plan_stitch::stitch_plans(
        &materialized,
        &bg_plan,
        StitchPointKind::JoinBuildComplete,
        &OperatorState::Join {
            build_side_complete: true,
            build_side_rows: 500,
            probe_side_cursor: 0,
        },
    );

    assert_eq!(stitched.stitch_points_applied, 1);
    assert!(stitched.stitch_overhead > 0.0);

    // The stitched plan should be a join with materialized left.
    if let RelExpr::Join { left, .. } = &stitched.plan {
        if let RelExpr::Scan { table, .. } = left.as_ref() {
            assert_eq!(table, "materialized_customers");
        } else {
            panic!("Expected materialized scan as left child");
        }
    } else {
        panic!("Expected Join in stitched plan");
    }
}
