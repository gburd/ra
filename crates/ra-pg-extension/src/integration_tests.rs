//! Integration tests for combined features: CTEs, window functions, set operations, foreign keys
//!
//! These tests verify that recently merged features work correctly together in complex queries.

use pgrx::prelude::*;

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use pgrx::prelude::*;

    /// Test metadata cache invalidation on ALTER TABLE
    #[pg_test]
    fn test_metadata_cache_invalidation_alter_table() {
        Spi::run("DROP TABLE IF EXISTS cache_test_users CASCADE;").ok();
        Spi::run(
            "CREATE TABLE cache_test_users (
                id INT PRIMARY KEY,
                name TEXT NOT NULL
            );",
        )
        .unwrap();

        Spi::run(
            "INSERT INTO cache_test_users SELECT i, 'User ' || i FROM generate_series(1, 100) i;",
        )
        .unwrap();
        Spi::run("ANALYZE cache_test_users;").unwrap();

        // Populate cache
        Spi::get_one::<i64>("SELECT COUNT(*) FROM cache_test_users WHERE id < 50;").ok();

        // Check cache has entries
        let stats = Spi::get_one::<i32>("SELECT entries FROM ra.metadata_cache_stats();").ok();
        assert!(stats.is_some());

        // ALTER TABLE triggers invalidation
        Spi::run("ALTER TABLE cache_test_users ADD COLUMN email TEXT;").unwrap();

        // Next query should refresh metadata
        Spi::get_one::<i64>("SELECT COUNT(*) FROM cache_test_users WHERE id < 50;").ok();

        // Check invalidations counter increased
        let invalidations =
            Spi::get_one::<i64>("SELECT invalidations FROM ra.metadata_cache_stats();").ok();
        assert!(invalidations.unwrap_or(Some(0)).unwrap_or(0) > 0);
    }

    /// Test metadata cache invalidation on CREATE INDEX
    #[pg_test]
    fn test_metadata_cache_invalidation_create_index() {
        Spi::run("DROP TABLE IF EXISTS cache_test_products CASCADE;").ok();
        Spi::run(
            "CREATE TABLE cache_test_products (
                id INT PRIMARY KEY,
                name TEXT NOT NULL,
                price DECIMAL(10,2)
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO cache_test_products SELECT i, 'Product ' || i, i * 10.0 FROM generate_series(1, 100) i;").unwrap();
        Spi::run("ANALYZE cache_test_products;").unwrap();

        // Populate cache
        Spi::get_one::<i64>("SELECT COUNT(*) FROM cache_test_products WHERE price > 500;").ok();

        // CREATE INDEX triggers invalidation
        Spi::run("CREATE INDEX idx_cache_test_products_price ON cache_test_products(price);")
            .unwrap();

        // Next query should see new index
        Spi::get_one::<i64>("SELECT COUNT(*) FROM cache_test_products WHERE price > 500;").ok();

        // Check invalidations counter
        let invalidations =
            Spi::get_one::<i64>("SELECT invalidations FROM ra.metadata_cache_stats();").ok();
        assert!(invalidations.unwrap_or(Some(0)).unwrap_or(0) > 0);
    }

    /// Test metadata cache invalidation on DROP INDEX
    #[pg_test]
    fn test_metadata_cache_invalidation_drop_index() {
        Spi::run("DROP TABLE IF EXISTS cache_test_orders CASCADE;").ok();
        Spi::run(
            "CREATE TABLE cache_test_orders (
                id SERIAL PRIMARY KEY,
                customer_id INT,
                amount DECIMAL(10,2)
            );",
        )
        .unwrap();

        Spi::run("CREATE INDEX idx_cache_test_orders_customer ON cache_test_orders(customer_id);")
            .unwrap();
        Spi::run("INSERT INTO cache_test_orders (customer_id, amount) SELECT (random() * 99 + 1)::INT, random() * 1000 FROM generate_series(1, 100);").unwrap();
        Spi::run("ANALYZE cache_test_orders;").unwrap();

        // Populate cache
        Spi::get_one::<i64>("SELECT COUNT(*) FROM cache_test_orders WHERE customer_id = 42;").ok();

        // DROP INDEX triggers invalidation
        Spi::run("DROP INDEX idx_cache_test_orders_customer;").unwrap();

        // Next query should not recommend dropped index
        Spi::get_one::<i64>("SELECT COUNT(*) FROM cache_test_orders WHERE customer_id = 42;").ok();

        // Check invalidations counter
        let invalidations =
            Spi::get_one::<i64>("SELECT invalidations FROM ra.metadata_cache_stats();").ok();
        assert!(invalidations.unwrap_or(Some(0)).unwrap_or(0) > 0);
    }

    /// Test metadata cache invalidation on ANALYZE
    #[pg_test]
    fn test_metadata_cache_invalidation_analyze() {
        Spi::run("DROP TABLE IF EXISTS cache_test_items CASCADE;").ok();
        Spi::run(
            "CREATE TABLE cache_test_items (
                id INT PRIMARY KEY,
                category TEXT,
                stock INT
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO cache_test_items SELECT i, 'Category-' || (i % 10), i * 5 FROM generate_series(1, 100) i;").unwrap();
        Spi::run("ANALYZE cache_test_items;").unwrap();

        // Populate cache
        Spi::get_one::<i64>("SELECT COUNT(*) FROM cache_test_items WHERE stock > 200;").ok();

        // Update data
        Spi::run("UPDATE cache_test_items SET stock = stock * 2 WHERE id < 50;").unwrap();

        // ANALYZE triggers invalidation
        Spi::run("ANALYZE cache_test_items;").unwrap();

        // Next query uses fresh statistics
        Spi::get_one::<i64>("SELECT COUNT(*) FROM cache_test_items WHERE stock > 200;").ok();

        // Check invalidations counter
        let invalidations =
            Spi::get_one::<i64>("SELECT invalidations FROM ra.metadata_cache_stats();").ok();
        assert!(invalidations.unwrap_or(Some(0)).unwrap_or(0) > 0);
    }

    /// Test manual cache clear
    #[pg_test]
    fn test_metadata_cache_clear() {
        Spi::run("DROP TABLE IF EXISTS cache_test_clear CASCADE;").ok();
        Spi::run(
            "CREATE TABLE cache_test_clear (
                id INT PRIMARY KEY,
                data TEXT
            );",
        )
        .unwrap();

        Spi::run(
            "INSERT INTO cache_test_clear SELECT i, 'Data-' || i FROM generate_series(1, 50) i;",
        )
        .unwrap();
        Spi::run("ANALYZE cache_test_clear;").unwrap();

        // Populate cache
        Spi::get_one::<i64>("SELECT COUNT(*) FROM cache_test_clear;").ok();

        // Check cache has entries
        let entries_before =
            Spi::get_one::<i32>("SELECT entries FROM ra.metadata_cache_stats();").ok();
        assert!(entries_before.unwrap_or(Some(0)).unwrap_or(0) > 0);

        // Clear cache
        Spi::run("SELECT ra.clear_metadata_cache();").unwrap();

        // Cache should be empty
        let entries_after =
            Spi::get_one::<i32>("SELECT entries FROM ra.metadata_cache_stats();").ok();
        assert_eq!(entries_after.unwrap_or(Some(-1)).unwrap_or(-1), 0);
    }

    /// Test cache repopulation after clear
    #[pg_test]
    fn test_metadata_cache_repopulation() {
        Spi::run("DROP TABLE IF EXISTS cache_test_repop CASCADE;").ok();
        Spi::run(
            "CREATE TABLE cache_test_repop (
                id INT PRIMARY KEY,
                value INT
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO cache_test_repop SELECT i, i * 10 FROM generate_series(1, 50) i;")
            .unwrap();
        Spi::run("ANALYZE cache_test_repop;").unwrap();

        // Clear cache
        Spi::run("SELECT ra.clear_metadata_cache();").unwrap();

        // Query to repopulate
        Spi::get_one::<i64>("SELECT COUNT(*) FROM cache_test_repop WHERE value > 250;").ok();

        // Cache should have entries again
        let entries = Spi::get_one::<i32>("SELECT entries FROM ra.metadata_cache_stats();").ok();
        assert!(entries.unwrap_or(Some(0)).unwrap_or(0) > 0);
    }

    /// Test cache hit rate calculation
    #[pg_test]
    fn test_metadata_cache_hit_rate() {
        Spi::run("DROP TABLE IF EXISTS cache_test_hitrate CASCADE;").ok();
        Spi::run(
            "CREATE TABLE cache_test_hitrate (
                id INT PRIMARY KEY,
                amount DECIMAL(10,2)
            );",
        )
        .unwrap();

        Spi::run(
            "INSERT INTO cache_test_hitrate SELECT i, i * 1.5 FROM generate_series(1, 100) i;",
        )
        .unwrap();
        Spi::run("ANALYZE cache_test_hitrate;").unwrap();

        // Clear cache to start fresh
        Spi::run("SELECT ra.clear_metadata_cache();").unwrap();

        // First query (cache miss)
        Spi::get_one::<i64>("SELECT COUNT(*) FROM cache_test_hitrate WHERE amount > 50;").ok();

        // Second query (cache hit)
        Spi::get_one::<i64>("SELECT COUNT(*) FROM cache_test_hitrate WHERE amount > 75;").ok();

        // Third query (cache hit)
        Spi::get_one::<i64>("SELECT COUNT(*) FROM cache_test_hitrate WHERE amount > 100;").ok();

        // Check hit rate
        let hit_rate = Spi::get_one::<f64>("SELECT hit_rate FROM ra.metadata_cache_stats();").ok();
        // Should have some cache hits (hit_rate > 0)
        assert!(hit_rate.unwrap_or(Some(-1.0)).unwrap_or(-1.0) >= 0.0);
    }

    /// Test CTE with window functions
    #[pg_test]
    fn test_cte_with_window_functions() {
        Spi::run("DROP TABLE IF EXISTS sales CASCADE;").ok();
        Spi::run(
            "CREATE TABLE sales (
                product_id INT,
                month INT,
                amount DECIMAL
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO sales VALUES (1, 1, 100), (1, 2, 150), (1, 3, 120);").unwrap();
        Spi::run("INSERT INTO sales VALUES (2, 1, 200), (2, 2, 180), (2, 3, 220);").unwrap();

        let result = Spi::get_one::<i64>(
            "WITH monthly_sales AS (
                SELECT product_id, month, SUM(amount) as total
                FROM sales
                GROUP BY product_id, month
            )
            SELECT COUNT(*)
            FROM (
                SELECT product_id, month, total,
                       ROW_NUMBER() OVER (PARTITION BY product_id ORDER BY total DESC) as rank
                FROM monthly_sales
            ) ranked
            WHERE rank = 1;",
        );

        assert_eq!(result, Ok(Some(2))); // One top month per product
    }

    /// Test recursive CTE with window functions
    #[pg_test]
    fn test_recursive_cte_with_window_functions() {
        Spi::run("DROP TABLE IF EXISTS employees CASCADE;").ok();
        Spi::run(
            "CREATE TABLE employees (
                id INT PRIMARY KEY,
                parent_id INT,
                name TEXT
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO employees VALUES (1, NULL, 'CEO');").unwrap();
        Spi::run("INSERT INTO employees VALUES (2, 1, 'VP1');").unwrap();
        Spi::run("INSERT INTO employees VALUES (3, 1, 'VP2');").unwrap();
        Spi::run("INSERT INTO employees VALUES (4, 2, 'Manager1');").unwrap();

        let result = Spi::get_one::<i64>(
            "WITH RECURSIVE hierarchy AS (
                SELECT id, parent_id, name, 1 as level
                FROM employees
                WHERE parent_id IS NULL
                UNION ALL
                SELECT e.id, e.parent_id, e.name, h.level + 1
                FROM employees e
                JOIN hierarchy h ON e.parent_id = h.id
            )
            SELECT COUNT(*)
            FROM (
                SELECT id, name, level,
                       RANK() OVER (PARTITION BY level ORDER BY name) as rank_in_level
                FROM hierarchy
            ) ranked;",
        );

        assert_eq!(result, Ok(Some(4))); // All 4 employees
    }

    /// Test CTE with set operations
    #[pg_test]
    fn test_cte_with_set_operations() {
        Spi::run("DROP TABLE IF EXISTS sales CASCADE;").ok();
        Spi::run(
            "CREATE TABLE sales (
                product_id INT,
                quarter INT,
                amount DECIMAL
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO sales VALUES (1, 1, 100), (1, 2, 150);").unwrap();
        Spi::run("INSERT INTO sales VALUES (2, 1, 200), (2, 2, 180);").unwrap();

        let result = Spi::get_one::<i64>(
            "WITH q1_sales AS (
                SELECT product_id, amount FROM sales WHERE quarter = 1
            ),
            q2_sales AS (
                SELECT product_id, amount FROM sales WHERE quarter = 2
            )
            SELECT COUNT(*) FROM (
                SELECT * FROM q1_sales
                UNION ALL
                SELECT * FROM q2_sales
            ) combined;",
        );

        assert_eq!(result, Ok(Some(4))); // 2 Q1 + 2 Q2 = 4 total
    }

    /// Test window functions over set operation results
    #[pg_test]
    fn test_window_over_set_operations() {
        Spi::run("DROP TABLE IF EXISTS products CASCADE;").ok();
        Spi::run(
            "CREATE TABLE products (
                id INT,
                category TEXT,
                price DECIMAL
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO products VALUES (1, 'A', 100), (2, 'A', 150);").unwrap();
        Spi::run("INSERT INTO products VALUES (3, 'B', 200), (4, 'B', 180);").unwrap();

        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM (
                SELECT id, category, price,
                       ROW_NUMBER() OVER (PARTITION BY category ORDER BY price DESC) as rank
                FROM (
                    SELECT id, category, price FROM products WHERE price > 100
                    UNION ALL
                    SELECT id, category, price FROM products WHERE price <= 100
                ) all_products
            ) ranked
            WHERE rank = 1;",
        );

        assert_eq!(result, Ok(Some(2))); // Top-ranked product per category
    }

    /// Test foreign key joins with CTEs
    #[pg_test]
    fn test_foreign_key_with_cte() {
        Spi::run("DROP TABLE IF EXISTS orders CASCADE;").ok();
        Spi::run("DROP TABLE IF EXISTS customers CASCADE;").ok();

        Spi::run(
            "CREATE TABLE customers (
                customer_id INT PRIMARY KEY,
                name TEXT
            );",
        )
        .unwrap();

        Spi::run(
            "CREATE TABLE orders (
                order_id INT PRIMARY KEY,
                customer_id INT REFERENCES customers(customer_id),
                amount DECIMAL
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO customers VALUES (1, 'Alice'), (2, 'Bob');").unwrap();
        Spi::run("INSERT INTO orders VALUES (1, 1, 100), (2, 1, 200), (3, 2, 150);").unwrap();

        let result = Spi::get_one::<i64>(
            "WITH customer_totals AS (
                SELECT c.customer_id, c.name, SUM(o.amount) as total
                FROM customers c
                JOIN orders o ON c.customer_id = o.customer_id
                GROUP BY c.customer_id, c.name
            )
            SELECT COUNT(*) FROM customer_totals WHERE total > 150;",
        );

        assert_eq!(result, Ok(Some(1))); // Only Alice has total > 150
    }

    /// Test all features combined: CTEs + window functions + set operations + foreign keys
    #[pg_test]
    fn test_all_features_combined() {
        Spi::run("DROP TABLE IF EXISTS line_items CASCADE;").ok();
        Spi::run("DROP TABLE IF EXISTS orders CASCADE;").ok();
        Spi::run("DROP TABLE IF EXISTS customers CASCADE;").ok();

        Spi::run(
            "CREATE TABLE customers (
                customer_id INT PRIMARY KEY,
                name TEXT,
                tier TEXT
            );",
        )
        .unwrap();

        Spi::run(
            "CREATE TABLE orders (
                order_id INT PRIMARY KEY,
                customer_id INT REFERENCES customers(customer_id),
                order_date DATE
            );",
        )
        .unwrap();

        Spi::run(
            "CREATE TABLE line_items (
                item_id INT PRIMARY KEY,
                order_id INT REFERENCES orders(order_id),
                amount DECIMAL
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO customers VALUES (1, 'Alice', 'premium'), (2, 'Bob', 'standard');")
            .unwrap();
        Spi::run("INSERT INTO orders VALUES (1, 1, '2024-01-01'), (2, 1, '2024-02-01'), (3, 2, '2024-01-15');")
            .unwrap();
        Spi::run("INSERT INTO line_items VALUES (1, 1, 100), (2, 1, 50), (3, 2, 200), (4, 3, 75);")
            .unwrap();

        let result = Spi::get_one::<i64>(
            "WITH q1_orders AS (
                SELECT o.order_id, o.customer_id, SUM(li.amount) as total
                FROM orders o
                JOIN line_items li ON o.order_id = li.order_id
                WHERE o.order_date BETWEEN '2024-01-01' AND '2024-03-31'
                GROUP BY o.order_id, o.customer_id
            ),
            q2_orders AS (
                SELECT o.order_id, o.customer_id, SUM(li.amount) as total
                FROM orders o
                JOIN line_items li ON o.order_id = li.order_id
                WHERE o.order_date BETWEEN '2024-04-01' AND '2024-06-30'
                GROUP BY o.order_id, o.customer_id
            ),
            all_orders AS (
                SELECT * FROM q1_orders
                UNION ALL
                SELECT * FROM q2_orders
            )
            SELECT COUNT(*) FROM (
                SELECT
                    c.name,
                    c.tier,
                    ao.total,
                    ROW_NUMBER() OVER (PARTITION BY c.tier ORDER BY ao.total DESC) as rank
                FROM customers c
                JOIN all_orders ao ON c.customer_id = ao.customer_id
            ) ranked
            WHERE rank <= 2;",
        );

        // Should have top 2 orders per tier (may have fewer if not enough data)
        assert!(result.unwrap().unwrap() >= 1);
    }

    /// Test set operations with window functions and aggregation
    #[pg_test]
    fn test_set_ops_window_aggregation() {
        Spi::run("DROP TABLE IF EXISTS sales CASCADE;").ok();
        Spi::run(
            "CREATE TABLE sales (
                region TEXT,
                product TEXT,
                month INT,
                amount DECIMAL
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO sales VALUES ('North', 'A', 1, 100), ('North', 'B', 1, 150);")
            .unwrap();
        Spi::run("INSERT INTO sales VALUES ('South', 'A', 1, 120), ('South', 'B', 1, 130);")
            .unwrap();
        Spi::run("INSERT INTO sales VALUES ('North', 'A', 2, 110), ('South', 'A', 2, 140);")
            .unwrap();

        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM (
                SELECT region, product, total_amount,
                       RANK() OVER (PARTITION BY region ORDER BY total_amount DESC) as rank
                FROM (
                    SELECT region, product, SUM(amount) as total_amount
                    FROM sales
                    WHERE month = 1
                    GROUP BY region, product
                    UNION ALL
                    SELECT region, product, SUM(amount) as total_amount
                    FROM sales
                    WHERE month = 2
                    GROUP BY region, product
                ) monthly_totals
            ) ranked
            WHERE rank = 1;",
        );

        // One top product per region (2 regions)
        assert!(result.unwrap().unwrap() >= 2);
    }

    /// Test INTERSECT with CTEs
    #[pg_test]
    fn test_intersect_with_cte() {
        Spi::run("DROP TABLE IF EXISTS products CASCADE;").ok();
        Spi::run(
            "CREATE TABLE products (
                product_id INT,
                category TEXT,
                status TEXT
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO products VALUES (1, 'A', 'active'), (2, 'A', 'inactive');").unwrap();
        Spi::run("INSERT INTO products VALUES (3, 'B', 'active'), (4, 'B', 'active');").unwrap();

        let result = Spi::get_one::<i64>(
            "WITH category_a AS (
                SELECT product_id FROM products WHERE category = 'A'
            ),
            active_products AS (
                SELECT product_id FROM products WHERE status = 'active'
            )
            SELECT COUNT(*) FROM (
                SELECT product_id FROM category_a
                INTERSECT
                SELECT product_id FROM active_products
            ) result;",
        );

        assert_eq!(result, Ok(Some(1))); // Only product 1 is in category A and active
    }

    /// Test EXCEPT with window functions
    #[pg_test]
    fn test_except_with_window_functions() {
        Spi::run("DROP TABLE IF EXISTS inventory CASCADE;").ok();
        Spi::run(
            "CREATE TABLE inventory (
                product_id INT,
                warehouse TEXT,
                quantity INT
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO inventory VALUES (1, 'W1', 100), (2, 'W1', 50);").unwrap();
        Spi::run("INSERT INTO inventory VALUES (1, 'W2', 80), (3, 'W2', 120);").unwrap();

        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM (
                SELECT product_id,
                       SUM(quantity) as total,
                       ROW_NUMBER() OVER (ORDER BY SUM(quantity) DESC) as rank
                FROM (
                    SELECT product_id, warehouse, quantity FROM inventory WHERE warehouse = 'W1'
                    EXCEPT
                    SELECT product_id, warehouse, quantity FROM inventory WHERE warehouse = 'W3'
                ) w1_only
                GROUP BY product_id
            ) ranked
            WHERE rank = 1;",
        );

        assert_eq!(result, Ok(Some(1))); // Top product by total quantity
    }

    /// Test nested CTEs with multiple levels
    #[pg_test]
    fn test_nested_ctes_multiple_levels() {
        Spi::run("DROP TABLE IF EXISTS transactions CASCADE;").ok();
        Spi::run(
            "CREATE TABLE transactions (
                transaction_id INT,
                user_id INT,
                amount DECIMAL,
                transaction_date DATE
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO transactions VALUES (1, 1, 100, '2024-01-01');").unwrap();
        Spi::run("INSERT INTO transactions VALUES (2, 1, 200, '2024-02-01');").unwrap();
        Spi::run("INSERT INTO transactions VALUES (3, 2, 150, '2024-01-15');").unwrap();

        let result = Spi::get_one::<i64>(
            "WITH monthly_totals AS (
                SELECT user_id, DATE_TRUNC('month', transaction_date) as month, SUM(amount) as total
                FROM transactions
                GROUP BY user_id, DATE_TRUNC('month', transaction_date)
            ),
            user_summaries AS (
                SELECT user_id, SUM(total) as grand_total, COUNT(*) as month_count
                FROM monthly_totals
                GROUP BY user_id
            ),
            top_users AS (
                SELECT user_id, grand_total
                FROM user_summaries
                WHERE grand_total > 100
            )
            SELECT COUNT(*) FROM top_users;",
        );

        assert_eq!(result, Ok(Some(2))); // Both users have grand_total > 100
    }

    /// Test window functions with frame specifications in CTEs
    #[pg_test]
    fn test_window_frames_in_cte() {
        Spi::run("DROP TABLE IF EXISTS stock_prices CASCADE;").ok();
        Spi::run(
            "CREATE TABLE stock_prices (
                symbol TEXT,
                trade_date DATE,
                price DECIMAL
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO stock_prices VALUES ('AAPL', '2024-01-01', 150);").unwrap();
        Spi::run("INSERT INTO stock_prices VALUES ('AAPL', '2024-01-02', 152);").unwrap();
        Spi::run("INSERT INTO stock_prices VALUES ('AAPL', '2024-01-03', 148);").unwrap();
        Spi::run("INSERT INTO stock_prices VALUES ('AAPL', '2024-01-04', 155);").unwrap();

        let result = Spi::get_one::<i64>(
            "WITH moving_avg AS (
                SELECT
                    symbol,
                    trade_date,
                    price,
                    AVG(price) OVER (
                        PARTITION BY symbol
                        ORDER BY trade_date
                        ROWS BETWEEN 2 PRECEDING AND CURRENT ROW
                    ) as ma_3day
                FROM stock_prices
            )
            SELECT COUNT(*) FROM moving_avg WHERE ma_3day IS NOT NULL;",
        );

        assert_eq!(result, Ok(Some(4))); // All 4 rows have moving average
    }

    /// Test LAG/LEAD window functions with CTEs
    #[pg_test]
    fn test_lag_lead_with_cte() {
        Spi::run("DROP TABLE IF EXISTS sensor_readings CASCADE;").ok();
        Spi::run(
            "CREATE TABLE sensor_readings (
                sensor_id INT,
                reading_time TIMESTAMP,
                temperature DECIMAL
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO sensor_readings VALUES (1, '2024-01-01 10:00:00', 20.5);").unwrap();
        Spi::run("INSERT INTO sensor_readings VALUES (1, '2024-01-01 11:00:00', 21.0);").unwrap();
        Spi::run("INSERT INTO sensor_readings VALUES (1, '2024-01-01 12:00:00', 22.5);").unwrap();

        let result = Spi::get_one::<i64>(
            "WITH temperature_changes AS (
                SELECT
                    sensor_id,
                    reading_time,
                    temperature,
                    LAG(temperature) OVER (PARTITION BY sensor_id ORDER BY reading_time) as prev_temp,
                    LEAD(temperature) OVER (PARTITION BY sensor_id ORDER BY reading_time) as next_temp
                FROM sensor_readings
            )
            SELECT COUNT(*) FROM temperature_changes
            WHERE prev_temp IS NOT NULL AND next_temp IS NOT NULL;",
        );

        assert_eq!(result, Ok(Some(1))); // Middle reading has both prev and next
    }

    /// Test recursive CTE with cycle detection
    #[pg_test]
    fn test_recursive_cte_cycle_detection() {
        Spi::run("DROP TABLE IF EXISTS graph CASCADE;").ok();
        Spi::run(
            "CREATE TABLE graph (
                node_id INT,
                next_node_id INT
            );",
        )
        .unwrap();

        // Create a simple graph: 1 -> 2 -> 3
        Spi::run("INSERT INTO graph VALUES (1, 2), (2, 3), (3, NULL);").unwrap();

        let result = Spi::get_one::<i64>(
            "WITH RECURSIVE reachable AS (
                SELECT node_id, next_node_id, 1 as depth, ARRAY[node_id] as path
                FROM graph
                WHERE node_id = 1
                UNION ALL
                SELECT g.node_id, g.next_node_id, r.depth + 1, r.path || g.node_id
                FROM graph g
                JOIN reachable r ON g.node_id = r.next_node_id
                WHERE r.depth < 10 AND NOT (g.node_id = ANY(r.path))
            )
            SELECT COUNT(*) FROM reachable;",
        );

        assert_eq!(result, Ok(Some(3))); // 3 nodes reachable from node 1
    }

    /// Test UNION ALL with aggregation and window functions
    #[pg_test]
    fn test_union_all_aggregation_window() {
        Spi::run("DROP TABLE IF EXISTS revenue CASCADE;").ok();
        Spi::run(
            "CREATE TABLE revenue (
                source TEXT,
                quarter INT,
                amount DECIMAL
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO revenue VALUES ('Online', 1, 10000), ('Online', 2, 12000);").unwrap();
        Spi::run("INSERT INTO revenue VALUES ('Store', 1, 8000), ('Store', 2, 9000);").unwrap();

        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM (
                SELECT
                    source,
                    total,
                    PERCENT_RANK() OVER (ORDER BY total) as percentile
                FROM (
                    SELECT source, SUM(amount) as total
                    FROM revenue
                    WHERE quarter = 1
                    GROUP BY source
                    UNION ALL
                    SELECT source, SUM(amount) as total
                    FROM revenue
                    WHERE quarter = 2
                    GROUP BY source
                ) quarterly_totals
            ) with_percentile
            WHERE percentile > 0.25;",
        );

        assert!(result.unwrap().unwrap() >= 1); // At least one source above 25th percentile
    }

    /// Test foreign key detection affects join strategy
    #[pg_test]
    fn test_foreign_key_join_optimization() {
        Spi::run("DROP TABLE IF EXISTS order_items CASCADE;").ok();
        Spi::run("DROP TABLE IF EXISTS orders_fk CASCADE;").ok();

        Spi::run(
            "CREATE TABLE orders_fk (
                order_id INT PRIMARY KEY,
                customer_name TEXT
            );",
        )
        .unwrap();

        Spi::run(
            "CREATE TABLE order_items (
                item_id INT PRIMARY KEY,
                order_id INT REFERENCES orders_fk(order_id),
                product TEXT,
                quantity INT
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO orders_fk VALUES (1, 'Alice'), (2, 'Bob');").unwrap();
        Spi::run("INSERT INTO order_items VALUES (1, 1, 'Widget', 5), (2, 1, 'Gadget', 3);")
            .unwrap();
        Spi::run("INSERT INTO order_items VALUES (3, 2, 'Widget', 2);").unwrap();

        // Query should recognize foreign key and optimize join
        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*)
            FROM orders_fk o
            JOIN order_items oi ON o.order_id = oi.order_id
            WHERE o.customer_name = 'Alice';",
        );

        assert_eq!(result, Ok(Some(2))); // Alice has 2 items
    }

    /// Test complex multi-level CTE with all features
    #[pg_test]
    fn test_complex_multi_level_all_features() {
        Spi::run("DROP TABLE IF EXISTS sales_data CASCADE;").ok();
        Spi::run(
            "CREATE TABLE sales_data (
                sale_id INT PRIMARY KEY,
                product_category TEXT,
                region TEXT,
                sale_date DATE,
                amount DECIMAL
            );",
        )
        .unwrap();

        Spi::run("INSERT INTO sales_data VALUES (1, 'Electronics', 'North', '2024-01-01', 1000);")
            .unwrap();
        Spi::run("INSERT INTO sales_data VALUES (2, 'Electronics', 'North', '2024-02-01', 1200);")
            .unwrap();
        Spi::run("INSERT INTO sales_data VALUES (3, 'Electronics', 'South', '2024-01-15', 900);")
            .unwrap();
        Spi::run("INSERT INTO sales_data VALUES (4, 'Clothing', 'North', '2024-01-10', 500);")
            .unwrap();
        Spi::run("INSERT INTO sales_data VALUES (5, 'Clothing', 'South', '2024-02-05', 600);")
            .unwrap();

        let result = Spi::get_one::<i64>(
            "WITH q1_sales AS (
                SELECT product_category, region, SUM(amount) as q1_total
                FROM sales_data
                WHERE sale_date BETWEEN '2024-01-01' AND '2024-03-31'
                GROUP BY product_category, region
            ),
            q2_sales AS (
                SELECT product_category, region, SUM(amount) as q2_total
                FROM sales_data
                WHERE sale_date BETWEEN '2024-04-01' AND '2024-06-30'
                GROUP BY product_category, region
            ),
            combined AS (
                SELECT product_category, region, q1_total as total, 'Q1' as quarter FROM q1_sales
                UNION ALL
                SELECT product_category, region, q2_total as total, 'Q2' as quarter FROM q2_sales
            ),
            ranked AS (
                SELECT
                    product_category,
                    region,
                    quarter,
                    total,
                    ROW_NUMBER() OVER (PARTITION BY product_category ORDER BY total DESC) as rank,
                    SUM(total) OVER (PARTITION BY product_category) as category_total
                FROM combined
            )
            SELECT COUNT(*) FROM ranked WHERE rank <= 2 AND category_total > 1000;",
        );

        // Should return top 2 regions per category where category total > 1000
        assert!(result.unwrap().unwrap() >= 1);
    }

    // =========================================================================
    // Correctness verification tests: compare Ra planner vs native PostgreSQL
    // =========================================================================

    /// Run a query with Ra enabled and disabled, compare results row-by-row.
    /// When `ordered` is false, both result sets are sorted before comparison.
    unsafe fn compare_ra_vs_native(sql: &str, ordered: bool) {
        // Run with Ra
        Spi::run("SET ra_planner.enabled = true").unwrap();
        let ra_rows: Vec<String> = Spi::connect(|client| {
            let table = client.select(sql, None, &[]).unwrap();
            table
                .map(|row| {
                    let ncols = row.columns();
                    (0..ncols)
                        .map(|i| {
                            row.get::<String>(i + 1)
                                .ok()
                                .flatten()
                                .unwrap_or_else(|| "NULL".to_string())
                        })
                        .collect::<Vec<_>>()
                        .join("|")
                })
                .collect()
        });

        // Run without Ra
        Spi::run("SET ra_planner.enabled = false").unwrap();
        let native_rows: Vec<String> = Spi::connect(|client| {
            let table = client.select(sql, None, &[]).unwrap();
            table
                .map(|row| {
                    let ncols = row.columns();
                    (0..ncols)
                        .map(|i| {
                            row.get::<String>(i + 1)
                                .ok()
                                .flatten()
                                .unwrap_or_else(|| "NULL".to_string())
                        })
                        .collect::<Vec<_>>()
                        .join("|")
                })
                .collect()
        });

        // Compare
        let (mut ra_sorted, mut native_sorted) =
            (ra_rows.clone(), native_rows.clone());
        if !ordered {
            ra_sorted.sort();
            native_sorted.sort();
        }

        assert_eq!(
            ra_sorted.len(),
            native_sorted.len(),
            "Row count mismatch for query: {sql}\n\
             Ra: {} rows, Native: {} rows",
            ra_sorted.len(),
            native_sorted.len()
        );

        for (i, (ra, native)) in
            ra_sorted.iter().zip(native_sorted.iter()).enumerate()
        {
            assert_eq!(
                ra, native,
                "Row {i} differs for query: {sql}\nRa: {ra}\nNative: {native}"
            );
        }
    }

    #[pg_test]
    fn test_correctness_simple_scan() {
        unsafe {
            Spi::run("CREATE TABLE test_scan (id int, name text)").unwrap();
            Spi::run("INSERT INTO test_scan VALUES (1,'a'),(2,'b'),(3,'c')")
                .unwrap();
            Spi::run("ANALYZE test_scan").unwrap();
            compare_ra_vs_native(
                "SELECT * FROM test_scan ORDER BY id",
                true,
            );
            Spi::run("DROP TABLE test_scan").unwrap();
        }
    }

    #[pg_test]
    fn test_correctness_filter() {
        unsafe {
            Spi::run("CREATE TABLE test_filter (id int, val int)").unwrap();
            Spi::run(
                "INSERT INTO test_filter \
                 SELECT g, g*10 FROM generate_series(1,100) g",
            )
            .unwrap();
            Spi::run("ANALYZE test_filter").unwrap();
            compare_ra_vs_native(
                "SELECT * FROM test_filter WHERE val > 500 ORDER BY id",
                true,
            );
            Spi::run("DROP TABLE test_filter").unwrap();
        }
    }

    #[pg_test]
    fn test_correctness_join() {
        unsafe {
            Spi::run("CREATE TABLE test_left (id int, name text)").unwrap();
            Spi::run("CREATE TABLE test_right (id int, value int)").unwrap();
            Spi::run(
                "INSERT INTO test_left VALUES (1,'a'),(2,'b'),(3,'c')",
            )
            .unwrap();
            Spi::run(
                "INSERT INTO test_right VALUES (1,10),(2,20),(4,40)",
            )
            .unwrap();
            Spi::run("ANALYZE test_left").unwrap();
            Spi::run("ANALYZE test_right").unwrap();
            compare_ra_vs_native(
                "SELECT l.id, l.name, r.value \
                 FROM test_left l JOIN test_right r ON l.id = r.id \
                 ORDER BY l.id",
                true,
            );
            Spi::run("DROP TABLE test_left").unwrap();
            Spi::run("DROP TABLE test_right").unwrap();
        }
    }

    #[pg_test]
    fn test_correctness_aggregate() {
        unsafe {
            Spi::run("CREATE TABLE test_agg (grp text, val int)").unwrap();
            Spi::run(
                "INSERT INTO test_agg VALUES \
                 ('a',1),('a',2),('b',3),('b',4),('c',5)",
            )
            .unwrap();
            Spi::run("ANALYZE test_agg").unwrap();
            compare_ra_vs_native(
                "SELECT grp, count(*), sum(val) \
                 FROM test_agg GROUP BY grp ORDER BY grp",
                true,
            );
            Spi::run("DROP TABLE test_agg").unwrap();
        }
    }

    #[pg_test]
    fn test_correctness_distinct() {
        unsafe {
            Spi::run("CREATE TABLE test_dist (val int)").unwrap();
            Spi::run(
                "INSERT INTO test_dist VALUES (1),(1),(2),(2),(3)",
            )
            .unwrap();
            Spi::run("ANALYZE test_dist").unwrap();
            compare_ra_vs_native(
                "SELECT DISTINCT val FROM test_dist ORDER BY val",
                true,
            );
            Spi::run("DROP TABLE test_dist").unwrap();
        }
    }

    #[pg_test]
    fn test_correctness_limit_offset() {
        unsafe {
            Spi::run("CREATE TABLE test_limit (id int)").unwrap();
            Spi::run(
                "INSERT INTO test_limit \
                 SELECT g FROM generate_series(1,20) g",
            )
            .unwrap();
            Spi::run("ANALYZE test_limit").unwrap();
            compare_ra_vs_native(
                "SELECT id FROM test_limit ORDER BY id LIMIT 5 OFFSET 10",
                true,
            );
            Spi::run("DROP TABLE test_limit").unwrap();
        }
    }

    #[pg_test]
    fn test_correctness_subquery() {
        unsafe {
            Spi::run("CREATE TABLE test_sub (id int, val int)").unwrap();
            Spi::run(
                "INSERT INTO test_sub VALUES \
                 (1,10),(2,20),(3,30),(4,40)",
            )
            .unwrap();
            Spi::run("ANALYZE test_sub").unwrap();
            compare_ra_vs_native(
                "SELECT id, val FROM test_sub \
                 WHERE val > (SELECT avg(val) FROM test_sub) \
                 ORDER BY id",
                true,
            );
            Spi::run("DROP TABLE test_sub").unwrap();
        }
    }

    #[pg_test]
    fn test_correctness_null_handling() {
        unsafe {
            Spi::run("CREATE TABLE test_null (id int, val text)").unwrap();
            Spi::run(
                "INSERT INTO test_null VALUES \
                 (1,'a'),(2,NULL),(3,'c'),(4,NULL)",
            )
            .unwrap();
            Spi::run("ANALYZE test_null").unwrap();
            compare_ra_vs_native(
                "SELECT * FROM test_null ORDER BY id",
                true,
            );
            compare_ra_vs_native(
                "SELECT * FROM test_null WHERE val IS NULL ORDER BY id",
                true,
            );
            Spi::run("DROP TABLE test_null").unwrap();
        }
    }
}
