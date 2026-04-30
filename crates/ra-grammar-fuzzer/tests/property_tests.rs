//! Integration property tests for grammar-based fuzzing.
//!
//! Short tests run in normal CI. Long-duration tests are gated
//! behind the `long-duration-testing` feature flag.

#![expect(clippy::unwrap_used, reason = "test code")]
#![expect(clippy::expect_used, reason = "test code")]

use proptest::prelude::*;
use ra_grammar_fuzzer::generator::SqlGenerator;
use ra_grammar_fuzzer::properties::{OptimizerProperty, PropertyValidator};
use ra_grammar_fuzzer::storyline::{arb_storyline, StorylinePattern};
use std::time::Duration;

// -------------------------------------------------------------------
// Standard CI property tests (fast, run on every build)
// -------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Every generated expression should pass rule safety.
    #[test]
    fn rule_safety_on_generated_expressions(
        expr in SqlGenerator::new().strategy()
    ) {
        let validator = PropertyValidator::new(vec![
            OptimizerProperty::RuleSafety,
        ]).with_time_limit(Duration::from_secs(10));
        let results = validator.validate(&expr);
        for result in &results {
            prop_assert!(
                result.passed,
                "property {} failed: {}",
                result.property,
                result.details
            );
        }
    }

    /// Every generated expression should pass roundtrip conversion.
    #[test]
    fn roundtrip_on_generated_expressions(
        expr in SqlGenerator::new().strategy()
    ) {
        let validator = PropertyValidator::new(vec![
            OptimizerProperty::Roundtrip,
        ]);
        let results = validator.validate(&expr);
        for result in &results {
            prop_assert!(
                result.passed,
                "property {} failed: {}",
                result.property,
                result.details
            );
        }
    }

    /// Optimization should converge within time limits.
    #[test]
    fn convergence_on_generated_expressions(
        expr in SqlGenerator::new().strategy()
    ) {
        let validator = PropertyValidator::new(vec![
            OptimizerProperty::Convergence,
        ]).with_time_limit(Duration::from_secs(5));
        let results = validator.validate(&expr);
        for result in &results {
            prop_assert!(
                result.passed,
                "property {} failed: {}",
                result.property,
                result.details
            );
        }
    }

    /// Join-heavy queries should preserve tables.
    #[test]
    fn table_preservation_on_joins(
        expr in SqlGenerator::new().join_strategy()
    ) {
        let validator = PropertyValidator::new(vec![
            OptimizerProperty::TablePreservation,
        ]).with_time_limit(Duration::from_secs(10));
        let results = validator.validate(&expr);
        for result in &results {
            prop_assert!(
                result.passed,
                "property {} failed: {}",
                result.property,
                result.details
            );
        }
    }

    /// Aggregate queries should pass plan validity.
    #[test]
    fn plan_validity_on_aggregates(
        expr in SqlGenerator::new().aggregate_strategy()
    ) {
        let validator = PropertyValidator::new(vec![
            OptimizerProperty::PlanValidity,
        ]).with_time_limit(Duration::from_secs(10));
        let results = validator.validate(&expr);
        for result in &results {
            prop_assert!(
                result.passed,
                "property {} failed: {}",
                result.property,
                result.details
            );
        }
    }

    /// Set operations should pass all core properties.
    #[test]
    fn all_properties_on_set_ops(
        expr in SqlGenerator::new().set_op_strategy()
    ) {
        let validator = PropertyValidator::all_properties()
            .with_time_limit(Duration::from_secs(10));
        let results = validator.validate(&expr);
        for result in &results {
            prop_assert!(
                result.passed,
                "property {} failed: {}",
                result.property,
                result.details
            );
        }
    }
}

// -------------------------------------------------------------------
// Storyline-based tests
// -------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    /// Full lifecycle storyline should pass all properties at each step.
    #[test]
    fn full_lifecycle_all_properties(
        storyline in arb_storyline(StorylinePattern::full_lifecycle())
    ) {
        let validator = PropertyValidator::all_properties()
            .with_time_limit(Duration::from_secs(10));

        for step in &storyline.steps {
            let results = validator.validate(&step.expr);
            for result in &results {
                prop_assert!(
                    result.passed,
                    "property {} failed at stage {}: {}",
                    result.property,
                    step.stage,
                    result.details
                );
            }
        }
    }

    /// Read-heavy pattern should maintain rule safety.
    #[test]
    fn read_heavy_rule_safety(
        storyline in arb_storyline(StorylinePattern::read_heavy())
    ) {
        let validator = PropertyValidator::new(vec![
            OptimizerProperty::RuleSafety,
        ]).with_time_limit(Duration::from_secs(10));

        for step in &storyline.steps {
            let results = validator.validate(&step.expr);
            for result in &results {
                prop_assert!(
                    result.passed,
                    "rule safety failed at stage {}: {}",
                    step.stage,
                    result.details
                );
            }
        }
    }
}

// -------------------------------------------------------------------
// Long-duration tests (feature-gated)
// -------------------------------------------------------------------

#[cfg(feature = "long-duration-testing")]
proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    /// Extended fuzzing: all properties on deeply nested expressions.
    #[test]
    fn extended_all_properties(
        expr in ra_grammar_fuzzer::generator::arb_rel_expr(5)
    ) {
        let validator = PropertyValidator::all_properties()
            .with_time_limit(Duration::from_secs(30));
        let results = validator.validate(&expr);
        for result in &results {
            prop_assert!(
                result.passed,
                "property {} failed: {}",
                result.property,
                result.details
            );
        }
    }

    /// Extended fuzzing: idempotence across all expression types.
    #[test]
    fn extended_idempotence(
        expr in ra_grammar_fuzzer::generator::arb_rel_expr(4)
    ) {
        let validator = PropertyValidator::new(vec![
            OptimizerProperty::Idempotence,
        ]).with_time_limit(Duration::from_secs(30));
        let results = validator.validate(&expr);
        for result in &results {
            prop_assert!(
                result.passed,
                "idempotence failed: {}",
                result.details
            );
        }
    }

    /// Extended storyline: mixed DML with all properties.
    #[test]
    fn extended_mixed_dml_all_properties(
        storyline in arb_storyline(StorylinePattern::mixed_dml())
    ) {
        let validator = PropertyValidator::all_properties()
            .with_time_limit(Duration::from_secs(30));

        for step in &storyline.steps {
            let results = validator.validate(&step.expr);
            for result in &results {
                prop_assert!(
                    result.passed,
                    "property {} failed at stage {}: {}",
                    result.property,
                    step.stage,
                    result.details
                );
            }
        }
    }
}
