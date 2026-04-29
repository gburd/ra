#![expect(
    clippy::unwrap_used,
    clippy::approx_constant,
    reason = "test code"
)]
//! Comprehensive tests for the formal pre-condition system.
//!
//! Tests `PreCondition` types, `FactsProvider` implementations,
//! `PreConditionEvaluator`, `FactsContext`, and optimizer integration.

use ra_core::{
    EmptyFactsProvider, FactValue, FactsProvider, LogicalOperator, PreCondition,
    PreConditionBuilder, SqlDialect,
};
use ra_engine::{FactsContext, FactsContextBuilder, Optimizer, PreConditionEvaluator};
use ra_hardware::HardwareProfile;
use std::sync::Arc;

// ========== FactValue Comparison Tests ==========

#[test]
fn fact_value_compare_integers() {
    let a = FactValue::Int(100);
    let b = FactValue::Int(50);

    assert!(a.compare(">", &b).unwrap());
    assert!(a.compare(">=", &b).unwrap());
    assert!(!a.compare("<", &b).unwrap());
    assert!(!a.compare("<=", &b).unwrap());
    assert!(!a.compare("==", &b).unwrap());
    assert!(a.compare("!=", &b).unwrap());
}

#[test]
fn fact_value_compare_floats() {
    let a = FactValue::Float(3.14);
    let b = FactValue::Float(2.71);

    assert!(a.compare(">", &b).unwrap());
    assert!(a.compare(">=", &b).unwrap());
    assert!(!a.compare("<", &b).unwrap());
}

#[test]
fn fact_value_compare_mixed_numeric() {
    let int_val = FactValue::Int(100);
    let float_val = FactValue::Float(99.5);

    assert!(int_val.compare(">", &float_val).unwrap());
    assert!(!float_val.compare(">", &int_val).unwrap());
}

#[test]
fn fact_value_compare_strings() {
    let a = FactValue::String("postgresql".into());
    let b = FactValue::String("post".into());

    assert!(a.compare("contains", &b).unwrap());
    assert!(a.compare("starts_with", &b).unwrap());
    assert!(!a.compare("ends_with", &b).unwrap());
    assert!(!a.compare("==", &b).unwrap());
}

#[test]
fn fact_value_compare_booleans() {
    let t = FactValue::Bool(true);
    let f = FactValue::Bool(false);

    assert!(t.compare("==", &t).unwrap());
    assert!(t.compare("!=", &f).unwrap());
    assert!(!t.compare("==", &f).unwrap());
}

#[test]
fn fact_value_compare_type_mismatch() {
    let int_val = FactValue::Int(42);
    let str_val = FactValue::String("42".into());

    let result = int_val.compare("==", &str_val);
    assert!(result.is_err());
}

// ========== PreConditionEvaluator Tests ==========

#[test]
fn evaluator_empty_preconditions() {
    let facts = Arc::new(EmptyFactsProvider::new());
    let evaluator = PreConditionEvaluator::new(facts);

    let result = evaluator.evaluate(&[]);
    assert!(result.is_satisfied());
}

#[test]
fn evaluator_pattern_always_passes() {
    let facts = Arc::new(EmptyFactsProvider::new());
    let evaluator = PreConditionEvaluator::new(facts);

    let preconditions = vec![PreCondition::Pattern {
        must_match: Some("(filter ?pred (join inner ?cond ?left ?right))".into()),
        must_not_match: None,
        description: None,
        optional: false,
    }];

    let result = evaluator.evaluate(&preconditions);
    assert!(result.is_satisfied());
}

#[test]
fn evaluator_hardware_cpu_cores() {
    let facts = Arc::new(EmptyFactsProvider::new());
    let evaluator = PreConditionEvaluator::new(facts.clone());

    let preconditions = vec![PreCondition::Fact {
        fact_type: "hardware.cpu_cores".into(),
        table: None,
        column: None,
        comparator: ">".into(),
        threshold: FactValue::Int(4),
        confidence: None,
        description: None,
        optional: false,
    }];

    let result = evaluator.evaluate(&preconditions);
    // EmptyFactsProvider has 8 cores
    assert!(result.is_satisfied());
}

#[test]
fn evaluator_hardware_gpu_unavailable() {
    let facts = Arc::new(EmptyFactsProvider::new());
    let evaluator = PreConditionEvaluator::new(facts);

    let preconditions = vec![PreCondition::Fact {
        fact_type: "hardware.has_gpu".into(),
        table: None,
        column: None,
        comparator: "==".into(),
        threshold: FactValue::Bool(true),
        confidence: None,
        description: None,
        optional: false,
    }];

    let result = evaluator.evaluate(&preconditions);
    // EmptyFactsProvider has no GPU
    assert!(!result.is_satisfied());
}

