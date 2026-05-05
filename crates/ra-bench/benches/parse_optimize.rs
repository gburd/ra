//! Criterion regression benchmarks for Ra parse + optimize pipeline.
//!
//! Benchmark groups:
//! - `parse_simple` / `parse_medium` / `parse_complex`
//! - `optimize_simple` / `optimize_medium` / `optimize_complex`
//! - `tpch_q01` … `tpch_q22`
//! - Per-corpus-category group

#![allow(clippy::panic, clippy::expect_used)]

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use ra_engine::Optimizer;
use ra_grammar_fuzzer::corpus::{all_queries, CorpusEntry};
use ra_parser::sql_to_relexpr::sql_to_relexpr;

// ---------------------------------------------------------------------------
// Representative query strings by complexity tier
// ---------------------------------------------------------------------------

const SIMPLE_QUERIES: &[(&str, &str)] = &[
    ("scan", "SELECT * FROM orders"),
    ("filter", "SELECT * FROM orders WHERE o_orderstatus = 'O'"),
    ("project", "SELECT o_orderkey, o_totalprice FROM orders"),
    ("count", "SELECT COUNT(*) FROM orders"),
    ("limit", "SELECT * FROM orders LIMIT 100"),
];

const MEDIUM_QUERIES: &[(&str, &str)] = &[
    ("join2",
     "SELECT o_orderkey, c_name FROM orders \
      JOIN customer ON o_custkey = c_custkey"),
    ("agg_group",
     "SELECT o_orderstatus, COUNT(*), SUM(o_totalprice) \
      FROM orders GROUP BY o_orderstatus"),
    ("filter_sort",
     "SELECT * FROM orders WHERE o_totalprice > 50000 ORDER BY o_orderdate DESC"),
    ("window",
     "SELECT row_number() OVER (ORDER BY o_totalprice DESC), o_orderkey FROM orders"),
    ("cte",
     "WITH big AS (SELECT * FROM orders WHERE o_totalprice > 100000) \
      SELECT COUNT(*) FROM big"),
];

const COMPLEX_QUERIES: &[(&str, &str)] = &[
    ("join5",
     "SELECT c_name, n_name, r_name \
      FROM customer \
      JOIN nation ON c_nationkey = n_nationkey \
      JOIN region ON n_regionkey = r_regionkey \
      JOIN orders ON c_custkey = o_custkey \
      JOIN lineitem ON o_orderkey = l_orderkey"),
    ("tpch_q1",
     "SELECT l_returnflag, l_linestatus, \
        SUM(l_quantity), SUM(l_extendedprice), COUNT(*) \
      FROM lineitem \
      WHERE l_shipdate <= '1998-09-02' \
      GROUP BY l_returnflag, l_linestatus \
      ORDER BY l_returnflag, l_linestatus"),
    ("tpch_q5",
     "SELECT n_name, SUM(l_extendedprice * (1 - l_discount)) AS revenue \
      FROM customer, orders, lineitem, supplier, nation, region \
      WHERE c_custkey = o_custkey AND l_orderkey = o_orderkey \
        AND l_suppkey = s_suppkey AND c_nationkey = s_nationkey \
        AND s_nationkey = n_nationkey AND n_regionkey = r_regionkey \
        AND r_name = 'ASIA' AND o_orderdate >= '1994-01-01' \
      GROUP BY n_name ORDER BY revenue DESC"),
    ("nested_subq",
     "SELECT * FROM orders WHERE o_custkey IN \
      (SELECT c_custkey FROM customer WHERE c_mktsegment = 'BUILDING')"),
];

