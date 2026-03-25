//! Query pattern fingerprinting for Bayesian pruning.
//!
//! Extracts structural features from `RelExpr` plan trees and
//! produces a coarse, hashable fingerprint. Two plans with the same
//! fingerprint are expected to have similar improvement probability
//! during optimization search.

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::Expr;

/// Discretized structural summary of a plan subtree.
///
/// Fields are bucketed so the total fingerprint space stays small
/// (4 x 4 x 3 x 2 x 2 x 2 = 384 possible values). This ensures
/// each bucket accumulates observations quickly.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlanFingerprint {
    /// Number of base tables (bucketed: 0-1, 2-3, 4-6, 7+).
    pub table_bucket: u8,
    /// Number of join operators (bucketed: 0, 1-2, 3-5, 6+).
    pub join_bucket: u8,
    /// Predicate complexity score (bucketed: low=0, medium=1, high=2).
    pub predicate_complexity: u8,
    /// Whether the subtree contains a cross join.
    pub has_cross_join: bool,
    /// Whether the subtree contains a correlated subquery.
    pub has_correlated_subquery: bool,
    /// Whether aggregation appears below a join.
    pub has_early_aggregation: bool,
}

impl PlanFingerprint {
    /// Build a fingerprint from a plan subtree.
    #[must_use]
    pub fn from_plan(plan: &RelExpr) -> Self {
        let tables = count_tables(plan);
        let joins = count_joins(plan);
        let pred = predicate_complexity(plan);

        Self {
            table_bucket: match tables {
                0..=1 => 0,
                2..=3 => 1,
                4..=6 => 2,
                _ => 3,
            },
            join_bucket: match joins {
                0 => 0,
                1..=2 => 1,
                3..=5 => 2,
                _ => 3,
            },
            predicate_complexity: match pred {
                0..=2 => 0,
                3..=6 => 1,
                _ => 2,
            },
            has_cross_join: contains_cross_join(plan),
            has_correlated_subquery: contains_correlated(plan),
            has_early_aggregation: has_agg_below_join(plan),
        }
    }
}

/// Count the number of base table scans in a plan tree.
#[must_use]
pub fn count_tables(plan: &RelExpr) -> usize {
    match plan {
        RelExpr::Scan { .. } | RelExpr::IndexScan { .. } => 1,
        RelExpr::Join { left, right, .. } => count_tables(left) + count_tables(right),
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. } => count_tables(input),
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => count_tables(left) + count_tables(right),
        RelExpr::CTE {
            definition, body, ..
        } => count_tables(definition) + count_tables(body),
        _ => 0,
    }
}

/// Count the number of join operators in a plan tree.
#[must_use]
pub fn count_joins(plan: &RelExpr) -> usize {
    match plan {
        RelExpr::Join { left, right, .. } => 1 + count_joins(left) + count_joins(right),
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. } => count_joins(input),
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => count_joins(left) + count_joins(right),
        _ => 0,
    }
}

/// Compute a predicate complexity score by counting nodes in all
/// predicate expressions across the plan tree.
#[must_use]
pub fn predicate_complexity(plan: &RelExpr) -> usize {
    match plan {
        RelExpr::Filter { predicate, input } => {
            expr_complexity(predicate) + predicate_complexity(input)
        }
        RelExpr::Join {
            condition,
            left,
            right,
            ..
        } => expr_complexity(condition) + predicate_complexity(left) + predicate_complexity(right),
        RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. } => predicate_complexity(input),
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            predicate_complexity(left) + predicate_complexity(right)
        }
        _ => 0,
    }
}

