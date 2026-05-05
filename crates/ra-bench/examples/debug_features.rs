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
    ];

    for sql in test_queries {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("SQL: {}", sql);

        match parse_sql(sql) {
            Ok(expr) => {
                println!("✓ Parsed successfully");

                // Print a simplified version of RelExpr to see structure
                println!("RelExpr type: {}", get_relexpr_type(&expr));
                print_relexpr_tree(&expr, 0);

                let features = extract_features(&expr);
                println!("\nFeatures:");
                println!("  table_count: {}", features.table_count);
                println!("  join_count: {}", features.join_count);
                println!("  filter_count: {}", features.filter_count);
                println!("  aggregate_count: {}", features.aggregate_count);
                println!("  group_by_count: {}", features.group_by_count);

                // Check expected vs actual
                if sql.contains("COUNT") {
                    if features.aggregate_count > 0.0 {
                        println!("  ✓ COUNT detected correctly");
                    } else {
                        println!("  ❌ COUNT NOT detected - should be > 0");
                    }
                }

                if sql.contains("WHERE") {
                    if features.filter_count > 0.0 {
                        println!("  ✓ WHERE detected correctly");
                    } else {
                        println!("  ❌ WHERE NOT detected - should be > 0");
                    }
                }

                if sql.contains("GROUP BY") {
                    if features.group_by_count > 0.0 {
                        println!("  ✓ GROUP BY detected correctly");
                    } else {
                        println!("  ❌ GROUP BY NOT detected - should be > 0");
                    }
                }
            }
            Err(e) => {
                println!("❌ Parse failed: {}", e);
            }
        }
        println!();
    }

    Ok(())
}

fn get_relexpr_type(expr: &ra_core::algebra::RelExpr) -> &'static str {
    use ra_core::algebra::RelExpr;
    match expr {
        RelExpr::Scan { .. } => "Scan",
        RelExpr::Filter { .. } => "Filter",
        RelExpr::Project { .. } => "Project",
        RelExpr::Join { .. } => "Join",
        RelExpr::Aggregate { .. } => "Aggregate",
        RelExpr::Sort { .. } => "Sort",
        RelExpr::Limit { .. } => "Limit",
        RelExpr::Distinct { .. } => "Distinct",
        RelExpr::Union { .. } => "Union",
        RelExpr::Values { .. } => "Values",
        _ => "Other",
    }
}

fn print_relexpr_tree(expr: &ra_core::algebra::RelExpr, depth: usize) {
    use ra_core::algebra::RelExpr;
    let indent = "  ".repeat(depth);

    match expr {
        RelExpr::Project { input, columns } => {
            println!("{}Project (columns: {})", indent, columns.len());
            print_relexpr_tree(input, depth + 1);
        }
        RelExpr::Scan { table, .. } => {
            println!("{}Scan (table: {})", indent, table);
        }
        RelExpr::Filter { input, .. } => {
            println!("{}Filter", indent);
            print_relexpr_tree(input, depth + 1);
        }
        RelExpr::Aggregate { input, aggregates, group_by, .. } => {
            println!("{}Aggregate (aggregates: {}, group_by: {})", indent, aggregates.len(), group_by.len());
            print_relexpr_tree(input, depth + 1);
        }
        RelExpr::Join { left, right, .. } => {
            println!("{}Join", indent);
            print_relexpr_tree(left, depth + 1);
            print_relexpr_tree(right, depth + 1);
        }
        _ => {
            println!("{}{}", indent, get_relexpr_type(expr));
        }
    }
}