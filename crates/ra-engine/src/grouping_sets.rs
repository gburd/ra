//! Grouping-set normalization.
//!
//! Expands `GROUP BY ROLLUP(...)` / `CUBE(...)` / `GROUPING SETS (...)` into a
//! `UNION ALL` of ordinary grouped aggregates with NULL-padded columns. This
//! is a `RelExpr` → `RelExpr` rewrite run as a pre-optimization normalization
//! (alongside subquery decorrelation), so the equivalence lives in the rewrite
//! layer rather than being hard-coded in the `PostgreSQL` plan builder, and the
//! e-graph sees ordinary aggregates and set operations it already understands.
//!
//! `GROUP BY GROUPING SETS S1, S2, ...` is, by definition, the union of the
//! per-set groupings; ROLLUP and CUBE are sugar for specific families of sets.
//! For each grouping set the corresponding branch groups by that set's columns
//! and projects a NULL in place of every grouping column not in the set.
//!
//! The parser encodes the markers as:
//! - `__rollup(c1, ..., cn)` — sets are the prefixes `{c1..cn}, ..., {c1}, {}`
//! - `__cube(c1, ..., cn)` — sets are all `2^n` subsets
//! - `__grouping_sets(__gs_item(...), ...)` — each `__gs_item` is one set
//!
//! NULL pads are emitted as untyped `Const::Null`; the engine layer is
//! catalog-free, so the column types are resolved later by the plan builder's
//! set-operation type unification (a NULL Const adopts its sibling branch's
//! column type).

use ra_core::algebra::{ProjectionColumn, RelExpr};
use ra_core::expr::{ColumnRef, Const, Expr};

const ROLLUP: &str = "__rollup";
const CUBE: &str = "__cube";
const GROUPING_SETS: &str = "__grouping_sets";
const GS_ITEM: &str = "__gs_item";

/// Maximum number of distinct grouping columns to expand. `CUBE(n)` produces
/// `2^n` branches; cap it to avoid pathological blow-up (such queries fall back
/// to `PostgreSQL`).
const MAX_GROUPING_COLS: usize = 12;

/// True if `group_by` is a single grouping-set marker function.
fn is_grouping_set_marker(group_by: &[Expr]) -> bool {
    matches!(
        group_by,
        [Expr::Function { name, .. }] if name == ROLLUP || name == CUBE || name == GROUPING_SETS
    )
}

/// True if the tree contains a grouping-set marker that [`expand`] rewrites.
#[must_use]
pub fn tree_contains_grouping_sets(expr: &RelExpr) -> bool {
    if let RelExpr::Aggregate { group_by, .. } = expr {
        if is_grouping_set_marker(group_by) {
            return true;
        }
    }
    expr.children().iter().any(|c| tree_contains_grouping_sets(c))
}

/// Expand every grouping-set aggregate in the tree into a `UNION ALL` of
/// ordinary grouped aggregates. Subtrees without a marker are returned
/// structurally unchanged.
#[must_use]
pub fn expand(expr: &RelExpr) -> RelExpr {
    match expr {
        // A grouping-set marker is only actionable at the Project that supplies
        // the output columns (the NULL padding depends on them).
        RelExpr::Project { columns, input } => {
            if let RelExpr::Aggregate {
                group_by,
                aggregates,
                input: agg_input,
            } = &**input
            {
                if let Some(rewritten) =
                    expand_marker(columns, group_by, aggregates, agg_input)
                {
                    return rewritten;
                }
            }
            RelExpr::Project {
                columns: columns.clone(),
                input: Box::new(expand(input)),
            }
        }
        RelExpr::Filter { predicate, input } => RelExpr::Filter {
            predicate: predicate.clone(),
            input: Box::new(expand(input)),
        },
        RelExpr::Aggregate {
            group_by,
            aggregates,
            input,
        } => RelExpr::Aggregate {
            group_by: group_by.clone(),
            aggregates: aggregates.clone(),
            input: Box::new(expand(input)),
        },
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => RelExpr::Join {
            join_type: *join_type,
            condition: condition.clone(),
            left: Box::new(expand(left)),
            right: Box::new(expand(right)),
        },
        RelExpr::Sort { keys, input } => RelExpr::Sort {
            keys: keys.clone(),
            input: Box::new(expand(input)),
        },
        RelExpr::Limit {
            count,
            offset,
            input,
        } => RelExpr::Limit {
            count: *count,
            offset: *offset,
            input: Box::new(expand(input)),
        },
        RelExpr::Distinct { input } => RelExpr::Distinct {
            input: Box::new(expand(input)),
        },
        RelExpr::Union { left, right, all } => RelExpr::Union {
            left: Box::new(expand(left)),
            right: Box::new(expand(right)),
            all: *all,
        },
        RelExpr::Intersect { left, right, all } => RelExpr::Intersect {
            left: Box::new(expand(left)),
            right: Box::new(expand(right)),
            all: *all,
        },
        RelExpr::Except { left, right, all } => RelExpr::Except {
            left: Box::new(expand(left)),
            right: Box::new(expand(right)),
            all: *all,
        },
        RelExpr::CTE {
            name,
            definition,
            body,
        } => RelExpr::CTE {
            name: name.clone(),
            definition: Box::new(expand(definition)),
            body: Box::new(expand(body)),
        },
        RelExpr::Window { functions, input } => RelExpr::Window {
            functions: functions.clone(),
            input: Box::new(expand(input)),
        },
        // Leaves and nodes a grouping set cannot legally appear under: clone
        // unchanged. A marker nested somewhere not handled here simply stays,
        // and the plan builder defers that query to PostgreSQL.
        other => other.clone(),
    }
}

