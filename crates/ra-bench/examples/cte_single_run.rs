//! Measure CTE optimization time with single runs (no caching warmup).
//!
//! Run with:
//!   cargo run --release --example cte_single_run -p ra-bench

use ra_engine::Optimizer;
use ra_grammar_fuzzer::corpus::all_queries;
use ra_parser::sql_to_relexpr::sql_to_relexpr;
use std::time::Instant;

fn main() {
    // Get all CTE queries
    let all = all_queries();
    let cte_queries: Vec<_> = all
        .iter()
        .filter(|e| e.category == "ctes")
        .collect();

    println!("Running {} CTE queries ONCE each (no cache warmup)...\n", cte_queries.len());

    let mut total = 0.0;
    for entry in &cte_queries {
        // Create fresh optimizer for each query (no cache)
        let optimizer = Optimizer::new();
        let plan = sql_to_relexpr(entry.sql).expect("parse failed");

        let start = Instant::now();
        let _ = optimizer.optimize(&plan);
        let elapsed = start.elapsed().as_micros() as f64 / 1000.0;

        println!("Query: {:.60}...", entry.sql);
        println!("  Time: {:.2}ms\n", elapsed);
        total += elapsed;
    }

    println!("Average: {:.2}ms", total / cte_queries.len() as f64);
}
