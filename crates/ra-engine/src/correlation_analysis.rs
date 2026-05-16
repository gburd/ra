//! Scope-based correlation analysis for subquery decorrelation.
//!
//! This module provides proper scope tracking to classify column references
//! as "inner" (belonging to the current subquery) vs. "outer" (referencing
//! an enclosing scope). It replaces the former TPC-H column prefix heuristic
//! with qualifier-based and scope-based lookup.

use ra_core::algebra::RelExpr;
use ra_core::expr::{BinOp, ColumnRef, Expr};

/// A set of columns and tables visible at a given scope level.
#[derive(Debug, Clone)]
pub struct Scope {
    /// Table names/aliases visible at this scope.
    pub tables: Vec<String>,
    /// All columns produced by relations at this scope.
    pub columns: Vec<ColumnRef>,
}

/// Result of analyzing an expression for outer references.
#[derive(Debug, Clone)]
pub struct CorrelationInfo {
    /// Columns from outer scopes referenced in this expression.
    pub outer_refs: Vec<ColumnRef>,
    /// Depth of outermost reference (1 = immediate parent).
    pub max_depth: u32,
    /// Equi-correlation pairs: (`inner_col`, `outer_col`).
    pub equi_pairs: Vec<(ColumnRef, ColumnRef)>,
    /// Non-equi predicates referencing outer columns.
    pub range_predicates: Vec<Expr>,
}

/// Collect all table names and aliases visible in a `RelExpr` subtree.
///
/// Walks the tree and gathers table names from `Scan` nodes,
/// including both the table name and any alias.
#[must_use]
pub fn collect_visible_tables(rel: &RelExpr) -> Vec<String> {
    let mut tables = Vec::new();
    collect_tables_recursive(rel, &mut tables);
    tables
}

fn collect_tables_recursive(rel: &RelExpr, tables: &mut Vec<String>) {
    match rel {
        RelExpr::Scan { table, alias } => {
            tables.push(table.clone());
            if let Some(a) = alias {
                tables.push(a.clone());
            }
        }
        _ => {
            for child in rel.children() {
                collect_tables_recursive(child, tables);
            }
        }
    }
}

/// Collect the "output schema" of a `RelExpr` — the columns it produces.
///
/// For `Scan`: generates qualified column references using the alias (or
/// table name) as qualifier. Since we don't have a catalog, this collects
/// columns referenced within the subtree that belong to the scan's tables.
///
/// For `Project`: the projected column names/aliases.
///
/// For `Join`: union of left + right schemas.
///
/// For other nodes: delegates to children.
#[must_use]
pub fn collect_output_columns(rel: &RelExpr) -> Vec<ColumnRef> {
    let mut cols = Vec::new();
    collect_output_recursive(rel, &mut cols);
    cols
}

fn collect_output_recursive(rel: &RelExpr, cols: &mut Vec<ColumnRef>) {
    match rel {
        RelExpr::Scan { table, alias } => {
            // Use the alias (or table name) as the qualifier for all
            // columns that reference this table. Since we don't have a
            // catalog schema, we record the table/alias itself so that
            // column_in_scope can match by qualifier.
            let qualifier = alias.as_deref().unwrap_or(table.as_str());
            cols.push(ColumnRef::qualified(qualifier, "*"));
        }
        RelExpr::Project { columns, .. } => {
            for pc in columns {
                match &pc.expr {
                    Expr::Column(cr) => {
                        if let Some(ref alias) = pc.alias {
                            cols.push(ColumnRef::new(alias));
                        } else {
                            cols.push(cr.clone());
                        }
                    }
                    _ => {
                        if let Some(ref alias) = pc.alias {
                            cols.push(ColumnRef::new(alias));
                        }
                    }
                }
            }
        }
        RelExpr::Aggregate {
            group_by, input, ..
        } => {
            // Group-by columns are in scope
            for expr in group_by {
                if let Expr::Column(cr) = expr {
                    cols.push(cr.clone());
                }
            }
            // Also inherit columns from input for qualification
            collect_output_recursive(input, cols);
        }
        _ => {
            for child in rel.children() {
                collect_output_recursive(child, cols);
            }
        }
    }
}

