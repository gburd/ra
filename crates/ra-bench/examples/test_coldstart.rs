//! Test if the 238ms is a cold-start issue by running queries in different orders.
//!
//! Run with:
//!   cargo run --release --example test_coldstart -p ra-bench

use ra_engine::Optimizer;
use ra_grammar_fuzzer::corpus::all_queries;
use ra_parser::sql_to_relexpr::sql_to_relexpr;
use std::time::Instant;

fn main() {
    let all = all_queries();
    let cte_queries: Vec<_> = all
        .iter()
        .filter(|e| e.category == "ctes")
        .collect();

    println!("Test 1: First query is 'big_orders' (normal order)");
    println!("======================================================\n");
    {
        let optimizer = Optimizer::new();
        for (i, entry) in cte_queries.iter().take(3).enumerate() {
            let plan = sql_to_relexpr(entry.sql).expect("parse failed");
            let start = Instant::now();
            let _ = optimizer.optimize(&plan);
            let elapsed = start.elapsed().as_micros() as f64 / 1000.0;
            println!("Query {}: {:.60}...", i+1, entry.sql);
            println!("  Time: {:.2}ms\n", elapsed);
        }
    }

    println!("\nTest 2: First query is 'top_customers' (skip big_orders)");
    println!("===========================================================\n");
    {
        let optimizer = Optimizer::new();
        for (i, entry) in cte_queries.iter().skip(1).take(3).enumerate() {
            let plan = sql_to_relexpr(entry.sql).expect("parse failed");
            let start = Instant::now();
            let _ = optimizer.optimize(&plan);
            let elapsed = start.elapsed().as_micros() as f64 / 1000.0;
            println!("Query {}: {:.60}...", i+1, entry.sql);
            println!("  Time: {:.2}ms\n", elapsed);
        }
    }

    println!("\nTest 3: Run a simple query first to warm up");
    println!("==============================================\n");
    {
        let optimizer = Optimizer::new();

        // Warm up with simple query
        let warmup = sql_to_relexpr("SELECT * FROM orders").unwrap();
        let start = Instant::now();
        let _ = optimizer.optimize(&warmup);
        let warmup_time = start.elapsed().as_micros() as f64 / 1000.0;
        println!("Warmup query: SELECT * FROM orders");
        println!("  Time: {:.2}ms\n", warmup_time);

        // Now run big_orders
        let plan = sql_to_relexpr(cte_queries[0].sql).expect("parse failed");
        let start = Instant::now();
        let _ = optimizer.optimize(&plan);
        let elapsed = start.elapsed().as_micros() as f64 / 1000.0;
        println!("After warmup - big_orders query:");
        println!("  Time: {:.2}ms\n", elapsed);
    }
}
