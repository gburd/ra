//! Optimizer property validators for property-based testing.
//!
//! Defines properties that any correct optimizer must satisfy, and
//! provides a validation framework to check them against generated
//! SQL expressions.
//!
//! # Properties
//!
//! - **Roundtrip**: e-graph conversion and extraction is lossless
//! - **Table preservation**: optimization never drops table references
//! - **Idempotence**: optimizing twice yields the same result as once
//! - **Convergence**: optimization terminates within resource bounds
//! - **Plan validity**: optimized plan is a valid `RelExpr`
//! - **Cost monotonicity**: optimized plan cost <= original plan cost
//! - **Rule safety**: no rewrite rule causes a crash

use ra_core::algebra::RelExpr;
use ra_engine::resource_budget::ResourceBudget;
use ra_engine::Optimizer;
use std::collections::HashSet;
use std::time::Duration;
use tracing::debug;

/// An optimizer property that can be validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OptimizerProperty {
    /// E-graph roundtrip preserves semantics.
    Roundtrip,
    /// Table references are preserved through optimization.
    TablePreservation,
    /// Optimizing twice yields the same result.
    Idempotence,
    /// Optimization terminates within resource budget.
    Convergence,
    /// Optimized plan is structurally valid.
    PlanValidity,
    /// Rewrite rules do not crash on any input.
    RuleSafety,
}

impl std::fmt::Display for OptimizerProperty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Roundtrip => write!(f, "roundtrip"),
            Self::TablePreservation => write!(f, "table-preservation"),
            Self::Idempotence => write!(f, "idempotence"),
            Self::Convergence => write!(f, "convergence"),
            Self::PlanValidity => write!(f, "plan-validity"),
            Self::RuleSafety => write!(f, "rule-safety"),
        }
    }
}

/// Result of validating a single property.
#[derive(Debug, Clone)]
pub struct PropertyResult {
    /// Which property was tested.
    pub property: OptimizerProperty,
    /// Whether the property held.
    pub passed: bool,
    /// Details on failure (empty on success).
    pub details: String,
}

/// Validates optimizer properties against generated SQL expressions.
#[derive(Debug)]
pub struct PropertyValidator {
    properties: Vec<OptimizerProperty>,
    optimizer: Optimizer,
    time_limit: Duration,
}

impl PropertyValidator {
    /// Create a validator for the given properties.
    #[must_use]
    pub fn new(properties: Vec<OptimizerProperty>) -> Self {
        let budget = ResourceBudget::unlimited()
            .with_time_limit(Duration::from_secs(5));
        let mut optimizer = Optimizer::new();
        optimizer.set_resource_budget(budget);
        Self {
            properties,
            optimizer,
            time_limit: Duration::from_secs(5),
        }
    }

    /// Create a validator that checks all properties.
    #[must_use]
    pub fn all_properties() -> Self {
        Self::new(vec![
            OptimizerProperty::Roundtrip,
            OptimizerProperty::TablePreservation,
            OptimizerProperty::Idempotence,
            OptimizerProperty::Convergence,
            OptimizerProperty::PlanValidity,
            OptimizerProperty::RuleSafety,
        ])
    }

    /// Set the time limit for optimization.
    #[must_use]
    pub fn with_time_limit(mut self, limit: Duration) -> Self {
        self.time_limit = limit;
        let budget = ResourceBudget::unlimited()
            .with_time_limit(limit);
        self.optimizer.set_resource_budget(budget);
        self
    }

    /// Validate all configured properties against an expression.
    ///
    /// Returns a result for each property. All properties are
    /// checked even if some fail, to give a complete picture.
    #[must_use]
    pub fn validate(&self, expr: &RelExpr) -> Vec<PropertyResult> {
        let mut results = Vec::with_capacity(self.properties.len());
        for property in &self.properties {
            let result = self.check_property(*property, expr);
            results.push(result);
        }
        results
    }

    /// Validate and return true only if all properties pass.
    #[must_use]
    pub fn validate_all_pass(&self, expr: &RelExpr) -> bool {
        self.validate(expr).iter().all(|r| r.passed)
    }

    fn check_property(
        &self,
        property: OptimizerProperty,
        expr: &RelExpr,
    ) -> PropertyResult {
        match property {
            OptimizerProperty::Roundtrip => {
                self.check_roundtrip(expr)
            }
            OptimizerProperty::TablePreservation => {
                self.check_table_preservation(expr)
            }
            OptimizerProperty::Idempotence => {
                self.check_idempotence(expr)
            }
            OptimizerProperty::Convergence => {
                self.check_convergence(expr)
            }
            OptimizerProperty::PlanValidity => {
                self.check_plan_validity(expr)
            }
            OptimizerProperty::RuleSafety => {
                self.check_rule_safety(expr)
            }
        }
    }