/// Check if a column reference belongs to a scope.
///
/// Uses qualifier match first (table.column format), then falls back
/// to checking if the column name exists unambiguously among the
/// scope's tables.
#[must_use]
pub fn column_in_scope(col: &ColumnRef, scope: &Scope) -> bool {
    // If qualified, check if the qualifier matches any table in scope
    if let Some(ref qualifier) = col.table {
        return scope.tables.iter().any(|t| t == qualifier);
    }

    // Unqualified: check if the column appears in the scope's column list.
    // A column is in scope if:
    // 1. It has a matching qualified entry in scope.columns, OR
    // 2. There's a wildcard entry for any table in scope (meaning we have
    //    scan tables that could own this column).
    //
    // For the wildcard case, we can't definitively say the column belongs
    // here without a catalog, so we use a conservative approach: if the
    // scope has explicit columns that match, it's in scope.
    for scope_col in &scope.columns {
        if scope_col.column == col.column {
            return true;
        }
    }

    false
}

/// Classify predicates into correlation predicates (referencing both inner
/// and outer columns) and local predicates (only inner columns).
///
/// A predicate is a "correlation predicate" if it is an equality where
/// one side belongs to the inner scope and the other does not.
///
/// This replaces the former `split_correlation_predicates` + TPC-H heuristic.
#[must_use]
pub fn classify_predicates(
    predicates: &[Expr],
    inner_scope: &Scope,
) -> (Vec<Expr>, Vec<Expr>) {
    let mut correlation = Vec::new();
    let mut local = Vec::new();

    for pred in predicates {
        if is_correlation_pred(pred, inner_scope) {
            correlation.push(pred.clone());
        } else {
            local.push(pred.clone());
        }
    }

    (correlation, local)
}

/// Check if a predicate references both inner-scope and outer-scope columns.
fn is_correlation_pred(pred: &Expr, inner_scope: &Scope) -> bool {
    if let Expr::BinOp {
        op: BinOp::Eq,
        left,
        right,
    } = pred
    {
        let left_col = extract_col_ref(left);
        let right_col = extract_col_ref(right);

        if let (Some(lc), Some(rc)) = (left_col, right_col) {
            let l_inner = column_in_scope(lc, inner_scope);
            let r_inner = column_in_scope(rc, inner_scope);
            // Correlation: one inner, one outer
            return l_inner != r_inner;
        }
    }
    false
}

/// Extract equi-correlation pairs from a set of correlation predicates.
///
/// Given predicates known to be correlation equalities, returns
/// `(inner_col, outer_col)` pairs based on scope membership.
#[must_use]
pub fn extract_equi_pairs(
    correlation_preds: &[Expr],
    inner_scope: &Scope,
) -> Vec<(ColumnRef, ColumnRef)> {
    let mut pairs = Vec::new();

    for pred in correlation_preds {
        if let Expr::BinOp {
            op: BinOp::Eq,
            left,
            right,
        } = pred
        {
            if let (Some(lc), Some(rc)) = (extract_col_ref(left), extract_col_ref(right)) {
                let l_inner = column_in_scope(lc, inner_scope);
                let r_inner = column_in_scope(rc, inner_scope);

                if l_inner && !r_inner {
                    pairs.push((lc.clone(), rc.clone()));
                } else if r_inner && !l_inner {
                    pairs.push((rc.clone(), lc.clone()));
                }
            }
        }
    }

    pairs
}

/// Build a `Scope` from a `RelExpr` subtree.
///
/// Collects both the visible tables and the output columns to form a
/// complete scope that can be used for classification.
#[must_use]
pub fn build_scope(rel: &RelExpr) -> Scope {
    Scope {
        tables: collect_visible_tables(rel),
        columns: collect_output_columns(rel),
    }
}

/// Extract a `ColumnRef` from an expression if it's a simple column reference.
fn extract_col_ref(expr: &Expr) -> Option<&ColumnRef> {
    match expr {
        Expr::Column(cr) => Some(cr),
        _ => None,
    }
}

