//! Benchmark join strategies comparing DuckDB native execution vs Ra optimization.
//!
//! This benchmark focuses on:
//! - Hash joins
//! - Nested loop joins
//! - Sort-merge joins
//! - Multi-way joins
//! - Self joins
//! - Cross joins

use ra_adapters::DuckDBAdapter;

fn main() -> anyhow::Result<()> {
    println!("DuckDB Join Benchmark - Native vs Ra Optimization\n");
    println!("=".repeat(80));

    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:")?;

    setup_test_data(&adapter)?;

    println!("\n1. Basic Join Queries");
    println!("-".repeat(80));
    benchmark_basic_joins(&adapter)?;

    println!("\n2. Multi-way Joins");
    println!("-".repeat(80));
    benchmark_multiway_joins(&adapter)?;

    println!("\n3. Self Joins");
    println!("-".repeat(80));
    benchmark_self_joins(&adapter)?;

    println!("\n4. Outer Joins");
    println!("-".repeat(80));
    benchmark_outer_joins(&adapter)?;

    println!("\n5. Join with Aggregations");
    println!("-".repeat(80));
    benchmark_join_aggregations(&adapter)?;

    println!("\n6. Complex Join Patterns");
    println!("-".repeat(80));
    benchmark_complex_joins(&adapter)?;

    println!("\n".repeat(2));
    println!("=".repeat(80));
    println!("Benchmark Complete");

    Ok(())
}

