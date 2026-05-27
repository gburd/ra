//! Shape-aware analysis of join graphs.
//!
//! Lesson (iii) of the GEQO comparison: `PostgreSQL`'s `gimme_tree`
//! consults `desirable_join` (an O(1) "do these relations share a
//! column?" check) before joining; the GA explores tours, but the
//! evaluator picks legal shapes. The Ra equivalent is one level
//! higher — pre-classify the join graph and tell the rule advisor
//! whether join-reordering rules are *useful* on this query at all.
//!
//! This module produces a [`JoinGraphShape`] from a [`RelExpr`].
//! The shape is a small, copyable summary (no string allocations
//! after construction) that the rule advisor consults to demote
//! rule categories that have no plausible work to do — for example,
//! join-reordering rules on a query whose join graph is a unique
//! spanning tree (every join order is forced) or contains LATERAL
//! references (which forbid most reorderings outright).
//!
//! **The shape is advisory**, not correctness-critical: a rule that
//! the shape suggests is "useless" can still produce a valid plan,
//! just typically a costlier one. Demoting it saves rule-application
//! budget without changing extraction semantics.

use ra_core::algebra::RelExpr;

use crate::join_graph::JoinGraph;

/// Compact, copyable summary of a query's join-graph topology.
///
/// Construct with [`JoinGraphShape::from_expr`]. All fields are
/// derived from one walk of the input [`RelExpr`] tree plus one
/// pass over the [`JoinGraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JoinGraphShape {
    /// Number of distinct base tables involved in joins.
    pub vertex_count: usize,
    /// Number of distinct join edges (table pairs with at least
    /// one shared join predicate).
    pub edge_count: usize,
    /// Number of weakly-connected components in the join graph.
    /// `0` for a query with no joins; `1` for the typical
    /// connected-graph case; `>1` indicates Cartesian / cross
    /// products between sub-graphs.
    pub component_count: usize,
    /// Maximum table-clique size — the largest set of tables that
    /// are all pairwise joinable. A clique of size `k` admits
    /// `k!` enumeration choices for that sub-graph.
    pub max_clique_size: usize,
    /// Whether the query contains any `LATERAL` reference
    /// (`Unnest`/`TableFunction` with an `input`). LATERAL forces
    /// a parent-before-child ordering and forbids most join
    /// reorderings.
    pub has_lateral: bool,
    /// Whether the join graph is a unique spanning tree —
    /// connected, acyclic, with `edge_count == vertex_count - 1`.
    /// A unique spanning tree has no choice of join order beyond
    /// the labeling of edges.
    pub has_unique_spanning: bool,
    /// Whether every legal join ordering is forced by the
    /// structure of the query. Derived from the other fields:
    /// LATERAL anywhere, ≤ 2 vertices, or a unique spanning tree
    /// → no productive reorderings exist. Rule categories that do
    /// nothing but reorder joins should be skipped on these
    /// queries.
    pub all_orderings_forced: bool,
}

impl JoinGraphShape {
    /// Build the shape summary for a query expression.
    ///
    /// Cost: O(n) where n is the number of `RelExpr` nodes.
    /// Constructs a [`JoinGraph`] internally; if the caller
    /// already has one, [`JoinGraphShape::from_graph_and_expr`]
    /// is cheaper.
    #[must_use]
    pub fn from_expr(expr: &RelExpr) -> Self {
        let graph = JoinGraph::from_expr(expr);
        Self::from_graph_and_expr(&graph, expr)
    }

