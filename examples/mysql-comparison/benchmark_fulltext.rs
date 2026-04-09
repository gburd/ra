//! Benchmark MySQL FULLTEXT search native vs Ra-optimized execution.
//!
//! This example compares MySQL's native FULLTEXT MATCH...AGAINST queries
//! with Ra-optimized alternatives.
//!
//! Setup:
//! 1. Create MySQL database: `CREATE DATABASE benchmark;`
//! 2. Set environment variable: `export TEST_MYSQL_URL="mysql://root@localhost:3306/benchmark"`
//! 3. Run: `cargo run --example benchmark_fulltext --features mysql`

use ra_adapters::{compare_mysql_queries, DatabaseAdapter, MySQLAdapter};
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let url = env::var("TEST_MYSQL_URL")
        .unwrap_or_else(|_| "mysql://root@localhost:3306/benchmark".to_string());

    println!("Connecting to MySQL: {url}");
    let mut adapter = MySQLAdapter::new();
    adapter.connect(&url)?;

    println!("Setting up test data...");
    setup_test_data(&adapter)?;

    println!("\nRunning FULLTEXT search benchmarks...\n");

    let queries = vec![
        // Simple FULLTEXT search
        "SELECT id, title FROM articles \
         WHERE MATCH(title, body) AGAINST('database')"
            .to_string(),
        // FULLTEXT with boolean mode
        "SELECT id, title FROM articles \
         WHERE MATCH(title, body) AGAINST('+mysql -oracle' IN BOOLEAN MODE)"
            .to_string(),
        // FULLTEXT with relevance scoring
        "SELECT id, title, MATCH(title, body) AGAINST('optimization') as relevance \
         FROM articles \
         WHERE MATCH(title, body) AGAINST('optimization') \
         ORDER BY relevance DESC \
         LIMIT 10"
            .to_string(),
        // Combined FULLTEXT and traditional filtering
        "SELECT id, title, created_at FROM articles \
         WHERE MATCH(title, body) AGAINST('mysql') \
         AND created_at > '2024-01-01' \
         ORDER BY created_at DESC"
            .to_string(),
        // FULLTEXT with JOIN
        "SELECT a.id, a.title, c.name as category \
         FROM articles a \
         JOIN categories c ON a.category_id = c.id \
         WHERE MATCH(a.title, a.body) AGAINST('performance')"
            .to_string(),
    ];

    let report = compare_mysql_queries(&adapter, &queries)?;

    println!("{}", report.to_markdown());

    if let Ok(json) = report.to_json() {
        std::fs::write("mysql_fulltext_benchmark.json", json)?;
        println!("\nDetailed results saved to mysql_fulltext_benchmark.json");
    }

    cleanup_test_data(&adapter)?;

    Ok(())
}

fn setup_test_data(adapter: &MySQLAdapter) -> Result<(), Box<dyn std::error::Error>> {
    adapter.execute_native("DROP TABLE IF EXISTS articles")?;
    adapter.execute_native("DROP TABLE IF EXISTS categories")?;

    adapter.execute_native(
        "CREATE TABLE categories (
            id INT PRIMARY KEY AUTO_INCREMENT,
            name VARCHAR(100)
        )",
    )?;

    adapter.execute_native(
        "CREATE TABLE articles (
            id INT PRIMARY KEY AUTO_INCREMENT,
            title VARCHAR(200),
            body TEXT,
            category_id INT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FULLTEXT(title, body),
            FOREIGN KEY (category_id) REFERENCES categories(id)
        )",
    )?;

    adapter.execute_native(
        "INSERT INTO categories (name) VALUES
        ('Databases'), ('Programming'), ('DevOps')",
    )?;

    adapter.execute_native(
        "INSERT INTO articles (title, body, category_id) VALUES
        ('MySQL Performance Tuning', 'Learn how to optimize MySQL database performance for large-scale applications', 1),
        ('Introduction to MySQL', 'MySQL is the most popular open source database management system', 1),
        ('PostgreSQL vs MySQL', 'A comparison between PostgreSQL and MySQL database systems', 1),
        ('Database Indexing Strategies', 'Understanding B-tree indexes and optimization techniques', 1),
        ('Python Database Programming', 'How to connect Python applications to MySQL and PostgreSQL', 2),
        ('Rust Database Libraries', 'Overview of database libraries available for Rust programming', 2),
        ('Docker for Databases', 'Running MySQL in Docker containers for development', 3),
        ('Kubernetes Database Operators', 'Managing MySQL clusters in Kubernetes', 3),
        ('Query Optimization Techniques', 'Advanced query optimization for complex SQL queries', 1),
        ('Full-Text Search in MySQL', 'Using FULLTEXT indexes for text search in MySQL', 1),
        ('MySQL Replication', 'Setting up master-slave replication for high availability', 1),
        ('Database Sharding', 'Horizontal partitioning strategies for MySQL', 1),
        ('NoSQL vs SQL', 'When to use NoSQL databases instead of MySQL', 1),
        ('MySQL 8.0 New Features', 'Window functions and CTEs in MySQL 8.0', 1),
        ('Database Security', 'Best practices for securing MySQL databases', 1)",
    )?;

    println!("Test data created successfully");
    Ok(())
}

fn cleanup_test_data(adapter: &MySQLAdapter) -> Result<(), Box<dyn std::error::Error>> {
    adapter.execute_native("DROP TABLE IF EXISTS articles")?;
    adapter.execute_native("DROP TABLE IF EXISTS categories")?;
    println!("\nTest data cleaned up");
    Ok(())
}
