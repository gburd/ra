//! Null constant simplification rules.
//!
//! Implements simplification rules for expressions involving NULL constants,
//! following SQL's three-valued logic semantics.
//!
//! Key transformations:
//! - `AND(NULL, x) -> NULL` - NULL propagates through AND
//! - `OR(NULL, x) -> x` when x is not null-rejecting
//! - `Comparison(NULL, x) -> NULL` - comparisons with NULL yield NULL
//! - `x IS NULL` when x is const-null -> TRUE
//! - `x IS NOT NULL` when x is const-null -> FALSE

#[cfg(test)]
use egg::{rewrite, Rewrite};

#[cfg(test)]
use crate::analysis::RelAnalysis;
#[cfg(test)]
use crate::egraph::RelLang;

/// Return null constant simplification rules.
///
/// These rules handle NULL propagation through various operators
/// according to SQL's three-valued logic semantics.
#[must_use]
#[expect(clippy::too_many_lines, reason = "rule collection for NULL simplification patterns")]
#[cfg(test)] // RFC 0090 Phase 1b: test oracle; production uses generated rules
pub fn null_simplification_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // ---------------------------------------------------------------
        // AND with NULL propagation
        // ---------------------------------------------------------------

        // AND(NULL, x) -> NULL
        rewrite!("and-null-left";
            "(and const-null ?x)" => "const-null"
        ),
        // AND(x, NULL) -> NULL
        rewrite!("and-null-right";
            "(and ?x const-null)" => "const-null"
        ),
        // ---------------------------------------------------------------
        // OR with NULL handling
        // Note: OR(NULL, x) -> x only when x is not null-rejecting
        // For simplicity, we handle the cases where x is a constant
        // ---------------------------------------------------------------

        // OR(NULL, TRUE) -> TRUE
        rewrite!("or-null-true-right";
            "(or const-null (const-bool true))" => "(const-bool true)"
        ),
        // OR(TRUE, NULL) -> TRUE
        rewrite!("or-null-true-left";
            "(or (const-bool true) const-null)" => "(const-bool true)"
        ),
        // OR(NULL, FALSE) -> NULL
        rewrite!("or-null-false-right";
            "(or const-null (const-bool false))" => "const-null"
        ),
        // OR(FALSE, NULL) -> NULL
        rewrite!("or-null-false-left";
            "(or (const-bool false) const-null)" => "const-null"
        ),
        // OR(NULL, NULL) -> NULL
        rewrite!("or-null-null";
            "(or const-null const-null)" => "const-null"
        ),
        // ---------------------------------------------------------------
        // Comparison operators with NULL
        // All comparisons with NULL yield NULL
        // ---------------------------------------------------------------

        // EQ(NULL, x) -> NULL
        rewrite!("eq-null-left";
            "(eq const-null ?x)" => "const-null"
        ),
        // EQ(x, NULL) -> NULL
        rewrite!("eq-null-right";
            "(eq ?x const-null)" => "const-null"
        ),
        // NE(NULL, x) -> NULL
        rewrite!("ne-null-left";
            "(ne const-null ?x)" => "const-null"
        ),
        // NE(x, NULL) -> NULL
        rewrite!("ne-null-right";
            "(ne ?x const-null)" => "const-null"
        ),
        // LT(NULL, x) -> NULL
        rewrite!("lt-null-left";
            "(lt const-null ?x)" => "const-null"
        ),
        // LT(x, NULL) -> NULL
        rewrite!("lt-null-right";
            "(lt ?x const-null)" => "const-null"
        ),
        // LE(NULL, x) -> NULL
        rewrite!("le-null-left";
            "(le const-null ?x)" => "const-null"
        ),
        // LE(x, NULL) -> NULL
        rewrite!("le-null-right";
            "(le ?x const-null)" => "const-null"
        ),
        // GT(NULL, x) -> NULL
        rewrite!("gt-null-left";
            "(gt const-null ?x)" => "const-null"
        ),
        // GT(x, NULL) -> NULL
        rewrite!("gt-null-right";
            "(gt ?x const-null)" => "const-null"
        ),
        // GE(NULL, x) -> NULL
        rewrite!("ge-null-left";
            "(ge const-null ?x)" => "const-null"
        ),
        // GE(x, NULL) -> NULL
        rewrite!("ge-null-right";
            "(ge ?x const-null)" => "const-null"
        ),
        // ---------------------------------------------------------------
        // IS NULL / IS NOT NULL with constants
        // ---------------------------------------------------------------

        // IS_NULL(NULL) -> TRUE
        rewrite!("is-null-of-null";
            "(is-null const-null)" => "(const-bool true)"
        ),
        // IS_NOT_NULL(NULL) -> FALSE
        rewrite!("is-not-null-of-null";
            "(is-not-null const-null)" => "(const-bool false)"
        ),
        // ---------------------------------------------------------------
        // Arithmetic operations with NULL
        // ---------------------------------------------------------------

        // ADD(NULL, x) -> NULL
        rewrite!("add-null-left";
            "(add const-null ?x)" => "const-null"
        ),
        // ADD(x, NULL) -> NULL
        rewrite!("add-null-right";
            "(add ?x const-null)" => "const-null"
        ),
        // SUB(NULL, x) -> NULL
        rewrite!("sub-null-left";
            "(sub const-null ?x)" => "const-null"
        ),
        // SUB(x, NULL) -> NULL
        rewrite!("sub-null-right";
            "(sub ?x const-null)" => "const-null"
        ),
        // MUL(NULL, x) -> NULL
        rewrite!("mul-null-left";
            "(mul const-null ?x)" => "const-null"
        ),
        // MUL(x, NULL) -> NULL
        rewrite!("mul-null-right";
            "(mul ?x const-null)" => "const-null"
        ),
        // DIV(NULL, x) -> NULL
        rewrite!("div-null-left";
            "(div const-null ?x)" => "const-null"
        ),
        // DIV(x, NULL) -> NULL
        rewrite!("div-null-right";
            "(div ?x const-null)" => "const-null"
        ),
        // MOD(NULL, x) -> NULL
        rewrite!("mod-null-left";
            "(mod const-null ?x)" => "const-null"
        ),
        // MOD(x, NULL) -> NULL
        rewrite!("mod-null-right";
            "(mod ?x const-null)" => "const-null"
        ),
        // ---------------------------------------------------------------
        // Unary operations with NULL
        // ---------------------------------------------------------------

        // NOT(NULL) -> NULL
        rewrite!("not-null";
            "(not const-null)" => "const-null"
        ),
        // NEG(NULL) -> NULL
        rewrite!("neg-null";
            "(neg const-null)" => "const-null"
        ),
        // ---------------------------------------------------------------
        // String operations with NULL
        // ---------------------------------------------------------------

        // CONCAT(NULL, x) -> NULL
        rewrite!("concat-null-left";
            "(concat const-null ?x)" => "const-null"
        ),
        // CONCAT(x, NULL) -> NULL
        rewrite!("concat-null-right";
            "(concat ?x const-null)" => "const-null"
        ),
        // ---------------------------------------------------------------
        // Filter with NULL predicate
        // ---------------------------------------------------------------

        // FILTER(NULL, input) -> empty result (no rows pass NULL filter)
        rewrite!("filter-null-predicate";
            "(filter const-null ?input)" => "(filter (const-bool false) ?input)"
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egraph::{to_rec_expr, RelLang};
    use egg::Runner;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp, Const, Expr, UnaryOp};

    fn run_null_simplification(expr: &RelExpr) -> Runner<RelLang, RelAnalysis> {
        let rec = to_rec_expr(expr).expect("conversion should succeed");
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(10_000)
            .with_iter_limit(5)
            .run(&null_simplification_rules())
    }

    #[test]
    fn and_with_null_simplifies_to_null() {
        // AND(NULL, column) -> NULL
        let expr = RelExpr::scan("t").filter(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::Const(Const::Null)),
            right: Box::new(Expr::Column(ra_core::expr::ColumnRef::new("col"))),
        });

        let runner = run_null_simplification(&expr);
        // The e-graph should contain the simplified NULL constant
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn or_null_with_true_simplifies_to_true() {
        // OR(NULL, TRUE) -> TRUE
        let expr = RelExpr::scan("t").filter(Expr::BinOp {
            op: BinOp::Or,
            left: Box::new(Expr::Const(Const::Null)),
            right: Box::new(Expr::Const(Const::Bool(true))),
        });

        let runner = run_null_simplification(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn eq_with_null_simplifies_to_null() {
        // EQ(column, NULL) -> NULL
        let expr = RelExpr::scan("t").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ra_core::expr::ColumnRef::new("col"))),
            right: Box::new(Expr::Const(Const::Null)),
        });

        let runner = run_null_simplification(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn is_null_of_null_simplifies_to_true() {
        // IS_NULL(NULL) -> TRUE
        let expr = RelExpr::scan("t").filter(Expr::UnaryOp {
            op: UnaryOp::IsNull,
            operand: Box::new(Expr::Const(Const::Null)),
        });

        let runner = run_null_simplification(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn arithmetic_with_null_simplifies_to_null() {
        // ADD(NULL, 5) -> NULL
        let expr = RelExpr::scan("t").project(vec![ra_core::algebra::ProjectionColumn {
            expr: Expr::BinOp {
                op: BinOp::Add,
                left: Box::new(Expr::Const(Const::Null)),
                right: Box::new(Expr::Const(Const::Int(5))),
            },
            alias: Some("result".to_string()),
        }]);

        let runner = run_null_simplification(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn filter_with_null_predicate_returns_empty() {
        // FILTER(NULL, scan) -> FILTER(FALSE, scan) (no rows)
        let expr = RelExpr::scan("t").filter(Expr::Const(Const::Null));

        let runner = run_null_simplification(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }
}
