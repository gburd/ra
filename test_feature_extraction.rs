#!/usr/bin/env rust-script
//! Test feature extraction from SQL queries
//!
//! ```cargo
//! [dependencies]
//! ra-parser = { path = "./crates/ra-parser" }
//! ra-engine = { path = "./crates/ra-engine" }
//! ra-core = { path = "./crates/ra-core" }
//! ```

fn main() {
    let test_queries = vec![
        ("Simple scan", "SELECT * FROM users"),
        ("Filter", "SELECT * FROM users WHERE age > 21"),
        ("Join", "SELECT o.id FROM orders o JOIN customers c ON o.customer_id = c.id"),
        ("Aggregate", "SELECT category, COUNT(*) FROM products GROUP BY category"),
        ("Complex", "SELECT c.region, COUNT(*), SUM(o.amount) FROM orders o JOIN customers c ON o.customer_id = c.id WHERE o.status = 'completed' AND c.age > 18 GROUP BY c.region ORDER BY COUNT(*) DESC LIMIT 10"),
        ("CTE", "WITH regional_sales AS (SELECT region, SUM(amount) AS total FROM orders GROUP BY region) SELECT * FROM regional_sales WHERE total > 1000"),
        ("Window", "SELECT id, amount, ROW_NUMBER() OVER (PARTITION BY category ORDER BY amount DESC) AS rank FROM orders"),
    ];

    println!("Testing feature extraction from SQL queries\n");
    println!("{:<15} {:>8} {:>6} {:>8} {:>8} {:>8} {:>6} {:>8}",
        "Query", "Tables", "Joins", "Filters", "Aggs", "GroupBy", "Sort", "Limit");
    println!("{}", "-".repeat(80));

    for (name, sql) in test_queries {
        match ra_parser::lime_parser::parse_sql(sql) {
            Ok(rel_expr) => {
                let features = ra_engine::cost_model::extract_features(&rel_expr);
                println!("{:<15} {:>8.0} {:>6.0} {:>8.0} {:>8.0} {:>8.0} {:>6.0} {:>8.0}",
                    name,
                    features.table_count,
                    features.join_count,
                    features.filter_count,
                    features.aggregate_count,
                    features.group_by_count,
                    features.order_by_count,
                    features.limit_present,
                );
            }
            Err(e) => {
                eprintln!("Failed to parse {}: {:?}", name, e);
            }
        }
    }

    println!("\nFeature extraction test complete!");
}