    /// Build the shape summary using a pre-built [`JoinGraph`].
    /// Used by callers (such as the saturation loop) that already
    /// computed the join graph for other purposes.
    #[must_use]
    pub fn from_graph_and_expr(graph: &JoinGraph, expr: &RelExpr) -> Self {
        let vertex_count = graph.tables().len();
        let edge_count = graph.edge_count();
        let component_count = count_components(graph);
        let max_clique_size = max_clique_size(graph);
        let has_lateral = contains_lateral(expr);
        let has_unique_spanning = component_count == 1
            && vertex_count > 0
            && edge_count == vertex_count.saturating_sub(1);
        let all_orderings_forced = has_lateral
            || vertex_count <= 2
            || has_unique_spanning;
        Self {
            vertex_count,
            edge_count,
            component_count,
            max_clique_size,
            has_lateral,
            has_unique_spanning,
            all_orderings_forced,
        }
    }

    /// Returns the labels of [rule groups][crate::rewrite::AnnotatedRuleGroup]
    /// the rule advisor can safely demote on this query. The
    /// caller should treat the result as advisory: removing a
    /// rule from saturation means the optimizer won't consider
    /// transformations that group encodes, but extraction will
    /// still produce a correct (typically costlier) plan.
    ///
    /// Today we recognise one category: `"join-reordering"` is
    /// pure shuffle of the join order. When every legal ordering
    /// is forced ([`Self::all_orderings_forced`]), running these
    /// rules consumes saturation budget for no plan-quality
    /// payoff, so the advisor should skip them.
    #[must_use]
    pub fn redundant_rule_groups(&self) -> &'static [&'static str] {
        if self.all_orderings_forced {
            &["join-reordering"]
        } else {
            &[]
        }
    }
}

/// Count weakly-connected components in the join graph via a
/// flood-fill from each unvisited table.
fn count_components(graph: &JoinGraph) -> usize {
    let tables: Vec<&String> = graph.tables().iter().collect();
    if tables.is_empty() {
        return 0;
    }
    let mut visited = std::collections::HashSet::new();
    let mut components = 0;
    for start in &tables {
        if visited.contains(start.as_str()) {
            continue;
        }
        components += 1;
        let mut stack: Vec<String> = vec![(*start).clone()];
        while let Some(t) = stack.pop() {
            if !visited.insert(t.clone()) {
                continue;
            }
            for other in &tables {
                if !visited.contains(other.as_str())
                    && graph.can_join(&t, other)
                {
                    stack.push((*other).clone());
                }
            }
        }
    }
    components
}

/// Find the size of the largest clique. Approximate — we use a
/// greedy expansion from each vertex rather than a true
/// maximum-clique solver (which is NP-hard). The greedy answer
/// is a lower bound on the true max clique size, which is fine
/// for this advisory predicate: under-counting clique size only
/// causes us to *miss* an opportunity to enable a join-reorder
/// rule, never to incorrectly disable one.
fn max_clique_size(graph: &JoinGraph) -> usize {
    let tables: Vec<&String> = graph.tables().iter().collect();
    if tables.is_empty() {
        return 0;
    }
    let mut best = 1;
    for &start in &tables {
        let mut clique: Vec<&String> = vec![start];
        for &candidate in &tables {
            if clique.iter().any(|t| t.as_str() == candidate.as_str()) {
                continue;
            }
            // Add candidate iff it joins with every member of the
            // current clique.
            if clique.iter().all(|m| graph.can_join(m, candidate)) {
                clique.push(candidate);
            }
        }
        best = best.max(clique.len());
    }
    best
}

