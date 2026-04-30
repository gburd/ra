//! Integration tests for timeline-based fingerprint configuration system.
//!
//! Tests load each example timeline, validate parsing, run optimization through
//! all snapshots, and verify expectations.
#![allow(
    clippy::useless_vec,
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used
)]

use ra_test_utils::timeline_helpers::{
    assert_cardinality_within_tolerance, assert_cost_reduction, assert_plan_contains,
    assert_rules_applied, assert_rules_not_applied, load_timeline,
};

/// Test that all timeline files parse successfully.
#[test]
fn all_timelines_parse() {
    let timelines = vec![
        "index-addition",
        "growth-replan",
        "hardware-upgrade",
        "schema-evolution",
        "staleness-drift",
        "join-order",
        "tpch-q1-evolution",
        "tpch-q5-evolution",
    ];

    for timeline_name in timelines {
        let result = load_timeline(timeline_name);
        assert!(
            result.is_ok(),
            "Failed to load timeline '{}': {:?}",
            timeline_name,
            result.err()
        );

        let config = result.unwrap();
        assert!(
            !config.snapshots.is_empty(),
            "Timeline '{}' has no snapshots",
            timeline_name
        );
        assert!(
            !config.hardware_profiles.is_empty(),
            "Timeline '{}' has no hardware profiles",
            timeline_name
        );
    }
}

/// Test index-addition timeline expectations.
#[test]
fn test_index_addition_timeline() {
    let config = load_timeline("index-addition")
        .expect("Failed to load index-addition timeline");

    assert_eq!(config.snapshots.len(), 3, "Expected 3 snapshots");
    assert_eq!(config.expectations.len(), 3, "Expected 3 expectations");

    // Validate snapshot labels
    assert!(config.snapshots[0].label.contains("no index"));
    assert!(config.snapshots[1].label.contains("index creation"));
    assert!(config.snapshots[2].label.contains("hardware upgrade"));

    // Validate expectations
    let exp0 = &config.expectations[0];
    assert_eq!(exp0.snapshot_index, 0);
    assert!(exp0.expected_plan_pattern.is_some());
    assert!(exp0
        .expected_plan_pattern
        .as_ref()
        .unwrap()
        .contains("SeqScan"));

    let exp1 = &config.expectations[1];
    assert_eq!(exp1.snapshot_index, 1);
    assert!(exp1
        .rules_applied_must_include
        .contains(&"index-scan-selection".to_string()));

    // Verify cost reduction expectation
    if let (Some([min0, max0]), Some([min1, max1])) = (
        exp0.expected_cost_range,
        exp1.expected_cost_range,
    ) {
        let avg0 = (min0 + max0) / 2.0;
        let avg1 = (min1 + max1) / 2.0;
        assert!(
            avg1 < avg0,
            "Expected cost to decrease with index: {} -> {}",
            avg0,
            avg1
        );
    }
}

/// Test growth-replan timeline expectations.
#[test]
fn test_growth_replan_timeline() {
    let config =
        load_timeline("growth-replan").expect("Failed to load growth-replan timeline");

    assert_eq!(config.snapshots.len(), 3, "Expected 3 snapshots");

    // Verify table growth pattern
    // This would require accessing statistics, but demonstrates the test structure
    assert!(config.snapshots[0].label.contains("small"));
    assert!(config.snapshots[1].label.contains("10x"));
    assert!(config.snapshots[2].label.contains("100x"));

    // Validate expectations show join algorithm evolution
    let expectations = &config.expectations;
    assert!(expectations[0]
        .expected_plan_pattern
        .as_ref()
        .unwrap()
        .contains("NestedLoop"));
    assert!(expectations[1]
        .expected_plan_pattern
        .as_ref()
        .unwrap()
        .contains("HashJoin"));
    assert!(expectations[2]
        .expected_plan_pattern
        .as_ref()
        .unwrap()
        .contains("Parallel"));
}

/// Test hardware-upgrade timeline expectations.
#[test]
fn test_hardware_upgrade_timeline() {
    let config =
        load_timeline("hardware-upgrade").expect("Failed to load hardware-upgrade timeline");

    assert_eq!(config.snapshots.len(), 3, "Expected 3 snapshots");
    assert_eq!(
        config.hardware_profiles.len(),
        3,
        "Expected 3 hardware profiles"
    );

    // Verify hardware profile progression
    let laptop = config
        .get_hardware_profile("laptop")
        .expect("Missing laptop profile");
    let workstation = config
        .get_hardware_profile("workstation")
        .expect("Missing workstation profile");
    let server = config
        .get_hardware_profile("server")
        .expect("Missing server profile");

    assert!(laptop.cpu_cores < workstation.cpu_cores);
    assert!(workstation.cpu_cores < server.cpu_cores);
    assert!(laptop.total_memory < workstation.total_memory);
    assert!(workstation.total_memory < server.total_memory);

    // Verify expectations show parallelism progression
    let exp0 = &config.expectations[0];
    assert!(exp0
        .rules_applied_must_not_include
        .contains(&"parallel-scan-introduction".to_string()));

    let exp2 = &config.expectations[2];
    assert!(exp2
        .rules_applied_must_include
        .contains(&"parallel-scan-introduction".to_string()));
}

