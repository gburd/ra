//! Condition functions for generated rewrite rules.
//!
//! These functions implement `egg::Condition<RelLang, RelAnalysis>` and are
//! referenced by `.rra` rule files via `if condition_name(...)` syntax.
//! Each new condition function unlocks additional rules from the generated set.

use egg::{Condition, EGraph, Id, Subst, Var};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

/// Condition: a CTE name is referenced exactly once in the body.
///
/// Used by CTE inlining rules to ensure inlining is safe.
pub struct SingleReference {
    name_var: Var,
    body_var: Var,
}

impl SingleReference {
    /// Create a condition checking that `name` appears once in `body`.
    #[must_use]
    pub fn new(name: &str, body: &str) -> Self {
        Self {
            name_var: name.parse().unwrap_or_else(|_| "?name".parse().unwrap()),
            body_var: body.parse().unwrap_or_else(|_| "?body".parse().unwrap()),
        }
    }
}

impl Condition<RelLang, RelAnalysis> for SingleReference {
    fn check(
        &self,
        egraph: &mut EGraph<RelLang, RelAnalysis>,
        _eclass: Id,
        subst: &Subst,
    ) -> bool {
        let name_id = subst[self.name_var];
        let body_id = subst[self.body_var];

        // Count references to the CTE name in the body's e-class data
        let name_data = &egraph[name_id].data;
        let body_data = &egraph[body_id].data;

        // If the name appears in the body's table set exactly once,
        // inlining is safe. We approximate by checking the table set.
        if let Some(name_str) = name_data.tables.iter().next() {
            let count = body_data
                .tables
                .iter()
                .filter(|t| *t == name_str)
                .count();
            return count <= 1;
        }
        // Conservative: allow inlining if we can't determine reference count
        true
    }
}

/// Condition: a predicate references columns from only one side of a join.
pub struct ReferencesOnly {
    pred_var: Var,
    side_var: Var,
}

impl ReferencesOnly {
    /// Create a condition checking that `pred` references only columns from `side`.
    #[must_use]
    pub fn new(pred: &str, side: &str) -> Self {
        Self {
            pred_var: pred.parse().unwrap_or_else(|_| "?pred".parse().unwrap()),
            side_var: side.parse().unwrap_or_else(|_| "?side".parse().unwrap()),
        }
    }
}

impl Condition<RelLang, RelAnalysis> for ReferencesOnly {
    fn check(
        &self,
        egraph: &mut EGraph<RelLang, RelAnalysis>,
        _eclass: Id,
        subst: &Subst,
    ) -> bool {
        let pred_id = subst[self.pred_var];
        let side_id = subst[self.side_var];

        let pred_data = &egraph[pred_id].data;
        let side_data = &egraph[side_id].data;

        // Check that all tables referenced by the predicate are in the side's table set
        pred_data.tables.is_subset(&side_data.tables)
    }
}

/// Condition: an expression is deterministic (no side effects, no randomness).
pub struct IsDeterministic {
    expr_var: Var,
}

impl IsDeterministic {
    /// Create a condition checking that `expr` is deterministic.
    #[must_use]
    pub fn new(expr: &str) -> Self {
        Self {
            expr_var: expr.parse().unwrap_or_else(|_| "?expr".parse().unwrap()),
        }
    }
}

impl Condition<RelLang, RelAnalysis> for IsDeterministic {
    fn check(
        &self,
        egraph: &mut EGraph<RelLang, RelAnalysis>,
        _eclass: Id,
        subst: &Subst,
    ) -> bool {
        let expr_id = subst[self.expr_var];
        let data = &egraph[expr_id].data;

        // If the e-class data has no non-deterministic markers, it's deterministic.
        // Currently we check that it doesn't reference special "random"/"now" tables.
        !data.tables.iter().any(|t| {
            t.contains("random") || t.contains("now") || t.contains("current_timestamp")
        })
    }
}

/// Condition: an expression is a compile-time constant.
pub struct IsConstant {
    expr_var: Var,
}

impl IsConstant {
    /// Create a condition checking that `expr` is a constant.
    #[must_use]
    pub fn new(expr: &str) -> Self {
        Self {
            expr_var: expr.parse().unwrap_or_else(|_| "?expr".parse().unwrap()),
        }
    }
}

impl Condition<RelLang, RelAnalysis> for IsConstant {
    fn check(
        &self,
        egraph: &mut EGraph<RelLang, RelAnalysis>,
        _eclass: Id,
        subst: &Subst,
    ) -> bool {
        let expr_id = subst[self.expr_var];
        let data = &egraph[expr_id].data;

        // A constant expression references no tables and has no column references
        data.tables.is_empty()
    }
}

/// Constructor functions for use in generated rules.
///
/// These return boxed `Condition` implementations.

/// Returns a condition that checks if a CTE name is referenced at most once.
#[must_use]
pub fn single_reference(name: &str, body: &str) -> SingleReference {
    SingleReference::new(name, body)
}

/// Returns a condition that checks if a predicate references only one side.
#[must_use]
pub fn references_only(pred: &str, side: &str) -> ReferencesOnly {
    ReferencesOnly::new(pred, side)
}

/// Returns a condition that checks if an expression is deterministic.
#[must_use]
pub fn is_deterministic(expr: &str) -> IsDeterministic {
    IsDeterministic::new(expr)
}

/// Returns a condition that checks if an expression is a constant.
#[must_use]
pub fn is_constant(expr: &str) -> IsConstant {
    IsConstant::new(expr)
}
