#![expect(clippy::unwrap_used, reason = "test code")]
//! `DuckDB` adapter integration tests with comparison benchmarking.
//!
//! Tests adapter creation, native vs Ra execution, Parquet file access,
//! and comparison metrics.

#[cfg(feature = "duckdb")]
use ra_adapters::{DatabaseAdapter, DuckDBAdapter};

#[test]
#[cfg(feature = "duckdb")]
fn test_duckdb_adapter_creation() {
    let adapter = DuckDBAdapter::new();
    assert_eq!(adapter.database_name(), "DuckDB");
    assert_eq!(adapter.sql_dialect(), ra_core::SqlDialect::Postgres);
}

#[test]
#[cfg(feature = "duckdb")]
fn test_connect_memory_database() {
    let mut adapter = DuckDBAdapter::new();
    let result = adapter.open(":memory:");
    assert!(result.is_ok(), "Failed to connect to in-memory database");
}

#[test]
#[cfg(feature = "duckdb")]
fn test_connect_file_database() {
    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path().to_str().unwrap();

    let mut adapter = DuckDBAdapter::new();
    let result = adapter.open(db_path);
    assert!(result.is_ok(), "Failed to connect to file database");
}

#[test]
#[cfg(feature = "duckdb")]
fn test_execute_simple_query() {
    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:").unwrap();

    let result = adapter.execute("SELECT 1 as num, 'hello' as text");
    assert!(result.is_ok(), "Failed to execute simple query");

    let query_result = result.unwrap();
    assert_eq!(query_result.row_count, 1);
    assert!(query_result.duration.as_micros() > 0);
}

#[test]
#[cfg(feature = "duckdb")]
fn test_create_and_query_table() {
    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:").unwrap();

    adapter
        .execute("CREATE TABLE test (id INTEGER, name VARCHAR)")
        .unwrap();
    adapter
        .execute("INSERT INTO test VALUES (1, 'Alice'), (2, 'Bob')")
        .unwrap();

    let result = adapter.execute("SELECT * FROM test ORDER BY id").unwrap();
    assert_eq!(result.row_count, 2);
}

#[test]
#[cfg(feature = "duckdb")]
fn test_execute_native() {
    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:").unwrap();

    adapter.execute("CREATE TABLE numbers (n INTEGER)").unwrap();
    adapter
        .execute("INSERT INTO numbers SELECT * FROM range(100)")
        .unwrap();

    let result = adapter.execute_native("SELECT COUNT(*) as count FROM numbers");
    assert!(result.is_ok());

    let query_result = result.unwrap();
    assert_eq!(query_result.row_count, 1);
}

#[test]
#[cfg(feature = "duckdb")]
fn test_execute_with_ra() {
    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:").unwrap();

    adapter.execute("CREATE TABLE numbers (n INTEGER)").unwrap();
    adapter
        .execute("INSERT INTO numbers SELECT * FROM range(100)")
        .unwrap();

    let result = adapter.execute_with_ra("SELECT COUNT(*) as count FROM numbers");
    assert!(result.is_ok());

    let query_result = result.unwrap();
    assert_eq!(query_result.row_count, 1);
}

#[test]
#[cfg(feature = "duckdb")]
fn test_get_explain_plan() {
    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:").unwrap();

    adapter.execute("CREATE TABLE test (id INTEGER)").unwrap();

    let result = adapter.get_explain_plan("SELECT * FROM test");
    assert!(result.is_ok(), "Failed to get explain plan");

    let plan = result.unwrap();
    assert!(!plan.plan_text.is_empty());
}

#[test]
#[cfg(feature = "duckdb")]
fn test_gather_statistics() {
    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:").unwrap();

    adapter.execute("CREATE TABLE test (id INTEGER)").unwrap();
    adapter
        .execute("INSERT INTO test SELECT * FROM range(100)")
        .unwrap();

    let stats = adapter.gather_statistics();
    assert!(stats.is_ok(), "Failed to gather statistics");

    let table_stats = stats.unwrap();
    assert!(table_stats.contains_key("test"));
    assert_eq!(table_stats.get("test").unwrap().row_count, 100);
}

#[test]
#[cfg(feature = "duckdb")]
fn test_gather_column_stats() {
    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:").unwrap();

    adapter
        .execute("CREATE TABLE test (id INTEGER, name VARCHAR)")
        .unwrap();
    adapter
        .execute("INSERT INTO test VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Alice')")
        .unwrap();

    let stats = adapter.gather_column_stats("test");
    assert!(stats.is_ok(), "Failed to gather column stats");

    let col_stats = stats.unwrap();
    assert!(col_stats.contains_key("id"));
    assert!(col_stats.contains_key("name"));
    assert_eq!(col_stats.get("id").unwrap().ndv, 3);
    assert_eq!(col_stats.get("name").unwrap().ndv, 2);
}

#[test]
#[cfg(feature = "duckdb")]
fn test_get_schema_info() {
    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:").unwrap();

    adapter
        .execute("CREATE TABLE test (id INTEGER, name VARCHAR, age INTEGER)")
        .unwrap();

    let schema = adapter.get_schema_info();
    assert!(schema.is_ok(), "Failed to get schema info");

    let schema_info = schema.unwrap();
    assert!(schema_info.tables.contains_key("test"));

    let table_info = schema_info.tables.get("test").unwrap();
    assert_eq!(table_info.columns.len(), 3);
    assert_eq!(table_info.columns[0].name, "id");
    assert_eq!(table_info.columns[1].name, "name");
    assert_eq!(table_info.columns[2].name, "age");
}

