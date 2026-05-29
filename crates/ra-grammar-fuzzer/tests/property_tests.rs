//! Integration property tests for grammar-based fuzzing.
//!
//! Short tests run in normal CI. Long-duration tests are gated
//! behind the `long-duration-testing` feature flag.
//!
//! All proptest-based tests run on a 32 MB stack thread because the
//! egg equality saturation engine recurses deeply during pattern
//! matching on generated RelExpr trees.

use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};
use ra_grammar_fuzzer::generator::SqlGenerator;
use ra_grammar_fuzzer::properties::{OptimizerProperty, PropertyValidator};
use ra_grammar_fuzzer::storyline::{arb_storyline, StorylinePattern};
use std::time::Duration;

/// Stack size for property tests (32 MB).
const PROPTEST_STACK_SIZE: usize = 32 * 1024 * 1024;

/// Run a proptest on a thread with sufficient stack.
fn run_on_large_stack(name: &str, f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .name(name.to_owned())
        .stack_size(PROPTEST_STACK_SIZE)
        .spawn(f)
        .expect("spawn test thread")
        .join()
        .expect("test thread panicked");
}

// -------------------------------------------------------------------
// Standard CI property tests (fast, run on every build)
// -------------------------------------------------------------------

/// Every generated expression should pass rule safety.
#[test]
fn rule_safety_on_generated_expressions() {
    run_on_large_stack("rule_safety_on_generated_expressions", || {
        let config = ProptestConfig::with_cases(50);
        let mut runner = TestRunner::new(config);
        runner
            .run(&SqlGenerator::new().strategy(), |expr| {
                let validator = PropertyValidator::new(vec![
                    OptimizerProperty::RuleSafety,
                ])
                .with_time_limit(Duration::from_secs(10));
                for result in &validator.validate(&expr) {
                    if !result.passed {
                        return Err(TestCaseError::Fail(
                            format!(
                                "property {} failed: {}",
                                result.property, result.details,
                            )
                            .into(),
                        ));
                    }
                }
                Ok(())
            })
            .unwrap();
    });
}

/// Every generated expression should pass roundtrip conversion.
#[test]
fn roundtrip_on_generated_expressions() {
    run_on_large_stack("roundtrip_on_generated_expressions", || {
        let config = ProptestConfig::with_cases(50);
        let mut runner = TestRunner::new(config);
        runner
            .run(&SqlGenerator::new().strategy(), |expr| {
                let validator = PropertyValidator::new(vec![
                    OptimizerProperty::Roundtrip,
                ]);
                for result in &validator.validate(&expr) {
                    if !result.passed {
                        return Err(TestCaseError::Fail(
                            format!(
                                "property {} failed: {}",
                                result.property, result.details,
                            )
                            .into(),
                        ));
                    }
                }
                Ok(())
            })
            .unwrap();
    });
}

/// Optimization should converge within time limits.
#[test]
fn convergence_on_generated_expressions() {
    run_on_large_stack("convergence_on_generated_expressions", || {
        let config = ProptestConfig::with_cases(50);
        let mut runner = TestRunner::new(config);
        runner
            .run(&SqlGenerator::new().strategy(), |expr| {
                let validator = PropertyValidator::new(vec![
                    OptimizerProperty::Convergence,
                ])
                .with_time_limit(Duration::from_secs(5));
                for result in &validator.validate(&expr) {
                    if !result.passed {
                        return Err(TestCaseError::Fail(
                            format!(
                                "property {} failed: {}",
                                result.property, result.details,
                            )
                            .into(),
                        ));
                    }
                }
                Ok(())
            })
            .unwrap();
    });
}

/// Join-heavy queries should preserve tables.
#[test]
fn table_preservation_on_joins() {
    run_on_large_stack("table_preservation_on_joins", || {
        let config = ProptestConfig::with_cases(50);
        let mut runner = TestRunner::new(config);
        runner
            .run(&SqlGenerator::new().join_strategy(), |expr| {
                let validator = PropertyValidator::new(vec![
                    OptimizerProperty::TablePreservation,
                ])
                .with_time_limit(Duration::from_secs(10));
                for result in &validator.validate(&expr) {
                    if !result.passed {
                        return Err(TestCaseError::Fail(
                            format!(
                                "property {} failed: {}",
                                result.property, result.details,
                            )
                            .into(),
                        ));
                    }
                }
                Ok(())
            })
            .unwrap();
    });
}

