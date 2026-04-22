//! Profile a single JOB query to measure optimizer performance.
//!
//! Run with:
//!   RUST_LOG=ra_engine=info cargo run --release --example profile_job_query

#![allow(clippy::expect_used)]

use ra_core::statistics::Statistics;
use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;

fn make_stats(rows: f64, avg_row_size: u64) -> Statistics {
    let mut s = Statistics::new(rows);
    s.avg_row_size = avg_row_size;
    s.total_size = (rows as u64) * avg_row_size;
    s
}

fn make_optimizer() -> Optimizer {
    let mut opt = Optimizer::new();

    for (name, rows, size) in [
        ("aka_name", 901_343.0, 100),
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
        opt.add_table_stats(name, make_stats(rows, size));
    }

    opt
}

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("Profiling JOB Query 13a (multi-table join)");
    println!("==========================================\n");

    let mut query_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    query_path.pop(); // crates/
    query_path.pop(); // project root
    query_path.push("benchmarks/job/queries/13a.sql");

    let sql = std::fs::read_to_string(&query_path).expect("JOB 13a SQL file not found");
    let query = sql_to_relexpr(&sql).expect("Failed to parse JOB 13a");

    let optimizer = make_optimizer();

    println!("Running optimization...\n");

    let start = std::time::Instant::now();
    match optimizer.optimize(&query) {
        Ok(_optimized) => {
            let elapsed = start.elapsed();
            println!("\nOptimization complete in {elapsed:?}");
        }
        Err(e) => {
            eprintln!("Optimization failed: {e}");
            std::process::exit(1);
        }
    }
}