#[test]
#[cfg(feature = "duckdb")]
fn test_get_capabilities() {
    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:").unwrap();

    let capabilities = adapter.get_capabilities();
    assert!(capabilities.is_ok(), "Failed to get capabilities");

    let caps = capabilities.unwrap();
    assert_eq!(caps.database_name, "DuckDB");
    assert!(caps.supports("window_functions"));
    assert!(caps.supports("columnar_storage"));
    assert!(caps.supports("vectorized_execution"));
    assert!(caps.supports("parquet_support"));
}

#[test]
#[cfg(feature = "duckdb")]
fn test_supports_feature() {
    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:").unwrap();

    assert!(adapter.supports_feature("window_functions").unwrap());
    assert!(adapter.supports_feature("columnar_storage").unwrap());
    assert!(adapter.supports_feature("parquet_support").unwrap());
    assert!(!adapter.supports_feature("nonexistent_feature").unwrap());
}

#[test]
#[cfg(feature = "duckdb")]
fn test_window_functions() {
    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:").unwrap();

    adapter
        .execute("CREATE TABLE sales (product VARCHAR, amount INTEGER)")
        .unwrap();
    adapter
        .execute("INSERT INTO sales VALUES ('A', 100), ('B', 200), ('A', 150), ('C', 300)")
        .unwrap();

    let result = adapter.execute(
        "SELECT product, amount, SUM(amount) OVER (PARTITION BY product) as total FROM sales",
    );
    assert!(result.is_ok(), "Window function query failed");

    let query_result = result.unwrap();
    assert_eq!(query_result.row_count, 4);
}

#[test]
#[cfg(feature = "duckdb")]
fn test_aggregate_query() {
    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:").unwrap();

    adapter
        .execute("CREATE TABLE orders (customer_id INTEGER, amount DECIMAL)")
        .unwrap();
    adapter
        .execute("INSERT INTO orders SELECT i % 10, i * 1.5 FROM range(1000) t(i)")
        .unwrap();

    let result = adapter.execute(
        "SELECT customer_id, COUNT(*) as order_count, SUM(amount) as total
         FROM orders GROUP BY customer_id ORDER BY customer_id",
    );
    assert!(result.is_ok(), "Aggregate query failed");

    let query_result = result.unwrap();
    assert_eq!(query_result.row_count, 10);
}

#[test]
#[cfg(feature = "duckdb")]
fn test_join_query() {
    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:").unwrap();

    adapter
        .execute("CREATE TABLE customers (id INTEGER, name VARCHAR)")
        .unwrap();
    adapter
        .execute("CREATE TABLE orders (id INTEGER, customer_id INTEGER, amount DECIMAL)")
        .unwrap();
    adapter
        .execute("INSERT INTO customers VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Charlie')")
        .unwrap();
    adapter
        .execute("INSERT INTO orders VALUES (1, 1, 100), (2, 1, 200), (3, 2, 150)")
        .unwrap();

    let result = adapter.execute(
        "SELECT c.name, COUNT(*) as order_count, SUM(o.amount) as total
         FROM customers c JOIN orders o ON c.id = o.customer_id
         GROUP BY c.name ORDER BY c.name",
    );
    assert!(result.is_ok(), "Join query failed");

    let query_result = result.unwrap();
    assert_eq!(query_result.row_count, 2);
}

#[test]
#[cfg(feature = "duckdb")]
fn test_compare_execution() {
    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:").unwrap();

    adapter.execute("CREATE TABLE numbers (n INTEGER)").unwrap();
    adapter
        .execute("INSERT INTO numbers SELECT * FROM range(10000)")
        .unwrap();

    let result = adapter.compare_execution("SELECT COUNT(*) as count FROM numbers WHERE n % 2 = 0");
    assert!(result.is_ok(), "Comparison execution failed");

    let metrics = result.unwrap();
    assert_eq!(metrics.row_count, 1);
    assert!(metrics.native_duration.as_micros() > 0);
    assert!(metrics.ra_duration.as_micros() > 0);
    assert!(metrics.speedup > 0.0);
}

#[test]
#[cfg(feature = "duckdb")]
fn test_load_csv() {
    use std::io::Write;

    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:").unwrap();

    let mut temp_file = NamedTempFile::new().unwrap();
    writeln!(temp_file, "id,name,value").unwrap();
    writeln!(temp_file, "1,Alice,100").unwrap();
    writeln!(temp_file, "2,Bob,200").unwrap();
    temp_file.flush().unwrap();

    let csv_path = temp_file.path().to_str().unwrap();
    let result = adapter.load_csv("csv_test", csv_path);
    assert!(result.is_ok(), "Failed to load CSV");

    let query_result = adapter
        .execute("SELECT COUNT(*) as count FROM csv_test")
        .unwrap();
    assert_eq!(query_result.row_count, 1);
}

#[test]
#[cfg(not(feature = "duckdb"))]
fn test_duckdb_feature_not_enabled() {
    println!("DuckDB tests require --features duckdb");
}
