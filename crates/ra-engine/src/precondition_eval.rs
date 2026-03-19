//! Pre-condition evaluation for optimization rules.
//!
//! This module implements runtime evaluation of rule pre-conditions against
//! available system facts. It determines whether a rule is applicable based
//! on pattern matching, predicate checks, and fact lookups.

use ra_core::{
    EvaluationResult, FactValue, FactsProvider, LogicalOperator, PreCondition,
};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, warn};

/// Errors that can occur during pre-condition evaluation
#[derive(Debug, Error)]
pub enum EvaluationError {
    /// Fact lookup failed
    #[error("Fact lookup failed for {fact_type}: {reason}")]
    FactLookupFailed {
        /// The fact type that failed
        fact_type: String,
        /// Reason for failure
        reason: String,
    },

    /// Predicate evaluation failed
    #[error("Predicate evaluation failed: {0}")]
    PredicateError(String),

    /// Comparison failed
    #[error("Comparison failed: {0}")]
    ComparisonError(String),

    /// Unknown fact type
    #[error("Unknown fact type: {0}")]
    UnknownFactType(String),
}

/// Evaluates pre-conditions against system facts
pub struct PreConditionEvaluator {
    facts: Arc<dyn FactsProvider>,
}

impl PreConditionEvaluator {
    /// Create a new evaluator with the given facts provider
    pub fn new(facts: Arc<dyn FactsProvider>) -> Self {
        Self { facts }
    }

    /// Evaluate a list of pre-conditions
    ///
    /// All pre-conditions must be satisfied (AND semantics) unless marked as optional.
    /// Optional pre-conditions that fail are logged but don't prevent rule application.
    pub fn evaluate(&self, preconditions: &[PreCondition]) -> EvaluationResult {
        if preconditions.is_empty() {
            return EvaluationResult::Satisfied;
        }

        for precond in preconditions {
            match self.evaluate_single(precond) {
                Ok(true) => continue,
                Ok(false) => {
                    if self.is_optional(precond) {
                        debug!("Optional precondition not satisfied: {:?}", precond);
                        continue;
                    }
                    return EvaluationResult::NotSatisfied {
                        condition: precond.clone(),
                        reason: "Condition evaluated to false".to_string(),
                    };
                }
                Err(e) => {
                    if self.is_optional(precond) {
                        warn!("Optional precondition error (ignored): {}", e);
                        continue;
                    }
                    return EvaluationResult::Error {
                        condition: precond.clone(),
                        error: e.to_string(),
                    };
                }
            }
        }

        EvaluationResult::Satisfied
    }

    /// Evaluate a single pre-condition
    fn evaluate_single(&self, precond: &PreCondition) -> Result<bool, EvaluationError> {
        match precond {
            PreCondition::Pattern { .. } => {
                // Pattern matching is handled by the egg rewrite system
                // By the time we're evaluating pre-conditions, patterns have already matched
                Ok(true)
            }

            PreCondition::Predicate { condition, .. } => self.evaluate_predicate(condition),

            PreCondition::Fact {
                fact_type,
                table,
                column,
                comparator,
                threshold,
                ..
            } => self.evaluate_fact(fact_type, table.as_deref(), column.as_deref(), comparator, threshold),

            PreCondition::Capability {
                database,
                requires,
                ..
            } => self.evaluate_capability(database, requires),

            PreCondition::Composite {
                operator,
                conditions,
                ..
            } => self.evaluate_composite(*operator, conditions),
        }
    }

    /// Check if a pre-condition is marked as optional
    fn is_optional(&self, precond: &PreCondition) -> bool {
        match precond {
            PreCondition::Pattern { optional, .. }
            | PreCondition::Predicate { optional, .. }
            | PreCondition::Fact { optional, .. }
            | PreCondition::Capability { optional, .. }
            | PreCondition::Composite { optional, .. } => *optional,
        }
    }

    /// Evaluate a predicate condition
    fn evaluate_predicate(&self, condition: &str) -> Result<bool, EvaluationError> {
        // For now, we'll recognize common predicate patterns
        // In a full implementation, this would call registered predicate functions

        if condition.contains("is_deterministic") {
            // Assume deterministic by default (would need actual implementation)
            Ok(true)
        } else if condition.contains("references_only") {
            // Assume references check passes (would need actual implementation)
            Ok(true)
        } else if condition.contains("references_both_sides") {
            // Assume references check passes (would need actual implementation)
            Ok(true)
        } else {
            // Unknown predicate - for now, assume it passes
            // In production, this should be an error or look up the predicate function
            warn!("Unknown predicate: {}", condition);
            Ok(true)
        }
    }