// ---------------------------------------------------------------------------
// Parse benchmarks
// ---------------------------------------------------------------------------

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_simple");
    for &(name, sql) in SIMPLE_QUERIES {
        group.bench_with_input(BenchmarkId::new("parse", name), sql, |b, sql| {
            b.iter(|| sql_to_relexpr(sql).ok());
        });
    }
    group.finish();

    let mut group = c.benchmark_group("parse_medium");
    for &(name, sql) in MEDIUM_QUERIES {
        group.bench_with_input(BenchmarkId::new("parse", name), sql, |b, sql| {
            b.iter(|| sql_to_relexpr(sql).ok());
        });
    }
    group.finish();

    let mut group = c.benchmark_group("parse_complex");
    for &(name, sql) in COMPLEX_QUERIES {
        group.bench_with_input(BenchmarkId::new("parse", name), sql, |b, sql| {
            b.iter(|| sql_to_relexpr(sql).ok());
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Optimize benchmarks
// ---------------------------------------------------------------------------

fn bench_optimize(c: &mut Criterion) {
    let optimizer = Optimizer::new();

    let mut group = c.benchmark_group("optimize_simple");
    for &(name, sql) in SIMPLE_QUERIES {
        if let Ok(plan) = sql_to_relexpr(sql) {
            group.bench_with_input(BenchmarkId::new("optimize", name), &plan, |b, plan| {
                b.iter(|| optimizer.optimize(plan).ok());
            });
        }
    }
    group.finish();

    let mut group = c.benchmark_group("optimize_medium");
    for &(name, sql) in MEDIUM_QUERIES {
        if let Ok(plan) = sql_to_relexpr(sql) {
            group.bench_with_input(BenchmarkId::new("optimize", name), &plan, |b, plan| {
                b.iter(|| optimizer.optimize(plan).ok());
            });
        }
    }
    group.finish();

    let mut group = c.benchmark_group("optimize_complex");
    for &(name, sql) in COMPLEX_QUERIES {
        if let Ok(plan) = sql_to_relexpr(sql) {
            group.bench_with_input(BenchmarkId::new("optimize", name), &plan, |b, plan| {
                b.iter(|| optimizer.optimize(plan).ok());
            });
        }
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// TPC-H query benchmarks
// ---------------------------------------------------------------------------

fn bench_tpch(c: &mut Criterion) {
    let optimizer = Optimizer::new();
    let corpus = all_queries();
    let tpch: Vec<&CorpusEntry> =
        corpus.iter().filter(|e| e.category == "tpch").collect();

    {
        let mut parse_group = c.benchmark_group("tpch_parse");
        for (i, entry) in tpch.iter().enumerate() {
            let label = format!("q{:02}", i + 1);
            parse_group.bench_with_input(
                BenchmarkId::new("tpch", &label),
                entry.sql,
                |b, sql| {
                    b.iter(|| sql_to_relexpr(sql).ok());
                },
            );
        }
        parse_group.finish();
    }

    {
        let mut opt_group = c.benchmark_group("tpch_optimize");
        for (i, entry) in tpch.iter().enumerate() {
            let label = format!("q{:02}", i + 1);
            if let Ok(plan) = sql_to_relexpr(entry.sql) {
                opt_group.bench_with_input(
                    BenchmarkId::new("tpch", &label),
                    &plan,
                    |b, plan| {
                        b.iter(|| optimizer.optimize(plan).ok());
                    },
                );
            }
        }
        opt_group.finish();
    }
}

// ---------------------------------------------------------------------------
// Per-category corpus benchmarks
// ---------------------------------------------------------------------------

fn bench_corpus_categories(c: &mut Criterion) {
    let optimizer = Optimizer::new();
    let corpus = all_queries();

    let categories: Vec<&str> = {
        let mut seen = std::collections::HashSet::new();
        corpus
            .iter()
            .filter_map(|e| {
                if seen.insert(e.category) {
                    Some(e.category)
                } else {
                    None
                }
            })
            .collect()
    };

    for cat in categories {
        let entries: Vec<&CorpusEntry> =
            corpus.iter().filter(|e| e.category == cat).collect();

        {
            let mut parse_group = c.benchmark_group(format!("corpus_{cat}_parse"));
            for (i, entry) in entries.iter().enumerate() {
                parse_group.bench_with_input(
                    BenchmarkId::from_parameter(i),
                    entry.sql,
                    |b, sql| {
                        b.iter(|| sql_to_relexpr(sql).ok());
                    },
                );
            }
            parse_group.finish();
        }

        {
            let mut opt_group = c.benchmark_group(format!("corpus_{cat}_optimize"));
            for (i, entry) in entries.iter().enumerate() {
                if let Ok(plan) = sql_to_relexpr(entry.sql) {
                    opt_group.bench_with_input(
                        BenchmarkId::from_parameter(i),
                        &plan,
                        |b, plan| {
                            b.iter(|| optimizer.optimize(plan).ok());
                        },
                    );
                }
            }
            opt_group.finish();
        }
    }
}

// ---------------------------------------------------------------------------
// Criterion entry points
// ---------------------------------------------------------------------------

criterion_group!(
    benches,
    bench_parse,
    bench_optimize,
    bench_tpch,
    bench_corpus_categories
);
criterion_main!(benches);
