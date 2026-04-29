//! Quick test to verify left-deep path is used for JOB queries.

#![expect(clippy::expect_used, clippy::print_stdout)]

use ra_core::statistics::Statistics;
use ra_engine::left_deep::can_use_left_deep;
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
    dir.pop();
    dir.pop();
    dir.push("benchmarks/job/queries");
    dir
}

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let dir = queries_dir();
    let optimizer = make_optimizer();
    let targets = ["1a", "3b", "5a", "17a", "8a", "10c"];

    for target in &targets {
        let path = dir.join(format!("{target}.sql"));
        let sql = fs::read_to_string(&path).expect("read");
        let relexpr = sql_to_relexpr(&sql).expect("parse");
        let tables = ra_engine::large_join::LargeJoinOptimizer::count_tables(&relexpr);
        let eligible = can_use_left_deep(&relexpr);

        let start = std::time::Instant::now();
        let result = optimizer.optimize(&relexpr);
        let elapsed = start.elapsed();

        println!(
            "{target}: {tables} tables, left_deep_eligible={eligible}, \
             optimize={:.1}ms, ok={}",
            elapsed.as_secs_f64() * 1000.0,
            result.is_ok(),
        );
    }
}
