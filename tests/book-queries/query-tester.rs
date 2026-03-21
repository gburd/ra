#!/usr/bin/env rust-script
//! Test SQL queries from database books against Ra's parser
//!
//! ```cargo
//! [dependencies]
//! ra-parser = { path = "../../crates/ra-parser" }
//! ra-core = { path = "../../crates/ra-core" }
//! anyhow = "1.0"
//! ```

use std::fs;
use std::path::Path;
use ra_parser::sql_to_relexpr;

fn main() -> anyhow::Result<()> {
    let test_dir = Path::new("tests/book-queries");
    let mut total = 0;
    let mut success = 0;
    let mut failures = Vec::new();

    // Process each SQL file
    for entry in fs::read_dir(test_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("sql") {
            println!("Processing: {:?}", path);

            let content = fs::read_to_string(&path)?;
            let queries = parse_sql_file(&content);

            for (idx, query) in queries.iter().enumerate() {
                total += 1;
                print!("  Query {}: ", idx + 1);

                match sql_to_relexpr(query) {
                    Ok(_) => {
                        success += 1;
                        println!("✓");
                    }
                    Err(e) => {
                        println!("✗");
                        failures.push((
                            path.file_name().unwrap().to_string_lossy().to_string(),
                            idx + 1,
                            query.clone(),
                            format!("{:?}", e),
                        ));
                    }
                }
            }
        }
    }

    println!("\n========================================");
    println!("Test Summary");
    println!("========================================");
    println!("Total Queries: {}", total);
    println!("Success: {}", success);
    println!("Failures: {}", failures.len());
    println!("Success Rate: {:.2}%", 100.0 * success as f64 / total as f64);

    if !failures.is_empty() {
        println!("\n========================================");
        println!("Failures");
        println!("========================================");
        for (file, idx, query, error) in failures {
            println!("\n{} - Query {}", file, idx);
            println!("Query: {}", query);
            println!("Error: {}", error);
        }
    }

    Ok(())
}

/// Parse SQL file into individual queries
fn parse_sql_file(content: &str) -> Vec<String> {
    let mut queries = Vec::new();
    let mut current_query = String::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines and comment-only lines
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }

        // Accumulate query lines
        current_query.push_str(line);
        current_query.push('\n');

        // If line ends with semicolon, we have a complete query
        if trimmed.ends_with(';') {
            queries.push(current_query.trim().trim_end_matches(';').to_string());
            current_query.clear();
        }
    }

    queries
}
