//! Automatic test case minimization for failure reproduction.
//!
//! When a property-based test finds a failing input, this module
//! reduces the expression to the smallest equivalent that still
//! triggers the failure. This is analogous to C-Reduce for compiler
//! testing.
//!
//! Reduction strategies:
//! - **Subtree replacement**: replace a subtree with a simpler one
//! - **Predicate simplification**: replace complex predicates with `TRUE`
//! - **Join elimination**: replace a join with one of its inputs
//! - **Operator removal**: remove filters, sorts, limits
//! - **Column pruning**: reduce projection lists

use ra_core::algebra::RelExpr;
use ra_core::expr::{Const, Expr};
use tracing::debug;

use crate::properties::{OptimizerProperty, PropertyValidator};

/// Result of minimization.
#[derive(Debug, Clone)]
pub struct MinimizationResult {
    /// The original failing expression.
    pub original: RelExpr,
    /// The minimized expression (smallest that still fails).
    pub minimized: RelExpr,
    /// Number of reduction steps taken.
    pub steps: usize,
    /// Which property failed.
    pub failed_property: OptimizerProperty,
}

/// Automatic test case minimizer.
///
/// Given an expression that fails a property check, iteratively
/// simplifies it while preserving the failure.
#[derive(Debug)]
pub struct TestMinimizer {
    /// Maximum reduction iterations before giving up.
    max_iterations: usize,
}

impl TestMinimizer {
    /// Create a minimizer with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            max_iterations: 100,
        }
    }

    /// Create a minimizer with custom iteration limit.
    #[must_use]
    pub fn with_max_iterations(max_iterations: usize) -> Self {
        Self { max_iterations }
    }

    /// Minimize an expression that fails the given property.
    ///
    /// Returns `None` if the original expression does not actually
    /// fail the property (nothing to minimize).
    #[must_use]
    pub fn minimize(
        &self,
        expr: &RelExpr,
        property: OptimizerProperty,
        validator: &PropertyValidator,
    ) -> Option<MinimizationResult> {
        // Verify the original actually fails
        if !self.still_fails(expr, property, validator) {
            return None;
        }

        let mut current = expr.clone();
        let mut steps = 0;

        for _ in 0..self.max_iterations {
            let candidates = self.generate_reductions(&current);
            if candidates.is_empty() {
                break;
            }

            let mut made_progress = false;
            for candidate in candidates {
                if self.still_fails(&candidate, property, validator) {
                    debug!(
                        "reduction step {steps}: \
                         simplified expression"
                    );
                    current = candidate;
                    steps += 1;
                    made_progress = true;
                    break;
                }
            }

            if !made_progress {
                break;
            }
        }

        Some(MinimizationResult {
            original: expr.clone(),
            minimized: current,
            steps,
            failed_property: property,
        })
    }

    #[expect(clippy::unused_self, reason = "self kept for consistency with other methods")]
    fn still_fails(
        &self,
        expr: &RelExpr,
        property: OptimizerProperty,
        validator: &PropertyValidator,
    ) -> bool {
        let results = validator.validate(expr);
        results
            .iter()
            .any(|r| r.property == property && !r.passed)
    }

    /// Generate candidate reductions of an expression.
    ///
    /// Each candidate is a simplified version that might still
    /// trigger the same failure.
    #[expect(
        clippy::self_only_used_in_recursion,
        reason = "self is needed for method dispatch in recursive calls"
    )]
    fn generate_reductions(&self, expr: &RelExpr) -> Vec<RelExpr> {
        let mut candidates = Vec::new();

        match expr {
            RelExpr::Filter { predicate, input } => {
                // Remove the filter entirely
                candidates.push(*input.clone());
                // Simplify predicate to TRUE
                candidates.push(RelExpr::Filter {
                    predicate: Expr::Const(Const::Bool(true)),
                    input: input.clone(),
                });
                // Recurse into input
                for reduced in self.generate_reductions(input) {
                    candidates.push(RelExpr::Filter {
                        predicate: predicate.clone(),
                        input: Box::new(reduced),
                    });
                }
            }
            RelExpr::Project { columns, input } => {
                // Remove the projection
                candidates.push(*input.clone());
                // Reduce to single column
                if columns.len() > 1 {
                    candidates.push(RelExpr::Project {
                        columns: vec![columns[0].clone()],
                        input: input.clone(),
                    });
                }
                // Recurse into input
                for reduced in self.generate_reductions(input) {
                    candidates.push(RelExpr::Project {
                        columns: columns.clone(),
                        input: Box::new(reduced),
                    });
                }
            }
            RelExpr::Join {
                join_type,
                condition,
                left,
                right,
            } => {
                // Replace join with left input
                candidates.push(*left.clone());
                // Replace join with right input
                candidates.push(*right.clone());
                // Simplify condition to TRUE
                candidates.push(RelExpr::Join {
                    join_type: *join_type,
                    condition: Expr::Const(Const::Bool(true)),
                    left: left.clone(),
                    right: right.clone(),
                });
                // Recurse into left
                for reduced in self.generate_reductions(left) {
                    candidates.push(RelExpr::Join {
                        join_type: *join_type,
                        condition: condition.clone(),
                        left: Box::new(reduced),
                        right: right.clone(),
                    });
                }
                // Recurse into right
                for reduced in self.generate_reductions(right) {
                    candidates.push(RelExpr::Join {
                        join_type: *join_type,
                        condition: condition.clone(),
                        left: left.clone(),
                        right: Box::new(reduced),
                    });
                }
            }
            RelExpr::Sort { input, .. } => {
                // Remove the sort
                candidates.push(*input.clone());
            }
            RelExpr::Limit { input, .. } => {
                // Remove the limit
                candidates.push(*input.clone());
            }
            RelExpr::Distinct { input } => {
                // Remove distinct
                candidates.push(*input.clone());
            }
            RelExpr::Aggregate { input, .. } => {
                // Replace aggregate with its input
                candidates.push(*input.clone());
            }
            RelExpr::Union { left, right, .. }
            | RelExpr::Intersect { left, right, .. }
            | RelExpr::Except { left, right, .. } => {
                // Replace with left side
                candidates.push(*left.clone());
                // Replace with right side
                candidates.push(*right.clone());
            }
            // Leaf nodes and other variants cannot be further reduced
            _ => {}
        }

        candidates
    }
}

