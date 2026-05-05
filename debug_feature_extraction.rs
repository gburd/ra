//! Debug feature extraction to see what's happening

use ra_engine::cost_model::extract_features;
use ra_parser::lime_parser::parse_sql;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Debugging feature extraction...\n");

    let test_queries = vec![
        "SELECT * FROM orders",
        "SELECT COUNT(*) FROM orders",
        "SELECT o_orderstatus, COUNT(*) FROM orders GROUP BY o_orderstatus",
        "SELECT * FROM orders WHERE o_custkey = 123",
        "SELECT c.c_name, o.o_orderdate FROM customer c JOIN orders o ON c.c_custkey = o.o_custkey",
    ];

    for sql in test_queries {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("SQL: {}", sql);

        match parse_sql(sql) {
            Ok(expr) => {
                println!("✓ Parsed successfully");
                println!("RelExpr: {:#?}", expr);

                let features = extract_features(&expr);
                println!("\nFeatures:");
                println!("  table_count: {}", features.table_count);
                println!("  join_count: {}", features.join_count);
                println!("  filter_count: {}", features.filter_count);
                println!("  aggregate_count: {}", features.aggregate_count);
                println!("  group_by_count: {}", features.group_by_count);
                println!("  order_by_count: {}", features.order_by_count);
                println!("  distinct_flag: {}", features.distinct_flag);
                println!("  limit_present: {}", features.limit_present);
                println!("  subquery_count: {}", features.subquery_count);
                println!("  cte_count: {}", features.cte_count);
                println!("  window_function_count: {}", features.window_function_count);
                println!("  max_join_cardinality: {}", features.max_join_cardinality);
            }
            Err(e) => {
                println!("❌ Parse failed: {}", e);
            }
        }
        println!();
    }

    Ok(())
}