/// Aggregate queries should pass plan validity.
#[test]
fn plan_validity_on_aggregates() {
    run_on_large_stack("plan_validity_on_aggregates", || {
        let config = ProptestConfig::with_cases(50);
        let mut runner = TestRunner::new(config);
        runner
            .run(&SqlGenerator::new().aggregate_strategy(), |expr| {
                let validator = PropertyValidator::new(vec![
                    OptimizerProperty::PlanValidity,
                ])
                .with_time_limit(Duration::from_secs(10));
                for result in &validator.validate(&expr) {
                    if !result.passed {
                        return Err(TestCaseError::Fail(
                            format!(
                                "property {} failed: {}",
                                result.property, result.details,
                            )
                            .into(),
                        ));
                    }
                }
                Ok(())
            })
            .unwrap();
    });
}

/// Set operations should pass all core properties.
#[test]
fn all_properties_on_set_ops() {
    run_on_large_stack("all_properties_on_set_ops", || {
        let config = ProptestConfig::with_cases(50);
        let mut runner = TestRunner::new(config);
        runner
            .run(&SqlGenerator::new().set_op_strategy(), |expr| {
                let validator = PropertyValidator::all_properties()
                    .with_time_limit(Duration::from_secs(10));
                for result in &validator.validate(&expr) {
                    if !result.passed {
                        return Err(TestCaseError::Fail(
                            format!(
                                "property {} failed: {}",
                                result.property, result.details,
                            )
                            .into(),
                        ));
                    }
                }
                Ok(())
            })
            .unwrap();
    });
}

// -------------------------------------------------------------------
// Storyline-based tests
// -------------------------------------------------------------------

/// Full lifecycle storyline should pass core properties at each step,
/// including idempotence (the historical `FullOuter` extraction bug
/// that previously forced its exclusion is fixed).
#[test]
fn full_lifecycle_all_properties() {
    run_on_large_stack("full_lifecycle_all_properties", || {
        let config = ProptestConfig::with_cases(10);
        let mut runner = TestRunner::new(config);
        runner
            .run(
                &arb_storyline(StorylinePattern::full_lifecycle()),
                |storyline| {
                    let validator = PropertyValidator::new(vec![
                        OptimizerProperty::Roundtrip,
                        OptimizerProperty::TablePreservation,
                        OptimizerProperty::Convergence,
                        OptimizerProperty::PlanValidity,
                        OptimizerProperty::RuleSafety,
                        OptimizerProperty::Idempotence,
                    ])
                    .with_time_limit(Duration::from_secs(10));

                    for step in &storyline.steps {
                        for result in &validator.validate(&step.expr) {
                            if !result.passed {
                                return Err(TestCaseError::Fail(
                                    format!(
                                        "property {} failed at stage {}: {}",
                                        result.property,
                                        step.stage,
                                        result.details,
                                    )
                                    .into(),
                                ));
                            }
                        }
                    }
                    Ok(())
                },
            )
            .unwrap();
    });
}

/// Read-heavy pattern should maintain rule safety.
#[test]
fn read_heavy_rule_safety() {
    run_on_large_stack("read_heavy_rule_safety", || {
        let config = ProptestConfig::with_cases(10);
        let mut runner = TestRunner::new(config);
        runner
            .run(
                &arb_storyline(StorylinePattern::read_heavy()),
                |storyline| {
                    let validator = PropertyValidator::new(vec![
                        OptimizerProperty::RuleSafety,
                    ])
                    .with_time_limit(Duration::from_secs(10));

                    for step in &storyline.steps {
                        for result in &validator.validate(&step.expr) {
                            if !result.passed {
                                return Err(TestCaseError::Fail(
                                    format!(
                                        "rule safety failed at stage {}: {}",
                                        step.stage, result.details,
                                    )
                                    .into(),
                                ));
                            }
                        }
                    }
                    Ok(())
                },
            )
            .unwrap();
    });
}

// -------------------------------------------------------------------
// Long-duration tests (feature-gated)
// -------------------------------------------------------------------

