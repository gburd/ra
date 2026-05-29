#![expect(
    clippy::unwrap_used,
    reason = "test code; unwrap is the conventional shorthand for surfacing failures in tests"
)]
#![expect(
    clippy::panic,
    reason = "test code; panic is how we report a failed expectation"
)]
//! Test that `OptimizationResult.physical_choices` is populated
//! cost-driven even without supplied advice, and that supplied
//! advice always wins over cost-driven defaults.
//!
//! See RFC 0087 (`rfcs/text/0087-physical-operator-selection.md`)
//! for the design.

use ra_core::algebra::{JoinType, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::statistics::{IndexStats, Statistics};
use ra_engine::plan_advice_physical::{
    JoinInnerStrategy, ScanStrategy,
};
use ra_engine::{Optimizer, OptimizerConfig};

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

fn filter_eq(input: RelExpr, table: &str, column: &str, value: i64) -> RelExpr {
    RelExpr::Filter {
        predicate: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified(table, column))),
            right: Box::new(Expr::Const(Const::Int(value))),
        },
        input: Box::new(input),
    }
}

fn medium_table_stats() -> Statistics {
    let mut stats = Statistics::new(10_000.0);
    stats.avg_row_size = 64;
    stats.total_size = 10_000 * 64;
    stats
}

fn tiny_table_stats() -> Statistics {
    let mut stats = Statistics::new(50.0);
    stats.avg_row_size = 64;
    stats
}

fn add_btree_index(stats: &mut Statistics, name: &str, columns: Vec<&str>) {
    let columns: Vec<String> = columns.into_iter().map(String::from).collect();
    let idx = IndexStats::new(columns, ra_core::facts::IndexType::BTree);
    stats.indexes.insert(name.to_string(), idx);
}

#[test]
fn cost_driven_join_picks_hash_for_equi_joins() {
    // No advice supplied — the optimizer should pick HASH for
    // an equi-join's inner side.
    let q = eq_join(scan("a"), scan("b"), "a", "b");
    let result = Optimizer::new().optimize_bounded(&q).unwrap();
    assert_eq!(
        result.physical_choices.join_for("b"),
        Some(&JoinInnerStrategy::Hash),
        "equi-join should default to Hash",
    );
}

#[test]
fn supplied_advice_overrides_cost_driven_default() {
    // Even though the cost-driven default would pick Hash for
    // this equi-join, MERGE_JOIN_PLAIN advice should win.
    let q = eq_join(scan("a"), scan("b"), "a", "b");
    let config = OptimizerConfig {
        plan_advice: Some("MERGE_JOIN_PLAIN(b)".into()),
        ..OptimizerConfig::default()
    };
    let result = Optimizer::with_config(config)
        .optimize_bounded(&q)
        .unwrap();
    assert_eq!(
        result.physical_choices.join_for("b"),
        Some(&JoinInnerStrategy::MergeJoinPlain),
        "supplied advice must win",
    );
}

#[test]
fn cost_driven_scan_picks_seq_when_no_stats_available() {
    let q = scan("t");
    let result = Optimizer::new().optimize_bounded(&q).unwrap();
    assert_eq!(
        result.physical_choices.scan_for("t"),
        Some(&ScanStrategy::Seq),
        "no stats → defaults to Seq",
    );
}

#[test]
fn cost_driven_scan_picks_seq_for_small_table() {
    let q = filter_eq(scan("t"), "t", "id", 42);
    let mut opt = Optimizer::new();
    let mut stats = tiny_table_stats();
    add_btree_index(&mut stats, "t_id_idx", vec!["id"]);
    opt.add_table_stats("t", stats);

    let result = opt.optimize_bounded(&q).unwrap();
    // 50 rows < SMALL_TABLE_ROW_THRESHOLD (200): seq scan is
    // the cost-correct choice even though an index on `id` is
    // available.
    assert_eq!(
        result.physical_choices.scan_for("t"),
        Some(&ScanStrategy::Seq),
        "tiny table should prefer SeqScan even with an index",
    );
}

#[test]
fn cost_driven_scan_picks_index_for_indexed_column_filter() {
    let q = filter_eq(scan("t"), "t", "id", 42);
    let mut opt = Optimizer::new();
    let mut stats = medium_table_stats();
    add_btree_index(&mut stats, "t_id_idx", vec!["id"]);
    opt.add_table_stats("t", stats);

    let result = opt.optimize_bounded(&q).unwrap();
    match result.physical_choices.scan_for("t") {
        Some(ScanStrategy::Index { name, .. }) => {
            assert_eq!(name, "t_id_idx");
        }
        other => panic!(
            "medium table with index on filtered column should prefer Index; got {other:?}",
        ),
    }
}