fn setup_test_data(adapter: &DuckDBAdapter) -> anyhow::Result<()> {
    println!("\nSetting up test data...");

    adapter.execute("CREATE TABLE orders (
        order_id INTEGER,
        customer_id INTEGER,
        order_date DATE,
        total_amount DECIMAL(10,2),
        status VARCHAR
    )")?;

    adapter.execute("INSERT INTO orders
        SELECT
            i as order_id,
            (i % 10000) + 1 as customer_id,
            DATE '2024-01-01' + INTERVAL (i % 365) DAY as order_date,
            (50.0 + (i % 950)) * 1.5 as total_amount,
            CASE (i % 5)
                WHEN 0 THEN 'PENDING'
                WHEN 1 THEN 'PROCESSING'
                WHEN 2 THEN 'SHIPPED'
                WHEN 3 THEN 'DELIVERED'
                ELSE 'CANCELLED'
            END as status
        FROM range(100000) t(i)
    ")?;

    adapter.execute("CREATE TABLE order_items (
        item_id INTEGER,
        order_id INTEGER,
        product_id INTEGER,
        quantity INTEGER,
        unit_price DECIMAL(10,2)
    )")?;

    adapter.execute("INSERT INTO order_items
        SELECT
            i as item_id,
            (i / 3) as order_id,
            (i % 1000) + 1 as product_id,
            (i % 5) + 1 as quantity,
            (10.0 + (i % 190)) * 1.2 as unit_price
        FROM range(300000) t(i)
    ")?;

    adapter.execute("CREATE TABLE customers (
        customer_id INTEGER,
        customer_name VARCHAR,
        email VARCHAR,
        signup_date DATE,
        customer_tier VARCHAR
    )")?;

    adapter.execute("INSERT INTO customers
        SELECT
            i as customer_id,
            'Customer_' || i as customer_name,
            'customer' || i || '@example.com' as email,
            DATE '2023-01-01' + INTERVAL (i % 730) DAY as signup_date,
            CASE (i % 4)
                WHEN 0 THEN 'GOLD'
                WHEN 1 THEN 'SILVER'
                WHEN 2 THEN 'BRONZE'
                ELSE 'STANDARD'
            END as customer_tier
        FROM range(10000) t(i)
    ")?;

    adapter.execute("CREATE TABLE products (
        product_id INTEGER,
        product_name VARCHAR,
        category VARCHAR,
        supplier_id INTEGER,
        list_price DECIMAL(10,2)
    )")?;

    adapter.execute("INSERT INTO products
        SELECT
            i as product_id,
            'Product_' || i as product_name,
            CASE (i % 10)
                WHEN 0 THEN 'Electronics'
                WHEN 1 THEN 'Clothing'
                WHEN 2 THEN 'Food'
                WHEN 3 THEN 'Books'
                WHEN 4 THEN 'Home'
                WHEN 5 THEN 'Sports'
                WHEN 6 THEN 'Toys'
                WHEN 7 THEN 'Beauty'
                WHEN 8 THEN 'Garden'
                ELSE 'Automotive'
            END as category,
            (i % 100) + 1 as supplier_id,
            (15.0 + (i % 285)) * 2.0 as list_price
        FROM range(1000) t(i)
    ")?;

    adapter.execute("CREATE TABLE suppliers (
        supplier_id INTEGER,
        supplier_name VARCHAR,
        country VARCHAR,
        rating DECIMAL(3,2)
    )")?;

    adapter.execute("INSERT INTO suppliers
        SELECT
            i as supplier_id,
            'Supplier_' || i as supplier_name,
            CASE (i % 5)
                WHEN 0 THEN 'USA'
                WHEN 1 THEN 'China'
                WHEN 2 THEN 'Germany'
                WHEN 3 THEN 'Japan'
                ELSE 'India'
            END as country,
            3.0 + (i % 20) * 0.1 as rating
        FROM range(100) t(i)
    ")?;

    println!("Test data loaded successfully");

    Ok(())
}

fn benchmark_basic_joins(adapter: &DuckDBAdapter) -> anyhow::Result<()> {
    let queries = vec![
        (
            "Inner join (orders-customers)",
            "SELECT o.order_id, c.customer_name, o.total_amount, o.status
             FROM orders o
             JOIN customers c ON o.customer_id = c.customer_id
             WHERE o.status = 'DELIVERED'
             LIMIT 10000"
        ),
        (
            "Inner join (order_items-products)",
            "SELECT oi.item_id, p.product_name, p.category, oi.quantity
             FROM order_items oi
             JOIN products p ON oi.product_id = p.product_id
             WHERE p.category = 'Electronics'
             LIMIT 10000"
        ),
        (
            "Join with aggregation",
            "SELECT c.customer_id, c.customer_name,
                    COUNT(*) as order_count,
                    SUM(o.total_amount) as total_spent
             FROM customers c
             JOIN orders o ON c.customer_id = o.customer_id
             GROUP BY c.customer_id, c.customer_name
             HAVING COUNT(*) > 5
             ORDER BY total_spent DESC
             LIMIT 100"
        ),
    ];

    for (name, query) in queries {
        println!("\n  {name}");
        run_comparison(adapter, query)?;
    }

    Ok(())
}

fn benchmark_multiway_joins(adapter: &DuckDBAdapter) -> anyhow::Result<()> {
    let queries = vec![
        (
            "Three-way join",
            "SELECT o.order_id, c.customer_name, oi.item_id, p.product_name
             FROM orders o
             JOIN customers c ON o.customer_id = c.customer_id
             JOIN order_items oi ON o.order_id = oi.order_id
             JOIN products p ON oi.product_id = p.product_id
             WHERE o.status = 'DELIVERED'
             LIMIT 10000"
        ),
        (
            "Four-way join with aggregation",
            "SELECT c.customer_tier, p.category,
                    COUNT(DISTINCT o.order_id) as order_count,
                    SUM(oi.quantity * oi.unit_price) as total_revenue
             FROM customers c
             JOIN orders o ON c.customer_id = o.customer_id
             JOIN order_items oi ON o.order_id = oi.order_id
             JOIN products p ON oi.product_id = p.product_id
             GROUP BY c.customer_tier, p.category
             ORDER BY total_revenue DESC"
        ),
        (
            "Five-way join (full supply chain)",
            "SELECT s.country, p.category, c.customer_tier,
                    COUNT(*) as item_count,
                    AVG(oi.unit_price) as avg_price
             FROM suppliers s
             JOIN products p ON s.supplier_id = p.supplier_id
             JOIN order_items oi ON p.product_id = oi.product_id
             JOIN orders o ON oi.order_id = o.order_id
             JOIN customers c ON o.customer_id = c.customer_id
             WHERE o.status = 'DELIVERED'
             GROUP BY s.country, p.category, c.customer_tier
             ORDER BY item_count DESC"
        ),
    ];

    for (name, query) in queries {
        println!("\n  {name}");
        run_comparison(adapter, query)?;
    }

    Ok(())
}

fn benchmark_self_joins(adapter: &DuckDBAdapter) -> anyhow::Result<()> {
    let queries = vec![
        (
            "Find customers with multiple orders on same day",
            "SELECT o1.customer_id, o1.order_date, COUNT(*) as same_day_orders
             FROM orders o1
             JOIN orders o2 ON o1.customer_id = o2.customer_id
                           AND o1.order_date = o2.order_date
                           AND o1.order_id < o2.order_id
             GROUP BY o1.customer_id, o1.order_date
             ORDER BY same_day_orders DESC
             LIMIT 100"
        ),
        (
            "Product co-purchase analysis",
            "SELECT oi1.product_id as product_a,
                    oi2.product_id as product_b,
                    COUNT(DISTINCT oi1.order_id) as orders_together
             FROM order_items oi1
             JOIN order_items oi2 ON oi1.order_id = oi2.order_id
                                  AND oi1.product_id < oi2.product_id
             GROUP BY oi1.product_id, oi2.product_id
             HAVING COUNT(DISTINCT oi1.order_id) > 10
             ORDER BY orders_together DESC
             LIMIT 100"
        ),
        (
            "Customer referral chain",
            "SELECT c1.customer_id as referrer,
                    c2.customer_id as referred,
                    DATEDIFF('day', c1.signup_date, c2.signup_date) as days_diff
             FROM customers c1
             JOIN customers c2 ON c1.customer_id = c2.customer_id / 10
                               AND c2.signup_date > c1.signup_date
             WHERE DATEDIFF('day', c1.signup_date, c2.signup_date) <= 30
             LIMIT 1000"
        ),
    ];

    for (name, query) in queries {
        println!("\n  {name}");
        run_comparison(adapter, query)?;
    }

    Ok(())
}

fn benchmark_outer_joins(adapter: &DuckDBAdapter) -> anyhow::Result<()> {
    let queries = vec![
        (
            "Left join (all customers with orders)",
            "SELECT c.customer_id, c.customer_name,
                    COUNT(o.order_id) as order_count,
                    COALESCE(SUM(o.total_amount), 0) as total_spent
             FROM customers c
             LEFT JOIN orders o ON c.customer_id = o.customer_id
             GROUP BY c.customer_id, c.customer_name
             ORDER BY order_count DESC
             LIMIT 1000"
        ),
        (
            "Find customers without orders",
            "SELECT c.customer_id, c.customer_name, c.signup_date
             FROM customers c
             LEFT JOIN orders o ON c.customer_id = o.customer_id
             WHERE o.order_id IS NULL
             LIMIT 1000"
        ),
        (
            "Products never ordered",
            "SELECT p.product_id, p.product_name, p.category, p.list_price
             FROM products p
             LEFT JOIN order_items oi ON p.product_id = oi.product_id
             WHERE oi.item_id IS NULL
             ORDER BY p.list_price DESC"
        ),
    ];

    for (name, query) in queries {
        println!("\n  {name}");
        run_comparison(adapter, query)?;
    }

    Ok(())
}

fn benchmark_join_aggregations(adapter: &DuckDBAdapter) -> anyhow::Result<()> {
    let queries = vec![
        (
            "Customer summary with joins",
            "SELECT c.customer_id, c.customer_name, c.customer_tier,
                    COUNT(DISTINCT o.order_id) as order_count,
                    COUNT(DISTINCT oi.item_id) as item_count,
                    SUM(oi.quantity * oi.unit_price) as total_revenue,
                    AVG(o.total_amount) as avg_order_value
             FROM customers c
             JOIN orders o ON c.customer_id = o.customer_id
             JOIN order_items oi ON o.order_id = oi.order_id
             WHERE o.status != 'CANCELLED'
             GROUP BY c.customer_id, c.customer_name, c.customer_tier
             ORDER BY total_revenue DESC
             LIMIT 100"
        ),
        (
            "Product performance by supplier",
            "SELECT s.supplier_name, s.country,
                    COUNT(DISTINCT p.product_id) as product_count,
                    COUNT(DISTINCT oi.order_id) as order_count,
                    SUM(oi.quantity) as units_sold,
                    SUM(oi.quantity * oi.unit_price) as revenue
             FROM suppliers s
             JOIN products p ON s.supplier_id = p.supplier_id
             JOIN order_items oi ON p.product_id = oi.product_id
             GROUP BY s.supplier_name, s.country
             ORDER BY revenue DESC"
        ),
        (
            "Monthly sales by category",
            "SELECT DATE_TRUNC('month', o.order_date) as month,
                    p.category,
                    COUNT(DISTINCT o.order_id) as orders,
                    SUM(oi.quantity) as units,
                    SUM(oi.quantity * oi.unit_price) as revenue
             FROM orders o
             JOIN order_items oi ON o.order_id = oi.order_id
             JOIN products p ON oi.product_id = p.product_id
             WHERE o.status = 'DELIVERED'
             GROUP BY month, p.category
             ORDER BY month, revenue DESC"
        ),
    ];

    for (name, query) in queries {
        println!("\n  {name}");
        run_comparison(adapter, query)?;
    }

    Ok(())
}

fn benchmark_complex_joins(adapter: &DuckDBAdapter) -> anyhow::Result<()> {
    let queries = vec![
        (
            "Top products per customer tier",
            "SELECT customer_tier, product_name, category, revenue,
                    RANK() OVER (PARTITION BY customer_tier ORDER BY revenue DESC) as rank
             FROM (
                 SELECT c.customer_tier, p.product_name, p.category,
                        SUM(oi.quantity * oi.unit_price) as revenue
                 FROM customers c
                 JOIN orders o ON c.customer_id = o.customer_id
                 JOIN order_items oi ON o.order_id = oi.order_id
                 JOIN products p ON oi.product_id = p.product_id
                 GROUP BY c.customer_tier, p.product_name, p.category
             ) ranked
             WHERE rank <= 10
             ORDER BY customer_tier, rank"
        ),
        (
            "Supplier performance comparison",
            "SELECT s1.supplier_name as supplier,
                    s1.country,
                    COALESCE(sales1.revenue, 0) as revenue,
                    COALESCE(sales1.orders, 0) as order_count,
                    AVG(s2_sales.revenue) as avg_competitor_revenue
             FROM suppliers s1
             LEFT JOIN (
                 SELECT p.supplier_id,
                        COUNT(DISTINCT oi.order_id) as orders,
                        SUM(oi.quantity * oi.unit_price) as revenue
                 FROM products p
                 JOIN order_items oi ON p.product_id = oi.product_id
                 GROUP BY p.supplier_id
             ) sales1 ON s1.supplier_id = sales1.supplier_id
             CROSS JOIN suppliers s2
             LEFT JOIN (
                 SELECT p.supplier_id,
                        SUM(oi.quantity * oi.unit_price) as revenue
                 FROM products p
                 JOIN order_items oi ON p.product_id = oi.product_id
                 GROUP BY p.supplier_id
             ) s2_sales ON s2.supplier_id = s2_sales.supplier_id
             WHERE s1.supplier_id != s2.supplier_id
             GROUP BY s1.supplier_name, s1.country, sales1.revenue, sales1.orders
             ORDER BY revenue DESC
             LIMIT 20"
        ),
        (
            "Cross-category purchase patterns",
            "SELECT p1.category as category_a,
                    p2.category as category_b,
                    COUNT(DISTINCT o.customer_id) as customers,
                    SUM(oi1.quantity * oi1.unit_price + oi2.quantity * oi2.unit_price) as revenue
             FROM order_items oi1
             JOIN order_items oi2 ON oi1.order_id = oi2.order_id
                                  AND oi1.item_id < oi2.item_id
             JOIN products p1 ON oi1.product_id = p1.product_id
             JOIN products p2 ON oi2.product_id = p2.product_id
             JOIN orders o ON oi1.order_id = o.order_id
             WHERE p1.category != p2.category
             GROUP BY p1.category, p2.category
             HAVING COUNT(DISTINCT o.customer_id) > 100
             ORDER BY revenue DESC
             LIMIT 50"
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