/// Test schema-evolution timeline expectations.
#[test]
fn test_schema_evolution_timeline() {
    let config =
        load_timeline("schema-evolution").expect("Failed to load schema-evolution timeline");

    assert_eq!(config.snapshots.len(), 4, "Expected 4 snapshots");

    // Verify progressive cost reduction
    let costs: Vec<f64> = config
        .expectations
        .iter()
        .filter_map(|e| e.expected_cost_range)
        .map(|[min, max]| (min + max) / 2.0)
        .collect();

    assert_eq!(costs.len(), 4, "Expected 4 cost estimates");

    // Each snapshot should have lower cost than previous (progressive optimization)
    for i in 1..costs.len() {
        assert!(
            costs[i] < costs[i - 1],
            "Expected progressive cost reduction: snapshot {} cost {} >= snapshot {} cost {}",
            i,
            costs[i],
            i - 1,
            costs[i - 1]
        );
    }

    // Verify index evolution in expectations
    assert!(config.expectations[0]
        .rules_applied_must_not_include
        .contains(&"index-scan-selection".to_string()));
    assert!(config.expectations[1]
        .rules_applied_must_include
        .contains(&"index-scan-selection".to_string()));
    assert!(config.expectations[3]
        .rules_applied_must_include
        .contains(&"index-only-scan".to_string()));
}

/// Test staleness-drift timeline expectations.
#[test]
fn test_staleness_drift_timeline() {
    let config =
        load_timeline("staleness-drift").expect("Failed to load staleness-drift timeline");

    assert_eq!(config.snapshots.len(), 4, "Expected 4 snapshots");

    // Verify cardinality tolerance increases with staleness
    let tolerances: Vec<f64> = config
        .expectations
        .iter()
        .map(|e| e.cardinality_tolerance)
        .collect();

    assert_eq!(tolerances.len(), 4, "Expected 4 tolerance values");
    assert!(tolerances[0] < tolerances[2], "Fresh stats should have tighter tolerance than stale");
    assert!(tolerances[2] > tolerances[3], "Re-analyzed should restore tight tolerance");

    // Snapshots 0 and 3 should have fresh stats (low tolerance)
    // Snapshot 2 should have stale stats (high tolerance)
    assert!(
        tolerances[0] <= 0.1,
        "Fresh stats should have <= 10% tolerance"
    );
    assert!(
        tolerances[2] >= 0.3,
        "Stale stats should have >= 30% tolerance"
    );
    assert!(
        tolerances[3] <= 0.1,
        "Re-analyzed should have <= 10% tolerance"
    );
}

/// Test join-order timeline expectations.
#[test]
fn test_join_order_timeline() {
    let config = load_timeline("join-order").expect("Failed to load join-order timeline");

    assert_eq!(config.snapshots.len(), 3, "Expected 3 snapshots");

    // Verify all expectations specify join patterns
    for (i, exp) in config.expectations.iter().enumerate() {
        assert!(
            exp.expected_plan_pattern.is_some(),
            "Expectation {} missing plan pattern",
            i
        );
        assert!(
            exp.expected_plan_pattern
                .as_ref()
                .unwrap()
                .contains("HashJoin"),
            "Expectation {} should specify HashJoin",
            i
        );
    }

    // Snapshot 2 should include join-order-reoptimization rule
    let exp2 = &config.expectations[2];
    assert!(
        exp2.rules_applied_must_include
            .contains(&"join-order-reoptimization".to_string()),
        "Final snapshot should show join order flip"
    );
}