/// Build the `UNION ALL` of grouped-aggregate branches for one marker, or
/// `None` if `group_by` is not a grouping-set marker or its columns are not
/// plain column references.
fn expand_marker(
    columns: &[ProjectionColumn],
    group_by: &[Expr],
    aggregates: &[ra_core::algebra::AggregateExpr],
    agg_input: &RelExpr,
) -> Option<RelExpr> {
    let [Expr::Function { name, args }] = group_by else {
        return None;
    };
    let (gcols, mut sets) = grouping_sets_for(name, args)?;
    // Order branches by descending set size so the leftmost branch covers the
    // most grouping columns. This lets the plan builder's set-op type
    // unification resolve every NULL pad against a typed sibling.
    sets.sort_by_key(|s| std::cmp::Reverse(s.len()));

    // Recurse into the aggregate input once; every branch shares the result.
    let expanded_input = expand(agg_input);

    let mut branches: Vec<RelExpr> = Vec::with_capacity(sets.len());
    for set in &sets {
        let group_keys: Vec<Expr> =
            set.iter().map(|&i| Expr::Column(gcols[i].clone())).collect();
        let out: Vec<ProjectionColumn> = columns
            .iter()
            .map(|pc| {
                if let Expr::Column(pcol) = &pc.expr {
                    if let Some(j) = gcols.iter().position(|g| {
                        g.column.eq_ignore_ascii_case(&pcol.column) && g.table == pcol.table
                    }) {
                        if !set.contains(&j) {
                            return ProjectionColumn {
                                expr: Expr::Const(Const::Null),
                                alias: pc.alias.clone(),
                            };
                        }
                    }
                }
                pc.clone()
            })
            .collect();
        branches.push(RelExpr::Project {
            columns: out,
            input: Box::new(RelExpr::Aggregate {
                group_by: group_keys,
                aggregates: aggregates.to_vec(),
                input: Box::new(expanded_input.clone()),
            }),
        });
    }
    let mut it = branches.into_iter();
    let mut acc = it.next()?;
    for branch in it {
        acc = RelExpr::Union {
            all: true,
            left: Box::new(acc),
            right: Box::new(branch),
        };
    }
    Some(acc)
}

