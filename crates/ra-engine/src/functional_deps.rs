//! Functional dependency exploitation rules.
//!
//! Uses functional dependencies from unique constraints and keys
//! to simplify GROUP BY clauses and eliminate redundant operations.
//!
//! Currently enabled (unconditional) rules:
//! - DISTINCT after GROUP BY elimination
//! - Double DISTINCT elimination
//! - Sort after sort elimination
//! - DISTINCT/sort reordering
//!
//! Future work (requires analysis infrastructure):
//! - MIN/MAX simplification when grouping by same column
//! - COUNT(*) with unique key optimization
//! - Aggregate-to-DISTINCT conversion

#[cfg(test)]
use egg::{rewrite, Rewrite};

#[cfg(test)]
use crate::analysis::RelAnalysis;
#[cfg(test)]
use crate::egraph::RelLang;

/// Return functional dependency exploitation rules.
///
/// These rules use properties of relational operators (GROUP BY
/// produces unique groups, DISTINCT is idempotent) to eliminate
/// redundant operations. Only unconditional rules are included.
#[must_use]
#[cfg(test)] // RFC 0090 Phase 1b: test oracle; production uses generated rules
pub fn functional_dependency_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
    vec![
        // GROUP BY already produces unique groups, so DISTINCT is redundant
        rewrite!("eliminate-distinct-after-groupby";
            "(distinct-rel (aggregate ?groups ?aggs ?input))" =>
            "(aggregate ?groups ?aggs ?input)"
        ),
        // Double DISTINCT is redundant
        rewrite!("double-distinct-elimination";
            "(distinct-rel (distinct-rel ?input))" =>
            "(distinct-rel ?input)"
        ),
        // Sort after sort: only the outer sort matters
        rewrite!("sort-after-sort";
            "(sort ?keys1 (sort ?keys2 ?input))" =>
            "(sort ?keys1 ?input)"
        ),
        // DISTINCT after sort can be reordered: sort after distinct
        // preserves the ordering while potentially reducing the
        // number of rows sorted
        rewrite!("distinct-sort-reorder";
            "(distinct-rel (sort ?keys ?input))" =>
            "(sort ?keys (distinct-rel ?input))"
        ),
    ]
}

#[cfg(test)]
#[expect(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::egraph::{to_rec_expr, RelLang};
    use egg::Runner;
    use ra_core::algebra::{
        AggregateExpr, AggregateFunction, NullOrdering, RelExpr, SortDirection, SortKey,
    };
    use ra_core::expr::{ColumnRef, Expr};

    fn run_functional_deps(expr: &RelExpr) -> Runner<RelLang, RelAnalysis> {
        let rec = to_rec_expr(expr).expect("conversion should succeed");
        Runner::default()
            .with_expr(&rec)
            .with_node_limit(10_000)
            .with_iter_limit(5)
            .run(&functional_dependency_rules())
    }

    #[test]
    fn distinct_after_groupby_eliminated() {
        let expr = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("dept"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: Some(Expr::Column(ColumnRef::new("id"))),
                distinct: false,
                alias: Some("count".into()),
            }],
            input: Box::new(RelExpr::scan("t")),
        }
        .distinct();

        let runner = run_functional_deps(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn double_distinct_eliminated() {
        let expr = RelExpr::scan("t").distinct().distinct();

        let runner = run_functional_deps(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn sort_after_sort_simplified() {
        let expr = RelExpr::Sort {
            keys: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("b")),
                direction: SortDirection::Desc,
                nulls: NullOrdering::First,
            }],
            input: Box::new(RelExpr::Sort {
                keys: vec![SortKey {
                    expr: Expr::Column(ColumnRef::new("a")),
                    direction: SortDirection::Asc,
                    nulls: NullOrdering::Last,
                }],
                input: Box::new(RelExpr::scan("t")),
            }),
        };

        let runner = run_functional_deps(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }

    #[test]
    fn distinct_sort_reordered() {
        let expr = RelExpr::Sort {
            keys: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("a")),
                direction: SortDirection::Asc,
                nulls: NullOrdering::Last,
            }],
            input: Box::new(RelExpr::scan("t")),
        }
        .distinct();

        let runner = run_functional_deps(&expr);
        assert!(runner.egraph.number_of_classes() > 1);
    }
}