/// Classify which side of an equality is inner vs outer.
///
/// Returns `Some((inner_col_ref, outer_col_ref))` or `None` if
/// classification fails (e.g., both sides are in the same scope).
#[must_use]
pub fn classify_eq_sides<'a>(
    left: &'a Expr,
    right: &'a Expr,
    inner_scope: &Scope,
) -> Option<(&'a ColumnRef, &'a ColumnRef)> {
    let lc = extract_col_ref(left)?;
    let rc = extract_col_ref(right)?;
    let l_inner = column_in_scope(lc, inner_scope);
    let r_inner = column_in_scope(rc, inner_scope);

    if l_inner && !r_inner {
        Some((lc, rc))
    } else if r_inner && !l_inner {
        Some((rc, lc))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::expr::ColumnRef;

    fn make_scope(tables: &[&str], columns: &[ColumnRef]) -> Scope {
        Scope {
            tables: tables.iter().map(|s| (*s).to_owned()).collect(),
            columns: columns.to_vec(),
        }
    }

    #[test]
    fn qualified_columns_classified_correctly() {
        // t1.id = t2.id with inner_scope = {t2}
        let inner_scope = make_scope(
            &["t2"],
            &[ColumnRef::qualified("t2", "id")],
        );

        let pred = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("t1", "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("t2", "id"))),
        };

        let (corr, local) = classify_predicates(&[pred], &inner_scope);
        assert_eq!(corr.len(), 1);
        assert_eq!(local.len(), 0);

        let pairs = extract_equi_pairs(&corr, &inner_scope);
        assert_eq!(pairs.len(), 1);
        // inner = t2.id, outer = t1.id
        assert_eq!(pairs[0].0, ColumnRef::qualified("t2", "id"));
        assert_eq!(pairs[0].1, ColumnRef::qualified("t1", "id"));
    }

    #[test]
    fn unqualified_column_in_scope_by_name() {
        let inner_scope = make_scope(
            &["employees"],
            &[
                ColumnRef::new("salary"),
                ColumnRef::new("department_id"),
            ],
        );

        // "salary" is in scope
        assert!(column_in_scope(&ColumnRef::new("salary"), &inner_scope));
        // "name" is not in scope
        assert!(!column_in_scope(&ColumnRef::new("name"), &inner_scope));
    }

    #[test]
    fn mixed_predicates_split_correctly() {
        // a.x = b.x AND b.y > 10
        // inner = {b}
        let inner_scope = make_scope(
            &["b"],
            &[
                ColumnRef::qualified("b", "x"),
                ColumnRef::qualified("b", "y"),
            ],
        );

        let eq_pred = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("a", "x"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("b", "x"))),
        };
        let local_pred = Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::qualified("b", "y"))),
            right: Box::new(Expr::Column(ColumnRef::new("10"))),
        };

        let (corr, local) =
            classify_predicates(&[eq_pred, local_pred], &inner_scope);
        assert_eq!(corr.len(), 1, "should have 1 correlation predicate");
        assert_eq!(local.len(), 1, "should have 1 local predicate");
    }

    #[test]
    fn both_sides_inner_is_local() {
        // b.x = b.y where inner = {b} → both inner, so it's local
        let inner_scope = make_scope(
            &["b"],
            &[
                ColumnRef::qualified("b", "x"),
                ColumnRef::qualified("b", "y"),
            ],
        );

        let pred = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("b", "x"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("b", "y"))),
        };

        let (corr, local) = classify_predicates(&[pred], &inner_scope);
        assert_eq!(corr.len(), 0);
        assert_eq!(local.len(), 1);
    }

    #[test]
    fn classify_eq_sides_correct() {
        let inner_scope = make_scope(
            &["t2"],
            &[ColumnRef::qualified("t2", "customer_id")],
        );

        let left = Expr::Column(ColumnRef::qualified("t2", "customer_id"));
        let right = Expr::Column(ColumnRef::qualified("t1", "customer_id"));

        let result = classify_eq_sides(&left, &right, &inner_scope);
        assert!(result.is_some());
        let (inner, outer) = result.unwrap();
        assert_eq!(inner.table.as_deref(), Some("t2"));
        assert_eq!(outer.table.as_deref(), Some("t1"));
    }

    #[test]
    fn build_scope_from_scan() {
        let rel = RelExpr::Scan {
            table: "orders".to_owned(),
            alias: Some("o".to_owned()),
        };
        let scope = build_scope(&rel);
        assert!(scope.tables.contains(&"orders".to_owned()));
        assert!(scope.tables.contains(&"o".to_owned()));
    }

    #[test]
    fn collect_visible_tables_from_join() {
        let left = RelExpr::Scan {
            table: "lineitem".to_owned(),
            alias: Some("l".to_owned()),
        };
        let right = RelExpr::Scan {
            table: "orders".to_owned(),
            alias: Some("o".to_owned()),
        };
        let join = RelExpr::Join {
            join_type: ra_core::algebra::JoinType::Inner,
            condition: Expr::Const(ra_core::expr::Const::Bool(true)),
            left: Box::new(left),
            right: Box::new(right),
        };
        let tables = collect_visible_tables(&join);
        assert!(tables.contains(&"lineitem".to_owned()));
        assert!(tables.contains(&"l".to_owned()));
        assert!(tables.contains(&"orders".to_owned()));
        assert!(tables.contains(&"o".to_owned()));
    }
}
