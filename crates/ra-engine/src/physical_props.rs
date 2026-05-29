//! Physical-property tracking framework (RFC 0025 MVP).
//!
//! Computes physical properties — ordering, partitioning, and
//! distribution — for the subtrees of an optimized [`RelExpr`].
//! This is a sidecar paralleling [`crate::plan_advice_physical::
//! PhysicalChoices`]: the optimizer produces a `RelExpr` with
//! pure logical structure, and a separate pass computes physical
//! properties from it.
//!
//! # Status
//!
//! This is the **ordering-only MVP** of RFC 0025. The
//! `Partitioning` and `Distribution` enums are defined for
//! forward compatibility but currently never populated;
//! consumers should treat them as `None` / `Single` /
//! `Singleton` defaults.
//!
//! Today's consumers:
//! - [`crate::egraph::result::OptimizationResult::physical_properties`]
//!   exposes the computed map so callers can ask "is this subtree
//!   sorted on column X?".
//!
//! Future consumers:
//! - RFC 0089 (e-graph cost-driven physical lowering): merge-join
//!   cost estimation needs to know whether children are
//!   already sorted on the join keys.
//! - Redundant-`Sort` elimination: if a `Sort` node's input is
//!   already sorted on the requested keys, the `Sort` is
//!   redundant.
//! - Distributed planning: exchange-operator placement uses
//!   `Partitioning` and `Distribution`.
//!
//! # Architectural choice
//!
//! Properties are computed post-extraction rather than tracked
//! inside the e-graph's `RelAnalysis`. Same reasoning as the
//! `PhysicalChoices` sidecar: keeps the e-graph small and fast
//! while still letting downstream code reason about properties.

use std::collections::HashMap;

use ra_core::algebra::{NullOrdering, ProjectionColumn, RelExpr, SortDirection};
use ra_core::expr::Expr;
use serde::{Deserialize, Serialize};

/// Sort-direction-aware reference to a single column's ordering
/// position in a key list.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrderingKey {
    /// Column name (unqualified). Future work: support qualified
    /// references when projection re-qualifies.
    pub column: String,
    /// Ascending or descending.
    pub direction: SortDirection,
    /// NULL placement within the ordering.
    pub nulls: NullOrdering,
}

/// Partitioning (forward-compatibility placeholder).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum Partitioning {
    /// Data is hash-partitioned on the listed columns.
    Hash { columns: Vec<String>, buckets: usize },
    /// Data is range-partitioned on a single column with the
    /// supplied boundary values (string-encoded for portability).
    Range { column: String, boundaries: Vec<String> },
    /// Data is round-robin partitioned across N nodes.
    RoundRobin { nodes: usize },
    /// Data is on a single node (or single partition).
    #[default]
    Single,
}

/// Distribution (forward-compatibility placeholder).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum Distribution {
    /// Same data on every node.
    Replicated,
    /// Data is split across nodes by the partitioning rule.
    Partitioned,
    /// All data on a single node.
    #[default]
    Singleton,
}

/// Physical properties of a single relational expression.
///
/// `ordering` is populated when the expression is provably sorted
/// on the listed columns (in order). An empty `ordering` means
/// "no ordering claim" — the expression *may* still be sorted in
/// practice but we can't prove it from the operator structure.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExprProperties {
    pub ordering: Vec<OrderingKey>,
    pub partitioning: Partitioning,
    pub distribution: Distribution,
}

/// Per-subtree property map keyed by a structural hash of the
/// `RelExpr`. The map is built once after extraction and cached
/// on [`crate::egraph::result::OptimizationResult`].
///
/// Lookup APIs (`for_subtree`, `is_sorted_on`) take a borrowed
/// `RelExpr` reference and use pointer/structural identity so
/// callers don't need to thread keys through their code.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PhysicalProperties {
    /// Properties for the root expression. Most consumers only
    /// need root-level ordering (e.g., "is the final plan
    /// sorted on the user's `ORDER BY`?"); deeper subtree
    /// queries use [`PhysicalProperties::compute_for`].
    pub root: ExprProperties,
}

impl PhysicalProperties {
    /// Compute physical properties for `expr`.
    #[must_use]
    pub fn compute(expr: &RelExpr) -> Self {
        Self {
            root: compute_props(expr),
        }
    }

    /// Compute properties for an arbitrary subtree of an already
    /// optimized plan. Useful for cost-model components that
    /// need to ask "is this child sorted?" without re-running
    /// the full computation.
    #[must_use]
    pub fn compute_for(expr: &RelExpr) -> ExprProperties {
        compute_props(expr)
    }