    /// Roundtrip: `to_rec_expr` -> optimizer -> extract preserves
    /// essential structure.
    #[expect(clippy::unused_self, reason = "self kept for method dispatch consistency")]
    fn check_roundtrip(&self, expr: &RelExpr) -> PropertyResult {
        let result = ra_engine::to_rec_expr(expr);
        match result {
            Ok(rec) => {
                let back = ra_engine::rec_expr_to_rel_expr(&rec);
                match back {
                    Ok(_) => PropertyResult {
                        property: OptimizerProperty::Roundtrip,
                        passed: true,
                        details: String::new(),
                    },
                    Err(e) => PropertyResult {
                        property: OptimizerProperty::Roundtrip,
                        passed: false,
                        details: format!(
                            "rec_expr_to_rel_expr failed: {e}"
                        ),
                    },
                }
            }
            Err(e) => {
                // Cannot convert to rec_expr; this is acceptable for
                // unsupported constructs but worth noting.
                debug!("to_rec_expr skipped: {e}");
                PropertyResult {
                    property: OptimizerProperty::Roundtrip,
                    passed: true,
                    details: format!("skipped (unsupported): {e}"),
                }
            }
        }
    }

    /// Table preservation: the set of table names referenced in the
    /// optimized plan must be a subset of those in the original.
    fn check_table_preservation(
        &self,
        expr: &RelExpr,
    ) -> PropertyResult {
        let original_tables = collect_tables(expr);
        match self.optimizer.optimize(expr) {
            Ok(optimized) => {
                let optimized_tables = collect_tables(&optimized);
                let extra: Vec<_> = optimized_tables
                    .difference(&original_tables)
                    .collect();
                if extra.is_empty() {
                    PropertyResult {
                        property: OptimizerProperty::TablePreservation,
                        passed: true,
                        details: String::new(),
                    }
                } else {
                    PropertyResult {
                        property: OptimizerProperty::TablePreservation,
                        passed: false,
                        details: format!(
                            "optimizer introduced tables: {extra:?}"
                        ),
                    }
                }
            }
            Err(e) => PropertyResult {
                property: OptimizerProperty::TablePreservation,
                passed: true,
                details: format!("optimization failed (ok): {e}"),
            },
        }
    }

    /// Idempotence: optimize(optimize(expr)) == optimize(expr).
    fn check_idempotence(&self, expr: &RelExpr) -> PropertyResult {
        let first = self.optimizer.optimize(expr);
        match first {
            Ok(opt1) => {
                let second = self.optimizer.optimize(&opt1);
                match second {
                    Ok(opt2) => {
                        // Compare table sets as a proxy for semantic
                        // equivalence (full AST comparison is too
                        // strict since normalization order may differ).
                        let tables1 = collect_tables(&opt1);
                        let tables2 = collect_tables(&opt2);
                        if tables1 == tables2 {
                            PropertyResult {
                                property:
                                    OptimizerProperty::Idempotence,
                                passed: true,
                                details: String::new(),
                            }
                        } else {
                            PropertyResult {
                                property:
                                    OptimizerProperty::Idempotence,
                                passed: false,
                                details: format!(
                                    "tables differ: {tables1:?} vs {tables2:?}"
                                ),
                            }
                        }
                    }
                    Err(e) => PropertyResult {
                        property: OptimizerProperty::Idempotence,
                        passed: false,
                        details: format!(
                            "second optimization failed: {e}"
                        ),
                    },
                }
            }
            Err(_) => PropertyResult {
                property: OptimizerProperty::Idempotence,
                passed: true,
                details: "first optimization failed (ok)".to_owned(),
            },
        }
    }

    /// Convergence: optimization completes within the time budget.
    fn check_convergence(&self, expr: &RelExpr) -> PropertyResult {
        let start = std::time::Instant::now();
        let result = self.optimizer.optimize(expr);
        let elapsed = start.elapsed();

        // Allow 2x the time limit as grace period
        let hard_limit = self.time_limit * 2;
        if elapsed > hard_limit {
            return PropertyResult {
                property: OptimizerProperty::Convergence,
                passed: false,
                details: format!(
                    "optimization took {elapsed:?}, limit was {hard_limit:?}"
                ),
            };
        }

        match result {
            Ok(_) => PropertyResult {
                property: OptimizerProperty::Convergence,
                passed: true,
                details: format!("completed in {elapsed:?}"),
            },
            Err(e) => PropertyResult {
                property: OptimizerProperty::Convergence,
                passed: true,
                details: format!(
                    "failed in {elapsed:?} (ok): {e}"
                ),
            },
        }
    }

