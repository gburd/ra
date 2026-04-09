//! Benchmark analytical queries comparing DuckDB native execution vs Ra optimization.
//!
//! This benchmark focuses on OLAP workloads including:
//! - Window functions
//! - Complex aggregations
//! - GROUP BY with multiple columns
//! - HAVING clauses
//! - Subqueries

use ra_adapters::DuckDBAdapter;
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    println!("DuckDB Analytics Benchmark - Native vs Ra Optimization\n");
    println!("=" .repeat(80));

    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:")?;

    setup_test_data(&adapter)?;

    println!("\n1. Window Function Queries");
    println!("-".repeat(80));
    benchmark_window_functions(&adapter)?;

    println!("\n2. Aggregation Queries");
    println!("-".repeat(80));
    benchmark_aggregations(&adapter)?;

    println!("\n3. Grouped Aggregation Queries");
    println!("-".repeat(80));
    benchmark_grouped_aggregations(&adapter)?;

    println!("\n4. Subquery Queries");
    println!("-".repeat(80));
    benchmark_subqueries(&adapter)?;

    println!("\n5. Complex Analytical Queries");
    println!("-".repeat(80));
    benchmark_complex_analytics(&adapter)?;

    println!("\n" .repeat(2));
    println!("=" .repeat(80));
    println!("Benchmark Complete");

    Ok(())
}