    /// Evaluate a fact check
    fn evaluate_fact(
        &self,
        fact_type: &str,
        table: Option<&str>,
        column: Option<&str>,
        comparator: &str,
        threshold: &FactValue,
    ) -> Result<bool, EvaluationError> {
        let actual_value = self.lookup_fact(fact_type, table, column)?;

        actual_value.compare(comparator, threshold).map_err(|e| {
            EvaluationError::ComparisonError(e)
        })
    }

    /// Look up a fact value from the facts provider
    fn lookup_fact(
        &self,
        fact_type: &str,
        table: Option<&str>,
        column: Option<&str>,
    ) -> Result<FactValue, EvaluationError> {
        match fact_type {
            // Statistics facts
            "statistics.cardinality" => {
                let table = table.ok_or_else(|| EvaluationError::FactLookupFailed {
                    fact_type: fact_type.to_string(),
                    reason: "Table name required for cardinality".to_string(),
                })?;

                self.facts
                    .get_table_stats(table)
                    .map(|stats| FactValue::Float(stats.row_count))
                    .ok_or_else(|| EvaluationError::FactLookupFailed {
                        fact_type: fact_type.to_string(),
                        reason: format!("No statistics for table {}", table),
                    })
            }

            "statistics.ndv" => {
                let table = table.ok_or_else(|| EvaluationError::FactLookupFailed {
                    fact_type: fact_type.to_string(),
                    reason: "Table name required for NDV".to_string(),
                })?;
                let col = column.ok_or_else(|| EvaluationError::FactLookupFailed {
                    fact_type: fact_type.to_string(),
                    reason: "Column name required for NDV".to_string(),
                })?;

                self.facts
                    .get_column_stats(table, col)
                    .map(|stats| FactValue::Float(stats.distinct_count))
                    .ok_or_else(|| EvaluationError::FactLookupFailed {
                        fact_type: fact_type.to_string(),
                        reason: format!("No column stats for {}.{}", table, col),
                    })
            }

            "statistics.null_fraction" => {
                let table = table.ok_or_else(|| EvaluationError::FactLookupFailed {
                    fact_type: fact_type.to_string(),
                    reason: "Table name required".to_string(),
                })?;
                let col = column.ok_or_else(|| EvaluationError::FactLookupFailed {
                    fact_type: fact_type.to_string(),
                    reason: "Column name required".to_string(),
                })?;

                self.facts
                    .get_column_stats(table, col)
                    .map(|stats| FactValue::Float(stats.null_fraction))
                    .ok_or_else(|| EvaluationError::FactLookupFailed {
                        fact_type: fact_type.to_string(),
                        reason: format!("No column stats for {}.{}", table, col),
                    })
            }

            // Hardware facts
            "hardware.memory" => Ok(FactValue::Int(self.facts.available_memory() as i64)),
            "hardware.cpu_cores" => Ok(FactValue::Int(i64::from(self.facts.cpu_cores()))),
            "hardware.simd_width" => Ok(FactValue::Int(i64::from(self.facts.simd_width()))),
            "hardware.has_gpu" => Ok(FactValue::Bool(self.facts.has_gpu())),
            "hardware.cache_size" => {
                let hw = self.facts.hardware_profile();
                Ok(FactValue::Int(hw.l3_cache_size as i64))
            }

            // Schema facts
            "schema.column_type" => {
                let table = table.ok_or_else(|| EvaluationError::FactLookupFailed {
                    fact_type: fact_type.to_string(),
                    reason: "Table name required".to_string(),
                })?;
                let col = column.ok_or_else(|| EvaluationError::FactLookupFailed {
                    fact_type: fact_type.to_string(),
                    reason: "Column name required".to_string(),
                })?;

                self.facts
                    .column_type(table, col)
                    .map(|dt| FactValue::String(dt.to_string()))
                    .ok_or_else(|| EvaluationError::FactLookupFailed {
                        fact_type: fact_type.to_string(),
                        reason: format!("Unknown column {}.{}", table, col),
                    })
            }

            "schema.has_primary_key" => {
                let table = table.ok_or_else(|| EvaluationError::FactLookupFailed {
                    fact_type: fact_type.to_string(),
                    reason: "Table name required".to_string(),
                })?;

                Ok(FactValue::Bool(self.facts.has_primary_key(table)))
            }

            "schema.table_size" => {
                let table = table.ok_or_else(|| EvaluationError::FactLookupFailed {
                    fact_type: fact_type.to_string(),
                    reason: "Table name required".to_string(),
                })?;

                self.facts
                    .get_table_stats(table)
                    .map(|stats| FactValue::Int(stats.table_size_bytes as i64))
                    .ok_or_else(|| EvaluationError::FactLookupFailed {
                        fact_type: fact_type.to_string(),
                        reason: format!("No statistics for table {}", table),
                    })
            }

            // Runtime facts
            "runtime.cardinality_error" => {
                let table = table.ok_or_else(|| EvaluationError::FactLookupFailed {
                    fact_type: fact_type.to_string(),
                    reason: "Operator ID required".to_string(),
                })?;

                self.facts
                    .cardinality_error(table)
                    .map(FactValue::Float)
                    .ok_or_else(|| EvaluationError::FactLookupFailed {
                        fact_type: fact_type.to_string(),
                        reason: format!("No runtime stats for operator {}", table),
                    })
            }

            "runtime.skew_detected" => {
                let table = table.ok_or_else(|| EvaluationError::FactLookupFailed {
                    fact_type: fact_type.to_string(),
                    reason: "Operator ID required".to_string(),
                })?;

                self.facts
                    .runtime_stats(table)
                    .map(|stats| FactValue::Bool(stats.skew_detected))
                    .ok_or_else(|| EvaluationError::FactLookupFailed {
                        fact_type: fact_type.to_string(),
                        reason: format!("No runtime stats for operator {}", table),
                    })
            }

            // Database facts
            "database.dialect" => {
                Ok(FactValue::String(self.facts.sql_dialect().to_string()))
            }

            _ => Err(EvaluationError::UnknownFactType(fact_type.to_string())),
        }
    }

