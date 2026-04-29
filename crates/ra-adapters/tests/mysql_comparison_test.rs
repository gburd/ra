#![expect(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
//! `MySQL` adapter and comparison tests.
//!
//! Run with: `cargo test -p ra-adapters --features mysql mysql_comparison`

#![cfg(feature = "mysql")]

use ra_adapters::{
    compare_mysql_queries, compare_single_mysql_query, DatabaseAdapter, MySQLAdapter,
};
use std::env;

fn get_test_url() -> String {
    env::var("TEST_MYSQL_URL").unwrap_or_else(|_| "mysql://root@localhost:3306/test".to_string())
}

fn setup_test_db() -> MySQLAdapter {
    let mut adapter = MySQLAdapter::new();
    adapter
        .connect(&get_test_url())
        .expect("Failed to connect to MySQL");
    adapter
}

#[test]
#[ignore = "requires live MySQL"]
fn test_adapter_creation() {
    let adapter = MySQLAdapter::new();
    assert_eq!(adapter.database_name(), "MySQL");
}

#[test]
#[ignore = "requires live MySQL"]
fn test_connection() {
    let mut adapter = MySQLAdapter::new();
    let result = adapter.connect(&get_test_url());
    assert!(result.is_ok(), "Failed to connect: {result:?}");
}

#[test]
#[ignore = "requires live MySQL"]
fn test_gather_statistics() {
    let adapter = setup_test_db();
    let stats = adapter.gather_statistics();
    assert!(stats.is_ok(), "Failed to gather stats: {stats:?}");

    let stats = stats.unwrap();
    assert!(!stats.is_empty(), "Expected some tables");
}

#[test]
#[ignore = "requires live MySQL"]
fn test_get_schema_info() {
    let adapter = setup_test_db();
    let schema = adapter.get_schema_info();
    assert!(schema.is_ok(), "Failed to get schema: {schema:?}");

    let schema = schema.unwrap();
    assert!(!schema.tables.is_empty(), "Expected some tables");
}

#[test]
#[ignore = "requires live MySQL"]
fn test_get_capabilities() {
    let adapter = setup_test_db();
    let caps = adapter.get_capabilities();
    assert!(caps.is_ok());

    let caps = caps.unwrap();
    assert_eq!(caps.database_name, "MySQL");
    assert!(caps.features.contains_key("fulltext"));
}

#[test]
#[ignore = "requires live MySQL"]
fn test_supports_feature() {
    let adapter = setup_test_db();
    let result = adapter.supports_feature("fulltext");
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[test]
#[ignore = "requires live MySQL"]
fn test_execute_native() {
    let adapter = setup_test_db();
    let result = adapter.execute_native("SELECT 1 as num, 'test' as str");
    assert!(result.is_ok(), "Query failed: {result:?}");

    let result = result.unwrap();
    assert_eq!(result.row_count, 1);
    assert!(!result.rows.is_empty());
}

#[test]
#[ignore = "requires live MySQL"]
fn test_execute_with_ra() {
    let adapter = setup_test_db();
    let result = adapter.execute_with_ra("SELECT 1 as num");
    assert!(result.is_ok());

    let result = result.unwrap();
    assert_eq!(result.row_count, 1);
}

#[test]
#[ignore = "requires live MySQL"]
fn test_get_explain_plan() {
    let adapter = setup_test_db();
    let plan = adapter.get_explain_plan("SELECT 1");
    assert!(plan.is_ok(), "EXPLAIN failed: {plan:?}");

    let plan = plan.unwrap();
    assert!(!plan.text.is_empty());
}

#[test]
#[ignore = "requires live MySQL"]
fn test_check_fulltext_indexes() {
    let adapter = setup_test_db();

    // First create a test table with FULLTEXT index
    let _result = adapter.execute_native(
        "CREATE TABLE IF NOT EXISTS test_fulltext (
            id INT PRIMARY KEY,
            content TEXT,
            FULLTEXT(content)
        )",
    );

    let indexes = adapter.check_fulltext_indexes("test_fulltext");
    assert!(indexes.is_ok());

    // Clean up
    let _ = adapter.execute_native("DROP TABLE IF EXISTS test_fulltext");
}

#[test]
#[ignore = "requires live MySQL"]
fn test_get_query_stats() {
    let adapter = setup_test_db();

    // Execute a query to generate stats
    let _ = adapter.execute_native("SELECT 1");

    let stats = adapter.get_query_stats();
    assert!(stats.is_ok(), "Failed to get stats: {stats:?}");

    let stats = stats.unwrap();
    assert!(stats.contains_key("Handler_read_first"));
}

