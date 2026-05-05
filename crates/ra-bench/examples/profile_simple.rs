//! Profile optimization of a simple query to identify hotspots.
//!
//! Run with:
//!   cargo flamegraph --example profile_simple -p ra-bench

use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr::sql_to_relexpr;

fn main() {
    let optimizer = Optimizer::new();

    // Simple query that's taking 23ms avg to optimize
    let sql = "SELECT * FROM orders WHERE o_totalprice > 10000";

    // Run optimization 1000 times to get enough samples for flamegraph
    for _ in 0..1000 {
        let plan = sql_to_relexpr(sql).expect("parse failed");
        let _optimized = optimizer.optimize(&plan);
    }
}