/// Resolve a marker into the universe of grouping columns and the grouping
/// sets expressed as index lists into that universe.
fn grouping_sets_for(name: &str, args: &[Expr]) -> Option<(Vec<ColumnRef>, Vec<Vec<usize>>)> {
    match name {
        ROLLUP | CUBE => {
            let mut gcols: Vec<ColumnRef> = Vec::with_capacity(args.len());
            for a in args {
                match a {
                    Expr::Column(c) => gcols.push(c.clone()),
                    _ => return None,
                }
            }
            let n = gcols.len();
            if n == 0 || n > MAX_GROUPING_COLS {
                return None;
            }
            let sets: Vec<Vec<usize>> = if name == CUBE {
                (0..(1usize << n))
                    .map(|mask| (0..n).filter(|i| mask & (1 << i) != 0).collect())
                    .collect()
            } else {
                (0..=n).rev().map(|k| (0..k).collect()).collect()
            };
            Some((gcols, sets))
        }
        GROUPING_SETS => {
            let mut gcols: Vec<ColumnRef> = Vec::new();
            let mut sets: Vec<Vec<usize>> = Vec::with_capacity(args.len());
            for item in args {
                let Expr::Function { name: inm, args: iargs } = item else {
                    return None;
                };
                if inm != GS_ITEM {
                    return None;
                }
                let mut set = Vec::with_capacity(iargs.len());
                for a in iargs {
                    let Expr::Column(c) = a else {
                        return None;
                    };
                    let idx = if let Some(i) = gcols.iter().position(|g| {
                        g.column.eq_ignore_ascii_case(&c.column) && g.table == c.table
                    }) {
                        i
                    } else {
                        gcols.push(c.clone());
                        gcols.len() - 1
                    };
                    set.push(idx);
                }
                sets.push(set);
            }
            if gcols.is_empty() || gcols.len() > MAX_GROUPING_COLS {
                return None;
            }
            Some((gcols, sets))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::ProjectionColumn;

    fn col(name: &str) -> Expr {
        Expr::Column(ColumnRef {
            table: None,
            column: name.to_string(),
        })
    }

    fn count_star() -> ProjectionColumn {
        ProjectionColumn {
            expr: Expr::Function {
                name: "count".to_string(),
                args: vec![col("*")],
            },
            alias: None,
        }
    }

    fn scan() -> RelExpr {
        RelExpr::Scan {
            table: "t".to_string(),
            alias: None,
        }
    }

    /// Project over an Aggregate carrying a grouping-set marker.
    fn grouping_query(marker: &str, marker_args: Vec<Expr>, out: Vec<ProjectionColumn>) -> RelExpr {
        RelExpr::Project {
            columns: out,
            input: Box::new(RelExpr::Aggregate {
                group_by: vec![Expr::Function {
                    name: marker.to_string(),
                    args: marker_args,
                }],
                aggregates: Vec::new(),
                input: Box::new(scan()),
            }),
        }
    }

    /// Collect the leaf branches of a UNION ALL spine.
    fn union_branches(expr: &RelExpr) -> Vec<&RelExpr> {
        let mut out = Vec::new();
        fn go<'a>(e: &'a RelExpr, out: &mut Vec<&'a RelExpr>) {
            if let RelExpr::Union { all: true, left, right } = e {
                go(left, out);
                go(right, out);
            } else {
                out.push(e);
            }
        }
        go(expr, &mut out);
        out
    }

    /// The number of NULL-padded output columns in a branch.
    fn null_pads(branch: &RelExpr) -> usize {
        let RelExpr::Project { columns, .. } = branch else {
            return 0;
        };
        columns
            .iter()
            .filter(|c| matches!(c.expr, Expr::Const(Const::Null)))
            .count()
    }

    #[test]
    fn rollup_one_column_expands_to_two_branches() {
        let q = grouping_query(
            ROLLUP,
            vec![col("a")],
            vec![
                ProjectionColumn { expr: col("a"), alias: None },
                count_star(),
            ],
        );
        assert!(tree_contains_grouping_sets(&q));
        let expanded = expand(&q);
        let branches = union_branches(&expanded);
        // ROLLUP(a) → {a}, {} → two branches.
        assert_eq!(branches.len(), 2);
        // Exactly one branch (the grand total) NULL-pads column a.
        let pads: usize = branches.iter().map(|b| null_pads(b)).sum();
        assert_eq!(pads, 1);
    }

    #[test]
    fn cube_two_columns_expands_to_four_branches() {
        let q = grouping_query(
            CUBE,
            vec![col("a"), col("b")],
            vec![
                ProjectionColumn { expr: col("a"), alias: None },
                ProjectionColumn { expr: col("b"), alias: None },
                count_star(),
            ],
        );
        let expanded = expand(&q);
        let branches = union_branches(&expanded);
        // CUBE(a,b) → 2^2 = 4 grouping sets.
        assert_eq!(branches.len(), 4);
        // Branches ordered largest-set-first: the first covers both columns
        // (no NULL pads), the last (grand total) pads both.
        assert_eq!(null_pads(branches[0]), 0);
        assert_eq!(null_pads(branches[3]), 2);
    }

    #[test]
    fn explicit_grouping_sets_one_branch_per_set() {
        let q = grouping_query(
            GROUPING_SETS,
            vec![
                Expr::Function {
                    name: GS_ITEM.to_string(),
                    args: vec![col("a")],
                },
                Expr::Function {
                    name: GS_ITEM.to_string(),
                    args: vec![col("b")],
                },
                Expr::Function {
                    name: GS_ITEM.to_string(),
                    args: vec![],
                },
            ],
            vec![
                ProjectionColumn { expr: col("a"), alias: None },
                ProjectionColumn { expr: col("b"), alias: None },
                count_star(),
            ],
        );
        let expanded = expand(&q);
        let branches = union_branches(&expanded);
        // Three explicit sets → three branches.
        assert_eq!(branches.len(), 3);
    }

    #[test]
    fn ordinary_group_by_is_unchanged() {
        // A plain GROUP BY (no marker) must not be rewritten.
        let q = RelExpr::Project {
            columns: vec![ProjectionColumn { expr: col("a"), alias: None }, count_star()],
            input: Box::new(RelExpr::Aggregate {
                group_by: vec![col("a")],
                aggregates: Vec::new(),
                input: Box::new(scan()),
            }),
        };
        assert!(!tree_contains_grouping_sets(&q));
        assert!(matches!(expand(&q), RelExpr::Project { .. }));
    }
}