#[test]
fn evaluator_optional_precondition_failure() {
    let facts = Arc::new(EmptyFactsProvider::new());
    let evaluator = PreConditionEvaluator::new(facts);

    let preconditions = vec![PreCondition::Fact {
        fact_type: "statistics.cardinality".into(),
        table: Some("nonexistent".into()),
        column: None,
        comparator: ">".into(),
        threshold: FactValue::Int(1000),
        confidence: None,
        description: None,
        optional: true, // Optional, so should not fail
    }];

    let result = evaluator.evaluate(&preconditions);
    assert!(result.is_satisfied());
}

#[test]
fn evaluator_required_precondition_failure() {
    let facts = Arc::new(EmptyFactsProvider::new());
    let evaluator = PreConditionEvaluator::new(facts);

    let preconditions = vec![PreCondition::Fact {
        fact_type: "statistics.cardinality".into(),
        table: Some("nonexistent".into()),
        column: None,
        comparator: ">".into(),
        threshold: FactValue::Int(1000),
        confidence: None,
        description: None,
        optional: false, // Required
    }];

    let result = evaluator.evaluate(&preconditions);
    assert!(!result.is_satisfied());
}

#[test]
fn evaluator_composite_and_all_pass() {
    let facts = Arc::new(EmptyFactsProvider::new());
    let evaluator = PreConditionEvaluator::new(facts);

    let preconditions = vec![PreCondition::Composite {
        operator: LogicalOperator::And,
        conditions: vec![
            PreCondition::Fact {
                fact_type: "hardware.cpu_cores".into(),
                table: None,
                column: None,
                comparator: ">".into(),
                threshold: FactValue::Int(4),
                confidence: None,
                description: None,
                optional: false,
            },
            PreCondition::Fact {
                fact_type: "hardware.simd_width".into(),
                table: None,
                column: None,
                comparator: ">=".into(),
                threshold: FactValue::Int(128),
                confidence: None,
                description: None,
                optional: false,
            },
        ],
        description: None,
        optional: false,
    }];

    let result = evaluator.evaluate(&preconditions);
    assert!(result.is_satisfied());
}

#[test]
fn evaluator_composite_and_one_fails() {
    let facts = Arc::new(EmptyFactsProvider::new());
    let evaluator = PreConditionEvaluator::new(facts);

    let preconditions = vec![PreCondition::Composite {
        operator: LogicalOperator::And,
        conditions: vec![
            PreCondition::Fact {
                fact_type: "hardware.cpu_cores".into(),
                table: None,
                column: None,
                comparator: ">".into(),
                threshold: FactValue::Int(4),
                confidence: None,
                description: None,
                optional: false,
            },
            PreCondition::Fact {
                fact_type: "hardware.has_gpu".into(),
                table: None,
                column: None,
                comparator: "==".into(),
                threshold: FactValue::Bool(true), // Will fail
                confidence: None,
                description: None,
                optional: false,
            },
        ],
        description: None,
        optional: false,
    }];

    let result = evaluator.evaluate(&preconditions);
    assert!(!result.is_satisfied());
}

#[test]
fn evaluator_composite_or_one_passes() {
    let facts = Arc::new(EmptyFactsProvider::new());
    let evaluator = PreConditionEvaluator::new(facts);

    let preconditions = vec![PreCondition::Composite {
        operator: LogicalOperator::Or,
        conditions: vec![
            PreCondition::Fact {
                fact_type: "hardware.has_gpu".into(),
                table: None,
                column: None,
                comparator: "==".into(),
                threshold: FactValue::Bool(true), // Will fail
                confidence: None,
                description: None,
                optional: false,
            },
            PreCondition::Fact {
                fact_type: "hardware.cpu_cores".into(),
                table: None,
                column: None,
                comparator: ">".into(),
                threshold: FactValue::Int(4), // Will pass
                confidence: None,
                description: None,
                optional: false,
            },
        ],
        description: None,
        optional: false,
    }];

    let result = evaluator.evaluate(&preconditions);
    assert!(result.is_satisfied());
}

#[test]
fn evaluator_composite_not() {
    let facts = Arc::new(EmptyFactsProvider::new());
    let evaluator = PreConditionEvaluator::new(facts);

    let preconditions = vec![PreCondition::Composite {
        operator: LogicalOperator::Not,
        conditions: vec![PreCondition::Fact {
            fact_type: "hardware.has_gpu".into(),
            table: None,
            column: None,
            comparator: "==".into(),
            threshold: FactValue::Bool(true),
            confidence: None,
            description: None,
            optional: false,
        }],
        description: None,
        optional: false,
    }];

    let result = evaluator.evaluate(&preconditions);
    // NOT(has_gpu == true) = NOT(false) = true
    assert!(result.is_satisfied());
}

#[test]
fn evaluator_capability_check_supported() {
    let facts = Arc::new(EmptyFactsProvider::new());
    let evaluator = PreConditionEvaluator::new(facts);

    let preconditions = vec![PreCondition::Capability {
        database: "generic".into(),
        requires: "any_feature".into(),
        description: None,
        optional: false,
    }];

    let result = evaluator.evaluate(&preconditions);
    // EmptyFactsProvider returns false for all features
    assert!(!result.is_satisfied());
}

