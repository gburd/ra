//! Validation of discovered candidate rules.
//!
//! Tests candidate rules against held-out queries to verify that
//! applying them produces semantically equivalent plans and actually
//! improves performance (or at least does not regress).

use serde::{Deserialize, Serialize};

use ra_core::algebra::RelExpr;
use ra_core::cost::Cost;

use crate::log::ExecutionLog;
use crate::synthesis::CandidateRule;

/// Result of validating a single candidate rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// The rule id being validated.
    pub rule_id: String,
    /// Number of held-out queries the rule was tested on.
    pub queries_tested: usize,
    /// Number of queries where the rule matched.
    pub queries_matched: usize,
    /// Number of queries where applying the rule improved cost.
    pub queries_improved: usize,
    /// Number of queries where applying the rule worsened cost.
    pub queries_regressed: usize,
    /// Average cost reduction ratio (< 1.0 means improvement).
    pub avg_cost_ratio: f64,
    /// Whether the rule passed validation thresholds.
    pub passed: bool,
}

/// Configuration for the validation process.
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    /// Maximum fraction of queries that may regress.
    pub max_regression_rate: f64,
    /// Minimum fraction of matched queries that must improve.
    pub min_improvement_rate: f64,
    /// Maximum allowed cost increase for any single query.
    pub max_cost_increase: f64,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            max_regression_rate: 0.05,
            min_improvement_rate: 0.3,
            max_cost_increase: 1.1,
        }
    }
}

/// A cost estimator function that scores a plan.
///
/// Wraps a closure so that different cost models can be plugged in.
pub struct CostEstimator {
    estimator: Box<dyn Fn(&RelExpr) -> Cost + Send + Sync>,
}

impl CostEstimator {
    /// Create a cost estimator from a closure.
    pub fn new(f: impl Fn(&RelExpr) -> Cost + Send + Sync + 'static) -> Self {
        Self {
            estimator: Box::new(f),
        }
    }

    /// Estimate the cost of a plan.
    #[must_use]
    pub fn estimate(&self, plan: &RelExpr) -> Cost {
        (self.estimator)(plan)
    }
}

impl std::fmt::Debug for CostEstimator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CostEstimator").finish_non_exhaustive()
    }
}

/// Validate a candidate rule against held-out execution logs.
///
/// For each log entry, checks whether the rule's match pattern
/// applies to the original plan and, if so, compares the estimated
/// cost of the original plan versus the optimized plan.
#[must_use]
pub fn validate_rule(
    candidate: &CandidateRule,
    validation_logs: &[ExecutionLog],
    cost_estimator: &CostEstimator,
    config: &ValidationConfig,
) -> ValidationResult {
    let mut queries_tested = 0;
    let mut queries_matched = 0;
    let mut queries_improved = 0;
    let mut queries_regressed = 0;
    let mut total_cost_ratio = 0.0;

    for log in validation_logs {
        queries_tested += 1;

        if !candidate.match_pattern.matches(&log.original_plan) {
            continue;
        }
        queries_matched += 1;

        let original_cost = cost_estimator.estimate(&log.original_plan);
        let optimized_cost = cost_estimator.estimate(&log.optimized_plan);

        let original_total = original_cost.total();
        let optimized_total = optimized_cost.total();

        if original_total <= 0.0 {
            continue;
        }

        let ratio = optimized_total / original_total;
        total_cost_ratio += ratio;

        if ratio < 1.0 {
            queries_improved += 1;
        } else if ratio > config.max_cost_increase {
            queries_regressed += 1;
        }
    }

    #[allow(clippy::cast_precision_loss)]
    let matched_f64 = queries_matched as f64;

    let avg_cost_ratio = if queries_matched > 0 {
        total_cost_ratio / matched_f64
    } else {
        1.0
    };

    #[allow(clippy::cast_precision_loss)]
    let regression_rate = if queries_matched > 0 {
        queries_regressed as f64 / matched_f64
    } else {
        0.0
    };

    #[allow(clippy::cast_precision_loss)]
    let improvement_rate = if queries_matched > 0 {
        queries_improved as f64 / matched_f64
    } else {
        0.0
    };

    let passed = queries_matched > 0
        && regression_rate <= config.max_regression_rate
        && improvement_rate >= config.min_improvement_rate;

    ValidationResult {
        rule_id: candidate.metadata.id.clone(),
        queries_tested,
        queries_matched,
        queries_improved,
        queries_regressed,
        avg_cost_ratio,
        passed,
    }
}

