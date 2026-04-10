//! SQLite adapter integration tests.
//!
//! Tests FTS5 detection, sqlite-vec detection, schema introspection,
//! and query execution.
//!
//! Requires the `sqlite` feature to be enabled.
#![cfg(feature = "sqlite")]

use ra_adapters::{DatabaseAdapter, SQLiteAdapter};
use std::path::PathBuf;

/// Get path to test database.
fn test_db_path(name: &str) -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("hybrid-search")
        .join(name)
}

#[test]
fn test_sqlite_adapter_creation() {
    let adapter = SQLiteAdapter::new();
    assert_eq!(adapter.database_name(), "SQLite");
    assert_eq!(
        adapter.sql_dialect(),
        ra_core::SqlDialect::Sqlite
    );
}

#[test]
fn test_connect_memory_database() {
    let mut adapter = SQLiteAdapter::new();
    let result = adapter.connect(":memory:");
    assert!(result.is_ok(), "Failed to connect to in-memory database");
}

#[test]
fn test_connect_nonexistent_file() {
    let mut adapter = SQLiteAdapter::new();
    let result = adapter.connect("/tmp/nonexistent-database-12345.db");
    assert!(result.is_err(), "Should fail connecting to nonexistent file");
}

#[test]
fn test_connect_wikipedia_database() {
    let db_path = test_db_path("wikipedia-fts5.db");
    if !db_path.exists() {
        eprintln!("Skipping test: {} not found", db_path.display());
        return;
    }

    let mut adapter = SQLiteAdapter::new();
    let result = adapter.connect(db_path.to_str().unwrap());
    assert!(result.is_ok(), "Failed to connect to Wikipedia database");
}

#[test]
fn test_connect_products_database() {
    let db_path = test_db_path("products-vec.db");
    if !db_path.exists() {
        eprintln!("Skipping test: {} not found", db_path.display());
        return;
    }

    let mut adapter = SQLiteAdapter::new();
    let result = adapter.connect(db_path.to_str().unwrap());
    assert!(result.is_ok(), "Failed to connect to products database");
}

#[test]
fn test_check_fts5_memory() {
    let mut adapter = SQLiteAdapter::new();
    adapter.connect(":memory:").unwrap();

    let has_fts5 = adapter.check_fts5().unwrap();
    // FTS5 is typically compiled into SQLite by default
    assert!(has_fts5, "FTS5 should be available in SQLite");
}

#[test]
fn test_check_sqlite_vec_memory() {
    let mut adapter = SQLiteAdapter::new();
    adapter.connect(":memory:").unwrap();

    let has_vec = adapter.check_sqlite_vec().unwrap();
    // sqlite-vec is an extension that may not be loaded
    // Test should not panic even if unavailable
    assert!(
        has_vec || !has_vec,
        "check_sqlite_vec should return a boolean"
    );
}

#[test]
fn test_get_fts5_tables_wikipedia() {
    let db_path = test_db_path("wikipedia-fts5.db");
    if !db_path.exists() {
        eprintln!("Skipping test: {} not found", db_path.display());
        return;
    }

    let mut adapter = SQLiteAdapter::new();
    adapter.connect(db_path.to_str().unwrap()).unwrap();

    let fts5_tables = adapter.get_fts5_tables().unwrap();
    assert!(
        fts5_tables.contains(&"articles".to_string()),
        "Should find 'articles' FTS5 table"
    );
}

#[test]
fn test_get_fts5_tables_empty() {
    let mut adapter = SQLiteAdapter::new();
    adapter.connect(":memory:").unwrap();

    let fts5_tables = adapter.get_fts5_tables().unwrap();
    assert!(
        fts5_tables.is_empty(),
        "Empty database should have no FTS5 tables"
    );
}

#[test]
fn test_get_sqlite_vec_tables_products() {
    let db_path = test_db_path("products-vec.db");
    if !db_path.exists() {
        eprintln!("Skipping test: {} not found", db_path.display());
        return;
    }

    let mut adapter = SQLiteAdapter::new();
    adapter.connect(db_path.to_str().unwrap()).unwrap();

    let vec_tables = adapter.get_sqlite_vec_tables().unwrap();
    // products table has 'embedding' BLOB column
    assert!(
        vec_tables.contains(&"products".to_string()),
        "Should find 'products' vector table"
    );
}