// ========== FactsContext Tests ==========

#[test]
fn facts_context_basic_creation() {
    let hw = HardwareProfile::cpu_only();
    let context = FactsContext::new(hw);

    assert_eq!(context.database_name(), "generic");
    assert_eq!(context.sql_dialect(), SqlDialect::Generic);
    assert!(!context.has_gpu());
}

#[test]
fn facts_context_builder_pattern() {
    let hw = HardwareProfile::cpu_only();
    let context = FactsContextBuilder::new(hw)
        .database("postgresql")
        .dialect(SqlDialect::Postgres)
        .feature("lateral_join", true)
        .feature("cte_recursive", true)
        .memory_limit(32 * 1024 * 1024 * 1024)
        .build();

    assert_eq!(context.database_name(), "postgresql");
    assert_eq!(context.sql_dialect(), SqlDialect::Postgres);
    assert!(context.supports_feature("lateral_join"));
    assert!(context.supports_feature("cte_recursive"));
    assert!(!context.supports_feature("unknown_feature"));
    assert_eq!(context.memory_limit(), Some(32 * 1024 * 1024 * 1024));
}

#[test]
fn facts_context_gpu_server() {
    let hw = HardwareProfile::gpu_server();
    let context = FactsContextBuilder::new(hw)
        .database("duckdb")
        .dialect(SqlDialect::DuckDb)
        .feature("parallel_scan", true)
        .build();

    assert_eq!(context.database_name(), "duckdb");
    assert!(context.has_gpu());
    assert!(context.supports_feature("parallel_scan"));
}

// ========== PreConditionBuilder Tests ==========

#[test]
fn precondition_builder_basic() {
    let preconditions = PreConditionBuilder::new()
        .pattern("(filter ?pred (join inner ?cond ?left ?right))")
        .predicate("is_deterministic(?pred)")
        .build();

    assert_eq!(preconditions.len(), 2);
}

#[test]
fn precondition_builder_with_facts() {
    let preconditions = PreConditionBuilder::new()
        .fact("statistics.cardinality", ">", FactValue::Int(10000))
        .capability("current", "lateral_join")
        .build();

    assert_eq!(preconditions.len(), 2);
}

// ========== Integration Tests ==========

#[test]
fn integration_optimizer_with_empty_facts() {
    use ra_core::algebra::RelExpr;

    let facts = EmptyFactsProvider::new();
    let optimizer = Optimizer::new();
    let expr = RelExpr::scan("users");

    let result = optimizer.optimize_with_facts(&expr, &facts);
    assert!(result.is_ok());
}

#[test]
fn integration_optimizer_with_full_context() {
    use ra_core::algebra::RelExpr;

    let hw = HardwareProfile::gpu_server();
    let context = FactsContextBuilder::new(hw)
        .database("postgresql")
        .dialect(SqlDialect::Postgres)
        .feature("parallel_hash_join", true)
        .build();

    let optimizer = Optimizer::new();
    let expr = RelExpr::scan("orders");

    let result = optimizer.optimize_with_facts(&expr, &context);
    assert!(result.is_ok());
}

#[test]
fn integration_evaluator_with_context() {
    let hw = HardwareProfile::gpu_server();
    let context = FactsContextBuilder::new(hw)
        .database("postgresql")
        .feature("cte_recursive", true)
        .build();

    let evaluator = PreConditionEvaluator::new(Arc::new(context));

    let preconditions = vec![
        PreCondition::Capability {
            database: "postgresql".into(),
            requires: "cte_recursive".into(),
            description: None,
            optional: false,
        },
        PreCondition::Fact {
            fact_type: "hardware.has_gpu".into(),
            table: None,
            column: None,
            comparator: "==".into(),
            threshold: FactValue::Bool(true),
            confidence: None,
            description: None,
            optional: false,
        },
    ];

    let result = evaluator.evaluate(&preconditions);
    assert!(result.is_satisfied());
}

// ========== Error Handling Tests ==========

#[test]
fn evaluator_unknown_fact_type() {
    let facts = Arc::new(EmptyFactsProvider::new());
    let evaluator = PreConditionEvaluator::new(facts);

    let preconditions = vec![PreCondition::Fact {
        fact_type: "unknown.fact.type".into(),
        table: None,
        column: None,
        comparator: "==".into(),
        threshold: FactValue::Int(42),
        confidence: None,
        description: None,
        optional: false,
    }];

    let result = evaluator.evaluate(&preconditions);
    assert!(!result.is_satisfied());
    assert!(result.is_error());
}

#[test]
fn evaluator_invalid_comparator() {
    let facts = Arc::new(EmptyFactsProvider::new());
    let _evaluator = PreConditionEvaluator::new(facts);

    // String comparison doesn't support numeric operators
    let a = FactValue::String("test".into());
    let b = FactValue::String("test".into());

    let result = a.compare(">", &b);
    assert!(result.is_err());
}