/// Walk the `RelExpr` looking for any LATERAL marker. We treat
/// `Unnest` and `TableFunction` with non-None `input` as LATERAL,
/// which matches `ra-parser`'s `sql_to_relexpr` lowering.
fn contains_lateral(expr: &RelExpr) -> bool {
    match expr {
        RelExpr::Unnest { input: Some(_), .. }
        | RelExpr::TableFunction { input: Some(_), .. } => true,
        // Anything with children: recurse via the `children`
        // helper. RelExpr has many variants (BitmapAnd, IndexScan,
        // RowPattern, parallel variants, DML, ...); we fall back
        // to a structural recursion rather than enumerate them.
        other => other.children().iter().any(|c| contains_lateral(c)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::JoinType;
    use ra_core::expr::{BinOp, ColumnRef, Expr};

    fn col(table: &str, c: &str) -> Expr {
        Expr::Column(ColumnRef::qualified(table, c))
    }

    fn eq(left: Expr, right: Expr) -> Expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn scan(name: &str) -> RelExpr {
        RelExpr::Scan {
            table: name.to_string(),
            alias: None,
        }
    }

    fn join(l: RelExpr, r: RelExpr, cond: Expr) -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: cond,
            left: Box::new(l),
            right: Box::new(r),
        }
    }

    #[test]
    fn single_table_has_one_vertex_no_orderings() {
        let shape = JoinGraphShape::from_expr(&scan("users"));
        assert_eq!(shape.vertex_count, 1);
        assert_eq!(shape.edge_count, 0);
        // 0 components for a non-join query is the spec; the
        // `all_orderings_forced` flag covers it via the n<=2 case.
        assert!(shape.all_orderings_forced);
    }

    #[test]
    fn two_table_join_is_forced() {
        let q = join(scan("a"), scan("b"), eq(col("a", "id"), col("b", "id")));
        let shape = JoinGraphShape::from_expr(&q);
        assert_eq!(shape.vertex_count, 2);
        assert_eq!(shape.edge_count, 1);
        assert_eq!(shape.component_count, 1);
        assert!(shape.all_orderings_forced, "n<=2 always forced");
    }

    #[test]
    fn three_table_chain_is_unique_spanning_and_forced() {
        // a -> b -> c (chain): two edges, three vertices,
        // unique spanning tree, no reorderings.
        let q = join(
            join(scan("a"), scan("b"), eq(col("a", "id"), col("b", "a_id"))),
            scan("c"),
            eq(col("b", "id"), col("c", "b_id")),
        );
        let shape = JoinGraphShape::from_expr(&q);
        assert_eq!(shape.vertex_count, 3);
        assert_eq!(shape.edge_count, 2);
        assert!(shape.has_unique_spanning);
        assert!(shape.all_orderings_forced);
    }

    #[test]
    fn three_table_triangle_admits_reorderings() {
        // Triangle: 3 edges, 3 vertices — clique of 3.
        // Each pair of tables joins on `id`.
        let chain = join(
            join(scan("a"), scan("b"), eq(col("a", "id"), col("b", "id"))),
            scan("c"),
            eq(col("a", "id"), col("c", "id")),
        );
        let triangle = join(
            chain,
            scan("c"),
            eq(col("b", "id"), col("c", "id")),
        );
        let shape = JoinGraphShape::from_expr(&triangle);
        assert_eq!(shape.vertex_count, 3, "shape: {shape:?}");
        // 3-clique → enumeration is meaningful.
        assert!(!shape.all_orderings_forced, "shape: {shape:?}");
    }

    #[test]
    fn redundant_groups_advice_tracks_orderings_forced() {
        // Forced shape: should advise skipping join-reordering.
        let q = join(scan("a"), scan("b"), eq(col("a", "id"), col("b", "id")));
        let forced = JoinGraphShape::from_expr(&q);
        assert!(forced.all_orderings_forced);
        assert_eq!(forced.redundant_rule_groups(), &["join-reordering"]);

        // Non-forced shape: nothing to demote.
        let chain = join(
            join(scan("a"), scan("b"), eq(col("a", "id"), col("b", "id"))),
            scan("c"),
            eq(col("a", "id"), col("c", "id")),
        );
        let triangle = join(
            chain,
            scan("c"),
            eq(col("b", "id"), col("c", "id")),
        );
        let shape = JoinGraphShape::from_expr(&triangle);
        assert!(!shape.all_orderings_forced);
        let advice: &[&str] = shape.redundant_rule_groups();
        assert!(advice.is_empty(), "advisory should be empty: {advice:?}");
    }
}