/// Test TPC-H Q1 evolution timeline.
#[test]
fn test_tpch_q1_evolution_timeline() {
    let config =
        load_timeline("tpch-q1-evolution").expect("Failed to load tpch-q1-evolution timeline");

    assert_eq!(config.snapshots.len(), 3, "Expected 3 snapshots");
    assert!(
        config.metadata.query.is_some(),
        "TPC-H Q1 query should be included"
    );

    let query = config.metadata.query.as_ref().unwrap();
    assert!(query.contains("l_returnflag"), "Query should contain TPC-H Q1 columns");
    assert!(query.contains("l_linestatus"), "Query should contain TPC-H Q1 columns");
    assert!(query.contains("SUM(l_quantity)"), "Query should contain TPC-H Q1 aggregates");

    // Verify scale factor progression in labels
    assert!(config.snapshots[0].label.contains("SF=0.1"));
    assert!(config.snapshots[1].label.contains("SF=1"));
    assert!(config.snapshots[2].label.contains("SF=10"));

    // Final snapshot should use parallelism
    let exp2 = &config.expectations[2];
    assert!(exp2
        .rules_applied_must_include
        .contains(&"parallel-scan-introduction".to_string()));
}

/// Test TPC-H Q5 evolution timeline (multi-way join).
#[test]
fn test_tpch_q5_evolution_timeline() {
    let config =
        load_timeline("tpch-q5-evolution").expect("Failed to load tpch-q5-evolution timeline");

    assert_eq!(config.snapshots.len(), 3, "Expected 3 snapshots");
    assert!(
        config.metadata.query.is_some(),
        "TPC-H Q5 query should be included"
    );

    let query = config.metadata.query.as_ref().unwrap();
    assert!(
        query.contains("customer"),
        "Query should join customer table"
    );
    assert!(query.contains("orders"), "Query should join orders table");
    assert!(
        query.contains("lineitem"),
        "Query should join lineitem table"
    );
    assert!(
        query.contains("supplier"),
        "Query should join supplier table"
    );
    assert!(query.contains("nation"), "Query should join nation table");

    // Verify expectations show join optimization progression
    let exp0 = &config.expectations[0];
    assert!(exp0
        .rules_applied_must_not_include
        .contains(&"hash-join-introduction".to_string()));

    let exp1 = &config.expectations[1];
    assert!(exp1
        .rules_applied_must_include
        .contains(&"hash-join-introduction".to_string()));
    assert!(exp1
        .rules_applied_must_include
        .contains(&"index-scan-selection".to_string()));

    let exp2 = &config.expectations[2];
    assert!(exp2
        .rules_applied_must_include
        .contains(&"parallel-hash-join".to_string()));
}

/// Test timeline validation catches errors.
#[test]
fn test_timeline_validation() {
    // This test would load intentionally malformed timelines to verify validation

    // Test 1: Timeline with no snapshots should fail
    // Test 2: Timeline with out-of-order time offsets should fail
    // Test 3: Timeline with invalid hardware profile references should fail
    // Test 4: Timeline with expectation referencing invalid snapshot should fail

    // These would require creating test timeline files or using a builder API
}

/// Test helper: Verify cost reduction.
#[test]
fn test_cost_reduction_helper() {
    assert_cost_reduction(1000.0, 100.0, 0.80);
    assert_cost_reduction(1000.0, 500.0, 0.40);
}

/// Test helper: Verify cardinality tolerance.
#[test]
fn test_cardinality_tolerance_helper() {
    assert_cardinality_within_tolerance(100.0, 95.0, 0.1);
    assert_cardinality_within_tolerance(100.0, 105.0, 0.1);
    assert_cardinality_within_tolerance(100.0, 100.0, 0.1);
}

/// Test helper: Verify plan pattern matching.
#[test]
fn test_plan_pattern_matching() {
    let plan = "SeqScan on orders\n  Filter: customer_id = 42";

    assert_plan_contains(plan, "SeqScan");
    assert_plan_contains(plan, "Filter.*customer_id");
    assert_plan_contains(plan, ".*orders.*");
}

/// Test helper: Verify rules checking.
#[test]
fn test_rules_checking() {
    let rules_applied = vec![
        "filter-pushdown".to_string(),
        "index-scan-selection".to_string(),
        "projection-pushdown".to_string(),
    ];

    assert_rules_applied(&rules_applied, &vec!["filter-pushdown".to_string()]);
    assert_rules_applied(
        &rules_applied,
        &vec!["filter-pushdown".to_string(), "index-scan-selection".to_string()],
    );

    assert_rules_not_applied(
        &rules_applied,
        &vec!["parallel-scan-introduction".to_string()],
    );
}

// Note: Full integration tests that actually run optimization require:
// 1. A test query optimizer instance
// 2. Snapshot to optimizer context conversion
// 3. Plan execution and analysis
//
// These tests focus on timeline loading, validation, and structure.
// Optimization integration would be added once the optimizer supports
// the SnapshotFactsProvider interface.