#[test]
fn cost_driven_scan_picks_seq_when_filter_doesnt_match_index() {
    // Index on `name`, but filter on `id` — no useful index.
    let q = filter_eq(scan("t"), "t", "id", 42);
    let mut opt = Optimizer::new();
    let mut stats = medium_table_stats();
    add_btree_index(&mut stats, "t_name_idx", vec!["name"]);
    opt.add_table_stats("t", stats);

    let result = opt.optimize_bounded(&q).unwrap();
    assert_eq!(
        result.physical_choices.scan_for("t"),
        Some(&ScanStrategy::Seq),
        "indexes that don't cover the filter column should be ignored",
    );
}

fn filter_and_eq(input: RelExpr, table: &str, l_col: &str, l: i64, r_col: &str, r: i64) -> RelExpr {
    RelExpr::Filter {
        predicate: Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified(table, l_col))),
                right: Box::new(Expr::Const(Const::Int(l))),
            }),
            right: Box::new(Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified(table, r_col))),
                right: Box::new(Expr::Const(Const::Int(r))),
            }),
        },
        input: Box::new(input),
    }
}

#[test]
fn cost_driven_scan_picks_compound_index_when_full_prefix_matches() {
    // Compound index on (a, b, c). Predicate touches a AND b.
    // Should pick the compound index since 2-column prefix is
    // covered.
    let q = filter_and_eq(scan("t"), "t", "a", 1, "b", 2);
    let mut opt = Optimizer::new();
    let mut stats = medium_table_stats();
    add_btree_index(&mut stats, "t_abc", vec!["a", "b", "c"]);
    opt.add_table_stats("t", stats);

    let result = opt.optimize_bounded(&q).unwrap();
    match result.physical_choices.scan_for("t") {
        Some(ScanStrategy::Index { name, .. }) => assert_eq!(name, "t_abc"),
        other => panic!("expected compound index; got {other:?}"),
    }
}

#[test]
fn cost_driven_scan_prefers_longer_prefix_match() {
    // Two indexes: t_a on (a) and t_ab on (a, b). Predicate
    // touches a AND b → t_ab covers more, should win.
    let q = filter_and_eq(scan("t"), "t", "a", 1, "b", 2);
    let mut opt = Optimizer::new();
    let mut stats = medium_table_stats();
    add_btree_index(&mut stats, "t_a", vec!["a"]);
    add_btree_index(&mut stats, "t_ab", vec!["a", "b"]);
    opt.add_table_stats("t", stats);

    let result = opt.optimize_bounded(&q).unwrap();
    match result.physical_choices.scan_for("t") {
        Some(ScanStrategy::Index { name, .. }) => assert_eq!(name, "t_ab"),
        other => panic!("expected longer-prefix index; got {other:?}"),
    }
}

#[test]
fn cost_driven_scan_skips_compound_index_when_leading_column_missing() {
    // Compound index on (a, b). Predicate only touches b
    // (no equality on a). The leading column isn't covered —
    // this index isn't useful for B-tree access.
    let q = filter_eq(scan("t"), "t", "b", 5);
    let mut opt = Optimizer::new();
    let mut stats = medium_table_stats();
    add_btree_index(&mut stats, "t_ab", vec!["a", "b"]);
    opt.add_table_stats("t", stats);

    let result = opt.optimize_bounded(&q).unwrap();
    assert_eq!(
        result.physical_choices.scan_for("t"),
        Some(&ScanStrategy::Seq),
        "compound index without leading-column coverage should be ignored",
    );
}

#[test]
fn cost_driven_scan_breaks_ties_with_primary_key() {
    // Two single-column indexes on the same column. Primary key
    // wins the tie.
    let q = filter_eq(scan("t"), "t", "id", 42);
    let mut opt = Optimizer::new();
    let mut stats = medium_table_stats();

    let mut pk = ra_core::statistics::IndexStats::new(
        vec!["id".into()],
        ra_core::facts::IndexType::BTree,
    );
    pk.is_primary = true;
    pk.is_unique = true;
    stats.indexes.insert("t_pkey".into(), pk);

    let secondary = ra_core::statistics::IndexStats::new(
        vec!["id".into()],
        ra_core::facts::IndexType::BTree,
    );
    stats.indexes.insert("t_id_dup".into(), secondary);

    opt.add_table_stats("t", stats);

    let result = opt.optimize_bounded(&q).unwrap();
    match result.physical_choices.scan_for("t") {
        Some(ScanStrategy::Index { name, .. }) => assert_eq!(name, "t_pkey"),
        other => panic!("expected primary-key index; got {other:?}"),
    }
}

