//! Debug CTE optimization to see which path it takes.
//!
//! Run with:
//!   cargo run --example debug_cte -p ra-bench

use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr::sql_to_relexpr;
use std::time::Instant;

fn main() {
    let optimizer = Optimizer::new();

    // Sample CTE query from the corpus
    let sql = "WITH big_orders AS (\
                SELECT * FROM orders WHERE o_totalprice > 100000\
              ) SELECT COUNT(*) FROM big_orders";

    println!("Parsing CTE query...");
    let plan = sql_to_relexpr(sql).expect("parse failed");
    println!("Parsed plan: {:#?}\n", plan);

    println!("Optimizing...");
    let start = Instant::now();
    let optimized = optimizer.optimize(&plan).expect("optimize failed");
    let elapsed = start.elapsed();

    println!("\nOptimized plan: {:#?}", optimized);
    println!("\nOptimization took: {:?}", elapsed);
}