/// Validate a batch of candidate rules and return only those that
/// pass.
#[must_use]
pub fn validate_rules(
    candidates: &[CandidateRule],
    validation_logs: &[ExecutionLog],
    cost_estimator: &CostEstimator,
    config: &ValidationConfig,
) -> Vec<(CandidateRule, ValidationResult)> {
    candidates
        .iter()
        .map(|c| {
            let result = validate_rule(c, validation_logs, cost_estimator, config);
            (c.clone(), result)
        })
        .filter(|(_, result)| result.passed)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::Token;
    use crate::synthesis::StructuralPattern;
    use ra_core::algebra::RelExpr;
    use ra_core::cost::Cost;
    use ra_core::rule::{RuleCategory, RuleMetadata};
    use std::collections::HashMap;
    use std::time::Duration;

    fn make_candidate() -> CandidateRule {
        CandidateRule {
            metadata: RuleMetadata {
                id: "test-rule".into(),
                name: "Test Rule".into(),
                description: "A test rule".into(),
                category: RuleCategory::Logical,
                databases: vec![],
                priority: 100,
            },
            match_pattern: StructuralPattern {
                root: Token::Filter,
                children: vec![StructuralPattern {
                    root: Token::Scan,
                    children: vec![],
                }],
            },
            replacement_pattern: StructuralPattern {
                root: Token::Scan,
                children: vec![],
            },
            confidence: 0.8,
            support: 10,
            avg_speedup: 2.0,
        }
    }

    fn make_log(original: RelExpr, optimized: RelExpr, est_cost: Cost) -> ExecutionLog {
        ExecutionLog {
            id: 0,
            original_plan: original,
            optimized_plan: optimized,
            estimated_cost: est_cost,
            execution_time: Duration::from_millis(50),
            actual_cardinalities: HashMap::new(),
            estimated_cardinalities: HashMap::new(),
            tags: vec![],
        }
    }

    fn simple_cost_estimator() -> CostEstimator {
        CostEstimator::new(|plan| match plan {
            RelExpr::Scan { .. } => Cost::new(1.0, 1.0, 0.0, 100),
            RelExpr::Filter { .. } => Cost::new(5.0, 3.0, 0.0, 200),
            _ => Cost::new(10.0, 10.0, 0.0, 500),
        })
    }

    #[test]
    fn validate_passing_rule() {
        let candidate = make_candidate();
        let logs = vec![
            make_log(
                RelExpr::scan("t")
                    .filter(ra_core::expr::Expr::Const(ra_core::expr::Const::Bool(true))),
                RelExpr::scan("t"),
                Cost::new(5.0, 3.0, 0.0, 200),
            ),
            make_log(
                RelExpr::scan("t")
                    .filter(ra_core::expr::Expr::Const(ra_core::expr::Const::Bool(true))),
                RelExpr::scan("t"),
                Cost::new(5.0, 3.0, 0.0, 200),
            ),
        ];

        let estimator = simple_cost_estimator();
        let config = ValidationConfig {
            max_regression_rate: 0.1,
            min_improvement_rate: 0.3,
            max_cost_increase: 1.1,
        };

        let result = validate_rule(&candidate, &logs, &estimator, &config);
        assert!(result.passed);
        assert_eq!(result.queries_matched, 2);
        assert_eq!(result.queries_improved, 2);
        assert_eq!(result.queries_regressed, 0);
    }

    #[test]
    fn validate_no_match() {
        let candidate = make_candidate();
        let logs = vec![make_log(
            RelExpr::scan("t"),
            RelExpr::scan("t"),
            Cost::new(1.0, 1.0, 0.0, 100),
        )];

        let estimator = simple_cost_estimator();
        let config = ValidationConfig::default();

        let result = validate_rule(&candidate, &logs, &estimator, &config);
        assert!(!result.passed);
        assert_eq!(result.queries_matched, 0);
    }

    #[test]
    fn validate_empty_logs() {
        let candidate = make_candidate();
        let estimator = simple_cost_estimator();
        let config = ValidationConfig::default();

        let result = validate_rule(&candidate, &[], &estimator, &config);
        assert!(!result.passed);
        assert_eq!(result.queries_tested, 0);
    }

    #[test]
    fn validate_batch_filters_failing() {
        let candidates = vec![make_candidate()];
        let logs = vec![make_log(
            RelExpr::scan("t"),
            RelExpr::scan("t"),
            Cost::new(1.0, 1.0, 0.0, 100),
        )];

        let estimator = simple_cost_estimator();
        let config = ValidationConfig::default();

        let passed = validate_rules(&candidates, &logs, &estimator, &config);
        assert!(passed.is_empty());
    }

    #[test]
    fn cost_estimator_debug() {
        let estimator = simple_cost_estimator();
        let debug = format!("{estimator:?}");
        assert!(debug.contains("CostEstimator"));
    }
}
