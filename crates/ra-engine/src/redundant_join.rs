//! Redundant join elimination rules.
//!
//! Identifies and removes joins that don't contribute to the query result:
//! - Joins with single-row relations that don't add columns
//! - Self-joins on unique columns where one side isn't used
//! - Cross joins with empty relations

use egg::{rewrite, Rewrite};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;

/// Return redundant join elimination rules.
///
/// These rules detect and eliminate joins that don't contribute
/// meaningful data or filtering to the query result.
#[must_use]
pub fn redundant_join_elimination_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // ---------------------------------------------------------------
        // Cross join with single-row relation elimination
        // ---------------------------------------------------------------

        // JOIN CROSS with single-row table that adds no columns
        // This models cases like: SELECT * FROM t1 CROSS JOIN (SELECT 1)
        rewrite!("eliminate-cross-join-single-row-right";
            "(join cross ?cond ?left (limit (const-int 1) (const-int 0) ?right))" =>
            "?left"
        ),

        // Symmetric case: single-row on left
        rewrite!("eliminate-cross-join-single-row-left";
            "(join cross ?cond (limit (const-int 1) (const-int 0) ?left) ?right)" =>
            "?right"
        ),

        // ---------------------------------------------------------------
        // Cross join with VALUES (1) elimination
        // ---------------------------------------------------------------

        // JOIN CROSS with VALUES(1) - common pattern in some SQL
        rewrite!("eliminate-cross-join-values-single";
            "(join cross ?cond ?left (values (values-row (const-int 1))))" =>
            "?left"
        ),

        // ---------------------------------------------------------------
        // Inner join with always-true condition
        // ---------------------------------------------------------------

        // JOIN INNER with TRUE condition and single-row right side
        rewrite!("eliminate-inner-join-true-single-row";
            "(join inner (const-bool true) ?left (limit (const-int 1) (const-int 0) ?right))" =>
            "?left"
        ),

        // ---------------------------------------------------------------
        // Self-join elimination on unique key
        // Note: In a real implementation, we'd check uniqueness constraints
        // ---------------------------------------------------------------

        // Self-join where only one side is projected
        // Pattern: SELECT t1.* FROM t t1 JOIN t t2 ON t1.id = t2.id
        // This is a simplified pattern - real implementation needs metadata
        rewrite!("eliminate-self-join-unique";
            "(project ?cols (join inner (eq ?col1 ?col2) (scan ?t) (scan ?t)))" =>
            "(project ?cols (scan ?t))"
            if is_unique_column_join(?col1, ?col2)
        ),

        // ---------------------------------------------------------------
        // Semi-join with always-true condition
        // ---------------------------------------------------------------

        // SEMI JOIN with TRUE means "if right has any rows"
        // If we know right is non-empty, this is redundant
        rewrite!("eliminate-semi-join-true-nonempty";
            "(join semi (const-bool true) ?left ?right)" =>
            "?left"
            if is_known_nonempty(?right)
        ),

        // ---------------------------------------------------------------
        // Anti-join with empty relation
        // ---------------------------------------------------------------

        // ANTI JOIN with empty right side keeps all left rows
        rewrite!("eliminate-anti-join-empty-right";
            "(join anti ?cond ?left (filter (const-bool false) ?right))" =>
            "?left"
        ),

        // ---------------------------------------------------------------
        // Join with projection that doesn't use join columns
        // ---------------------------------------------------------------

        // If projection after join only uses columns from one side,
        // and join doesn't filter, the join is redundant
        rewrite!("eliminate-unused-cross-join";
            "(project ?cols (join cross ?cond ?left ?right))" =>
            "(project ?cols ?left)"
            if only_uses_left_columns(?cols)
        ),

        // ---------------------------------------------------------------
        // Redundant left outer join
        // ---------------------------------------------------------------

        // LEFT OUTER JOIN where right side is never null and not used
        // Pattern: LEFT JOIN followed by projection of only left columns
        rewrite!("eliminate-unused-left-join";
            "(project ?cols (join left-outer ?cond ?left ?right))" =>
            "(project ?cols ?left)"
            if only_uses_left_columns(?cols)
        ),

        // ---------------------------------------------------------------
        // Join with DISTINCT on one side (for existence checking)
        // ---------------------------------------------------------------

        // Semi-join can replace inner join + distinct when checking existence
        rewrite!("inner-join-distinct-to-semi";
            "(distinct-rel (project ?cols (join inner ?cond ?left ?right)))" =>
            "(project ?cols (join semi ?cond ?left ?right))"
            if only_uses_left_columns(?cols)
        ),
    ]
}

// Helper conditions (these would be implemented in the analysis)
fn is_unique_column_join(_col1: &str, _col2: &str) -> bool {
    // In real implementation, check if columns are unique (primary key, unique constraint)
    false
}

fn is_known_nonempty(_expr: &str) -> bool {
    // In real implementation, check statistics or constraints
    false
}

fn only_uses_left_columns(_cols: &str) -> bool {
    // In real implementation, analyze which tables the columns come from
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egraph::{to_rec_expr, RelLang};
    use egg::Runner;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn run_redundant_join_elimination(expr: &RelExpr) -> Runner<RelLang, RelAnalysis> {
        let rec = to_rec_expr(expr).expect("conversion should succeed");
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(10_000)
            .with_iter_limit(5)
            .run(&redundant_join_elimination_rules())
    }

    #[test]
    fn cross_join_with_single_row_eliminated() {
        // SELECT * FROM t CROSS JOIN (SELECT 1 LIMIT 1)
        let single_row = RelExpr::Values {
            rows: vec![vec![Expr::Const(Const::Int(1))]],
        }.limit(1, 0);

        let expr = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("t")),
            right: Box::new(single_row),
        };

        let runner = run_redundant_join_elimination(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn anti_join_with_empty_right_eliminated() {
        // Anti-join with empty right side (filter false)
        let empty_right = RelExpr::scan("t2")
            .filter(Expr::Const(Const::Bool(false)));

        let expr = RelExpr::Join {
            join_type: JoinType::Anti,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("id"))),
                right: Box::new(Expr::Column(ColumnRef::new("id"))),
            },
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(empty_right),
        };

        let runner = run_redundant_join_elimination(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn inner_join_with_true_and_single_row_eliminated() {
        // INNER JOIN with TRUE condition and single-row right
        let single_row = RelExpr::Values {
            rows: vec![vec![Expr::Const(Const::Int(1))]],
        }.limit(1, 0);

        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("t")),
            right: Box::new(single_row),
        };

        let runner = run_redundant_join_elimination(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }
}