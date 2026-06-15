//! SQL/PGQ (`GRAPH_TABLE`, SQL:2023) pipeline coverage.
//!
//! PostgreSQL 19devel does not yet ship the SQL/PGQ grammar, so a
//! `GRAPH_TABLE` query is rejected by PG's raw parser before Ra's planner
//! hook ever sees it — there is no way to exercise the feature end-to-end
//! against that build. These tests cover the part of the pipeline that *is*
//! reachable and owned by Ra: Lime parse (`sql_to_relexpr`) and the optimizer.
//!
//! `GRAPH_TABLE` is opaque to the rewrite rules (the `MATCH` pattern is not
//! representable in the e-graph), so the optimizer bypasses equality
//! saturation and returns the expression unchanged. These tests assert that
//! (a) the SQL parses into a `RelExpr::GraphTable` with the expected pattern,
//! and (b) optimization preserves it (a passthrough, no panic, no rewrite).

use ra_core::algebra::{EdgeDirection, GraphPatternElement, RelExpr};
use ra_engine::Optimizer;

fn find_graph_table(e: &RelExpr) -> Option<&RelExpr> {
    if matches!(e, RelExpr::GraphTable { .. }) {
        return Some(e);
    }
    e.children().into_iter().find_map(find_graph_table)
}

/// Parse `sql`, assert it contains a `GraphTable`, optimize it, and assert the
/// `GraphTable` survives optimization with the same graph name, pattern length
/// and column count. Returns nothing; panics on failure.
fn assert_pgq_roundtrip(sql: &str, pattern_len: usize, columns_len: usize) {
    let parsed = ra_parser::sql_to_relexpr(sql).expect("GRAPH_TABLE should parse");
    let before = find_graph_table(&parsed).expect("parse should yield a GraphTable");
    let RelExpr::GraphTable { graph, pattern, columns, .. } = before else {
        unreachable!()
    };
    let (graph, plen, clen) = (graph.clone(), pattern.len(), columns.len());
    assert_eq!(plen, pattern_len, "pattern length for: {sql}");
    assert_eq!(clen, columns_len, "columns length for: {sql}");

    let optimized = Optimizer::new().optimize(&parsed).expect("optimize must not fail on GRAPH_TABLE");
    let after = find_graph_table(&optimized)
        .expect("GraphTable must survive optimization (passthrough)");
    let RelExpr::GraphTable { graph: g2, pattern: p2, columns: c2, .. } = after else {
        unreachable!()
    };
    assert_eq!(*g2, graph, "graph name preserved: {sql}");
    assert_eq!(p2.len(), plen, "pattern preserved: {sql}");
    assert_eq!(c2.len(), clen, "columns preserved: {sql}");
}

#[test]
fn pgq_right_edge_with_labels() {
    assert_pgq_roundtrip(
        "SELECT * FROM GRAPH_TABLE (g MATCH (a IS person)-[e IS knows]->(b IS person) \
         COLUMNS (a.name AS who, b.name AS friend))",
        3,
        2,
    );
}

#[test]
fn pgq_left_and_undirected_edges() {
    assert_pgq_roundtrip(
        "SELECT * FROM GRAPH_TABLE (g MATCH (a)<-[e]-(b)-[f]-(c) COLUMNS (a.id AS x)) AS gt",
        5,
        1,
    );
}

#[test]
fn pgq_optimize_is_passthrough_direction() {
    // The first edge must remain a right-directed edge after optimization.
    let sql = "SELECT * FROM GRAPH_TABLE (g MATCH (a)-[e]->(b) COLUMNS (a.id AS x))";
    let optimized = Optimizer::new()
        .optimize(&ra_parser::sql_to_relexpr(sql).expect("parse"))
        .expect("optimize");
    let gt = find_graph_table(&optimized).expect("GraphTable present");
    let RelExpr::GraphTable { pattern, .. } = gt else { unreachable!() };
    assert!(matches!(pattern[0], GraphPatternElement::Vertex { .. }));
    assert!(matches!(
        pattern[1],
        GraphPatternElement::Edge { direction: EdgeDirection::Right, .. }
    ));
}