/// Extended fuzzing: all properties on deeply nested expressions.
///
/// Covers all six optimizer properties including idempotence.
/// The historical `Filter(col_id, FullOuter(...))` extraction bug
/// that previously forced idempotence exclusion is fixed; the
/// dedicated `extended_idempotence` test plus the persisted
/// regression seeds guard against recurrence.
#[cfg(feature = "long-duration-testing")]
#[test]
fn extended_all_properties() {
    run_on_large_stack("extended_all_properties", || {
        let config = ProptestConfig::with_cases(1000);
        let mut runner = TestRunner::new(config);
        runner
            .run(
                &ra_grammar_fuzzer::generator::arb_rel_expr(5),
                |expr| {
                    let validator = PropertyValidator::new(vec![
                        OptimizerProperty::Roundtrip,
                        OptimizerProperty::TablePreservation,
                        OptimizerProperty::Convergence,
                        OptimizerProperty::PlanValidity,
                        OptimizerProperty::RuleSafety,
                        OptimizerProperty::Idempotence,
                    ])
                    .with_time_limit(Duration::from_secs(30));
                    for result in &validator.validate(&expr) {
                        if !result.passed {
                            return Err(TestCaseError::Fail(
                                format!(
                                    "property {} failed: {}",
                                    result.property, result.details,
                                )
                                .into(),
                            ));
                        }
                    }
                    Ok(())
                },
            )
            .unwrap();
    });
}

/// Extended fuzzing: idempotence across all expression types.
///
/// Idempotence — `optimize(optimize(x)) == optimize(x)` — was
/// historically a known-failing property on a subset of
/// `Filter(col_id, FullOuter(...))` shapes, where the e-graph's
/// second extraction pass could drop a table reference the first
/// pass retained. That bug is fixed (subquery-decorrelation and
/// extraction work); this test runs 1000 random cases plus the
/// persisted regression seeds to guard against recurrence. It
/// stays gated behind `long-duration-testing` because, like its
/// sibling `extended_all_properties`, it's a multi-second
/// proptest sweep not suited to the fast inner-loop suite.
#[cfg(feature = "long-duration-testing")]
#[test]
fn extended_idempotence() {
    run_on_large_stack("extended_idempotence", || {
        let config = ProptestConfig::with_cases(1000);
        let mut runner = TestRunner::new(config);
        runner
            .run(
                &ra_grammar_fuzzer::generator::arb_rel_expr(4),
                |expr| {
                    let validator = PropertyValidator::new(vec![
                        OptimizerProperty::Idempotence,
                    ])
                    .with_time_limit(Duration::from_secs(30));
                    for result in &validator.validate(&expr) {
                        if !result.passed {
                            return Err(TestCaseError::Fail(
                                format!(
                                    "idempotence failed: {}",
                                    result.details,
                                )
                                .into(),
                            ));
                        }
                    }
                    Ok(())
                },
            )
            .unwrap();
    });
}

/// Extended storyline: mixed DML across the five fast-to-check
/// properties. Idempotence is intentionally NOT included here:
/// it double-optimizes every expression, and across 1000
/// multi-step storylines that pushes this sweep past several
/// minutes with no coverage the dedicated `extended_idempotence`
/// test (1000 cases over `arb_rel_expr`) doesn't already provide.
#[cfg(feature = "long-duration-testing")]
#[test]
fn extended_mixed_dml_all_properties() {
    run_on_large_stack("extended_mixed_dml_all_properties", || {
        let config = ProptestConfig::with_cases(1000);
        let mut runner = TestRunner::new(config);
        runner
            .run(
                &arb_storyline(StorylinePattern::mixed_dml()),
                |storyline| {
                    let validator = PropertyValidator::new(vec![
                        OptimizerProperty::Roundtrip,
                        OptimizerProperty::TablePreservation,
                        OptimizerProperty::Convergence,
                        OptimizerProperty::PlanValidity,
                        OptimizerProperty::RuleSafety,
                    ])
                    .with_time_limit(Duration::from_secs(30));

                    for step in &storyline.steps {
                        for result in &validator.validate(&step.expr) {
                            if !result.passed {
                                return Err(TestCaseError::Fail(
                                    format!(
                                        "property {} failed at stage {}: {}",
                                        result.property,
                                        step.stage,
                                        result.details,
                                    )
                                    .into(),
                                ));
                            }
                        }
                    }
                    Ok(())
                },
            )
            .unwrap();
    });
}