#[test]
fn supplied_index_advice_wins_over_cost_driven_seq() {
    // Tiny table — cost-driven would pick Seq. INDEX_SCAN advice
    // overrides.
    let q = scan("t");
    let mut stats = tiny_table_stats();
    add_btree_index(&mut stats, "t_pk", vec!["id"]);
    let mut opt = Optimizer::with_config(OptimizerConfig {
        plan_advice: Some("INDEX_SCAN(t t_pk)".into()),
        ..OptimizerConfig::default()
    });
    opt.add_table_stats("t", stats);

    let result = opt.optimize_bounded(&q).unwrap();
    match result.physical_choices.scan_for("t") {
        Some(ScanStrategy::Index { name, .. }) => assert_eq!(name, "t_pk"),
        other => panic!("supplied INDEX_SCAN should win; got {other:?}"),
    }
}

fn add_btree_index_with_ndv(
    stats: &mut Statistics,
    name: &str,
    columns: Vec<&str>,
) {
    let columns: Vec<String> = columns.into_iter().map(String::from).collect();
    let idx = IndexStats::new(columns, ra_core::facts::IndexType::BTree);
    stats.indexes.insert(name.to_string(), idx);
}

fn set_column_ndv(stats: &mut Statistics, column: &str, ndv: f64) {
    let cs = ra_core::statistics::ColumnStats {
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
    stats.columns.insert(column.into(), cs);
}

#[test]
fn cost_driven_scan_picks_more_selective_index_on_tie() {
    // Two indexes on different columns, both with same prefix
    // length (1). Same uniqueness tier (regular). Tie-break
    // by selectivity: column with higher NDV is more selective.
    // Predicate: a = 1 AND b = 2.
    let q = filter_and_eq(scan("t"), "t", "a", 1, "b", 2);
    let mut opt = Optimizer::new();
    let mut stats = medium_table_stats();

    add_btree_index_with_ndv(&mut stats, "t_a", vec!["a"]);
    add_btree_index_with_ndv(&mut stats, "t_b", vec!["b"]);

    // Column `a` has 10 distinct values (low selectivity:
    // each = test matches 10% of rows). Column `b` has 1000
    // distinct (each = matches 0.1%). Pick `t_b`.
    set_column_ndv(&mut stats, "a", 10.0);
    set_column_ndv(&mut stats, "b", 1000.0);

    opt.add_table_stats("t", stats);

    let result = opt.optimize_bounded(&q).unwrap();
    match result.physical_choices.scan_for("t") {
        Some(ScanStrategy::Index { name, .. }) => assert_eq!(name, "t_b"),
        other => panic!("expected more-selective index t_b; got {other:?}"),
    }
}

#[test]
fn cost_driven_scan_uniqueness_beats_selectivity() {
    // Tie-break order: prefix_len > uniqueness > selectivity.
    // A unique index with poor selectivity should still beat a
    // regular index with good selectivity (uniqueness implies
    // perfect selectivity for `=` anyway, and PG's planner
    // mirrors this preference).
    let q = filter_eq(scan("t"), "t", "id", 42);
    let mut opt = Optimizer::new();
    let mut stats = medium_table_stats();

    let mut unique_idx = IndexStats::new(
        vec!["id".into()],
        ra_core::facts::IndexType::BTree,
    );
    unique_idx.is_unique = true;
    stats.indexes.insert("t_unique".into(), unique_idx);

    add_btree_index_with_ndv(&mut stats, "t_regular", vec!["id"]);

    // Make the regular index look extremely selective on its
    // column. The unique index's `is_unique` tier still wins.
    set_column_ndv(&mut stats, "id", 100.0);

    opt.add_table_stats("t", stats);
    let result = opt.optimize_bounded(&q).unwrap();
    match result.physical_choices.scan_for("t") {
        Some(ScanStrategy::Index { name, .. }) => assert_eq!(name, "t_unique"),
        other => panic!("expected unique index; got {other:?}"),
    }
}

#[test]
fn cost_driven_scan_handles_missing_ndv_neutrally() {
    // Two indexes, both regular, on different columns. Neither
    // column has NDV stats. Selectivity defaults to 1.0 for
    // both — they tie completely, and the iteration order
    // determines the winner (HashMap is randomized but
    // deterministic per-run). We just check that one was picked.
    let q = filter_and_eq(scan("t"), "t", "a", 1, "b", 2);
    let mut opt = Optimizer::new();
    let mut stats = medium_table_stats();
    add_btree_index_with_ndv(&mut stats, "t_a", vec!["a"]);
    add_btree_index_with_ndv(&mut stats, "t_b", vec!["b"]);
    opt.add_table_stats("t", stats);
    let result = opt.optimize_bounded(&q).unwrap();
    match result.physical_choices.scan_for("t") {
        Some(ScanStrategy::Index { name, .. }) => {
            assert!(name == "t_a" || name == "t_b", "got: {name:?}");
        }
        other => panic!("expected an index; got {other:?}"),
    }
}

fn add_index_typed(
    stats: &mut Statistics,
    name: &str,
    columns: Vec<&str>,
    index_type: ra_core::facts::IndexType,
) {
    let columns: Vec<String> = columns.into_iter().map(String::from).collect();
    let idx = IndexStats::new(columns, index_type);
    stats.indexes.insert(name.to_string(), idx);
}

#[test]
fn cost_driven_scan_skips_gin_index_for_equality_predicate() {
    // RFC 0039: GIN indexes don't support B-tree-style equality
    // matching. Even if the index covers the predicate column,
    // it should be skipped for an `=` filter.
    let q = filter_eq(scan("t"), "t", "id", 42);
    let mut opt = Optimizer::new();
    let mut stats = medium_table_stats();
    add_index_typed(&mut stats, "t_gin", vec!["id"], ra_core::facts::IndexType::Gin);
    opt.add_table_stats("t", stats);

    let result = opt.optimize_bounded(&q).unwrap();
    assert_eq!(
        result.physical_choices.scan_for("t"),
        Some(&ScanStrategy::Seq),
        "GIN index must not be selected for plain equality predicate",
    );
}

#[test]
fn cost_driven_scan_skips_gist_index_for_equality_predicate() {
    let q = filter_eq(scan("t"), "t", "geom", 0);
    let mut opt = Optimizer::new();
    let mut stats = medium_table_stats();
    add_index_typed(
        &mut stats,
        "t_gist",
        vec!["geom"],
        ra_core::facts::IndexType::Gist,
    );
    opt.add_table_stats("t", stats);

    let result = opt.optimize_bounded(&q).unwrap();
    assert_eq!(
        result.physical_choices.scan_for("t"),
        Some(&ScanStrategy::Seq),
        "GiST index must not be selected for equality predicate",
    );
}

#[test]
fn cost_driven_scan_skips_brin_index_for_equality_predicate() {
    let q = filter_eq(scan("t"), "t", "ts", 0);
    let mut opt = Optimizer::new();
    let mut stats = medium_table_stats();
    add_index_typed(
        &mut stats,
        "t_brin",
        vec!["ts"],
        ra_core::facts::IndexType::Brin,
    );
    opt.add_table_stats("t", stats);

    let result = opt.optimize_bounded(&q).unwrap();
    assert_eq!(
        result.physical_choices.scan_for("t"),
        Some(&ScanStrategy::Seq),
        "BRIN index must not be selected for equality predicate",
    );
}

#[test]
fn cost_driven_scan_picks_btree_when_btree_and_gin_both_match() {
    // BTree on id, GIN on id — only the BTree should be picked.
    let q = filter_eq(scan("t"), "t", "id", 42);
    let mut opt = Optimizer::new();
    let mut stats = medium_table_stats();
    add_index_typed(
        &mut stats,
        "t_btree",
        vec!["id"],
        ra_core::facts::IndexType::BTree,
    );
    add_index_typed(
        &mut stats,
        "t_gin_dup",
        vec!["id"],
        ra_core::facts::IndexType::Gin,
    );
    opt.add_table_stats("t", stats);

    let result = opt.optimize_bounded(&q).unwrap();
    match result.physical_choices.scan_for("t") {
        Some(ScanStrategy::Index { name, .. }) => assert_eq!(name, "t_btree"),
        other => panic!("expected BTree index t_btree; got {other:?}"),
    }
}

#[test]
fn cost_driven_scan_picks_hash_index_for_equality() {
    // Hash indexes support equality and are valid choices for
    // `=` predicates.
    let q = filter_eq(scan("t"), "t", "id", 42);
    let mut opt = Optimizer::new();
    let mut stats = medium_table_stats();
    add_index_typed(
        &mut stats,
        "t_hash",
        vec!["id"],
        ra_core::facts::IndexType::Hash,
    );
    opt.add_table_stats("t", stats);

    let result = opt.optimize_bounded(&q).unwrap();
    match result.physical_choices.scan_for("t") {
        Some(ScanStrategy::Index { name, .. }) => assert_eq!(name, "t_hash"),
        other => panic!("expected Hash index t_hash; got {other:?}"),
    }
}
