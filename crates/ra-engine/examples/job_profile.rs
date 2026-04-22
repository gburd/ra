//! Profile where time is spent in JOB query optimization.
//!
//! Run with: cargo run --example job_profile --package ra-engine --release

use ra_core::statistics::Statistics;
use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

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
    dir.push("benchmarks");
    dir.push("job");
    dir.push("queries");
    dir
}

fn main() {
    let optimizer = make_optimizer();
    let dir = queries_dir();

    // Pick representative queries for each path
    let test_queries = vec![
        "2a.sql",  // 5 tables, left-deep
        "6a.sql",  // 5 tables, left-deep
        "7a.sql",  // 8 tables, e-graph
        "13a.sql", // 9 tables, e-graph
        "22a.sql", // 11 tables, large-join
        "29a.sql", // 17 tables, large-join
    ];

    // Warmup: run each query once to heat caches
    for name in &test_queries {
        let path = dir.join(name);
        let sql = fs::read_to_string(&path).unwrap();
        let relexpr = sql_to_relexpr(&sql).unwrap();
        let _ = optimizer.optimize(&relexpr);
    }

    // Profile: run each query 10 times
    println!(
        "{:<8} {:>6} {:>10} {:>10} {:>10} {:>10}",
        "Query", "Tables", "Min(us)", "Avg(us)", "Max(us)", "Path"
    );
    println!("{}", "-".repeat(70));

    for name in &test_queries {
        let path = dir.join(name);
        let sql = fs::read_to_string(&path).unwrap();
        let relexpr = sql_to_relexpr(&sql).unwrap();

        let tables = count_tables(&relexpr);
        let eligible = ra_engine::left_deep::can_use_left_deep(&relexpr);
        let path_name = if eligible {
            "left-deep"
        } else if tables >= 10 {
            "large-join"
        } else {
            "e-graph"
        };

        let mut times = Vec::new();
        for _ in 0..10 {
            let start = Instant::now();
            let _ = optimizer.optimize(&relexpr);
            times.push(start.elapsed().as_micros() as f64);
        }

        let min = times.iter().copied().fold(f64::INFINITY, f64::min);
        let max = times.iter().copied().fold(0.0_f64, f64::max);
        let avg = times.iter().sum::<f64>() / times.len() as f64;

        println!(
            "{:<8} {:>6} {:>10.0} {:>10.0} {:>10.0} {:>10}",
            name, tables, min, avg, max, path_name
        );
    }

    // Micro-benchmark: just all_rules() creation
    println!();
    println!("=== Component Benchmarks ===");

    let iters = 100;

    // all_rules
    let start = Instant::now();
    for _ in 0..iters {
        let _rules = ra_engine::rewrite::all_rules();
    }
    let elapsed = start.elapsed();
    println!(
        "all_rules() x {}: {:.1}ms total, {:.1}us/call",
        iters,
        elapsed.as_millis(),
        elapsed.as_micros() as f64 / iters as f64
    );

    // hardware_profile()
    let start = Instant::now();
    for _ in 0..iters {
        let _hw = ra_hardware::detect_hardware();
    }
    let elapsed = start.elapsed();
    println!(
        "detect_hardware() x {}: {:.1}ms total, {:.1}us/call",
        iters,
        elapsed.as_millis(),
        elapsed.as_micros() as f64 / iters as f64
    );

    // sql_to_relexpr for a simple query
    let sql = fs::read_to_string(dir.join("2a.sql")).unwrap();
    let start = Instant::now();
    for _ in 0..iters {
        let _ = sql_to_relexpr(&sql);
    }
    let elapsed = start.elapsed();
    println!(
        "sql_to_relexpr(2a) x {}: {:.1}ms total, {:.1}us/call",
        iters,
        elapsed.as_millis(),
        elapsed.as_micros() as f64 / iters as f64
    );

    // to_rec_expr (conversion to e-graph format)
    let relexpr = sql_to_relexpr(&sql).unwrap();
    let start = Instant::now();
    for _ in 0..iters {
        let _ = ra_engine::egraph::to_rec_expr(&relexpr);
    }
    let elapsed = start.elapsed();
    println!(
        "to_rec_expr(2a) x {}: {:.1}ms total, {:.1}us/call",
        iters,
        elapsed.as_millis(),
        elapsed.as_micros() as f64 / iters as f64
    );
}

fn count_tables(expr: &ra_core::algebra::RelExpr) -> usize {
    use ra_core::algebra::RelExpr;
    match expr {
        RelExpr::Scan { .. } => 1,
        RelExpr::Join { left, right, .. } => count_tables(left) + count_tables(right),
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Distinct { input } => count_tables(input),
        _ => 0,
    }
}
