//! Join Order Benchmark (JOB) optimizer benchmarks.
//!
//! All 113 JOB queries from the IMDB dataset, parsed from SQL files
//! using `ra-parser::sql_to_relexpr`. Measures optimizer latency for
//! join ordering across varying query complexity (2-17 tables).
//!
//! Reference: "How Good Are Query Optimizers, Really?" (Leis et al.)
//!
//! Run with:
//!   cargo bench --package ra-engine --bench job_benchmark

#![allow(clippy::expect_used)]

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ra_core::algebra::RelExpr;
use ra_core::statistics::Statistics;
use ra_core::EmptyFactsProvider;
use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;
use std::fs;
use std::path::PathBuf;

fn mk_stats(rows: f64, avg: u64) -> Statistics {
    let mut s = Statistics::new(rows);
    s.avg_row_size = avg;
    s.total_size = (rows as u64) * avg;
    s
}

fn make_optimizer() -> Optimizer {
    let mut opt = Optimizer::new();
    for (name, rows, sz) in [
        ("aka_name", 901_343.0, 100_u64),
        ("aka_title", 361_472.0, 150),
        ("cast_info", 36_244_344.0, 80),
        ("char_name", 3_140_339.0, 90),
        ("comp_cast_type", 4.0, 50),
        ("company_name", 234_997.0, 120),
        ("company_type", 4.0, 50),
        ("complete_cast", 135_086.0, 60),
        ("info_type", 113.0, 50),
        ("keyword", 134_170.0, 100),
        ("kind_type", 7.0, 50),
        ("link_type", 18.0, 50),
        ("movie_companies", 2_609_129.0, 100),
        ("movie_info", 14_835_720.0, 150),
        ("movie_info_idx", 1_380_035.0, 100),
        ("movie_keyword", 4_523_930.0, 60),
        ("movie_link", 29_997.0, 80),
        ("name", 4_167_491.0, 110),
        ("person_info", 2_963_664.0, 120),
        ("role_type", 12.0, 50),
        ("title", 2_528_312.0, 180),
    ] {
        opt.add_table_stats(name, mk_stats(rows, sz));
    }
    opt
}

fn queries_dir() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.pop(); // crates/
    dir.pop(); // project root
    dir.push("benchmarks");
    dir.push("job");
    dir.push("queries");
    dir
}

fn load_queries() -> Vec<(String, RelExpr)> {
    let dir = queries_dir();
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "sql"))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            let query_id = path
                .file_stem()
                .expect("file has stem")
                .to_str()
                .expect("valid utf8")
                .to_owned();
            let sql = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            match sql_to_relexpr(&sql) {
                Ok(relexpr) => Some((query_id, relexpr)),
                Err(e) => {
                    eprintln!("WARN: skipping {query_id}: {e}");
                    None
                }
            }
        })
        .collect()
}

fn bench_job_optimize_all(c: &mut Criterion) {
    let optimizer = make_optimizer();
    let facts = EmptyFactsProvider::new();
    let queries = load_queries();
    let mut group = c.benchmark_group("job_optimize");

    for (name, plan) in &queries {
        group.bench_with_input(BenchmarkId::new("optimize", name), plan, |b, p| {
            b.iter(|| {
                let _ = black_box(optimizer.optimize_with_facts(p, &facts));
            });
        });
    }
    group.finish();
}

fn bench_job_by_category(c: &mut Criterion) {
    let optimizer = make_optimizer();
    let facts = EmptyFactsProvider::new();
    let queries = load_queries();

    let simple: Vec<_> = queries
        .iter()
        .filter(|(n, _)| {
            let num: u32 = n
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0);
            (1..=6).contains(&num)
        })
        .collect();

    let medium: Vec<_> = queries
        .iter()
        .filter(|(n, _)| {
            let num: u32 = n
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0);
            (7..=18).contains(&num)
        })
        .collect();

    let complex: Vec<_> = queries
        .iter()
        .filter(|(n, _)| {
            let num: u32 = n
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0);
            num >= 19
        })
        .collect();

    let mut grp = c.benchmark_group("job_simple");
    for (name, plan) in &simple {
        grp.bench_with_input(BenchmarkId::new("optimize", name), plan, |b, p| {
            b.iter(|| {
                let _ = black_box(optimizer.optimize_with_facts(p, &facts));
            });
        });
    }
    grp.finish();

    let mut grp = c.benchmark_group("job_medium");
    for (name, plan) in &medium {
        grp.bench_with_input(BenchmarkId::new("optimize", name), plan, |b, p| {
            b.iter(|| {
                let _ = black_box(optimizer.optimize_with_facts(p, &facts));
            });
        });
    }
    grp.finish();

    let mut grp = c.benchmark_group("job_complex");
    for (name, plan) in &complex {
        grp.bench_with_input(BenchmarkId::new("optimize", name), plan, |b, p| {
            b.iter(|| {
                let _ = black_box(optimizer.optimize_with_facts(p, &facts));
            });
        });
    }
    grp.finish();
}

criterion_group!(benches, bench_job_optimize_all, bench_job_by_category,);
criterion_main!(benches);
