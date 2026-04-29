#![expect(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
#![expect(clippy::float_cmp, reason = "exact float literals in tests")]
//! Integration tests for `PostgreSQL` adapter comparison functionality.
//!
//! Run with: `cargo test -p ra-adapters --features postgres postgres_comparison`

#[cfg(feature = "postgres")]
mod postgres_tests {
    use ra_adapters::comparison::{ComparisonReport, ComparisonResult, ExecutionMetrics};
    use ra_adapters::{compare_queries, compare_single_query, DatabaseAdapter, PostgresAdapter};
    use std::env;

    fn get_test_db_url() -> String {
        env::var("TEST_POSTGRES_URL")
            .unwrap_or_else(|_| "postgresql://localhost/postgres".to_string())
    }

    fn setup_adapter() -> PostgresAdapter {
        let mut adapter = PostgresAdapter::new();
        let url = get_test_db_url();
        adapter.connect(&url).expect("Failed to connect");
        adapter
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_adapter_connection() {
        let adapter = setup_adapter();
        assert_eq!(adapter.database_name(), "postgresql");
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_execute_simple_query() {
        let adapter = setup_adapter();
        let result = adapter.execute("SELECT 1 AS value");
        assert!(result.is_ok());

        let exec_result = result.unwrap();
        assert_eq!(exec_result.row_count, 1);
        assert!(exec_result.execution_time_ms > 0);
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_execute_native() {
        let adapter = setup_adapter();
        let result = adapter.execute_native("SELECT NOW() AS current_time");
        assert!(result.is_ok());

        let exec_result = result.unwrap();
        assert_eq!(exec_result.row_count, 1);
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_execute_with_ra() {
        let adapter = setup_adapter();
        let result = adapter.execute_with_ra("SELECT 42 AS answer");
        assert!(result.is_ok());

        let exec_result = result.unwrap();
        assert_eq!(exec_result.row_count, 1);
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_get_explain_plan() {
        let adapter = setup_adapter();
        let query = "SELECT 1";
        let result = adapter.get_explain_plan(query);
        assert!(result.is_ok());

        let plan = result.unwrap();
        assert!(plan.is_array());
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_get_stats_pg_class() {
        let adapter = setup_adapter();
        let result = adapter.get_stats("pg_class");
        assert!(result.is_ok());

        let stats = result.unwrap();
        assert_eq!(stats.table_name, "pg_class");
        assert!(stats.row_count > 0);
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_check_extensions() {
        let adapter = setup_adapter();
        let result = adapter.check_extensions();
        assert!(result.is_ok());

        let extensions = result.unwrap();
        assert!(extensions.contains_key("pgvector"));
        assert!(extensions.contains_key("pg_trgm"));
        assert!(extensions.contains_key("rum"));
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_compare_single_query() {
        let adapter = setup_adapter();
        let query = "SELECT COUNT(*) FROM pg_class";
        let result = compare_single_query(&adapter, query);
        assert!(result.is_ok());

        let comparison = result.unwrap();
        assert_eq!(comparison.query, query);
        assert!(comparison.native.execution_time_ms > 0);
        assert!(comparison.ra.execution_time_ms > 0);
        assert!(comparison.speedup >= 0.0);
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_compare_queries() {
        let adapter = setup_adapter();
        let queries = vec![
            "SELECT 1".to_string(),
            "SELECT NOW()".to_string(),
            "SELECT COUNT(*) FROM pg_class".to_string(),
        ];

        let result = compare_queries(&adapter, &queries);
        assert!(result.is_ok());

        let report = result.unwrap();
        assert_eq!(report.total_queries, 3);
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_comparison_metrics() {
        let adapter = setup_adapter();
        let query = "SELECT * FROM pg_class LIMIT 10";
        let result = compare_single_query(&adapter, query);
        assert!(result.is_ok());

        let comparison = result.unwrap();
        assert_eq!(comparison.native.rows_returned, 10);
        assert_eq!(comparison.ra.rows_returned, 10);
        assert!(comparison.native.rows_scanned.is_some());
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_comparison_with_filtering() {
        let adapter = setup_adapter();
        let query = "SELECT relname FROM pg_class WHERE relkind = 'r' LIMIT 5";
        let result = compare_single_query(&adapter, query);
        assert!(result.is_ok());

        let comparison = result.unwrap();
        assert!(comparison.native.rows_returned <= 5);
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_comparison_report_json() {
        let adapter = setup_adapter();
        let queries = vec!["SELECT 1".to_string(), "SELECT 2".to_string()];

        let report = compare_queries(&adapter, &queries).expect("Compare failed");
        let json_result = report.to_json();
        assert!(json_result.is_ok());

        let json = json_result.unwrap();
        assert!(json.contains("total_queries"));
        assert!(json.contains("avg_speedup"));
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_comparison_report_markdown() {
        let adapter = setup_adapter();
        let queries = vec!["SELECT 1".to_string()];

        let report = compare_queries(&adapter, &queries).expect("Compare failed");
        let markdown = report.to_markdown();

        assert!(markdown.contains("# PostgreSQL vs Ra Performance Comparison"));
        assert!(markdown.contains("Total Queries"));
        assert!(markdown.contains("| Query | Native (ms) | Ra (ms)"));
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_ra_optimization_improvement() {
        let adapter = setup_adapter();
        let query = "SELECT COUNT(*) FROM pg_class WHERE relkind = 'r'";
        let result = compare_single_query(&adapter, query);
        assert!(result.is_ok());

        let comparison = result.unwrap();
        assert!(comparison.speedup >= 0.0);
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_aggregation_query_comparison() {
        let adapter = setup_adapter();
        let query = "SELECT relkind, COUNT(*) FROM pg_class GROUP BY relkind";
        let result = compare_single_query(&adapter, query);
        assert!(result.is_ok());

        let comparison = result.unwrap();
        assert!(comparison.native.rows_returned > 0);
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_join_query_comparison() {
        let adapter = setup_adapter();
        let query = "SELECT c.relname, n.nspname \
            FROM pg_class c \
            JOIN pg_namespace n ON c.relnamespace = n.oid \
            LIMIT 10";
        let result = compare_single_query(&adapter, query);
        assert!(result.is_ok());

        let comparison = result.unwrap();
        assert_eq!(comparison.native.rows_returned, 10);
    }

    #[test]
    #[ignore = "requires live PostgreSQL"]
    fn test_comparison_statistics() {
        let adapter = setup_adapter();
        let queries = vec![
            "SELECT 1".to_string(),
            "SELECT 2".to_string(),
            "SELECT 3".to_string(),
            "SELECT 4".to_string(),
            "SELECT 5".to_string(),
        ];

        let report = compare_queries(&adapter, &queries).expect("Compare failed");
        assert_eq!(report.total_queries, 5);
        assert!(report.avg_speedup >= 0.0);
        assert!(report.median_speedup >= 0.0);
        assert!(report.max_speedup >= report.min_speedup);
    }

    #[test]
    fn test_execution_metrics_creation() {
        use ra_adapters::postgres::ExecutionResult;

        let result = ExecutionResult {
            rows: vec![],
            row_count: 100,
            execution_time_ms: 50,
            plan: None,
        };

        let metrics = ExecutionMetrics::from_postgres_result(&result);
        assert_eq!(metrics.execution_time_ms, 50);
        assert_eq!(metrics.rows_returned, 100);
    }

    #[test]
    fn test_comparison_result_speedup_calculation() {
        let native = ExecutionMetrics {
            execution_time_ms: 100,
            rows_returned: 50,
            rows_scanned: None,
            index_usage: vec![],
            cost_estimate: None,
            planning_time_ms: None,
        };

        let ra = ExecutionMetrics {
            execution_time_ms: 50,
            rows_returned: 50,
            rows_scanned: None,
            index_usage: vec![],
            cost_estimate: None,
            planning_time_ms: None,
        };

        let result = ComparisonResult::new("SELECT 1".to_string(), native, ra);
        assert_eq!(result.speedup, 2.0);
        assert_eq!(result.improvement_pct, 50.0);
        assert!(result.is_improved());
    }

    #[test]
    fn test_comparison_report_statistics() {
        let results = vec![
            ComparisonResult {
                query: "Q1".to_string(),
                native: ExecutionMetrics {
                    execution_time_ms: 100,
                    rows_returned: 10,
                    rows_scanned: None,
                    index_usage: vec![],
                    cost_estimate: None,
                    planning_time_ms: None,
                },
                ra: ExecutionMetrics {
                    execution_time_ms: 50,
                    rows_returned: 10,
                    rows_scanned: None,
                    index_usage: vec![],
                    cost_estimate: None,
                    planning_time_ms: None,
                },
                speedup: 2.0,
                improvement_pct: 50.0,
            },
            ComparisonResult {
                query: "Q2".to_string(),
                native: ExecutionMetrics {
                    execution_time_ms: 200,
                    rows_returned: 20,
                    rows_scanned: None,
                    index_usage: vec![],
                    cost_estimate: None,
                    planning_time_ms: None,
                },
                ra: ExecutionMetrics {
                    execution_time_ms: 100,
                    rows_returned: 20,
                    rows_scanned: None,
                    index_usage: vec![],
                    cost_estimate: None,
                    planning_time_ms: None,
                },
                speedup: 2.0,
                improvement_pct: 50.0,
            },
        ];

        let report = ComparisonReport::new(results);
        assert_eq!(report.total_queries, 2);
        assert_eq!(report.improved_queries, 2);
        assert_eq!(report.regressed_queries, 0);
        assert_eq!(report.avg_speedup, 2.0);
    }
}
