//! RFC 0027 (Runtime Filters) detection-layer tests.
//!
//! Exercises `RuntimeFilters::detect` end-to-end through the
//! optimizer: an optimized plan with a star-schema equi-join and
//! supplied statistics should surface a runtime-filter
//! opportunity on `OptimizationResult.runtime_filters`.

#![expect(
    clippy::unwrap_used,
    reason = "test code; unwrap is the conventional shorthand for surfacing failures in tests"
)]

use std::collections::HashMap;

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Expr};
use ra_core::statistics::{ColumnStats, Statistics};
use ra_engine::runtime_filters::RuntimeFilters;

fn scan(name: &str) -> RelExpr {
    RelExpr::Scan {
        table: name.into(),
        alias: None,
    }
}

fn eq_join(left: RelExpr, right: RelExpr, l: &str, r: &str, col: &str) -> RelExpr {
    RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified(l, col))),
            right: Box::new(Expr::Column(ColumnRef::qualified(r, col))),
        },
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn stats_with_ndv(rows: f64, column: &str, ndv: f64) -> Statistics {
    let mut s = Statistics::new(rows);
    s.avg_row_size = 64;
    let cs = ColumnStats {
        distinct_count: ndv,
        null_fraction: 0.0,
        min_value: None,
        max_value: None,
        avg_length: None,
        histogram: None,
        correlation: None,
        most_common_values: None,
        most_common_freqs: None,
    };
    s.columns.insert(column.into(), cs);
    s
}

#[test]
fn star_schema_join_surfaces_opportunity() {
    // Filtered dimension: dim has 50 rows / 50 distinct keys
    // (e.g. after WHERE category = 'Electronics'). fact has 5M
    // rows but 500 distinct keys total. A bloom filter built on
    // dim's 50 keys filters out ~90% of fact rows
    // (selectivity 50/500 = 0.1), a clear win.
    let q = eq_join(scan("dim"), scan("fact"), "dim", "fact", "key");
    let mut stats = HashMap::new();
    stats.insert("dim".into(), stats_with_ndv(50.0, "key", 50.0));
    stats.insert("fact".into(), stats_with_ndv(5_000_000.0, "key", 500.0));

    let rf = RuntimeFilters::detect(&q, &stats);
    assert!(!rf.is_empty(), "expected a runtime-filter opportunity");
    let o = rf.probe_for("fact").unwrap();
    assert_eq!(o.build_table, "dim");
    assert_eq!(o.probe_table, "fact");
}

#[test]
fn similar_cardinality_join_no_opportunity() {
    // Two tables of similar size with matching NDV — a bloom
    // filter has no selectivity benefit, so the cost gate
    // rejects it.
    let q = eq_join(scan("a"), scan("b"), "a", "b", "id");
    let mut stats = HashMap::new();
    stats.insert("a".into(), stats_with_ndv(10_000.0, "id", 10_000.0));
    stats.insert("b".into(), stats_with_ndv(12_000.0, "id", 12_000.0));

    let rf = RuntimeFilters::detect(&q, &stats);
    assert!(
        rf.is_empty(),
        "non-selective join should not surface an opportunity",
    );
}

#[test]
fn full_outer_join_no_opportunity() {
    let q = RelExpr::Join {
        join_type: JoinType::FullOuter,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("dim", "key"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("fact", "key"))),
        },
        left: Box::new(scan("dim")),
        right: Box::new(scan("fact")),
    };
    let mut stats = HashMap::new();
    stats.insert("dim".into(), stats_with_ndv(500.0, "key", 500.0));
    stats.insert("fact".into(), stats_with_ndv(5_000_000.0, "key", 500.0));

    let rf = RuntimeFilters::detect(&q, &stats);
    assert!(
        rf.is_empty(),
        "full outer join must not produce a runtime filter",
    );
}

#[test]
fn no_stats_no_opportunity() {
    let q = eq_join(scan("dim"), scan("fact"), "dim", "fact", "key");
    let rf = RuntimeFilters::detect(&q, &HashMap::new());
    assert!(rf.is_empty());
}
