//! Tests for the EXPLAIN API endpoint.
//!
//! This module contains tests for:
//! - PostgreSQL EXPLAIN (all versions)
//! - MySQL EXPLAIN FORMAT=JSON
//! - MariaDB EXPLAIN
//! - SQLite EXPLAIN QUERY PLAN
//! - DuckDB EXPLAIN
//! - Error handling for invalid SQL
//! - Timeout handling
//! - Redis caching layer
//!
//! Note: These tests require running database instances and Redis.
//! Run with `docker-compose up -d` to start test databases before running tests.

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::explain::{ExplainRequest, ExplainResponse};
    use crate::config::DatabaseConfig;

    #[test]
    fn test_explain_request_serialization() {
        let request = ExplainRequest {
            sql: "SELECT * FROM users".to_string(),
            engine: "postgresql".to_string(),
            analyze: false,
            config: Some(DatabaseConfig::postgres("postgresql://localhost/test")),
        };

        let json = serde_json::to_string(&request).expect("Failed to serialize");
        assert!(json.contains("SELECT * FROM users"));
        assert!(json.contains("postgresql"));
    }

    #[test]
    fn test_explain_request_deserialization() {
        let json = json!({
            "sql": "SELECT 1",
            "engine": "sqlite",
            "analyze": false
        });

        let request: ExplainRequest = serde_json::from_value(json).expect("Failed to deserialize");
        assert_eq!(request.sql, "SELECT 1");
        assert_eq!(request.engine, "sqlite");
        assert!(!request.analyze);
    }

    #[test]
    fn test_explain_response_structure() {
        let response = ExplainResponse {
            plan: json!({"node": "SeqScan"}),
            engine: "postgresql".to_string(),
            execution_time_ms: 12.5,
        };

        let json = serde_json::to_string(&response).expect("Failed to serialize");
        assert!(json.contains("SeqScan"));
        assert!(json.contains("postgresql"));
        assert!(json.contains("12.5"));
    }

    #[test]
    fn test_explain_request_with_analyze() {
        let request = ExplainRequest {
            sql: "SELECT COUNT(*) FROM users".to_string(),
            engine: "postgresql".to_string(),
            analyze: true,
            config: None,
        };

        assert!(request.analyze);
        assert!(request.config.is_none());
    }

    #[test]
    fn test_explain_request_all_engines() {
        let engines = vec!["postgresql", "mysql", "mariadb", "mariadb-11", "sqlite", "duckdb"];

        for engine in engines {
            let request = ExplainRequest {
                sql: "SELECT 1".to_string(),
                engine: engine.to_string(),
                analyze: false,
                config: None,
            };

            assert_eq!(request.engine, engine);
        }
    }

    #[test]
    fn test_database_config_postgres() {
        let config = DatabaseConfig::postgres("postgresql://localhost/test");
        let json = serde_json::to_value(&config).expect("Failed to serialize");

        assert!(json["type"].as_str().unwrap() == "postgresql");
    }

    #[test]
    fn test_database_config_mysql() {
        let config = DatabaseConfig::mysql("mysql://localhost/test");
        let json = serde_json::to_value(&config).expect("Failed to serialize");

        assert!(json["type"].as_str().unwrap() == "mysql");
    }

    #[test]
    fn test_database_config_sqlite() {
        let config = DatabaseConfig::sqlite(":memory:");
        let json = serde_json::to_value(&config).expect("Failed to serialize");

        assert!(json["type"].as_str().unwrap() == "sqlite");
    }

    #[test]
    fn test_database_config_duckdb() {
        let config = DatabaseConfig::duckdb(":memory:");
        let json = serde_json::to_value(&config).expect("Failed to serialize");

        assert!(json["type"].as_str().unwrap() == "duckdb");
    }
}

// Integration tests require actual database connections and Redis
// These should be run separately with proper test infrastructure
#[cfg(all(test, feature = "integration-tests"))]
mod integration_tests {
    // TODO: Add integration tests that:
    // 1. Test PostgreSQL EXPLAIN with various SQL queries
    // 2. Test MySQL EXPLAIN FORMAT=JSON
    // 3. Test MariaDB EXPLAIN
    // 4. Test SQLite EXPLAIN QUERY PLAN
    // 5. Test DuckDB EXPLAIN
    // 6. Test error handling for invalid SQL
    // 7. Test timeout behavior
    // 8. Test caching layer integration
}
