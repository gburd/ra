#![expect(
    clippy::print_stderr,
    clippy::panic,
    reason = "benchmark diagnostic output"
)]
//! TPC-H full 22-query optimizer benchmarks.
//!
//! Measures Ra optimizer latency for all 22 TPC-H queries with
//! SF=1 table statistics. Queries are parsed from SQL files using
//! `ra-parser::sql_to_relexpr`. Compares single-node optimization
//! time across query complexity categories.
//!
//! Run with:
//!   `cargo bench --package ra-engine --bench tpch_all22`

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ra_core::algebra::RelExpr;
use ra_core::statistics::Statistics;
use ra_core::EmptyFactsProvider;
use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;
use std::fs;
use std::path::PathBuf;

fn make_stats(rows: f64, avg_row_size: u64) -> Statistics {
    let mut s = Statistics::new(rows);
    s.avg_row_size = avg_row_size;
    s.total_size = (rows as u64) * avg_row_size;
    s
}

fn make_optimizer() -> Optimizer {
    let mut opt = Optimizer::new();
    for (name, stats) in [
        ("lineitem", make_stats(6_001_215.0, 128)),
        ("orders", make_stats(1_500_000.0, 150)),
        ("customer", make_stats(150_000.0, 200)),
        ("supplier", make_stats(10_000.0, 180)),
        ("nation", make_stats(25.0, 64)),
        ("region", make_stats(5.0, 48)),
        ("part", make_stats(200_000.0, 160)),
        ("partsupp", make_stats(800_000.0, 144)),
    ] {
        opt.add_table_stats(name, stats);
    }
    opt
}

fn queries_dir() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.pop(); // crates/
    dir.pop(); // project root
    dir.push("benchmarks");
    dir.push("tpch");
    dir.push("queries");
    dir
}

fn load_queries() -> Vec<(String, RelExpr)> {
    let dir = queries_dir();
    let mut queries = Vec::with_capacity(22);

    for i in 1..=22 {
        let filename = format!("q{i}.sql");
        let path = dir.join(&filename);
        let sql = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let label = format!("Q{i:02}");
        match sql_to_relexpr(&sql) {
            Ok(relexpr) => queries.push((label, relexpr)),
            Err(e) => {
                eprintln!("WARN: skipping Q{i}: {e}");
            }
        }
    }
    queries
}

fn bench_tpch_optimize_all(c: &mut Criterion) {
    let optimizer = make_optimizer();
    let facts = EmptyFactsProvider::new();
    let queries = load_queries();
    let mut group = c.benchmark_group("tpch_optimize");

    for (name, plan) in &queries {
        group.bench_with_input(BenchmarkId::new("optimize", name), plan, |b, p| {
            b.iter(|| {
                let _ = black_box(optimizer.optimize_with_facts(p, &facts));
            });
        });
    }
    group.finish();
}

fn bench_tpch_by_category(c: &mut Criterion) {
    let optimizer = make_optimizer();
    let facts = EmptyFactsProvider::new();
    let queries = load_queries();

    let simple_ids = ["Q01", "Q06"];
    let medium_ids = ["Q03", "Q04", "Q12", "Q14", "Q15", "Q17", "Q19"];
    let complex_ids = ["Q02", "Q05", "Q07", "Q08", "Q09", "Q10", "Q11"];
    let advanced_ids = ["Q13", "Q16", "Q18", "Q20", "Q21", "Q22"];

    let mut grp = c.benchmark_group("tpch_simple");
    for (name, plan) in queries
        .iter()
        .filter(|(n, _)| simple_ids.contains(&n.as_str()))
    {
        grp.bench_with_input(BenchmarkId::new("optimize", name), plan, |b, p| {
            b.iter(|| {
                let _ = black_box(optimizer.optimize_with_facts(p, &facts));
            });
        });
    }
    grp.finish();

    let mut grp = c.benchmark_group("tpch_medium_joins");
    for (name, plan) in queries
        .iter()
        .filter(|(n, _)| medium_ids.contains(&n.as_str()))
    {
        grp.bench_with_input(BenchmarkId::new("optimize", name), plan, |b, p| {
            b.iter(|| {
                let _ = black_box(optimizer.optimize_with_facts(p, &facts));
            });
        });
    }
    grp.finish();

    let mut grp = c.benchmark_group("tpch_complex_joins");
    for (name, plan) in queries
        .iter()
        .filter(|(n, _)| complex_ids.contains(&n.as_str()))
    {
        grp.bench_with_input(BenchmarkId::new("optimize", name), plan, |b, p| {
            b.iter(|| {
                let _ = black_box(optimizer.optimize_with_facts(p, &facts));
            });
        });
    }
    grp.finish();

    let mut grp = c.benchmark_group("tpch_advanced");
    for (name, plan) in queries
        .iter()
        .filter(|(n, _)| advanced_ids.contains(&n.as_str()))
    {
        grp.bench_with_input(BenchmarkId::new("optimize", name), plan, |b, p| {
            b.iter(|| {
                let _ = black_box(optimizer.optimize_with_facts(p, &facts));
            });
        });
    }
    grp.finish();
}

criterion_group!(benches, bench_tpch_optimize_all, bench_tpch_by_category,);
criterion_main!(benches);