    /// True if the root expression is provably sorted such that
    /// the supplied `keys` are a prefix of its ordering.
    /// Direction and null-ordering must match exactly.
    #[must_use]
    pub fn is_sorted_on(&self, keys: &[OrderingKey]) -> bool {
        is_prefix(&self.root.ordering, keys)
    }
}

/// Walk `expr` to compute its physical properties. Today only
/// `ordering` is populated; partitioning and distribution stay
/// at their defaults (`Single` / `Singleton`) until future RFCs
/// extend this pass.
fn compute_props(expr: &RelExpr) -> ExprProperties {
    ExprProperties {
        ordering: compute_ordering(expr),
        ..ExprProperties::default()
    }
}

/// Recursively compute the ordering claim for `expr`.
///
/// Operator semantics:
/// - `Sort(keys, _)`: produces ordering = keys (overwriting
///   whatever the input had).
/// - `Filter(_, child)`, `Project(_, child)` (when projection
///   keeps every order column), `Limit(_, _, child)`: preserve
///   `child`'s ordering.
/// - `Distinct(child)`: preserves ordering when input is sorted
///   on a prefix of distinct's grouping; conservative MVP
///   returns empty.
/// - All other operators (`Join`, `Aggregate`, `Union`, ...):
///   no ordering claim.
fn compute_ordering(expr: &RelExpr) -> Vec<OrderingKey> {
    match expr {
        RelExpr::Sort { keys, .. } => keys
            .iter()
            .filter_map(|k| {
                let column = sort_key_column(&k.expr)?;
                Some(OrderingKey {
                    column,
                    direction: k.direction,
                    nulls: k.nulls,
                })
            })
            .collect(),
        RelExpr::Filter { input, .. } | RelExpr::Limit { input, .. } => {
            compute_ordering(input)
        }
        RelExpr::Project { input, columns } => {
            // Conservative: keep only ordering columns that the
            // projection actually emits. If the projection is
            // SELECT * (Wildcard) we preserve fully.
            let child = compute_ordering(input);
            if is_wildcard_projection(columns) {
                return child;
            }
            let projected: std::collections::HashSet<String> = columns
                .iter()
                .filter_map(projected_column_name)
                .collect();
            child
                .into_iter()
                .take_while(|k| {
                    projected
                        .iter()
                        .any(|c| c.eq_ignore_ascii_case(&k.column))
                })
                .collect()
        }
        // No ordering claim for all other variants. Future
        // RFCs (0089) extend this with HashJoin (no ordering
        // on output) vs MergeJoin (ordering on the merge keys
        // preserved on the outer side).
        _ => Vec::new(),
    }
}

/// Extract the underlying column name from a sort-key
/// expression. Handles `Column` and `Column AS alias` shapes;
/// returns `None` for compound expressions (e.g., `lower(x)`)
/// since the e-class equivalence isn't simple to track.
fn sort_key_column(expr: &Expr) -> Option<String> {
    if let Expr::Column(c) = expr {
        Some(c.column.clone())
    } else {
        None
    }
}

/// True when the projection list is a single `Wildcard` /
/// `Column("*")` entry. The wildcard preserves every column,
/// so ordering is preserved fully.
fn is_wildcard_projection(columns: &[ProjectionColumn]) -> bool {
    columns.len() == 1
        && matches!(
            &columns[0].expr,
            Expr::Column(c) if c.column == "*"
        )
}

/// Extract a projected column name when the projection entry
/// is a bare column reference. Compound projections (functions,
/// arithmetic) return `None` — we can't claim the ordering
/// applies to the computed value.
fn projected_column_name(col: &ProjectionColumn) -> Option<String> {
    if let Expr::Column(c) = &col.expr {
        Some(c.column.clone())
    } else {
        None
    }
}

/// True iff `claimed` starts with the same `OrderingKey`s as
/// `wanted`, in order. Direction and NULL ordering must match.
fn is_prefix(claimed: &[OrderingKey], wanted: &[OrderingKey]) -> bool {
    if wanted.len() > claimed.len() {
        return false;
    }
    claimed.iter().zip(wanted.iter()).all(|(a, b)| {
        a.column.eq_ignore_ascii_case(&b.column)
            && a.direction == b.direction
            && a.nulls == b.nulls
    })
}