    /// Evaluate a capability requirement
    fn evaluate_capability(&self, database: &str, feature: &str) -> Result<bool, EvaluationError> {
        if database == "current" || database == self.facts.database_name() {
            Ok(self.facts.supports_feature(feature))
        } else {
            Ok(false)
        }
    }

    /// Evaluate a composite condition
    fn evaluate_composite(
        &self,
        operator: LogicalOperator,
        conditions: &[PreCondition],
    ) -> Result<bool, EvaluationError> {
        match operator {
            LogicalOperator::And => {
                for cond in conditions {
                    if !self.evaluate_single(cond)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }

            LogicalOperator::Or => {
                for cond in conditions {
                    if self.evaluate_single(cond)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }

            LogicalOperator::Not => {
                if conditions.len() != 1 {
                    return Err(EvaluationError::PredicateError(
                        "NOT operator requires exactly one condition".to_string(),
                    ));
                }
                Ok(!self.evaluate_single(&conditions[0])?)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::{EmptyFactsProvider, PreConditionBuilder};

    #[test]
    fn empty_preconditions_satisfied() {
        let facts = Arc::new(EmptyFactsProvider::new());
        let evaluator = PreConditionEvaluator::new(facts);

        let result = evaluator.evaluate(&[]);
        assert!(result.is_satisfied());
    }

    #[test]
    fn pattern_precondition_always_passes() {
        let facts = Arc::new(EmptyFactsProvider::new());
        let evaluator = PreConditionEvaluator::new(facts);

        let preconditions = PreConditionBuilder::new()
            .pattern("(filter ?pred (join inner ?cond ?left ?right))")
            .build();

        let result = evaluator.evaluate(&preconditions);
        assert!(result.is_satisfied());
    }

    #[test]
    fn hardware_fact_check() {
        let facts = Arc::new(EmptyFactsProvider::new());
        let evaluator = PreConditionEvaluator::new(facts.clone());

        let preconditions = vec![PreCondition::Fact {
            fact_type: "hardware.cpu_cores".to_string(),
            table: None,
            column: None,
            comparator: ">".to_string(),
            threshold: FactValue::Int(4),
            confidence: None,
            description: None,
            optional: false,
        }];

        let result = evaluator.evaluate(&preconditions);
        // EmptyFactsProvider has 8 cores, so this should pass
        assert!(result.is_satisfied());
    }

    #[test]
    fn missing_fact_with_optional() {
        let facts = Arc::new(EmptyFactsProvider::new());
        let evaluator = PreConditionEvaluator::new(facts);

        let preconditions = vec![PreCondition::Fact {
            fact_type: "statistics.cardinality".to_string(),
            table: Some("nonexistent".to_string()),
            column: None,
            comparator: ">".to_string(),
            threshold: FactValue::Int(1000),
            confidence: None,
            description: None,
            optional: true, // Optional, so should pass
        }];

        let result = evaluator.evaluate(&preconditions);
        assert!(result.is_satisfied());
    }

    #[test]
    fn composite_and_condition() {
        let facts = Arc::new(EmptyFactsProvider::new());
        let evaluator = PreConditionEvaluator::new(facts);

        let preconditions = vec![PreCondition::Composite {
            operator: LogicalOperator::And,
            conditions: vec![
                PreCondition::Fact {
                    fact_type: "hardware.cpu_cores".to_string(),
                    table: None,
                    column: None,
                    comparator: ">".to_string(),
                    threshold: FactValue::Int(4),
                    confidence: None,
                    description: None,
                    optional: false,
                },
                PreCondition::Fact {
                    fact_type: "hardware.simd_width".to_string(),
                    table: None,
                    column: None,
                    comparator: ">=".to_string(),
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
}