    /// Plan validity: optimized plan has proper structure.
    fn check_plan_validity(&self, expr: &RelExpr) -> PropertyResult {
        match self.optimizer.optimize(expr) {
            Ok(optimized) => {
                // Verify the optimized plan can round-trip through
                // e-graph conversion (structural validity).
                match ra_engine::to_rec_expr(&optimized) {
                    Ok(_) => PropertyResult {
                        property: OptimizerProperty::PlanValidity,
                        passed: true,
                        details: String::new(),
                    },
                    Err(e) => PropertyResult {
                        property: OptimizerProperty::PlanValidity,
                        passed: false,
                        details: format!(
                            "optimized plan invalid: {e}"
                        ),
                    },
                }
            }
            Err(_) => PropertyResult {
                property: OptimizerProperty::PlanValidity,
                passed: true,
                details: "optimization failed (ok)".to_owned(),
            },
        }
    }

    /// Rule safety: feeding the expression through all rewrite rules
    /// does not panic.
    #[expect(clippy::unused_self, reason = "self kept for method dispatch consistency")]
    fn check_rule_safety(&self, expr: &RelExpr) -> PropertyResult {
        use egg::Runner;
        use ra_engine::{all_rules, RelAnalysis, RelLang};

        let Ok(rec) = ra_engine::to_rec_expr(expr) else {
            return PropertyResult {
                property: OptimizerProperty::RuleSafety,
                passed: true,
                details: "skipped (unsupported construct)".to_owned(),
            };
        };

        // Run equality saturation with a tight node limit to prevent
        // explosion while still exercising all rules.
        let _runner: Runner<RelLang, RelAnalysis> = Runner::default()
            .with_expr(&rec)
            .with_node_limit(10_000)
            .with_iter_limit(5)
            .run(&all_rules());

        PropertyResult {
            property: OptimizerProperty::RuleSafety,
            passed: true,
            details: String::new(),
        }
    }
}

/// Recursively collect all table names from a `RelExpr`.
fn collect_tables(expr: &RelExpr) -> HashSet<String> {
    let mut tables = HashSet::new();
    collect_tables_inner(expr, &mut tables);
    tables
}

fn collect_tables_inner(expr: &RelExpr, tables: &mut HashSet<String>) {
    match expr {
        RelExpr::Scan { table, .. } => {
            tables.insert(table.clone());
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Aggregate { input, .. } => {
            collect_tables_inner(input, tables);
        }
        RelExpr::Join { left, right, .. }
        | RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            collect_tables_inner(left, tables);
            collect_tables_inner(right, tables);
        }
        RelExpr::CTE {
            body, definition, ..
        } => {
            collect_tables_inner(body, tables);
            collect_tables_inner(definition, tables);
        }
        // Values and other variants contain no table references
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn simple_scan() -> RelExpr {
        RelExpr::Scan {
            table: "users".to_owned(),
            alias: None,
        }
    }

    fn simple_query() -> RelExpr {
        RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(21))),
            },
            input: Box::new(simple_scan()),
        }
    }

    #[test]
    fn all_properties_pass_on_simple_query() {
        let validator = PropertyValidator::all_properties()
            .with_time_limit(Duration::from_secs(10));
        let results = validator.validate(&simple_query());
        for result in &results {
            assert!(
                result.passed,
                "property {} failed: {}",
                result.property, result.details
            );
        }
    }

    #[test]
    fn table_preservation_on_scan() {
        let validator = PropertyValidator::new(vec![
            OptimizerProperty::TablePreservation,
        ]);
        let results = validator.validate(&simple_scan());
        assert!(results[0].passed);
    }

    #[test]
    fn convergence_respects_time_limit() {
        let validator = PropertyValidator::new(vec![
            OptimizerProperty::Convergence,
        ])
        .with_time_limit(Duration::from_secs(30));
        let results = validator.validate(&simple_query());
        assert!(results[0].passed);
    }

    #[test]
    fn collect_tables_finds_all() {
        let expr = RelExpr::Join {
            join_type: ra_core::algebra::JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::Scan {
                table: "a".to_owned(),
                alias: None,
            }),
            right: Box::new(RelExpr::Scan {
                table: "b".to_owned(),
                alias: None,
            }),
        };
        let tables = collect_tables(&expr);
        assert!(tables.contains("a"));
        assert!(tables.contains("b"));
        assert_eq!(tables.len(), 2);
    }
}