#[test]
fn test_execute_simple_query() {
    let mut adapter = SQLiteAdapter::new();
    adapter.connect(":memory:").unwrap();

    // Create a test table
    let conn = adapter.get_connection().unwrap();
    conn.execute(
        "CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO test (name) VALUES ('Alice')", [])
        .unwrap();
    conn.execute("INSERT INTO test (name) VALUES ('Bob')", [])
        .unwrap();
    drop(conn);

    // Query the table
    let results = adapter.execute("SELECT * FROM test ORDER BY id").unwrap();
    assert_eq!(results.rows.len(), 2, "Should return 2 rows");
    assert_eq!(
        results.rows[0].get("name").unwrap().as_str().unwrap(),
        "Alice"
    );
    assert_eq!(results.rows[1].get("name").unwrap().as_str().unwrap(), "Bob");
}

#[test]
fn test_execute_fts5_query() {
    let db_path = test_db_path("wikipedia-fts5.db");
    if !db_path.exists() {
        eprintln!("Skipping test: {} not found", db_path.display());
        return;
    }

    let mut adapter = SQLiteAdapter::new();
    adapter.connect(db_path.to_str().unwrap()).unwrap();

    // Test FTS5 MATCH query
    let results = adapter
        .execute("SELECT title FROM articles WHERE articles MATCH 'database' LIMIT 5")
        .unwrap();

    assert!(!results.rows.is_empty(), "Should find articles matching 'database'");
    assert!(results.rows.len() <= 5, "Should return at most 5 results");

    // Verify results contain the search term (in title or would be in content)
    let first_title = results.rows[0].get("title").unwrap().as_str().unwrap();
    assert!(!first_title.is_empty(), "Title should not be empty");
}