/// Count the number of nodes in a scalar expression tree.
fn expr_complexity(expr: &Expr) -> usize {
    match expr {
        Expr::Column(_) | Expr::Const(_) => 1,
        Expr::BinOp { left, right, .. } => 1 + expr_complexity(left) + expr_complexity(right),
        Expr::UnaryOp { operand, .. } => 1 + expr_complexity(operand),
        Expr::Function { args, .. } => 1 + args.iter().map(expr_complexity).sum::<usize>(),
        Expr::Case {
            operand,
            when_clauses,
            else_result,
        } => {
            let base = operand.as_ref().map_or(0, |e| expr_complexity(e));
            let whens: usize = when_clauses
                .iter()
                .map(|(w, t)| expr_complexity(w) + expr_complexity(t))
                .sum();
            let else_cost = else_result.as_ref().map_or(0, |e| expr_complexity(e));
            1 + base + whens + else_cost
        }
        Expr::Cast { expr, .. } => 1 + expr_complexity(expr),
        Expr::Array(elems) => 1 + elems.iter().map(expr_complexity).sum::<usize>(),
        Expr::ArrayIndex(arr, idx) => 1 + expr_complexity(arr) + expr_complexity(idx),
        Expr::ArraySlice { array, start, end } => {
            1 + expr_complexity(array)
                + start.as_ref().map_or(0, |e| expr_complexity(e))
                + end.as_ref().map_or(0, |e| expr_complexity(e))
        }
        _ => 1,
    }
}

/// Check whether a plan tree contains any cross join.
#[must_use]
pub fn contains_cross_join(plan: &RelExpr) -> bool {
    match plan {
        RelExpr::Join {
            join_type,
            left,
            right,
            ..
        } => {
            *join_type == JoinType::Cross || contains_cross_join(left) || contains_cross_join(right)
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. } => contains_cross_join(input),
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            contains_cross_join(left) || contains_cross_join(right)
        }
        _ => false,
    }
}

/// Check whether a plan tree contains a correlated subquery pattern.
///
/// Detects `RecursiveCTE` nodes or `CTE` nodes with body references
/// that indicate correlation. This is a conservative approximation
/// since the `Expr` type does not have explicit subquery variants.
#[must_use]
pub fn contains_correlated(plan: &RelExpr) -> bool {
    match plan {
        RelExpr::RecursiveCTE { .. } => true,
        RelExpr::CTE {
            definition, body, ..
        } => contains_correlated(definition) || contains_correlated(body),
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. } => contains_correlated(input),
        RelExpr::Join { left, right, .. } => {
            contains_correlated(left) || contains_correlated(right)
        }
        _ => false,
    }
}

/// Check whether aggregation appears below a join in the plan tree.
///
/// Early aggregation (aggregate pushed below join) is a structural
/// pattern that has distinctive optimization characteristics.
#[must_use]
pub fn has_agg_below_join(plan: &RelExpr) -> bool {
    has_agg_below_join_rec(plan, false)
}

