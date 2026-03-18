//! Pattern matching types for relational expressions.
//!
//! Patterns describe the shape of a relational expression tree that
//! a rule wants to match. They support wildcards (via [`PatternVar`])
//! and structural matching against the [`RelExpr`] variants.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::algebra::{JoinType, RelExpr};
use crate::expr::Expr;

/// A pattern for matching against [`RelExpr`] trees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Pattern {
    /// Match any expression and optionally bind it to a variable.
    Wildcard(PatternVar),

    /// Match a Scan with an optional table name constraint.
    Scan {
        /// If set, only match scans of this table.
        table: Option<String>,
    },

    /// Match a Filter node.
    Filter {
        /// Pattern for the predicate (if constrained).
        predicate: Option<Box<ExprPattern>>,
        /// Pattern for the input.
        input: Box<Pattern>,
    },

    /// Match a Project node.
    Project {
        /// Pattern for the input.
        input: Box<Pattern>,
    },

    /// Match a Join node.
    Join {
        /// If set, only match this join type.
        join_type: Option<JoinType>,
        /// Pattern for the left input.
        left: Box<Pattern>,
        /// Pattern for the right input.
        right: Box<Pattern>,
    },

    /// Match an Aggregate node.
    Aggregate {
        /// Pattern for the input.
        input: Box<Pattern>,
    },

    /// Match a Sort node.
    Sort {
        /// Pattern for the input.
        input: Box<Pattern>,
    },

    /// Match a Limit node.
    Limit {
        /// Pattern for the input.
        input: Box<Pattern>,
    },

    /// Match a Union node.
    Union {
        /// Pattern for the left input.
        left: Box<Pattern>,
        /// Pattern for the right input.
        right: Box<Pattern>,
    },

    /// Match an Intersect node.
    Intersect {
        /// Pattern for the left input.
        left: Box<Pattern>,
        /// Pattern for the right input.
        right: Box<Pattern>,
    },

    /// Match an Except node.
    Except {
        /// Pattern for the left input.
        left: Box<Pattern>,
        /// Pattern for the right input.
        right: Box<Pattern>,
    },

    /// Match a CTE node.
    CTE {
        /// Pattern for the definition.
        definition: Box<Pattern>,
        /// Pattern for the body.
        body: Box<Pattern>,
    },

    /// Match a Window node.
    Window {
        /// Pattern for the input.
        input: Box<Pattern>,
    },

    /// Match a Distinct node.
    Distinct {
        /// Pattern for the input.
        input: Box<Pattern>,
    },
}

/// A pattern for matching scalar expressions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExprPattern {
    /// Match any expression.
    Any,
    /// Match a specific constant.
    Const(crate::expr::Const),
    /// Match a column reference.
    Column,
}

/// A named wildcard variable used in pattern matching.
///
/// When a pattern matches, the matched sub-expression is bound
/// to the variable name so it can be referenced in the
/// transformation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PatternVar {
    /// The variable name.
    pub name: String,
}

impl PatternVar {
    /// Create a new pattern variable.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

/// A set of bindings from pattern variables to matched expressions.
#[derive(Debug, Clone, Default)]
pub struct Bindings {
    /// Map from variable name to the matched relational expression.
    pub rel_bindings: HashMap<String, RelExpr>,
    /// Map from variable name to the matched scalar expression.
    pub expr_bindings: HashMap<String, Expr>,
}

impl Bindings {
    /// Look up a relational expression by variable name.
    #[must_use]
    pub fn get_rel(&self, name: &str) -> Option<&RelExpr> {
        self.rel_bindings.get(name)
    }

    /// Look up a scalar expression by variable name.
    #[must_use]
    pub fn get_expr(&self, name: &str) -> Option<&Expr> {
        self.expr_bindings.get(name)
    }

    /// Bind a relational expression to a variable name.
    pub fn bind_rel(
        &mut self,
        name: impl Into<String>,
        expr: RelExpr,
    ) {
        self.rel_bindings.insert(name.into(), expr);
    }

    /// Bind a scalar expression to a variable name.
    pub fn bind_expr(
        &mut self,
        name: impl Into<String>,
        expr: Expr,
    ) {
        self.expr_bindings.insert(name.into(), expr);
    }
}

impl Pattern {
    /// Create a wildcard pattern bound to a variable.
    #[must_use]
    pub fn wildcard(name: impl Into<String>) -> Self {
        Self::Wildcard(PatternVar::new(name))
    }

    /// Attempt to match this pattern against a relational expression.
    ///
    /// Returns `Some(bindings)` if the match succeeds, `None` otherwise.
    #[must_use]
    pub fn match_expr(&self, expr: &RelExpr) -> Option<Bindings> {
        let mut bindings = Bindings::default();
        if self.do_match(expr, &mut bindings) {
            Some(bindings)
        } else {
            None
        }
    }

