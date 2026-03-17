//! Integration tests for the full synthesis pipeline.
//!
//! Tests natural language -> intent -> `RelExpr` -> SQL for a variety
//! of query patterns against a realistic e-commerce schema.

#![allow(clippy::expect_used)]

use ra_synthesis::schema::{ColumnInfo, ForeignKey, SchemaInfo, TableInfo};
use ra_synthesis::{Synthesizer};

fn ecommerce_schema() -> SchemaInfo {
    let mut schema = SchemaInfo::new();

    schema.add_table(TableInfo::new(
        "users",
        vec![
            ColumnInfo::new("id", "INTEGER").primary_key(),
            ColumnInfo::new("name", "TEXT").not_null(),
            ColumnInfo::new("email", "TEXT").not_null(),
            ColumnInfo::new("age", "INTEGER"),
            ColumnInfo::new("city", "TEXT"),
        ],
    ));

    let mut orders = TableInfo::new(
        "orders",
        vec![
            ColumnInfo::new("id", "INTEGER").primary_key(),
            ColumnInfo::new("user_id", "INTEGER").not_null(),
            ColumnInfo::new("amount", "REAL").not_null(),
            ColumnInfo::new("status", "TEXT").not_null(),
            ColumnInfo::new("created_at", "TEXT"),
        ],
    );
    orders.add_foreign_key(ForeignKey {
        columns: vec!["user_id".into()],
        referenced_table: "users".into(),
        referenced_columns: vec!["id".into()],
    });
    schema.add_table(orders);

    schema.add_table(TableInfo::new(
        "products",
        vec![
            ColumnInfo::new("id", "INTEGER").primary_key(),
            ColumnInfo::new("name", "TEXT").not_null(),
            ColumnInfo::new("price", "REAL").not_null(),
            ColumnInfo::new("category", "TEXT"),
        ],
    ));

    schema
}

// --- Simple select queries ---

#[test]
fn select_all_from_table() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth.synthesize("show all users").expect("test");
    assert!(result.sql.contains("FROM users"));
    assert!(result.warnings.is_empty());
}

#[test]
fn select_from_products() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth.synthesize("list all products").expect("test");
    assert!(result.sql.contains("FROM products"));
}

// --- Filter queries ---

#[test]
fn filter_numeric_greater_than() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth
        .synthesize("find users where age greater than 30")
        .expect("test");
    assert!(result.sql.contains("WHERE"));
    assert!(result.sql.contains("age"));
    assert!(result.sql.contains("30"));
}

#[test]
fn filter_with_above_keyword() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth
        .synthesize("orders with amount above 100")
        .expect("test");
    assert!(result.sql.contains("amount"));
    assert!(result.sql.contains("100"));
}

// --- Aggregate queries ---

#[test]
fn count_aggregate() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth.synthesize("count of users").expect("test");
    assert!(result.sql.contains("COUNT"));
}

#[test]
fn sum_aggregate() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth
        .synthesize("total amount of orders")
        .expect("test");
    assert!(result.sql.contains("SUM") || result.sql.contains("amount"));
}

#[test]
fn average_aggregate() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth
        .synthesize("average price of products")
        .expect("test");
    assert!(result.sql.contains("AVG") || result.sql.contains("price"));
}

// --- Limit queries ---

#[test]
fn top_n_query() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth.synthesize("show top 10 users").expect("test");
    assert!(result.sql.contains("LIMIT 10"));
}

#[test]
fn first_n_query() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth
        .synthesize("show first 5 orders")
        .expect("test");
    assert!(result.sql.contains("LIMIT 5"));
}

// --- Sort queries ---

#[test]
fn order_ascending() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth
        .synthesize("show users sorted by name")
        .expect("test");
    assert!(result.sql.contains("ORDER BY"));
    assert!(result.sql.contains("name"));
}

#[test]
fn order_descending() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth
        .synthesize("show users ordered by age descending")
        .expect("test");
    assert!(result.sql.contains("ORDER BY"));
    assert!(result.sql.contains("DESC"));
}

// --- Join queries ---

#[test]
fn join_two_tables() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth
        .synthesize("show users and their orders")
        .expect("test");
    assert!(result.sql.contains("JOIN"), "SQL: {}", result.sql);
    assert!(
        result.sql.contains("users") && result.sql.contains("orders"),
        "SQL should reference both tables: {}",
        result.sql
    );
}

// --- Error cases ---

#[test]
fn unknown_table_returns_error() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth.synthesize("show all employees");
    assert!(result.is_err());
}

#[test]
fn empty_query_returns_error() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth.synthesize("hello world");
    assert!(result.is_err());
}

// --- Serialization ---

#[test]
fn result_is_serializable_to_json() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth.synthesize("show all users").expect("test");
    let json = serde_json::to_string(&result).expect("serialize");
    assert!(!json.is_empty());
    assert!(json.contains("sql"));
    assert!(json.contains("rel_expr"));
}

// --- Combined operations ---

#[test]
fn filter_and_limit() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth
        .synthesize("top 5 users with age above 25")
        .expect("test");
    assert!(result.sql.contains("LIMIT 5"));
}

#[test]
fn sort_and_limit() {
    let schema = ecommerce_schema();
    let synth = Synthesizer::new(&schema);
    let result = synth
        .synthesize("top 10 users sorted by name")
        .expect("test");
    assert!(result.sql.contains("ORDER BY"));
    assert!(result.sql.contains("LIMIT 10"));
}

// --- Schema API tests ---

#[test]
fn schema_find_table_case_insensitive() {
    let schema = ecommerce_schema();
    assert!(schema.find_table("USERS").is_some());
    assert!(schema.find_table("Orders").is_some());
    assert!(schema.find_table("nonexistent").is_none());
}

#[test]
fn schema_tables_with_column() {
    let schema = ecommerce_schema();
    let tables_with_id = schema.tables_with_column("id");
    assert_eq!(tables_with_id.len(), 3);
}

#[test]
fn column_info_numeric_detection() {
    assert!(ColumnInfo::new("x", "INTEGER").is_numeric());
    assert!(ColumnInfo::new("x", "REAL").is_numeric());
    assert!(ColumnInfo::new("x", "FLOAT").is_numeric());
    assert!(ColumnInfo::new("x", "DOUBLE").is_numeric());
    assert!(ColumnInfo::new("x", "NUMERIC(10,2)").is_numeric());
    assert!(!ColumnInfo::new("x", "TEXT").is_numeric());
    assert!(!ColumnInfo::new("x", "BLOB").is_numeric());
}
