#![expect(
    clippy::unwrap_used,
    reason = "test code; unwrap is the conventional shorthand for surfacing failures in tests"
)]
//! Plan-advice round-trip oracle test.
//!
//! Equivalent to PG's `src/test/modules/test_plan_advice/` test
//! module. For each query, the test:
//!
//! 1. Optimizes once without supplied advice.
//! 2. Generates plan advice from the resulting `RelExpr` via
//!    [`ra_engine::plan_advice_emit::emit_advice`].
//! 3. Optimizes a second time with the generated advice supplied
//!    via `OptimizerConfig.plan_advice`.
//! 4. Asserts both plans contain the same set of base tables and
//!    the same join-tree shape (which is what the supplied advice
//!    constrains).
//!
//! The test is constrained to **structural equivalence** rather
//! than exact `RelExpr` equality because the second optimization
//! may apply different non-join rewrites (predicate pushdown,
//! projection pruning, ...) than the first; the contract of
//! plan advice is that the constrained dimensions match, not
//! that every rewrite is identical.

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Expr};
use ra_engine::plan_advice_emit::emit_advice;
use ra_engine::{Optimizer, OptimizerConfig};
use ra_plan_advice::render_advice;

fn scan(name: &str) -> RelExpr {
    RelExpr::Scan { table: name.into(), alias: None }
}

fn eq_join(left: RelExpr, right: RelExpr, l: &str, r: &str) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified(l, "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified(r, "id"))),
        },
        left: Box::new(left),
        right: Box::new(right),
    }
}

/// Collect base-table names from a `RelExpr` in left-to-right
/// scan order.
fn collect_tables(expr: &RelExpr) -> Vec<String> {
    fn walk(e: &RelExpr, out: &mut Vec<String>) {
        if let RelExpr::Scan { table, alias } = e {
            out.push(alias.clone().unwrap_or_else(|| table.clone()));
        } else {
            for child in e.children() {
                walk(child, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(expr, &mut out);
    out
}

/// Simple structural fingerprint: every `Join` becomes `J`,
/// every `Scan` becomes its alias name. Subexpressions that
/// don't appear in PG's join-problem definition (Filter,
/// Project, etc.) are skipped. This is the dimension plan
/// advice constrains.
fn join_shape(expr: &RelExpr) -> String {
    fn walk(e: &RelExpr, buf: &mut String) {
        match e {
            RelExpr::Join { left, right, .. } => {
                buf.push('(');
                walk(left, buf);
                buf.push(' ');
                walk(right, buf);
                buf.push(')');
            }
            RelExpr::Scan { table, alias } => {
                let n = alias.as_deref().unwrap_or(table);
                buf.push_str(n);
            }
            // For pass-through wrappers, descend.
            RelExpr::Filter { input, .. }
            | RelExpr::Project { input, .. }
            | RelExpr::Sort { input, .. }
            | RelExpr::Limit { input, .. }
            | RelExpr::Distinct { input } => walk(input, buf),
            other => {
                // Other variants (set ops, aggregates, CTEs)
                // start new join problems; just descend
                // through children.
                buf.push('[');
                for (i, c) in other.children().iter().enumerate() {
                    if i > 0 {
                        buf.push(' ');
                    }
                    walk(c, buf);
                }
                buf.push(']');
            }
        }
    }
    let mut buf = String::new();
    walk(expr, &mut buf);
    buf
}

/// Run the round-trip and assert structural equivalence.
fn assert_round_trip(query: &RelExpr, label: &str) {
    let baseline = Optimizer::new().optimize(query).unwrap();

    let advice = emit_advice(&baseline);
    let advice_str = render_advice(&advice);

    let config = OptimizerConfig {
        plan_advice: Some(advice_str.clone()),
        ..OptimizerConfig::default()
    };
    let pinned = Optimizer::with_config(config).optimize(query).unwrap();

    // Same set of base tables.
    let mut a_tables = collect_tables(&baseline);
    let mut b_tables = collect_tables(&pinned);
    a_tables.sort();
    b_tables.sort();
    assert_eq!(
        a_tables, b_tables,
        "{label}: tables differ\n  advice={advice_str:?}",
    );

    // Join-shape equivalence: the dimension plan advice
    // constrains. We don't compare every rewrite because
    // the second pass can legitimately apply different
    // non-join rewrites (e.g. additional filter pushdowns).
    let a_shape = join_shape(&baseline);
    let b_shape = join_shape(&pinned);
    assert_eq!(
        a_shape, b_shape,
        "{label}: join shape differs\n  advice={advice_str:?}\n  baseline={a_shape}\n  pinned  ={b_shape}",
    );
}

#[test]
fn round_trip_single_scan() {
    assert_round_trip(&scan("t"), "single_scan");
}

#[test]
fn round_trip_two_table_join() {
    let q = eq_join(scan("a"), scan("b"), "a", "b");
    assert_round_trip(&q, "two_table_join");
}

#[test]
fn round_trip_three_table_chain() {
    let q = eq_join(
        eq_join(scan("a"), scan("b"), "a", "b"),
        scan("c"),
        "a",
        "c",
    );
    assert_round_trip(&q, "three_table_chain");
}

#[test]
fn round_trip_four_table_chain() {
    let q = eq_join(
        eq_join(
            eq_join(scan("a"), scan("b"), "a", "b"),
            scan("c"),
            "a",
            "c",
        ),
        scan("d"),
        "a",
        "d",
    );
    assert_round_trip(&q, "four_table_chain");
}

#[test]
fn round_trip_bushy_join() {
    // (a JOIN b) JOIN (c JOIN d)
    let bc = eq_join(scan("c"), scan("d"), "c", "d");
    let q = eq_join(eq_join(scan("a"), scan("b"), "a", "b"), bc, "a", "c");
    assert_round_trip(&q, "bushy_4_table");
}