/// Re-export the per-subtree map type so consumers can build
/// caches keyed on subtree pointer if needed.
pub type SubtreePropertyMap = HashMap<*const RelExpr, ExprProperties>;

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::{JoinType, SortKey};
    use ra_core::expr::{ColumnRef, Const, Expr};

    fn scan(name: &str) -> RelExpr {
        RelExpr::Scan {
            table: name.into(),
            alias: None,
        }
    }

    fn sort_asc(child: RelExpr, columns: &[&str]) -> RelExpr {
        let keys = columns
            .iter()
            .map(|c| SortKey {
                expr: Expr::Column(ColumnRef::new(*c)),
                direction: SortDirection::Asc,
                nulls: NullOrdering::Last,
            })
            .collect();
        RelExpr::Sort {
            keys,
            input: Box::new(child),
        }
    }

    fn key(name: &str) -> OrderingKey {
        OrderingKey {
            column: name.into(),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }
    }

    #[test]
    fn scan_has_no_ordering_claim() {
        let p = PhysicalProperties::compute(&scan("t"));
        assert!(p.root.ordering.is_empty());
    }

    #[test]
    fn sort_produces_its_keys() {
        let q = sort_asc(scan("t"), &["a", "b"]);
        let p = PhysicalProperties::compute(&q);
        assert_eq!(p.root.ordering, vec![key("a"), key("b")]);
    }

    #[test]
    fn filter_preserves_input_ordering() {
        let inner = sort_asc(scan("t"), &["a"]);
        let q = RelExpr::Filter {
            predicate: Expr::Const(Const::Bool(true)),
            input: Box::new(inner),
        };
        let p = PhysicalProperties::compute(&q);
        assert_eq!(p.root.ordering, vec![key("a")]);
    }

    #[test]
    fn limit_preserves_input_ordering() {
        let inner = sort_asc(scan("t"), &["a"]);
        let q = RelExpr::Limit {
            count: 10,
            offset: 0,
            input: Box::new(inner),
        };
        let p = PhysicalProperties::compute(&q);
        assert_eq!(p.root.ordering, vec![key("a")]);
    }

    #[test]
    fn wildcard_projection_preserves_ordering() {
        let inner = sort_asc(scan("t"), &["a"]);
        let q = RelExpr::Project {
            columns: vec![ra_core::algebra::ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("*")),
                alias: None,
            }],
            input: Box::new(inner),
        };
        let p = PhysicalProperties::compute(&q);
        assert_eq!(p.root.ordering, vec![key("a")]);
    }

    #[test]
    fn projection_keeps_ordering_columns_only() {
        // Sort on (a, b), projection drops `b`: the prefix
        // (a) is preserved; b is dropped.
        let inner = sort_asc(scan("t"), &["a", "b"]);
        let q = RelExpr::Project {
            columns: vec![ra_core::algebra::ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("a")),
                alias: None,
            }],
            input: Box::new(inner),
        };
        let p = PhysicalProperties::compute(&q);
        assert_eq!(p.root.ordering, vec![key("a")]);
    }

    #[test]
    fn projection_dropping_lead_loses_ordering() {
        // Sort on (a, b), projection drops `a`: the lead
        // column is gone, so no prefix survives.
        let inner = sort_asc(scan("t"), &["a", "b"]);
        let q = RelExpr::Project {
            columns: vec![ra_core::algebra::ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("b")),
                alias: None,
            }],
            input: Box::new(inner),
        };
        let p = PhysicalProperties::compute(&q);
        assert!(p.root.ordering.is_empty());
    }

    #[test]
    fn join_loses_ordering() {
        let q = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(sort_asc(scan("a"), &["x"])),
            right: Box::new(sort_asc(scan("b"), &["x"])),
        };
        let p = PhysicalProperties::compute(&q);
        assert!(p.root.ordering.is_empty());
    }

    #[test]
    fn aggregate_loses_ordering() {
        let q = RelExpr::Aggregate {
            group_by: Vec::new(),
            aggregates: Vec::new(),
            input: Box::new(sort_asc(scan("t"), &["a"])),
        };
        let p = PhysicalProperties::compute(&q);
        assert!(p.root.ordering.is_empty());
    }

    #[test]
    fn is_sorted_on_prefix_match() {
        let q = sort_asc(scan("t"), &["a", "b", "c"]);
        let p = PhysicalProperties::compute(&q);
        assert!(p.is_sorted_on(&[key("a")]));
        assert!(p.is_sorted_on(&[key("a"), key("b")]));
        assert!(p.is_sorted_on(&[key("a"), key("b"), key("c")]));
    }

    #[test]
    fn is_sorted_on_rejects_wrong_order() {
        let q = sort_asc(scan("t"), &["a", "b"]);
        let p = PhysicalProperties::compute(&q);
        // Wanted (b, a) is NOT a prefix of claimed (a, b).
        assert!(!p.is_sorted_on(&[key("b"), key("a")]));
    }

    #[test]
    fn is_sorted_on_rejects_longer_request() {
        let q = sort_asc(scan("t"), &["a"]);
        let p = PhysicalProperties::compute(&q);
        assert!(!p.is_sorted_on(&[key("a"), key("b")]));
    }
}
