//! Benchmark MySQL GROUP BY and aggregate queries.
//!
//! Compares aggregate query performance between native MySQL and Ra optimization.
//!
//! Run: `cargo run --example benchmark_aggregates --features mysql`

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

    println!("\nRunning aggregate query benchmarks...\n");

    let queries = vec![
        // Simple GROUP BY with COUNT
        "SELECT region, COUNT(*) as total_sales \
         FROM sales \
         GROUP BY region \
         ORDER BY total_sales DESC"
            .to_string(),
        // GROUP BY with multiple aggregates
        "SELECT product_category, \
                COUNT(*) as num_sales, \
                SUM(amount) as total_revenue, \
                AVG(amount) as avg_sale, \
                MIN(amount) as min_sale, \
                MAX(amount) as max_sale \
         FROM sales \
         GROUP BY product_category"
            .to_string(),
        // GROUP BY with HAVING
        "SELECT salesperson_id, SUM(amount) as total_sales \
         FROM sales \
         WHERE sale_date >= '2024-01-01' \
         GROUP BY salesperson_id \
         HAVING total_sales > 10000 \
         ORDER BY total_sales DESC"
            .to_string(),
        // Multiple GROUP BY columns
        "SELECT region, product_category, \
                COUNT(*) as sales_count, \
                SUM(amount) as revenue \
         FROM sales \
         GROUP BY region, product_category \
         ORDER BY revenue DESC \
         LIMIT 20"
            .to_string(),
        // GROUP BY with JOIN
        "SELECT c.name, \
                COUNT(s.id) as num_purchases, \
                SUM(s.amount) as total_spent \
         FROM customers c \
         LEFT JOIN sales s ON c.id = s.customer_id \
         GROUP BY c.id, c.name \
         HAVING num_purchases > 0 \
         ORDER BY total_spent DESC"
            .to_string(),
        // Window functions (MySQL 8.0+)
        "SELECT region, product_category, amount, \
                AVG(amount) OVER (PARTITION BY region) as region_avg, \
                RANK() OVER (PARTITION BY region ORDER BY amount DESC) as region_rank \
         FROM sales \
         WHERE sale_date >= '2024-01-01'"
            .to_string(),
        // Complex aggregation with subquery
        "SELECT region, \
                total_sales, \
                ROUND(total_sales / overall_total * 100, 2) as pct_of_total \
         FROM (\
             SELECT region, SUM(amount) as total_sales \
             FROM sales \
             GROUP BY region\
         ) regional_sales \
         CROSS JOIN (\
             SELECT SUM(amount) as overall_total \
             FROM sales\
         ) totals \
         ORDER BY pct_of_total DESC"
            .to_string(),
        // COUNT DISTINCT
        "SELECT region, \
                COUNT(DISTINCT customer_id) as unique_customers, \
                COUNT(DISTINCT product_category) as categories_sold \
         FROM sales \
         GROUP BY region"
            .to_string(),
    ];

    let report = compare_mysql_queries(&adapter, &queries)?;

    println!("{}", report.to_markdown());

    if let Ok(json) = report.to_json() {
        std::fs::write("mysql_aggregates_benchmark.json", json)?;
        println!("\nDetailed results saved to mysql_aggregates_benchmark.json");
    }

    cleanup_test_data(&adapter)?;

    Ok(())
}

fn setup_test_data(adapter: &MySQLAdapter) -> Result<(), Box<dyn std::error::Error>> {
    adapter.execute_native("DROP TABLE IF EXISTS sales")?;
    adapter.execute_native("DROP TABLE IF EXISTS customers")?;

    adapter.execute_native(
        "CREATE TABLE customers (
            id INT PRIMARY KEY AUTO_INCREMENT,
            name VARCHAR(100),
            email VARCHAR(100)
        )",
    )?;

    adapter.execute_native(
        "CREATE TABLE sales (
            id INT PRIMARY KEY AUTO_INCREMENT,
            customer_id INT,
            salesperson_id INT,
            product_category VARCHAR(50),
            region VARCHAR(50),
            amount DECIMAL(10, 2),
            sale_date DATE,
            FOREIGN KEY (customer_id) REFERENCES customers(id),
            INDEX idx_region (region),
            INDEX idx_category (product_category),
            INDEX idx_date (sale_date)
        )",
    )?;

    // Insert customers
    adapter.execute_native(
        "INSERT INTO customers (name, email) VALUES
        ('Customer 1', 'c1@example.com'),
        ('Customer 2', 'c2@example.com'),
        ('Customer 3', 'c3@example.com'),
        ('Customer 4', 'c4@example.com'),
        ('Customer 5', 'c5@example.com')",
    )?;

    // Insert sales data
    adapter.execute_native(
        "INSERT INTO sales (customer_id, salesperson_id, product_category, region, amount, sale_date) VALUES
        (1, 1, 'Electronics', 'North', 1299.99, '2024-01-15'),
        (2, 1, 'Electronics', 'North', 899.99, '2024-01-16'),
        (3, 2, 'Furniture', 'South', 2499.99, '2024-01-17'),
        (1, 2, 'Furniture', 'North', 1899.99, '2024-01-18'),
        (4, 3, 'Clothing', 'East', 149.99, '2024-01-19'),
        (5, 3, 'Clothing', 'West', 299.99, '2024-01-20'),
        (2, 1, 'Electronics', 'North', 1599.99, '2024-02-01'),
        (3, 2, 'Furniture', 'South', 3299.99, '2024-02-02'),
        (4, 3, 'Electronics', 'East', 799.99, '2024-02-03'),
        (5, 1, 'Clothing', 'West', 399.99, '2024-02-04'),
        (1, 2, 'Electronics', 'North', 2199.99, '2024-02-05'),
        (2, 3, 'Furniture', 'South', 1799.99, '2024-02-06'),
        (3, 1, 'Clothing', 'East', 249.99, '2024-02-07'),
        (4, 2, 'Electronics', 'West', 1099.99, '2024-02-08'),
        (5, 3, 'Furniture', 'North', 2799.99, '2024-02-09'),
        (1, 1, 'Clothing', 'South', 179.99, '2024-03-01'),
        (2, 2, 'Electronics', 'East', 1499.99, '2024-03-02'),
        (3, 3, 'Furniture', 'West', 3599.99, '2024-03-03'),
        (4, 1, 'Clothing', 'North', 329.99, '2024-03-04'),
        (5, 2, 'Electronics', 'South', 999.99, '2024-03-05')",
    )?;

    println!("Test data created successfully");
    Ok(())
}

fn cleanup_test_data(adapter: &MySQLAdapter) -> Result<(), Box<dyn std::error::Error>> {
    adapter.execute_native("DROP TABLE IF EXISTS sales")?;
    adapter.execute_native("DROP TABLE IF EXISTS customers")?;
    println!("\nTest data cleaned up");
    Ok(())
}