fn setup_test_data(adapter: &DuckDBAdapter) -> anyhow::Result<()> {
    println!("\nSetting up test data...");

    adapter.execute("CREATE TABLE sales (
        sale_id INTEGER,
        product_id INTEGER,
        customer_id INTEGER,
        region VARCHAR,
        sale_date DATE,
        amount DECIMAL(10,2),
        quantity INTEGER
    )")?;

    adapter.execute("INSERT INTO sales
        SELECT
            i as sale_id,
            (i % 100) + 1 as product_id,
            (i % 1000) + 1 as customer_id,
            CASE (i % 4)
                WHEN 0 THEN 'North'
                WHEN 1 THEN 'South'
                WHEN 2 THEN 'East'
                ELSE 'West'
            END as region,
            DATE '2024-01-01' + INTERVAL (i % 365) DAY as sale_date,
            (50.0 + (i % 450)) * 1.5 as amount,
            (i % 10) + 1 as quantity
        FROM range(100000) t(i)
    ")?;

    adapter.execute("CREATE TABLE products (
        product_id INTEGER,
        product_name VARCHAR,
        category VARCHAR,
        unit_price DECIMAL(10,2)
    )")?;

    adapter.execute("INSERT INTO products
        SELECT
            i as product_id,
            'Product_' || i as product_name,
            CASE (i % 5)
                WHEN 0 THEN 'Electronics'
                WHEN 1 THEN 'Clothing'
                WHEN 2 THEN 'Food'
                WHEN 3 THEN 'Books'
                ELSE 'Home'
            END as category,
            (10.0 + (i % 90)) * 2.5 as unit_price
        FROM range(100) t(i)
    ")?;

    adapter.execute("CREATE TABLE customers (
        customer_id INTEGER,
        customer_name VARCHAR,
        customer_type VARCHAR,
        signup_date DATE
    )")?;

    adapter.execute("INSERT INTO customers
        SELECT
            i as customer_id,
            'Customer_' || i as customer_name,
            CASE (i % 3)
                WHEN 0 THEN 'Premium'
                WHEN 1 THEN 'Standard'
                ELSE 'Basic'
            END as customer_type,
            DATE '2023-01-01' + INTERVAL (i % 365) DAY as signup_date
        FROM range(1000) t(i)
    ")?;

    println!("Test data loaded successfully");

    Ok(())
}

fn benchmark_window_functions(adapter: &DuckDBAdapter) -> anyhow::Result<()> {
    let queries = vec![
        (
            "Running total by region",
            "SELECT region, sale_date, amount,
                    SUM(amount) OVER (PARTITION BY region ORDER BY sale_date) as running_total
             FROM sales
             ORDER BY region, sale_date
             LIMIT 1000"
        ),
        (
            "Rank products by sales",
            "SELECT product_id, region, SUM(amount) as total_sales,
                    RANK() OVER (PARTITION BY region ORDER BY SUM(amount) DESC) as rank
             FROM sales
             GROUP BY product_id, region
             ORDER BY region, rank"
        ),
        (
            "Moving average",
            "SELECT sale_date, AVG(amount) as daily_avg,
                    AVG(AVG(amount)) OVER (
                        ORDER BY sale_date
                        ROWS BETWEEN 6 PRECEDING AND CURRENT ROW
                    ) as moving_avg_7day
             FROM sales
             GROUP BY sale_date
             ORDER BY sale_date"
        ),
    ];

    for (name, query) in queries {
        println!("\n  {name}");
        run_comparison(adapter, query)?;
    }

    Ok(())
}

fn benchmark_aggregations(adapter: &DuckDBAdapter) -> anyhow::Result<()> {
    let queries = vec![
        (
            "Total sales by region",
            "SELECT region, COUNT(*) as sale_count, SUM(amount) as total_sales,
                    AVG(amount) as avg_sale, MIN(amount) as min_sale, MAX(amount) as max_sale
             FROM sales
             GROUP BY region
             ORDER BY total_sales DESC"
        ),
        (
            "Monthly aggregations",
            "SELECT DATE_TRUNC('month', sale_date) as month,
                    COUNT(*) as transactions,
                    SUM(amount) as revenue,
                    AVG(quantity) as avg_quantity
             FROM sales
             GROUP BY month
             ORDER BY month"
        ),
        (
            "Product category stats",
            "SELECT p.category,
                    COUNT(DISTINCT s.customer_id) as unique_customers,
                    COUNT(*) as total_sales,
                    SUM(s.amount) as revenue
             FROM sales s
             JOIN products p ON s.product_id = p.product_id
             GROUP BY p.category
             ORDER BY revenue DESC"
        ),
    ];

    for (name, query) in queries {
        println!("\n  {name}");
        run_comparison(adapter, query)?;
    }

    Ok(())
}

fn benchmark_grouped_aggregations(adapter: &DuckDBAdapter) -> anyhow::Result<()> {
    let queries = vec![
        (
            "Multi-level grouping",
            "SELECT region, DATE_TRUNC('month', sale_date) as month,
                    COUNT(*) as sales_count, SUM(amount) as total_amount
             FROM sales
             GROUP BY region, month
             HAVING SUM(amount) > 10000
             ORDER BY region, month"
        ),
        (
            "Customer type analysis",
            "SELECT c.customer_type, p.category,
                    COUNT(*) as purchase_count,
                    AVG(s.amount) as avg_purchase,
                    SUM(s.amount) as total_spent
             FROM sales s
             JOIN customers c ON s.customer_id = c.customer_id
             JOIN products p ON s.product_id = p.product_id
             GROUP BY c.customer_type, p.category
             HAVING COUNT(*) > 100
             ORDER BY total_spent DESC"
        ),
        (
            "Regional product performance",
            "SELECT s.region, p.category, p.product_name,
                    COUNT(*) as times_sold,
                    SUM(s.quantity) as total_quantity,
                    SUM(s.amount) as revenue
             FROM sales s
             JOIN products p ON s.product_id = p.product_id
             GROUP BY s.region, p.category, p.product_name
             HAVING COUNT(*) > 50
             ORDER BY revenue DESC
             LIMIT 100"
        ),
    ];

    for (name, query) in queries {
        println!("\n  {name}");
        run_comparison(adapter, query)?;
    }

    Ok(())
}

fn benchmark_subqueries(adapter: &DuckDBAdapter) -> anyhow::Result<()> {
    let queries = vec![
        (
            "Above average sales",
            "SELECT sale_id, customer_id, amount
             FROM sales
             WHERE amount > (SELECT AVG(amount) FROM sales)
             ORDER BY amount DESC
             LIMIT 100"
        ),
        (
            "Top customers by region",
            "SELECT region, customer_id, total_spent
             FROM (
                 SELECT region, customer_id, SUM(amount) as total_spent,
                        RANK() OVER (PARTITION BY region ORDER BY SUM(amount) DESC) as rank
                 FROM sales
                 GROUP BY region, customer_id
             ) ranked
             WHERE rank <= 10
             ORDER BY region, total_spent DESC"
        ),
        (
            "Products with sales in all regions",
            "SELECT p.product_id, p.product_name, p.category
             FROM products p
             WHERE (
                 SELECT COUNT(DISTINCT region)
                 FROM sales s
                 WHERE s.product_id = p.product_id
             ) = 4
             ORDER BY p.product_id"
        ),
    ];

    for (name, query) in queries {
        println!("\n  {name}");
        run_comparison(adapter, query)?;
    }

    Ok(())
}

fn benchmark_complex_analytics(adapter: &DuckDBAdapter) -> anyhow::Result<()> {
    let queries = vec![
        (
            "Customer cohort analysis",
            "SELECT DATE_TRUNC('month', c.signup_date) as cohort_month,
                    DATE_TRUNC('month', s.sale_date) as activity_month,
                    COUNT(DISTINCT s.customer_id) as active_customers,
                    SUM(s.amount) as revenue
             FROM customers c
             JOIN sales s ON c.customer_id = s.customer_id
             GROUP BY cohort_month, activity_month
             HAVING COUNT(DISTINCT s.customer_id) > 10
             ORDER BY cohort_month, activity_month"
        ),
        (
            "Product affinity analysis",
            "SELECT s1.product_id as product_a,
                    s2.product_id as product_b,
                    COUNT(DISTINCT s1.customer_id) as customers_bought_both
             FROM sales s1
             JOIN sales s2 ON s1.customer_id = s2.customer_id
                           AND s1.product_id < s2.product_id
             GROUP BY s1.product_id, s2.product_id
             HAVING COUNT(DISTINCT s1.customer_id) > 50
             ORDER BY customers_bought_both DESC
             LIMIT 50"
        ),
        (
            "Seasonal trend analysis",
            "SELECT EXTRACT(QUARTER FROM sale_date) as quarter,
                    region, category,
                    COUNT(*) as sales_count,
                    SUM(s.amount) as revenue,
                    AVG(s.amount) as avg_sale
             FROM sales s
             JOIN products p ON s.product_id = p.product_id
             GROUP BY quarter, region, category
             ORDER BY quarter, region, revenue DESC"
        ),
    ];

    for (name, query) in queries {
        println!("\n  {name}");
        run_comparison(adapter, query)?;
    }

    Ok(())
}

fn run_comparison(adapter: &DuckDBAdapter, query: &str) -> anyhow::Result<()> {
    let metrics = adapter.compare_execution(query)?;

    println!("    Native: {:>8} μs ({} rows)",
        metrics.native_duration.as_micros(), metrics.row_count);
    println!("    Ra:     {:>8} μs ({} rows)",
        metrics.ra_duration.as_micros(), metrics.row_count);
    println!("    Speedup: {:.2}x {}",
        metrics.speedup,
        if metrics.speedup > 1.0 { "✓" } else { "✗" });

    Ok(())
}