fn has_agg_below_join_rec(plan: &RelExpr, inside_join: bool) -> bool {
    match plan {
        RelExpr::Aggregate { input, .. } => {
            if inside_join {
                return true;
            }
            has_agg_below_join_rec(input, inside_join)
        }
        RelExpr::Join { left, right, .. } => {
            has_agg_below_join_rec(left, true) || has_agg_below_join_rec(right, true)
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. } => has_agg_below_join_rec(input, inside_join),
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            has_agg_below_join_rec(left, inside_join) || has_agg_below_join_rec(right, inside_join)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::{AggregateExpr, AggregateFunction, JoinType};
    use ra_core::expr::{BinOp, ColumnRef, Const};

    fn eq_cond() -> Expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Column(ColumnRef::new("id"))),
        }
    }

    fn make_join(left: RelExpr, right: RelExpr, jt: JoinType) -> RelExpr {
        RelExpr::Join {
            join_type: jt,
            condition: eq_cond(),
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn make_inner_join(left: RelExpr, right: RelExpr) -> RelExpr {
        make_join(left, right, JoinType::Inner)
    }

    fn make_aggregate(input: RelExpr) -> RelExpr {
        RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("c".into()),
            }],
            input: Box::new(input),
        }
    }

    // -- count_tables --

    #[test]
    fn count_tables_single_scan() {
        assert_eq!(count_tables(&RelExpr::scan("t")), 1);
    }

    #[test]
    fn count_tables_join() {
        let plan = make_inner_join(RelExpr::scan("a"), RelExpr::scan("b"));
        assert_eq!(count_tables(&plan), 2);
    }

    #[test]
    fn count_tables_nested_joins() {
        let inner = make_inner_join(RelExpr::scan("a"), RelExpr::scan("b"));
        let outer = make_inner_join(inner, RelExpr::scan("c"));
        assert_eq!(count_tables(&outer), 3);
    }

    #[test]
    fn count_tables_through_filter() {
        let plan = RelExpr::Filter {
            predicate: Expr::Const(Const::Bool(true)),
            input: Box::new(RelExpr::scan("t")),
        };
        assert_eq!(count_tables(&plan), 1);
    }

    #[test]
    fn count_tables_union() {
        let plan = RelExpr::Union {
            all: true,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        assert_eq!(count_tables(&plan), 2);
    }

    #[test]
    fn count_tables_values_is_zero() {
        let plan = RelExpr::Values {
            rows: vec![vec![Expr::Const(Const::Int(1))]],
        };
        assert_eq!(count_tables(&plan), 0);
    }

    #[test]
    fn count_tables_index_scan() {
        let plan = RelExpr::IndexScan {
            table: "t".into(),
            column: "id".into(),
        };
        assert_eq!(count_tables(&plan), 1);
    }

    // -- count_joins --

    #[test]
    fn count_joins_no_joins() {
        assert_eq!(count_joins(&RelExpr::scan("t")), 0);
    }

    #[test]
    fn count_joins_single_join() {
        let plan = make_inner_join(RelExpr::scan("a"), RelExpr::scan("b"));
        assert_eq!(count_joins(&plan), 1);
    }

    #[test]
    fn count_joins_nested() {
        let inner = make_inner_join(RelExpr::scan("a"), RelExpr::scan("b"));
        let outer = make_inner_join(inner, RelExpr::scan("c"));
        assert_eq!(count_joins(&outer), 2);
    }

    #[test]
    fn count_joins_through_filter_and_project() {
        let plan = RelExpr::Filter {
            predicate: Expr::Const(Const::Bool(true)),
            input: Box::new(RelExpr::Project {
                columns: vec![],
                input: Box::new(make_inner_join(RelExpr::scan("a"), RelExpr::scan("b"))),
            }),
        };
        assert_eq!(count_joins(&plan), 1);
    }

    // -- predicate_complexity --

    #[test]
    fn predicate_complexity_no_predicates() {
        assert_eq!(predicate_complexity(&RelExpr::scan("t")), 0);
    }

    #[test]
    fn predicate_complexity_simple_filter() {
        let plan = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("a"))),
                right: Box::new(Expr::Const(Const::Int(1))),
            },
            input: Box::new(RelExpr::scan("t")),
        };
        assert_eq!(predicate_complexity(&plan), 3); // binop + col + const
    }

    #[test]
    fn predicate_complexity_join_condition() {
        let plan = make_inner_join(RelExpr::scan("a"), RelExpr::scan("b"));
        assert_eq!(predicate_complexity(&plan), 3); // binop + 2 columns
    }

    #[test]
    fn predicate_complexity_nested() {
        let inner = RelExpr::Filter {
            predicate: Expr::Column(ColumnRef::new("x")),
            input: Box::new(RelExpr::scan("t")),
        };
        let outer = RelExpr::Filter {
            predicate: Expr::Column(ColumnRef::new("y")),
            input: Box::new(inner),
        };
        assert_eq!(predicate_complexity(&outer), 2); // 1 + 1
    }

    // -- expr_complexity --

    #[test]
    fn expr_complexity_column() {
        assert_eq!(
            expr_complexity(&Expr::Column(ColumnRef::new("a"))),
            1
        );
    }

    #[test]
    fn expr_complexity_const() {
        assert_eq!(expr_complexity(&Expr::Const(Const::Int(42))), 1);
    }

    #[test]
    fn expr_complexity_binop() {
        let e = Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(Expr::Const(Const::Int(1))),
            right: Box::new(Expr::Const(Const::Int(2))),
        };
        assert_eq!(expr_complexity(&e), 3);
    }

    #[test]
    fn expr_complexity_function() {
        let e = Expr::Function {
            name: "upper".into(),
            args: vec![Expr::Column(ColumnRef::new("x"))],
        };
        assert_eq!(expr_complexity(&e), 2);
    }

    #[test]
    fn expr_complexity_case() {
        let e = Expr::Case {
            operand: None,
            when_clauses: vec![(
                Expr::Const(Const::Bool(true)),
                Expr::Const(Const::Int(1)),
            )],
            else_result: Some(Box::new(Expr::Const(Const::Int(0)))),
        };
        // 1 (case) + 1 + 1 (when clause) + 1 (else) = 4
        assert_eq!(expr_complexity(&e), 4);
    }

    #[test]
    fn expr_complexity_cast() {
        let e = Expr::Cast {
            expr: Box::new(Expr::Const(Const::Int(1))),
            target_type: "text".into(),
        };
        assert_eq!(expr_complexity(&e), 2);
    }

    #[test]
    fn expr_complexity_array() {
        let e = Expr::Array(vec![
            Expr::Const(Const::Int(1)),
            Expr::Const(Const::Int(2)),
        ]);
        assert_eq!(expr_complexity(&e), 3);
    }

    #[test]
    fn expr_complexity_array_index() {
        let e = Expr::ArrayIndex(
            Box::new(Expr::Column(ColumnRef::new("a"))),
            Box::new(Expr::Const(Const::Int(0))),
        );
        assert_eq!(expr_complexity(&e), 3);
    }

    // -- contains_cross_join --

    #[test]
    fn contains_cross_join_yes() {
        let plan = make_join(RelExpr::scan("a"), RelExpr::scan("b"), JoinType::Cross);
        assert!(contains_cross_join(&plan));
    }

    #[test]
    fn contains_cross_join_no() {
        let plan = make_inner_join(RelExpr::scan("a"), RelExpr::scan("b"));
        assert!(!contains_cross_join(&plan));
    }

    #[test]
    fn contains_cross_join_nested() {
        let inner = make_join(RelExpr::scan("a"), RelExpr::scan("b"), JoinType::Cross);
        let outer = make_inner_join(inner, RelExpr::scan("c"));
        assert!(contains_cross_join(&outer));
    }

    #[test]
    fn contains_cross_join_through_filter() {
        let plan = RelExpr::Filter {
            predicate: Expr::Const(Const::Bool(true)),
            input: Box::new(make_join(
                RelExpr::scan("a"),
                RelExpr::scan("b"),
                JoinType::Cross,
            )),
        };
        assert!(contains_cross_join(&plan));
    }

    // -- contains_correlated --

    #[test]
    fn contains_correlated_recursive_cte() {
        let plan = RelExpr::RecursiveCTE {
            name: "r".into(),
            base_case: Box::new(RelExpr::scan("t")),
            recursive_case: Box::new(RelExpr::scan("r")),
            body: Box::new(RelExpr::scan("r")),
            cycle_detection: None,
        };
        assert!(contains_correlated(&plan));
    }

    #[test]
    fn contains_correlated_no() {
        assert!(!contains_correlated(&RelExpr::scan("t")));
    }

    #[test]
    fn contains_correlated_through_join() {
        let plan = make_inner_join(
            RelExpr::RecursiveCTE {
                name: "r".into(),
                base_case: Box::new(RelExpr::scan("t")),
                recursive_case: Box::new(RelExpr::scan("r")),
                body: Box::new(RelExpr::scan("r")),
                cycle_detection: None,
            },
            RelExpr::scan("b"),
        );
        assert!(contains_correlated(&plan));
    }

    // -- has_agg_below_join --

    #[test]
    fn has_agg_below_join_yes() {
        let plan = make_inner_join(
            make_aggregate(RelExpr::scan("a")),
            RelExpr::scan("b"),
        );
        assert!(has_agg_below_join(&plan));
    }

    #[test]
    fn has_agg_below_join_no_agg_above_join() {
        let plan = make_aggregate(make_inner_join(
            RelExpr::scan("a"),
            RelExpr::scan("b"),
        ));
        assert!(!has_agg_below_join(&plan));
    }

    #[test]
    fn has_agg_below_join_no_join() {
        let plan = make_aggregate(RelExpr::scan("t"));
        assert!(!has_agg_below_join(&plan));
    }

    // -- PlanFingerprint --

    #[test]
    fn fingerprint_single_scan() {
        let fp = PlanFingerprint::from_plan(&RelExpr::scan("t"));
        assert_eq!(fp.table_bucket, 0); // 0-1 bucket
        assert_eq!(fp.join_bucket, 0);
        assert_eq!(fp.predicate_complexity, 0);
        assert!(!fp.has_cross_join);
        assert!(!fp.has_correlated_subquery);
        assert!(!fp.has_early_aggregation);
    }

    #[test]
    fn fingerprint_two_table_join() {
        let plan = make_inner_join(RelExpr::scan("a"), RelExpr::scan("b"));
        let fp = PlanFingerprint::from_plan(&plan);
        assert_eq!(fp.table_bucket, 1); // 2-3 bucket
        assert_eq!(fp.join_bucket, 1); // 1-2 bucket
    }

    #[test]
    fn fingerprint_many_tables() {
        let j1 = make_inner_join(RelExpr::scan("a"), RelExpr::scan("b"));
        let j2 = make_inner_join(j1, RelExpr::scan("c"));
        let j3 = make_inner_join(j2, RelExpr::scan("d"));
        let j4 = make_inner_join(j3, RelExpr::scan("e"));
        let j5 = make_inner_join(j4, RelExpr::scan("f"));
        let j6 = make_inner_join(j5, RelExpr::scan("g"));
        let plan = make_inner_join(j6, RelExpr::scan("h"));
        let fp = PlanFingerprint::from_plan(&plan);
        assert_eq!(fp.table_bucket, 3); // 7+ bucket
        assert_eq!(fp.join_bucket, 3); // 6+ bucket
    }

    #[test]
    fn fingerprint_with_cross_join() {
        let plan = make_join(RelExpr::scan("a"), RelExpr::scan("b"), JoinType::Cross);
        let fp = PlanFingerprint::from_plan(&plan);
        assert!(fp.has_cross_join);
    }

    #[test]
    fn fingerprint_with_early_aggregation() {
        let plan = make_inner_join(
            make_aggregate(RelExpr::scan("a")),
            RelExpr::scan("b"),
        );
        let fp = PlanFingerprint::from_plan(&plan);
        assert!(fp.has_early_aggregation);
    }

    #[test]
    fn fingerprint_equality_same_structure() {
        let plan = make_inner_join(RelExpr::scan("a"), RelExpr::scan("b"));
        let fp1 = PlanFingerprint::from_plan(&plan);
        let fp2 = PlanFingerprint::from_plan(&plan);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn fingerprint_hashable() {
        use std::collections::HashSet;
        let plan = make_inner_join(RelExpr::scan("a"), RelExpr::scan("b"));
        let fp = PlanFingerprint::from_plan(&plan);
        let mut set = HashSet::new();
        set.insert(fp.clone());
        assert!(set.contains(&fp));
    }

    #[test]
    fn fingerprint_four_to_six_tables() {
        let j1 = make_inner_join(RelExpr::scan("a"), RelExpr::scan("b"));
        let j2 = make_inner_join(j1, RelExpr::scan("c"));
        let j3 = make_inner_join(j2, RelExpr::scan("d"));
        let plan = make_inner_join(j3, RelExpr::scan("e"));
        let fp = PlanFingerprint::from_plan(&plan);
        assert_eq!(fp.table_bucket, 2); // 4-6 bucket
    }
}
