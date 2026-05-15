//! Property tests using dynamic facts and statistics.
//!
//! These tests demonstrate the enhanced fuzzer's ability to find
//! optimization bugs by varying database facts and statistics.
//!
//! The proptest-based tests run on a thread with a 32 MB stack because
//! the egg equality saturation engine uses deep recursion during
//! pattern matching on generated RelExpr trees.

#![expect(clippy::unwrap_used, reason = "test code")]

use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};
use ra_grammar_fuzzer::dynamic_facts::{
    arb_database_scenario, DatabaseScenario, EnhancedPropertyValidator,
};
use ra_grammar_fuzzer::generator::SqlGenerator;
use ra_grammar_fuzzer::properties::OptimizerProperty;
use std::time::Duration;

/// Stack size for property tests (32 MB).
///
/// The egg equality saturation engine recurses deeply during pattern
/// matching and e-class merging. The default 8 MB test thread stack
/// is insufficient for generated expressions with ~128 nodes.
const PROPTEST_STACK_SIZE: usize = 32 * 1024 * 1024;

/// Test that all properties hold across different database scenarios.
///
/// This is the core dynamic facts test — the same query is tested
/// against multiple database configurations to find scenario-specific bugs.
#[test]
fn properties_hold_across_scenarios() {
    std::thread::Builder::new()
        .name("properties_hold_across_scenarios".into())
        .stack_size(PROPTEST_STACK_SIZE)
        .spawn(|| {
            let config = ProptestConfig::with_cases(20);
            let mut runner = TestRunner::new(config);
            let strategy = (SqlGenerator::new().strategy(), arb_database_scenario());

            runner
                .run(&strategy, |(expr, _scenario)| {
                    let validator = EnhancedPropertyValidator::new(vec![
                        OptimizerProperty::RuleSafety,
                        OptimizerProperty::PlanValidity,
                        OptimizerProperty::Convergence,
                    ]);

                    let results = validator.validate_across_scenarios(&expr);

                    for (test_scenario, property_results) in results {
                        for result in &property_results {
                            if !result.passed {
                                return Err(TestCaseError::Fail(
                                    format!(
                                        "Property {} failed in scenario {:?}: {}",
                                        result.property,
                                        test_scenario,
                                        result.details,
                                    )
                                    .into(),
                                ));
                            }
                        }
                    }
                    Ok(())
                })
                .unwrap();
        })
        .expect("spawn test thread")
        .join()
        .expect("test thread panicked");
}

/// Test that the same query produces consistent results when
/// tested multiple times with the same scenario (determinism test).
#[test]
fn consistent_results_per_scenario() {
    std::thread::Builder::new()
        .name("consistent_results_per_scenario".into())
        .stack_size(PROPTEST_STACK_SIZE)
        .spawn(|| {
            let config = ProptestConfig::with_cases(20);
            let mut runner = TestRunner::new(config);
            let strategy = SqlGenerator::new().strategy();

            runner
                .run(&strategy, |expr| {
                    let validator = EnhancedPropertyValidator::new(vec![
                        OptimizerProperty::RuleSafety,
                        OptimizerProperty::PlanValidity,
                    ]);

                    let results1 = validator.validate_across_scenarios(&expr);
                    let results2 = validator.validate_across_scenarios(&expr);

                    for ((scenario1, props1), (scenario2, props2)) in
                        results1.iter().zip(results2.iter())
                    {
                        if scenario1 != scenario2 {
                            return Err(TestCaseError::Fail(
                                "Scenario order should be consistent".into(),
                            ));
                        }
                        if props1.len() != props2.len() {
                            return Err(TestCaseError::Fail(
                                "Same number of properties tested".into(),
                            ));
                        }

                        for (prop1, prop2) in props1.iter().zip(props2.iter()) {
                            if prop1.passed != prop2.passed {
                                return Err(TestCaseError::Fail(
                                    format!(
                                        "Property {} should have consistent results \
                                         in scenario {:?}",
                                        prop1.property, scenario1,
                                    )
                                    .into(),
                                ));
                            }
                        }
                    }
                    Ok(())
                })
                .unwrap();
        })
        .expect("spawn test thread")
        .join()
        .expect("test thread panicked");
}

/// Test specific database scenarios individually.
#[cfg(test)]
mod scenario_tests {
    use super::*;
    use ra_core::facts::FactsProvider;
    use ra_grammar_fuzzer::dynamic_facts::DynamicFactsProvider;

    #[test]
    fn small_dev_scenario_has_limited_resources() {
        let facts = DynamicFactsProvider::new(DatabaseScenario::SmallDev);
        let hardware = facts.hardware_profile();

        assert_eq!(hardware.cpu_cores, 4);
        assert_eq!(hardware.available_memory, 8 * 1024 * 1024 * 1024); // 8 GB
        assert!(!hardware.has_gpu);

        // SmallDev should have limited features
        assert!(facts.supports_feature("btree_indexes"));
        assert!(!facts.supports_feature("gpu_acceleration"));
        assert!(!facts.supports_feature("columnar_storage"));
    }

