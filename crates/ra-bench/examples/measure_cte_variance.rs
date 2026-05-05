//! Measure CTE optimization time variance to determine if regression is real.
//!
//! Run with:
//!   cargo run --release --example measure_cte_variance -p ra-bench

use ra_engine::Optimizer;
use ra_grammar_fuzzer::corpus::all_queries;
use ra_parser::sql_to_relexpr::sql_to_relexpr;
use std::time::Instant;

fn main() {
    let optimizer = Optimizer::new();

    // Get all CTE queries
    let all = all_queries();
    let cte_queries: Vec<_> = all
        .iter()
        .filter(|e| e.category == "ctes")
        .collect();

    println!("Running {} CTE queries 30 times each to measure variance...\n", cte_queries.len());

    for entry in &cte_queries {
        let plan = sql_to_relexpr(entry.sql).expect("parse failed");

        let mut times = Vec::new();
        for _ in 0..30 {
            let start = Instant::now();
            let _ = optimizer.optimize(&plan);
            times.push(start.elapsed().as_micros());
        }

        times.sort_unstable();
        let avg = times.iter().sum::<u128>() as f64 / times.len() as f64 / 1000.0;
        let median = times[times.len() / 2] as f64 / 1000.0;
        let p95 = times[(times.len() as f64 * 0.95) as usize] as f64 / 1000.0;
        let min = times[0] as f64 / 1000.0;
        let max = times[times.len() - 1] as f64 / 1000.0;

        println!("Query: {:.60}...", entry.sql);
        println!("  Avg: {:.2}ms, Median: {:.2}ms, P95: {:.2}ms, Min: {:.2}ms, Max: {:.2}ms",
                 avg, median, p95, min, max);
        println!();
    }
}
