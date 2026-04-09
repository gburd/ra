//! Benchmark MySQL JOIN queries native vs Ra-optimized execution.
//!
//! Compares different JOIN strategies and optimizations.
//!
//! Run: `cargo run --example benchmark_joins --features mysql`

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

    println!("\nRunning JOIN benchmarks...\n");

    let queries = vec![
        // Simple INNER JOIN
        "SELECT o.id, o.order_date, c.name, c.email \
         FROM orders o \
         INNER JOIN customers c ON o.customer_id = c.id \
         WHERE o.status = 'completed'"
            .to_string(),
        // LEFT JOIN with NULL check
        "SELECT c.id, c.name, COUNT(o.id) as order_count \
         FROM customers c \
         LEFT JOIN orders o ON c.id = o.customer_id \
         GROUP BY c.id, c.name \
         HAVING order_count > 5"
            .to_string(),
        // Multiple JOINs
        "SELECT o.id, c.name, p.product_name, oi.quantity, oi.price \
         FROM orders o \
         INNER JOIN customers c ON o.customer_id = c.id \
         INNER JOIN order_items oi ON o.id = oi.order_id \
         INNER JOIN products p ON oi.product_id = p.id \
         WHERE o.order_date >= '2024-01-01'"
            .to_string(),
        // Self JOIN
        "SELECT e1.name as employee, e2.name as manager \
         FROM employees e1 \
         LEFT JOIN employees e2 ON e1.manager_id = e2.id \
         WHERE e1.department = 'Engineering'"
            .to_string(),
        // JOIN with aggregation
        "SELECT c.name, SUM(oi.quantity * oi.price) as total_spent \
         FROM customers c \
         INNER JOIN orders o ON c.id = o.customer_id \
         INNER JOIN order_items oi ON o.id = oi.order_id \
         WHERE o.status = 'completed' \
         GROUP BY c.id, c.name \
         ORDER BY total_spent DESC \
         LIMIT 10"
            .to_string(),
        // JOIN with subquery
        "SELECT c.name, recent_orders.order_count \
         FROM customers c \
         INNER JOIN (\
             SELECT customer_id, COUNT(*) as order_count \
             FROM orders \
             WHERE order_date >= DATE_SUB(NOW(), INTERVAL 30 DAY) \
             GROUP BY customer_id\
         ) recent_orders ON c.id = recent_orders.customer_id \
         WHERE recent_orders.order_count >= 3"
            .to_string(),
    ];

    let report = compare_mysql_queries(&adapter, &queries)?;

    println!("{}", report.to_markdown());

    if let Ok(json) = report.to_json() {
        std::fs::write("mysql_joins_benchmark.json", json)?;
        println!("\nDetailed results saved to mysql_joins_benchmark.json");
    }

    cleanup_test_data(&adapter)?;

    Ok(())
}

fn setup_test_data(adapter: &MySQLAdapter) -> Result<(), Box<dyn std::error::Error>> {
    adapter.execute_native("DROP TABLE IF EXISTS order_items")?;
    adapter.execute_native("DROP TABLE IF EXISTS orders")?;
    adapter.execute_native("DROP TABLE IF EXISTS customers")?;
    adapter.execute_native("DROP TABLE IF EXISTS products")?;
    adapter.execute_native("DROP TABLE IF EXISTS employees")?;

    adapter.execute_native(
        "CREATE TABLE customers (
            id INT PRIMARY KEY AUTO_INCREMENT,
            name VARCHAR(100),
            email VARCHAR(100),
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
    )?;

    adapter.execute_native(
        "CREATE TABLE products (
            id INT PRIMARY KEY AUTO_INCREMENT,
            product_name VARCHAR(100),
            price DECIMAL(10, 2)
        )",
    )?;

    adapter.execute_native(
        "CREATE TABLE orders (
            id INT PRIMARY KEY AUTO_INCREMENT,
            customer_id INT,
            order_date DATETIME DEFAULT CURRENT_TIMESTAMP,
            status VARCHAR(20),
            FOREIGN KEY (customer_id) REFERENCES customers(id),
            INDEX idx_customer_date (customer_id, order_date),
            INDEX idx_status (status)
        )",
    )?;

    adapter.execute_native(
        "CREATE TABLE order_items (
            id INT PRIMARY KEY AUTO_INCREMENT,
            order_id INT,
            product_id INT,
            quantity INT,
            price DECIMAL(10, 2),
            FOREIGN KEY (order_id) REFERENCES orders(id),
            FOREIGN KEY (product_id) REFERENCES products(id)
        )",
    )?;

    adapter.execute_native(
        "CREATE TABLE employees (
            id INT PRIMARY KEY AUTO_INCREMENT,
            name VARCHAR(100),
            manager_id INT,
            department VARCHAR(50),
            FOREIGN KEY (manager_id) REFERENCES employees(id)
        )",
    )?;

    // Insert sample data
    adapter.execute_native(
        "INSERT INTO customers (name, email) VALUES
        ('Alice Johnson', 'alice@example.com'),
        ('Bob Smith', 'bob@example.com'),
        ('Charlie Brown', 'charlie@example.com'),
        ('Diana Prince', 'diana@example.com'),
        ('Eve Wilson', 'eve@example.com')",
    )?;

    adapter.execute_native(
        "INSERT INTO products (product_name, price) VALUES
        ('Widget A', 19.99),
        ('Widget B', 29.99),
        ('Widget C', 39.99),
        ('Gadget X', 49.99),
        ('Gadget Y', 59.99)",
    )?;

    adapter.execute_native(
        "INSERT INTO orders (customer_id, status) VALUES
        (1, 'completed'), (1, 'completed'), (1, 'pending'),
        (2, 'completed'), (2, 'completed'),
        (3, 'completed'), (3, 'cancelled'),
        (4, 'completed'), (5, 'pending')",
    )?;

    adapter.execute_native(
        "INSERT INTO order_items (order_id, product_id, quantity, price) VALUES
        (1, 1, 2, 19.99), (1, 2, 1, 29.99),
        (2, 3, 1, 39.99), (3, 1, 3, 19.99),
        (4, 4, 2, 49.99), (5, 5, 1, 59.99),
        (6, 2, 2, 29.99), (7, 1, 1, 19.99),
        (8, 3, 1, 39.99), (9, 4, 1, 49.99)",
    )?;

    adapter.execute_native(
        "INSERT INTO employees (name, manager_id, department) VALUES
        ('CEO', NULL, 'Executive'),
        ('CTO', 1, 'Engineering'),
        ('VP Eng', 2, 'Engineering'),
        ('Lead Dev', 3, 'Engineering'),
        ('Developer 1', 4, 'Engineering'),
        ('Developer 2', 4, 'Engineering')",
    )?;

    println!("Test data created successfully");
    Ok(())
}

fn cleanup_test_data(adapter: &MySQLAdapter) -> Result<(), Box<dyn std::error::Error>> {
    adapter.execute_native("DROP TABLE IF EXISTS order_items")?;
    adapter.execute_native("DROP TABLE IF EXISTS orders")?;
    adapter.execute_native("DROP TABLE IF EXISTS customers")?;
    adapter.execute_native("DROP TABLE IF EXISTS products")?;
    adapter.execute_native("DROP TABLE IF EXISTS employees")?;
    println!("\nTest data cleaned up");
    Ok(())
}