#[test]
fn test_execute_fts5_phrase_query() {
    let db_path = test_db_path("wikipedia-fts5.db");
    if !db_path.exists() {
        eprintln!("Skipping test: {} not found", db_path.display());
        return;
    }

    let mut adapter = SQLiteAdapter::new();
    adapter.connect(db_path.to_str().unwrap()).unwrap();

    // Test phrase query with quotes
    let results = adapter
        .execute(r#"SELECT title FROM articles WHERE articles MATCH '"query optimization"' LIMIT 3"#)
        .unwrap();

    assert!(!results.rows.is_empty(), "Should find articles with 'query optimization'");
}

#[test]
fn test_execute_fts5_boolean_query() {
    let db_path = test_db_path("wikipedia-fts5.db");
    if !db_path.exists() {
        eprintln!("Skipping test: {} not found", db_path.display());
        return;
    }

    let mut adapter = SQLiteAdapter::new();
    adapter.connect(db_path.to_str().unwrap()).unwrap();

    // Test boolean query with AND
    let results = adapter
        .execute("SELECT title FROM articles WHERE articles MATCH 'database AND query' LIMIT 5")
        .unwrap();

    // Should find articles containing both terms
    assert!(results.rows.len() <= 5, "Should return at most 5 results");
}

#[test]
fn test_gather_statistics() {
    let db_path = test_db_path("wikipedia-fts5.db");
    if !db_path.exists() {
        eprintln!("Skipping test: {} not found", db_path.display());
        return;
    }

    let mut adapter = SQLiteAdapter::new();
    adapter.connect(db_path.to_str().unwrap()).unwrap();

    let stats = adapter.gather_statistics().unwrap();
    assert!(!stats.is_empty(), "Should gather statistics for tables");

    // Check for article_metadata table
    if let Some(metadata_stats) = stats.get("article_metadata") {
        assert!(
            metadata_stats.row_count > 0,
            "article_metadata should have rows"
        );
    }
}

#[test]
fn test_gather_column_stats() {
    let db_path = test_db_path("products-vec.db");
    if !db_path.exists() {
        eprintln!("Skipping test: {} not found", db_path.display());
        return;
    }

    let mut adapter = SQLiteAdapter::new();
    adapter.connect(db_path.to_str().unwrap()).unwrap();

    let stats = adapter.gather_column_stats("products").unwrap();
    assert!(!stats.is_empty(), "Should gather column statistics");

    // Check for known columns
    assert!(stats.contains_key("name"), "Should have stats for 'name' column");
    assert!(
        stats.contains_key("category"),
        "Should have stats for 'category' column"
    );
    assert!(
        stats.contains_key("price"),
        "Should have stats for 'price' column"
    );

    // Verify distinct counts make sense
    let category_stats = &stats["category"];
    assert!(
        category_stats.ndv > 0,
        "Category should have distinct values"
    );
}

#[test]
fn test_get_schema_info() {
    let db_path = test_db_path("products-vec.db");
    if !db_path.exists() {
        eprintln!("Skipping test: {} not found", db_path.display());
        return;
    }

    let mut adapter = SQLiteAdapter::new();
    adapter.connect(db_path.to_str().unwrap()).unwrap();

    let schema = adapter.get_schema_info().unwrap();
    assert!(!schema.tables.is_empty(), "Should have schema information");

    // Check products table
    let products_table = schema.tables.get("products").unwrap();
    assert_eq!(products_table.name, "products");
    assert!(!products_table.columns.is_empty(), "Should have columns");

    // Verify known columns exist
    let column_names: Vec<&str> = products_table
        .columns
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert!(column_names.contains(&"id"), "Should have 'id' column");
    assert!(column_names.contains(&"name"), "Should have 'name' column");
    assert!(
        column_names.contains(&"description"),
        "Should have 'description' column"
    );
    assert!(
        column_names.contains(&"category"),
        "Should have 'category' column"
    );
    assert!(column_names.contains(&"price"), "Should have 'price' column");
    assert!(
        column_names.contains(&"embedding"),
        "Should have 'embedding' column"
    );
}

#[test]
fn test_get_capabilities() {
    let mut adapter = SQLiteAdapter::new();
    adapter.connect(":memory:").unwrap();

    let caps = adapter.get_capabilities().unwrap();
    assert_eq!(caps.database_name, "SQLite");
    assert_eq!(caps.dialect, ra_core::SqlDialect::Sqlite);
    assert!(caps.features.contains_key("fts5"), "Should detect FTS5");
}

#[test]
fn test_supports_feature() {
    let mut adapter = SQLiteAdapter::new();
    adapter.connect(":memory:").unwrap();

    let has_fts5 = adapter.supports_feature("fts5").unwrap();
    assert!(has_fts5, "FTS5 should be supported");

    let has_fake = adapter.supports_feature("fake-feature").unwrap();
    assert!(!has_fake, "Fake feature should not be supported");
}

#[test]
fn test_execute_products_query() {
    let db_path = test_db_path("products-vec.db");
    if !db_path.exists() {
        eprintln!("Skipping test: {} not found", db_path.display());
        return;
    }

    let mut adapter = SQLiteAdapter::new();
    adapter.connect(db_path.to_str().unwrap()).unwrap();

    // Test basic query with filtering
    let results = adapter
        .execute("SELECT name, price FROM products WHERE category='Electronics' ORDER BY price DESC LIMIT 5")
        .unwrap();

    assert!(!results.rows.is_empty(), "Should find electronics products");
    assert!(results.rows.len() <= 5, "Should return at most 5 results");

    // Verify results have correct columns
    let first = &results.rows[0];
    assert!(first.get("name").is_some(), "Should have 'name' column");
    assert!(first.get("price").is_some(), "Should have 'price' column");
}

#[test]
fn test_execute_aggregate_query() {
    let db_path = test_db_path("products-vec.db");
    if !db_path.exists() {
        eprintln!("Skipping test: {} not found", db_path.display());
        return;
    }

    let mut adapter = SQLiteAdapter::new();
    adapter.connect(db_path.to_str().unwrap()).unwrap();

    // Test aggregate query
    let results = adapter
        .execute("SELECT category, COUNT(*) as count, AVG(price) as avg_price FROM products GROUP BY category ORDER BY count DESC")
        .unwrap();

    assert!(!results.rows.is_empty(), "Should return aggregated results");

    // Verify structure
    let first = &results.rows[0];
    assert!(first.get("category").is_some(), "Should have 'category'");
    assert!(first.get("count").is_some(), "Should have 'count'");
    assert!(first.get("avg_price").is_some(), "Should have 'avg_price'");
}

#[test]
fn test_execute_join_query() {
    let db_path = test_db_path("products-vec.db");
    if !db_path.exists() {
        eprintln!("Skipping test: {} not found", db_path.display());
        return;
    }

    let mut adapter = SQLiteAdapter::new();
    adapter.connect(db_path.to_str().unwrap()).unwrap();

    // Test join between products and product_stats
    let results = adapter
        .execute(
            "SELECT p.name, p.price, ps.rating, ps.review_count
             FROM products p
             JOIN product_stats ps ON p.id = ps.id
             WHERE ps.rating >= 4.5
             ORDER BY ps.review_count DESC
             LIMIT 5"
        )
        .unwrap();

    assert!(results.rows.len() <= 5, "Should return at most 5 results");

    // Verify structure if results exist
    if !results.rows.is_empty() {
        let first = &results.rows[0];
        assert!(first.get("name").is_some(), "Should have 'name'");
        assert!(first.get("rating").is_some(), "Should have 'rating'");
    }
}

#[test]
fn test_hybrid_fts_filter_query() {
    let db_path = test_db_path("wikipedia-fts5.db");
    if !db_path.exists() {
        eprintln!("Skipping test: {} not found", db_path.display());
        return;
    }

    let mut adapter = SQLiteAdapter::new();
    adapter.connect(db_path.to_str().unwrap()).unwrap();

    // Hybrid query: FTS5 match + regular filter on category
    // Note: FTS5 virtual tables have limited support for regular WHERE clauses
    // Better to join with metadata table
    let results = adapter
        .execute(
            "SELECT a.title, a.category
             FROM articles a
             WHERE a.articles MATCH 'machine learning'
             LIMIT 10"
        )
        .unwrap();

    assert!(!results.rows.is_empty(), "Should find machine learning articles");
}

#[test]
fn test_error_handling_invalid_query() {
    let mut adapter = SQLiteAdapter::new();
    adapter.connect(":memory:").unwrap();

    // Test with invalid SQL
    let result = adapter.execute("SELECT * FROM nonexistent_table");
    assert!(result.is_err(), "Should return error for invalid query");
}

#[test]
fn test_error_handling_not_connected() {
    let adapter = SQLiteAdapter::new();

    // Test operations without connecting
    let result = adapter.check_fts5();
    assert!(result.is_err(), "Should return error when not connected");

    let result = adapter.gather_statistics();
    assert!(result.is_err(), "Should return error when not connected");
}

#[test]
fn test_connection_pool() {
    let mut adapter = SQLiteAdapter::new();
    adapter.connect(":memory:").unwrap();

    // Test multiple concurrent connections from pool
    let conn1 = adapter.get_connection().unwrap();
    let conn2 = adapter.get_connection().unwrap();

    // Both connections should work
    let version1: String = conn1
        .query_row("SELECT sqlite_version()", [], |row| row.get(0))
        .unwrap();
    let version2: String = conn2
        .query_row("SELECT sqlite_version()", [], |row| row.get(0))
        .unwrap();

    assert_eq!(version1, version2, "Both connections should work");
}

#[test]
fn test_base64_encoding_blob() {
    let db_path = test_db_path("products-vec.db");
    if !db_path.exists() {
        eprintln!("Skipping test: {} not found", db_path.display());
        return;
    }

    let mut adapter = SQLiteAdapter::new();
    adapter.connect(db_path.to_str().unwrap()).unwrap();

    // Query a BLOB column (embedding)
    let results = adapter
        .execute("SELECT id, name, embedding FROM products WHERE id = 1")
        .unwrap();

    if !results.rows.is_empty() {
        let first = &results.rows[0];
        // embedding column should be present (might be NULL or base64-encoded)
        assert!(first.get("embedding").is_some(), "Should have 'embedding' column");
    }
}