#[test]
#[ignore = "requires live MySQL"]
fn test_comparison_single_query() {
    let adapter = setup_test_db();

    // Create test table
    let _ = adapter.execute_native(
        "CREATE TABLE IF NOT EXISTS test_comparison (
            id INT PRIMARY KEY,
            name VARCHAR(100),
            value INT
        )",
    );

    let _ = adapter
        .execute_native("INSERT INTO test_comparison VALUES (1, 'test1', 100), (2, 'test2', 200)");

    let result = compare_single_mysql_query(&adapter, "SELECT * FROM test_comparison WHERE id = 1");
    assert!(result.is_ok(), "Comparison failed: {result:?}");

    let result = result.unwrap();
    assert_eq!(result.native.rows_returned, 1);
    assert!(result.speedup > 0.0);

    // Clean up
    let _ = adapter.execute_native("DROP TABLE IF EXISTS test_comparison");
}

#[test]
#[ignore = "requires live MySQL"]
fn test_comparison_multiple_queries() {
    let adapter = setup_test_db();

    // Create test table
    let _ = adapter.execute_native(
        "CREATE TABLE IF NOT EXISTS test_multi (
            id INT PRIMARY KEY,
            data VARCHAR(100)
        )",
    );

    let _ = adapter.execute_native("INSERT INTO test_multi VALUES (1, 'a'), (2, 'b'), (3, 'c')");

    let queries = vec![
        "SELECT * FROM test_multi WHERE id = 1".to_string(),
        "SELECT COUNT(*) FROM test_multi".to_string(),
        "SELECT * FROM test_multi ORDER BY id".to_string(),
    ];

    let report = compare_mysql_queries(&adapter, &queries);
    assert!(report.is_ok(), "Comparison failed: {report:?}");

    let report = report.unwrap();
    assert_eq!(report.total_queries, 3);
    assert!(report.avg_speedup > 0.0);

    // Clean up
    let _ = adapter.execute_native("DROP TABLE IF EXISTS test_multi");
}

#[test]
#[ignore = "requires live MySQL"]
fn test_gather_column_stats() {
    let adapter = setup_test_db();

    // Create test table
    let _ = adapter.execute_native(
        "CREATE TABLE IF NOT EXISTS test_col_stats (
            id INT PRIMARY KEY,
            name VARCHAR(100),
            value INT
        )",
    );

    let _ = adapter.execute_native(
        "INSERT INTO test_col_stats VALUES
        (1, 'a', 100), (2, 'b', 200), (3, 'c', 300), (4, NULL, 400)",
    );

    let stats = adapter.gather_column_stats("test_col_stats");
    assert!(stats.is_ok(), "Failed to gather column stats: {stats:?}");

    let stats = stats.unwrap();
    assert!(stats.contains_key("id"));
    assert!(stats.contains_key("name"));
    assert!(stats.contains_key("value"));

    // Check null fraction
    if let Some(name_stats) = stats.get("name") {
        assert!(name_stats.null_fraction > 0.0);
    }

    // Clean up
    let _ = adapter.execute_native("DROP TABLE IF EXISTS test_col_stats");
}

#[test]
#[ignore = "requires live MySQL"]
fn test_fulltext_search_comparison() {
    let adapter = setup_test_db();

    // Create table with FULLTEXT index
    let _ = adapter.execute_native(
        "CREATE TABLE IF NOT EXISTS articles (
            id INT PRIMARY KEY AUTO_INCREMENT,
            title VARCHAR(200),
            body TEXT,
            FULLTEXT(title, body)
        )",
    );

    let _ = adapter.execute_native(
        "INSERT INTO articles (title, body) VALUES
        ('MySQL Tutorial', 'This is a MySQL tutorial for beginners'),
        ('How To Use MySQL', 'MySQL is a popular database'),
        ('Optimizing MySQL', 'Learn to optimize MySQL queries')",
    );

    // Test FULLTEXT search
    let query = "SELECT * FROM articles WHERE MATCH(title, body) AGAINST('MySQL')";
    let result = compare_single_mysql_query(&adapter, query);
    assert!(result.is_ok(), "FULLTEXT comparison failed: {result:?}");

    let result = result.unwrap();
    assert!(result.native.rows_returned > 0);

    // Clean up
    let _ = adapter.execute_native("DROP TABLE IF EXISTS articles");
}
