//! Tests for the polyglot SQL transpiler backend.

#![cfg(feature = "polyglot-backend")]

use ra_dialect::{Dialect, DialectTranslator, TranslationBackend};

#[test]
fn test_postgres_to_bigquery() {
    let translator = DialectTranslator::with_backend(
        Dialect::PostgreSql,
        Dialect::BigQuery,
        TranslationBackend::Polyglot,
    );

    let sql = "SELECT * FROM users WHERE age > 18";
    let result = translator.translate(sql).unwrap();

    assert!(!result.sql.is_empty());
    // BigQuery should preserve the basic structure
    assert!(result.sql.contains("SELECT") || result.sql.contains("select"));
    assert!(result.sql.contains("users"));
}

#[test]
fn test_mysql_to_postgres_ifnull() {
    let translator = DialectTranslator::with_backend(
        Dialect::MySql,
        Dialect::PostgreSql,
        TranslationBackend::Polyglot,
    );

    let sql = "SELECT IFNULL(name, 'Unknown') FROM users";
    let result = translator.translate(sql).unwrap();

    assert!(!result.sql.is_empty());
    // MySQL's IFNULL should be translated to PostgreSQL's COALESCE
    assert!(
        result.sql.contains("COALESCE") || result.sql.contains("coalesce"),
        "Expected COALESCE in translated SQL, got: {}",
        result.sql
    );
}

#[test]
fn test_postgres_to_snowflake() {
    let translator = DialectTranslator::with_backend(
        Dialect::PostgreSql,
        Dialect::Snowflake,
        TranslationBackend::Polyglot,
    );

    let sql = "SELECT COUNT(*) FROM orders WHERE created_at > NOW() - INTERVAL '1 day'";
    let result = translator.translate(sql).unwrap();

    assert!(!result.sql.is_empty());
    assert!(result.sql.contains("COUNT") || result.sql.contains("count"));
}

#[test]
fn test_sqlite_to_clickhouse() {
    let translator = DialectTranslator::with_backend(
        Dialect::Sqlite,
        Dialect::ClickHouse,
        TranslationBackend::Polyglot,
    );

    let sql = "SELECT datetime('now')";
    let result = translator.translate(sql).unwrap();

    assert!(!result.sql.is_empty());
}

#[test]
fn test_duckdb_to_databricks() {
    let translator = DialectTranslator::with_backend(
        Dialect::DuckDb,
        Dialect::Databricks,
        TranslationBackend::Polyglot,
    );

    let sql = "SELECT * FROM users LIMIT 10";
    let result = translator.translate(sql).unwrap();

    assert!(!result.sql.is_empty());
    assert!(result.sql.contains("LIMIT") || result.sql.contains("limit"));
}

#[test]
fn test_oracle_to_redshift() {
    let translator = DialectTranslator::with_backend(
        Dialect::Oracle,
        Dialect::Redshift,
        TranslationBackend::Polyglot,
    );

    let sql = "SELECT ROWNUM, name FROM users WHERE ROWNUM <= 10";
    let result = translator.translate(sql).unwrap();

    assert!(!result.sql.is_empty());
}

#[test]
fn test_trino_to_presto() {
    let translator = DialectTranslator::with_backend(
        Dialect::Trino,
        Dialect::Presto,
        TranslationBackend::Polyglot,
    );

    let sql = "SELECT * FROM catalog.schema.table";
    let result = translator.translate(sql).unwrap();

    assert!(!result.sql.is_empty());
    // Catalog.schema.table notation should be preserved
    assert!(result.sql.contains("catalog") || result.sql.contains("CATALOG"));
}

#[test]
fn test_backend_selection() {
    // Native backend (default)
    let native_translator = DialectTranslator::new(Dialect::PostgreSql, Dialect::MySql);
    assert_eq!(native_translator.backend(), TranslationBackend::Native);

    // Polyglot backend (explicit)
    let polyglot_translator = DialectTranslator::with_backend(
        Dialect::PostgreSql,
        Dialect::BigQuery,
        TranslationBackend::Polyglot,
    );
    assert_eq!(polyglot_translator.backend(), TranslationBackend::Polyglot);
}

#[test]
fn test_extended_dialects_available() {
    // Verify we can create translators for all the new dialects
    let dialects = [
        Dialect::BigQuery,
        Dialect::Snowflake,
        Dialect::Databricks,
        Dialect::Redshift,
        Dialect::ClickHouse,
        Dialect::Trino,
        Dialect::Presto,
        Dialect::Athena,
        Dialect::Hive,
        Dialect::Spark,
        Dialect::Teradata,
        Dialect::Exasol,
        Dialect::Fabric,
        Dialect::Dremio,
        Dialect::Drill,
        Dialect::Druid,
        Dialect::CockroachDb,
        Dialect::Materialize,
        Dialect::RisingWave,
        Dialect::SingleStore,
        Dialect::StarRocks,
        Dialect::Doris,
        Dialect::TiDb,
        Dialect::Tableau,
        Dialect::Solr,
        Dialect::Dune,
    ];

    for dialect in dialects {
        let translator = DialectTranslator::with_backend(
            Dialect::PostgreSql,
            dialect,
            TranslationBackend::Polyglot,
        );
        let result = translator.translate("SELECT 1");
        assert!(result.is_ok(), "Failed for dialect: {}", dialect);
    }
}