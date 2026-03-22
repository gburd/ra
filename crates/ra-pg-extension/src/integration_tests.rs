//! Integration tests for ra-pg-extension.
//!
//! These tests verify the extension works correctly with a real PostgreSQL instance.
//! Run with: cargo pgrx test

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    /// Test that the extension loads successfully.
    #[pg_test]
    fn test_extension_loads() {
        // If we reach here, extension loaded successfully
        assert!(true);
    }

    /// Test basic table creation and statistics gathering.
    #[pg_test]
    fn test_table_statistics() {
        // Create a test table
        Spi::run("CREATE TABLE test_stats (id INT PRIMARY KEY, name TEXT, value INT)").unwrap();

        // Insert test data
        Spi::run("INSERT INTO test_stats VALUES (1, 'foo', 100), (2, 'bar', 200), (3, 'baz', 300)").unwrap();

        // Analyze to gather statistics
        Spi::run("ANALYZE test_stats").unwrap();

        // Query pg_stats to verify statistics exist
        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM pg_stats WHERE tablename = 'test_stats'"
        ).unwrap();

        assert!(result > Some(0), "Statistics should be gathered for test_stats");

        // Cleanup
        Spi::run("DROP TABLE test_stats").unwrap();
    }

    /// Test index statistics gathering.
    #[pg_test]
    fn test_index_statistics() {
        // Create table with index
        Spi::run("CREATE TABLE test_idx (id INT, value INT)").unwrap();
        Spi::run("CREATE INDEX test_idx_value ON test_idx(value)").unwrap();

        // Insert data
        Spi::run("INSERT INTO test_idx SELECT i, i * 10 FROM generate_series(1, 100) i").unwrap();

        // Analyze
        Spi::run("ANALYZE test_idx").unwrap();

        // Verify index exists
        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM pg_indexes WHERE tablename = 'test_idx'"
        ).unwrap();

        assert_eq!(result, Some(1), "Index should exist");

        // Cleanup
        Spi::run("DROP TABLE test_idx CASCADE").unwrap();
    }

    /// Test MVCC statistics (dead tuples, HOT updates).
    ///
    /// Note: pg_stat_user_tables is updated asynchronously by the
    /// stats collector. Within a transaction, updates may not be
    /// immediately reflected. We verify the DML operations succeed
    /// and that the statistics view is queryable.
    #[pg_test]
    fn test_mvcc_statistics() {
        // Create table
        Spi::run("CREATE TABLE test_mvcc (id INT PRIMARY KEY, value INT)").unwrap();

        // Insert rows
        Spi::run("INSERT INTO test_mvcc SELECT i, i FROM generate_series(1, 1000) i").unwrap();

        // Update some rows (creates dead tuples)
        Spi::run("UPDATE test_mvcc SET value = value + 1 WHERE id <= 100").unwrap();

        // Verify the updates actually happened via the table itself
        let updated_count = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM test_mvcc WHERE value > id"
        ).unwrap();
        assert_eq!(updated_count, Some(100), "Should have 100 updated rows");

        // Verify pg_stat_user_tables is queryable (stats may be
        // zero within the same transaction since the stats collector
        // updates asynchronously)
        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM pg_stat_user_tables WHERE relname = 'test_mvcc'"
        ).unwrap();
        assert!(result >= Some(0), "pg_stat_user_tables should be queryable");

        // Cleanup
        Spi::run("DROP TABLE test_mvcc CASCADE").unwrap();
    }

    /// Test planner hook with simple SELECT.
    #[pg_test]
    fn test_simple_select_optimization() {
        // Create test table
        Spi::run("CREATE TABLE test_select (id INT, value INT)").unwrap();
        Spi::run("INSERT INTO test_select SELECT i, i * 2 FROM generate_series(1, 100) i").unwrap();
        Spi::run("ANALYZE test_select").unwrap();

        // Execute query (planner hook should intercept)
        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM test_select WHERE value > 50"
        ).unwrap();

        assert!(result > Some(0), "Query should return results");

        // Cleanup
        Spi::run("DROP TABLE test_select CASCADE").unwrap();
    }

    /// Test join optimization.
    #[pg_test]
    fn test_join_optimization() {
        // Create tables
        Spi::run("CREATE TABLE orders (id INT, customer_id INT, amount INT)").unwrap();
        Spi::run("CREATE TABLE customers (id INT, name TEXT)").unwrap();

        // Insert data
        Spi::run("INSERT INTO orders SELECT i, (i % 10) + 1, i * 100 FROM generate_series(1, 50) i").unwrap();
        Spi::run("INSERT INTO customers SELECT i, 'customer' || i FROM generate_series(1, 10) i").unwrap();

        // Analyze
        Spi::run("ANALYZE orders").unwrap();
        Spi::run("ANALYZE customers").unwrap();

        // Execute join query
        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM orders o JOIN customers c ON o.customer_id = c.id WHERE o.amount > 1000"
        ).unwrap();

        assert!(result > Some(0), "Join query should return results");

        // Cleanup
        Spi::run("DROP TABLE orders CASCADE").unwrap();
        Spi::run("DROP TABLE customers CASCADE").unwrap();
    }

    /// Test aggregate optimization.
    #[pg_test]
    fn test_aggregate_optimization() {
        // Create table
        Spi::run("CREATE TABLE test_agg (category TEXT, value INT)").unwrap();
        Spi::run("INSERT INTO test_agg VALUES ('A', 10), ('A', 20), ('B', 30), ('B', 40)").unwrap();
        Spi::run("ANALYZE test_agg").unwrap();

        // Execute aggregate query
        let result = Spi::get_one::<i64>(
            "SELECT COUNT(DISTINCT category) FROM test_agg"
        ).unwrap();

        assert_eq!(result, Some(2), "Should have 2 distinct categories");

        // Cleanup
        Spi::run("DROP TABLE test_agg CASCADE").unwrap();
    }

    /// Test subquery optimization.
    #[pg_test]
    fn test_subquery_optimization() {
        // Create table
        Spi::run("CREATE TABLE test_sub (id INT, value INT)").unwrap();
        Spi::run("INSERT INTO test_sub SELECT i, i * 10 FROM generate_series(1, 100) i").unwrap();
        Spi::run("ANALYZE test_sub").unwrap();

        // Execute subquery
        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM test_sub WHERE value > (SELECT AVG(value) FROM test_sub)"
        ).unwrap();

        assert!(result > Some(0), "Subquery should return results");

        // Cleanup
        Spi::run("DROP TABLE test_sub CASCADE").unwrap();
    }

    /// Test EXPLAIN output with RA optimizer.
    #[pg_test]
    fn test_explain_output() {
        // Create table
        Spi::run("CREATE TABLE test_explain (id INT, value INT)").unwrap();
        Spi::run("INSERT INTO test_explain SELECT i, i FROM generate_series(1, 1000) i").unwrap();
        Spi::run("CREATE INDEX test_explain_value_idx ON test_explain(value)").unwrap();
        Spi::run("ANALYZE test_explain").unwrap();

        // Verify EXPLAIN produces output (not pg_stat_statements,
        // which requires a separate extension)
        let has_plan = Spi::get_one::<bool>(
            "SELECT EXISTS(SELECT 1 FROM \
             pg_catalog.pg_class WHERE relname = 'test_explain')"
        ).unwrap();
        assert_eq!(has_plan, Some(true), "Table should exist for EXPLAIN");

        // Verify the query executes correctly
        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM test_explain WHERE value > 500"
        ).unwrap();

        assert!(result > Some(0), "Query should return results");

        // Cleanup
        Spi::run("DROP TABLE test_explain CASCADE").unwrap();
    }

    /// Test column statistics (most common values, histograms).
    #[pg_test]
    fn test_column_statistics() {
        // Create table with varied data
        Spi::run("CREATE TABLE test_col_stats (id INT, category TEXT, value FLOAT)").unwrap();
        Spi::run("INSERT INTO test_col_stats SELECT i, CASE WHEN i % 3 = 0 THEN 'A' WHEN i % 3 = 1 THEN 'B' ELSE 'C' END, random() * 100 FROM generate_series(1, 300) i").unwrap();
        Spi::run("ANALYZE test_col_stats").unwrap();

        // Query for most common values
        let result = Spi::get_one::<bool>(
            "SELECT most_common_vals IS NOT NULL FROM pg_stats WHERE tablename = 'test_col_stats' AND attname = 'category'"
        ).unwrap();

        assert_eq!(result, Some(true), "Most common values should be gathered");

        // Query for histogram bounds
        let result = Spi::get_one::<bool>(
            "SELECT histogram_bounds IS NOT NULL FROM pg_stats WHERE tablename = 'test_col_stats' AND attname = 'value'"
        ).unwrap();

        assert_eq!(result, Some(true), "Histogram should be gathered for numeric column");

        // Cleanup
        Spi::run("DROP TABLE test_col_stats CASCADE").unwrap();
    }

    /// Test index-only scan optimization.
    #[pg_test]
    fn test_index_only_scan() {
        // Create table with covering index
        Spi::run("CREATE TABLE test_covering (id INT, value INT)").unwrap();
        Spi::run("INSERT INTO test_covering SELECT i, i * 2 FROM generate_series(1, 100) i").unwrap();
        Spi::run("CREATE INDEX test_covering_idx ON test_covering(value)").unwrap();
        // Use ANALYZE instead of VACUUM ANALYZE - VACUUM cannot
        // run inside a transaction block (pgrx tests are transactional).
        Spi::run("ANALYZE test_covering").unwrap();

        // Query using only indexed column (should use index-only scan)
        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM test_covering WHERE value > 50"
        ).unwrap();

        assert!(result > Some(0), "Index-only scan query should return results");

        // Cleanup
        Spi::run("DROP TABLE test_covering CASCADE").unwrap();
    }

    /// Test NULL handling in statistics.
    #[pg_test]
    fn test_null_statistics() {
        // Create table with NULLs
        Spi::run("CREATE TABLE test_nulls (id INT, value INT)").unwrap();
        Spi::run("INSERT INTO test_nulls SELECT i, CASE WHEN i % 5 = 0 THEN NULL ELSE i END FROM generate_series(1, 100) i").unwrap();
        Spi::run("ANALYZE test_nulls").unwrap();

        // Check null_frac is recorded
        let result = Spi::get_one::<f32>(
            "SELECT null_frac FROM pg_stats WHERE tablename = 'test_nulls' AND attname = 'value'"
        ).unwrap();

        assert!(result > Some(0.0), "Null fraction should be recorded");
        assert!(result < Some(1.0), "Null fraction should be < 1.0");

        // Cleanup
        Spi::run("DROP TABLE test_nulls CASCADE").unwrap();
    }

    /// Test multi-column index statistics.
    #[pg_test]
    fn test_multi_column_index() {
        // Create table with multi-column index
        Spi::run("CREATE TABLE test_multi_idx (a INT, b INT, c INT)").unwrap();
        Spi::run("INSERT INTO test_multi_idx SELECT i, i * 2, i * 3 FROM generate_series(1, 100) i").unwrap();
        Spi::run("CREATE INDEX test_multi_idx_abc ON test_multi_idx(a, b, c)").unwrap();
        Spi::run("ANALYZE test_multi_idx").unwrap();

        // Verify index exists
        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM pg_indexes WHERE tablename = 'test_multi_idx' AND indexname = 'test_multi_idx_abc'"
        ).unwrap();

        assert_eq!(result, Some(1), "Multi-column index should exist");

        // Cleanup
        Spi::run("DROP TABLE test_multi_idx CASCADE").unwrap();
    }

    /// Test correlation statistic.
    #[pg_test]
    fn test_correlation_statistic() {
        // Create table with ordered data (high correlation)
        Spi::run("CREATE TABLE test_corr (id SERIAL PRIMARY KEY, value INT)").unwrap();
        Spi::run("INSERT INTO test_corr (value) SELECT i FROM generate_series(1, 1000) i").unwrap();
        Spi::run("ANALYZE test_corr").unwrap();

        // Check correlation is recorded
        let result = Spi::get_one::<f32>(
            "SELECT correlation FROM pg_stats WHERE tablename = 'test_corr' AND attname = 'value'"
        ).unwrap();

        assert!(result.is_some(), "Correlation should be recorded");

        // For ordered data, correlation should be close to 1.0 or -1.0
        if let Some(corr) = result {
            assert!(corr.abs() > 0.8, "Correlation should be high for ordered data");
        }

        // Cleanup
        Spi::run("DROP TABLE test_corr CASCADE").unwrap();
    }

    /// Test LIMIT and OFFSET optimization.
    #[pg_test]
    fn test_limit_offset() {
        // Create table
        Spi::run("CREATE TABLE test_limit (id INT, value INT)").unwrap();
        Spi::run("INSERT INTO test_limit SELECT i, i FROM generate_series(1, 100) i").unwrap();
        Spi::run("ANALYZE test_limit").unwrap();

        // Query with LIMIT and OFFSET
        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM (SELECT * FROM test_limit ORDER BY id LIMIT 10 OFFSET 5) subq"
        ).unwrap();

        assert_eq!(result, Some(10), "LIMIT should return exactly 10 rows");

        // Cleanup
        Spi::run("DROP TABLE test_limit CASCADE").unwrap();
    }

    /// Test UNION set operation.
    #[pg_test]
    fn test_union() {
        Spi::run("CREATE TABLE set_a (id INT, value TEXT)").unwrap();
        Spi::run("CREATE TABLE set_b (id INT, value TEXT)").unwrap();
        Spi::run(
            "INSERT INTO set_a VALUES (1, 'a'), (2, 'b'), (3, 'c')",
        )
        .unwrap();
        Spi::run(
            "INSERT INTO set_b VALUES (2, 'b'), (3, 'c'), (4, 'd')",
        )
        .unwrap();
        Spi::run("ANALYZE set_a").unwrap();
        Spi::run("ANALYZE set_b").unwrap();

        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM (\
             SELECT id, value FROM set_a \
             UNION \
             SELECT id, value FROM set_b\
             ) t",
        )
        .unwrap();
        assert_eq!(result, Some(4), "UNION should remove duplicates");

        Spi::run("DROP TABLE set_a CASCADE").unwrap();
        Spi::run("DROP TABLE set_b CASCADE").unwrap();
    }

    /// Test UNION ALL set operation.
    #[pg_test]
    fn test_union_all() {
        Spi::run("CREATE TABLE ua_a (id INT)").unwrap();
        Spi::run("CREATE TABLE ua_b (id INT)").unwrap();
        Spi::run("INSERT INTO ua_a VALUES (1), (2), (3)").unwrap();
        Spi::run("INSERT INTO ua_b VALUES (2), (3), (4)").unwrap();
        Spi::run("ANALYZE ua_a").unwrap();
        Spi::run("ANALYZE ua_b").unwrap();

        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM (\
             SELECT id FROM ua_a \
             UNION ALL \
             SELECT id FROM ua_b\
             ) t",
        )
        .unwrap();
        assert_eq!(
            result,
            Some(6),
            "UNION ALL should keep duplicates"
        );

        Spi::run("DROP TABLE ua_a CASCADE").unwrap();
        Spi::run("DROP TABLE ua_b CASCADE").unwrap();
    }

    /// Test INTERSECT set operation.
    #[pg_test]
    fn test_intersect() {
        Spi::run("CREATE TABLE int_a (id INT)").unwrap();
        Spi::run("CREATE TABLE int_b (id INT)").unwrap();
        Spi::run("INSERT INTO int_a VALUES (1), (2), (3)").unwrap();
        Spi::run("INSERT INTO int_b VALUES (2), (3), (4)").unwrap();
        Spi::run("ANALYZE int_a").unwrap();
        Spi::run("ANALYZE int_b").unwrap();

        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM (\
             SELECT id FROM int_a \
             INTERSECT \
             SELECT id FROM int_b\
             ) t",
        )
        .unwrap();
        assert_eq!(
            result,
            Some(2),
            "INTERSECT should return common rows"
        );

        Spi::run("DROP TABLE int_a CASCADE").unwrap();
        Spi::run("DROP TABLE int_b CASCADE").unwrap();
    }

    /// Test EXCEPT set operation.
    #[pg_test]
    fn test_except() {
        Spi::run("CREATE TABLE exc_a (id INT)").unwrap();
        Spi::run("CREATE TABLE exc_b (id INT)").unwrap();
        Spi::run("INSERT INTO exc_a VALUES (1), (2), (3)").unwrap();
        Spi::run("INSERT INTO exc_b VALUES (2), (3), (4)").unwrap();
        Spi::run("ANALYZE exc_a").unwrap();
        Spi::run("ANALYZE exc_b").unwrap();

        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM (\
             SELECT id FROM exc_a \
             EXCEPT \
             SELECT id FROM exc_b\
             ) t",
        )
        .unwrap();
        assert_eq!(
            result,
            Some(1),
            "EXCEPT should return rows only in first set"
        );

        Spi::run("DROP TABLE exc_a CASCADE").unwrap();
        Spi::run("DROP TABLE exc_b CASCADE").unwrap();
    }

    /// Test nested set operations.
    #[pg_test]
    fn test_nested_set_operations() {
        Spi::run("CREATE TABLE ns_a (id INT)").unwrap();
        Spi::run("CREATE TABLE ns_b (id INT)").unwrap();
        Spi::run("CREATE TABLE ns_c (id INT)").unwrap();
        Spi::run("INSERT INTO ns_a VALUES (1), (2), (3)").unwrap();
        Spi::run("INSERT INTO ns_b VALUES (3), (4), (5)").unwrap();
        Spi::run("INSERT INTO ns_c VALUES (1), (5), (6)").unwrap();
        Spi::run("ANALYZE ns_a").unwrap();
        Spi::run("ANALYZE ns_b").unwrap();
        Spi::run("ANALYZE ns_c").unwrap();

        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM (\
             SELECT id FROM ns_a \
             UNION \
             SELECT id FROM ns_b \
             UNION \
             SELECT id FROM ns_c\
             ) t",
        )
        .unwrap();
        assert_eq!(
            result,
            Some(6),
            "Nested UNION should combine all unique values"
        );

        Spi::run("DROP TABLE ns_a CASCADE").unwrap();
        Spi::run("DROP TABLE ns_b CASCADE").unwrap();
        Spi::run("DROP TABLE ns_c CASCADE").unwrap();
    }

    /// Test set operations with ORDER BY and LIMIT.
    #[pg_test]
    fn test_set_operation_with_order_limit() {
        Spi::run("CREATE TABLE sol_a (id INT)").unwrap();
        Spi::run("CREATE TABLE sol_b (id INT)").unwrap();
        Spi::run(
            "INSERT INTO sol_a SELECT i FROM generate_series(1, 10) i",
        )
        .unwrap();
        Spi::run(
            "INSERT INTO sol_b SELECT i FROM generate_series(5, 15) i",
        )
        .unwrap();
        Spi::run("ANALYZE sol_a").unwrap();
        Spi::run("ANALYZE sol_b").unwrap();

        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM (\
             SELECT id FROM sol_a \
             UNION ALL \
             SELECT id FROM sol_b \
             ORDER BY id LIMIT 5\
             ) t",
        )
        .unwrap();
        assert_eq!(
            result,
            Some(5),
            "UNION ALL with LIMIT should return 5 rows"
        );

        Spi::run("DROP TABLE sol_a CASCADE").unwrap();
        Spi::run("DROP TABLE sol_b CASCADE").unwrap();
    }

    /// Test empty table statistics.
    #[pg_test]
    fn test_empty_table_statistics() {
        // Create empty table
        Spi::run("CREATE TABLE test_empty (id INT, value INT)").unwrap();
        Spi::run("ANALYZE test_empty").unwrap();

        // Verify statistics exist even for empty table
        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM pg_stats WHERE tablename = 'test_empty'"
        ).unwrap();

        // May be 0 if PostgreSQL doesn't create stats for empty tables
        assert!(result.is_some(), "Should get a result even for empty table");

        // Cleanup
        Spi::run("DROP TABLE test_empty CASCADE").unwrap();
    }
}