    fn do_match(
        &self,
        expr: &RelExpr,
        bindings: &mut Bindings,
    ) -> bool {
        match (self, expr) {
            (Self::Wildcard(var), _) => {
                bindings.bind_rel(&var.name, expr.clone());
                true
            }
            (
                Self::Scan { table: pat_table },
                RelExpr::Scan { table, .. },
            ) => {
                pat_table.as_ref().map_or(true, |pt| pt == table)
            }
            (
                Self::Filter {
                    predicate: _,
                    input: pat_input,
                },
                RelExpr::Filter { input, .. },
            )
            | (
                Self::Project {
                    input: pat_input,
                },
                RelExpr::Project { input, .. },
            )
            | (
                Self::Aggregate {
                    input: pat_input,
                },
                RelExpr::Aggregate { input, .. },
            )
            | (
                Self::Sort {
                    input: pat_input,
                },
                RelExpr::Sort { input, .. },
            )
            | (
                Self::Limit {
                    input: pat_input,
                },
                RelExpr::Limit { input, .. },
            )
            | (
                Self::Window {
                    input: pat_input,
                },
                RelExpr::Window { input, .. },
            )
            | (
                Self::Distinct {
                    input: pat_input,
                },
                RelExpr::Distinct { input, .. },
            ) => pat_input.do_match(input, bindings),
            (
                Self::Join {
                    join_type: pat_jt,
                    left: pat_left,
                    right: pat_right,
                },
                RelExpr::Join {
                    join_type,
                    left,
                    right,
                    ..
                },
            ) => {
                if let Some(pjt) = pat_jt {
                    if pjt != join_type {
                        return false;
                    }
                }
                pat_left.do_match(left, bindings)
                    && pat_right.do_match(right, bindings)
            }
            (
                Self::Union {
                    left: pat_left,
                    right: pat_right,
                },
                RelExpr::Union { left, right, .. },
            )
            | (
                Self::Intersect {
                    left: pat_left,
                    right: pat_right,
                },
                RelExpr::Intersect { left, right, .. },
            )
            | (
                Self::Except {
                    left: pat_left,
                    right: pat_right,
                },
                RelExpr::Except { left, right, .. },
            ) => {
                pat_left.do_match(left, bindings)
                    && pat_right.do_match(right, bindings)
            }
            (
                Self::CTE {
                    definition: pat_def,
                    body: pat_body,
                },
                RelExpr::CTE {
                    definition, body, ..
                },
            ) => {
                pat_def.do_match(definition, bindings)
                    && pat_body.do_match(body, bindings)
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algebra::JoinType;
    use crate::expr::{BinOp, ColumnRef, Const, Expr};

    #[test]
    fn wildcard_matches_anything() {
        let pattern = Pattern::wildcard("x");
        let expr = RelExpr::scan("t");
        let bindings = pattern.match_expr(&expr);
        assert!(bindings.is_some());
        let b = bindings.expect("already checked");
        assert_eq!(b.get_rel("x"), Some(&RelExpr::scan("t")));
    }

    #[test]
    fn scan_pattern_matches_table() {
        let pattern = Pattern::Scan {
            table: Some("users".into()),
        };
        let expr = RelExpr::scan("users");
        assert!(pattern.match_expr(&expr).is_some());
    }

    #[test]
    fn scan_pattern_rejects_wrong_table() {
        let pattern = Pattern::Scan {
            table: Some("users".into()),
        };
        let expr = RelExpr::scan("orders");
        assert!(pattern.match_expr(&expr).is_none());
    }

    #[test]
    fn filter_pattern_matches() {
        let pattern = Pattern::Filter {
            predicate: None,
            input: Box::new(Pattern::wildcard("child")),
        };
        let expr =
            RelExpr::scan("t").filter(Expr::Const(Const::Bool(true)));
        let bindings = pattern.match_expr(&expr);
        assert!(bindings.is_some());
        let b = bindings.expect("already checked");
        assert!(b.get_rel("child").is_some());
    }

    #[test]
    fn join_pattern_with_type_constraint() {
        let pattern = Pattern::Join {
            join_type: Some(JoinType::Inner),
            left: Box::new(Pattern::wildcard("l")),
            right: Box::new(Pattern::wildcard("r")),
        };

        let inner_join = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("a"))),
                right: Box::new(Expr::Column(ColumnRef::new("b"))),
            },
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(RelExpr::scan("t2")),
        };
        assert!(pattern.match_expr(&inner_join).is_some());

        let left_join = RelExpr::Join {
            join_type: JoinType::LeftOuter,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(RelExpr::scan("t2")),
        };
        assert!(pattern.match_expr(&left_join).is_none());
    }

    #[test]
    fn bindings_lookup() {
        let mut b = Bindings::default();
        b.bind_rel("x", RelExpr::scan("t"));
        b.bind_expr("p", Expr::Const(Const::Int(42)));

        assert!(b.get_rel("x").is_some());
        assert!(b.get_expr("p").is_some());
        assert!(b.get_rel("missing").is_none());
        assert!(b.get_expr("missing").is_none());
    }

    #[test]
    fn pattern_var_new() {
        let v = PatternVar::new("input");
        assert_eq!(v.name, "input");
    }
}