    #[test]
    fn memory_constrained_scenario_has_memory_limits() {
        let facts =
            DynamicFactsProvider::new(DatabaseScenario::MemoryConstrained);
        let hardware = facts.hardware_profile();

        assert_eq!(hardware.cpu_cores, 2);
        assert_eq!(hardware.available_memory, 2 * 1024 * 1024 * 1024); // 2 GB
        assert!(!hardware.has_gpu);

        // Memory constrained should have explicit memory limit
        assert!(facts.memory_limit().is_some());
        assert_eq!(
            facts.memory_limit().unwrap(),
            hardware.available_memory / 2
        );
    }

    #[test]
    fn high_performance_scenario_has_advanced_features() {
        let facts =
            DynamicFactsProvider::new(DatabaseScenario::HighPerformance);
        let hardware = facts.hardware_profile();

        assert_eq!(hardware.cpu_cores, 128);
        assert_eq!(hardware.available_memory, 1024 * 1024 * 1024 * 1024); // 1 TB
        assert!(hardware.has_gpu);
        assert!(hardware.gpu_memory.is_some());

        // High performance should have all advanced features
        assert!(facts.supports_feature("gpu_acceleration"));
        assert!(facts.supports_feature("vectorized_execution"));
        assert!(facts.supports_feature("parallel_execution"));
        assert!(facts.supports_feature("columnar_storage"));
    }

    #[test]
    fn data_warehouse_scenario_has_columnar_storage() {
        let facts = DynamicFactsProvider::new(DatabaseScenario::DataWarehouse);

        // Data warehouse should support columnar features
        assert!(facts.supports_feature("columnar_storage"));
        assert!(facts.supports_feature("compression"));
        assert!(facts.supports_feature("vectorized_execution"));

        // Should have large timeout for complex analytical queries
        assert!(facts.optimizer_timeout() >= Duration::from_secs(30));
    }

    #[test]
    fn stale_stats_scenario_has_high_staleness() {
        let mut facts =
            DynamicFactsProvider::new(DatabaseScenario::StaleStats);

        // Generate stats for a test table
        facts.generate_table_stats("users");
        let stats = facts.get_table_stats("users").unwrap();

        // Stale stats should have high staleness factor
        let staleness = stats.staleness_factor();
        assert!(
            staleness > 5.0,
            "Stale stats should have high staleness factor, got {staleness}"
        );

        // Should have low confidence
        assert!(
            stats.confidence < 0.5,
            "Stale stats should have low confidence, got {}",
            stats.confidence
        );

        // Should have many estimated modifications
        assert!(
            stats.estimated_modifications > 0,
            "Stale stats should have modifications"
        );
    }

    #[test]
    fn skewed_data_scenario_generates_skewed_columns() {
        let mut facts =
            DynamicFactsProvider::new(DatabaseScenario::SkewedData);

        // Generate column stats for a test column
        facts.generate_column_stats("users", "status");
        let stats = facts.get_column_stats("users", "status").unwrap();

        // Skewed data should have correlation with physical ordering
        assert!(stats.correlation.is_some());
        if let Some(correlation) = stats.correlation {
            assert!(
                correlation > 0.5,
                "Skewed data should have high correlation, got {correlation}"
            );
        }
    }

    #[test]
    fn dynamic_facts_generate_realistic_table_sizes() {
        let scenarios = [
            (DatabaseScenario::SmallDev, 1_000u64, 100_000u64),
            (DatabaseScenario::MediumProd, 100_000u64, 10_000_000u64),
            (
                DatabaseScenario::LargeEnterprise,
                10_000_000u64,
                1_000_000_000u64,
            ),
            (
                DatabaseScenario::DataWarehouse,
                1_000_000_000u64,
                10_000_000_000u64,
            ),
        ];

        for (scenario, min_rows, max_rows) in scenarios {
            let mut facts = DynamicFactsProvider::new(scenario);
            facts.generate_table_stats("test_table");
            let stats = facts.get_table_stats("test_table").unwrap();

            assert!(
                stats.row_count >= min_rows as f64
                    && stats.row_count <= max_rows as f64,
                "Scenario {:?} should generate row count between {} and {}, got {}",
                scenario,
                min_rows,
                max_rows,
                stats.row_count
            );

            // Verify derived statistics are reasonable
            assert!(
                stats.average_row_size > 0.0,
                "Average row size should be positive"
            );
            assert!(stats.table_size_bytes > 0, "Table size should be positive");
            assert!(stats.page_count > 0, "Page count should be positive");
        }
    }
}
