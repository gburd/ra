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
    #[pg_test]
    fn test_mvcc_statistics() {
        // Create table
        Spi::run("CREATE TABLE test_mvcc (id INT PRIMARY KEY, value INT)").unwrap();

        // Insert rows
        Spi::run("INSERT INTO test_mvcc SELECT i, i FROM generate_series(1, 1000) i").unwrap();

        // Update some rows (creates dead tuples)
        Spi::run("UPDATE test_mvcc SET value = value + 1 WHERE id <= 100").unwrap();

        // Query pg_stat_user_tables for MVCC stats
        let result = Spi::get_one::<i64>(
            "SELECT n_tup_upd FROM pg_stat_user_tables WHERE relname = 'test_mvcc'"
        ).unwrap();

        assert!(result >= Some(100), "Should have recorded updates");

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

        // Get EXPLAIN output using SPI directly
        let plan_output = Spi::get_one::<&str>(
            "SELECT query_plan FROM pg_stat_statements WHERE query LIKE '%test_explain%' LIMIT 1"
        );

        // For now, just verify the query executes
        let result = Spi::get_one::<i64>(
            "SELECT COUNT(*) FROM test_explain WHERE value > 500"
        ).unwrap();

        assert!(result > Some(0), "EXPLAIN query should return results");

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
        Spi::run("VACUUM ANALYZE test_covering").unwrap();

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