impl Default for TestMinimizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Estimate the "size" of a relational expression tree (number of
/// nodes) for comparing minimization progress.
#[must_use]
pub fn expr_size(expr: &RelExpr) -> usize {
    match expr {
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Aggregate { input, .. } => 1 + expr_size(input),
        RelExpr::Join { left, right, .. }
        | RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            1 + expr_size(left) + expr_size(right)
        }
        RelExpr::CTE {
            body, definition, ..
        } => 1 + expr_size(body) + expr_size(definition),
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::JoinType;
    use ra_core::expr::{BinOp, ColumnRef};

    fn scan(name: &str) -> RelExpr {
        RelExpr::Scan {
            table: name.to_owned(),
            alias: None,
        }
    }

    #[test]
    fn generate_reductions_for_filter() {
        let expr = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("x"))),
                right: Box::new(Expr::Const(Const::Int(5))),
            },
            input: Box::new(scan("t")),
        };
        let minimizer = TestMinimizer::new();
        let reductions = minimizer.generate_reductions(&expr);
        assert!(
            !reductions.is_empty(),
            "filter should have reductions"
        );
        // First reduction should be the input (filter removed)
        assert!(
            matches!(reductions[0], RelExpr::Scan { .. }),
            "first reduction should remove filter"
        );
    }

    #[test]
    fn generate_reductions_for_join() {
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
        };
        let minimizer = TestMinimizer::new();
        let reductions = minimizer.generate_reductions(&expr);
        assert!(reductions.len() >= 2, "join should reduce to left or right");
    }

    #[test]
    fn scan_has_no_reductions() {
        let minimizer = TestMinimizer::new();
        let reductions = minimizer.generate_reductions(&scan("t"));
        assert!(reductions.is_empty(), "scan is a leaf");
    }

    #[test]
    fn expr_size_counts_nodes() {
        let expr = RelExpr::Filter {
            predicate: Expr::Const(Const::Bool(true)),
            input: Box::new(RelExpr::Join {
                join_type: JoinType::Inner,
                condition: Expr::Const(Const::Bool(true)),
                left: Box::new(scan("a")),
                right: Box::new(scan("b")),
            }),
        };
        assert_eq!(expr_size(&expr), 4); // filter + join + 2 scans
    }

    #[test]
    fn minimize_returns_none_when_no_failure() {
        let validator = PropertyValidator::new(vec![
            OptimizerProperty::RuleSafety,
        ]);
        let minimizer = TestMinimizer::new();
        let result = minimizer.minimize(
            &scan("t"),
            OptimizerProperty::RuleSafety,
            &validator,
        );
        assert!(
            result.is_none(),
            "should return None when property passes"
        );
    }
}
