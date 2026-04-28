//! Integration tests for federated query optimization.
//!
//! Tests cover: FederatedQuery model, FederatedCostModel,
//! FederatedOptimizer strategy selection, and end-to-end analysis.

use std::collections::HashMap;

use ra_core::algebra::{AggregateExpr, AggregateFunction, JoinType, ProjectionColumn, RelExpr};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::federated::{
    format_bytes, DataSource, DatabaseType, ExecutionLocation, FederatedQuery, QueryCapabilities,
    RemoteConnection,
};
use ra_core::statistics::Statistics;
use ra_engine::federated_cost::FederatedCostModel;
use ra_engine::federated_optimizer::FederatedOptimizer;

// ── Helpers ──────────────────────────────────────────────────

fn pg_connection() -> RemoteConnection {
    RemoteConnection::new(
        DatabaseType::PostgreSQL,
        "postgres://pg.example.com:5432/db",
        10,
        100,
    )
}

fn mysql_connection() -> RemoteConnection {
    RemoteConnection::new(
        DatabaseType::MySQL,
        "mysql://mysql.example.com:3306/db",
        15,
        50,
    )
}

fn sqlite_connection() -> RemoteConnection {
    RemoteConnection::new(DatabaseType::SQLite, "sqlite://remote-nfs/data.db", 5, 200)
}

fn slow_connection() -> RemoteConnection {
    RemoteConnection::new(
        DatabaseType::PostgreSQL,
        "postgres://slow-link.example.com:5432/db",
        200,
        10,
    )
}

fn fast_connection() -> RemoteConnection {
    RemoteConnection::new(
        DatabaseType::PostgreSQL,
        "postgres://fast-link.example.com:5432/db",
        1,
        1000,
    )
}

fn small_stats() -> Statistics {
    let mut stats = Statistics::new(1000.0);
    stats.avg_row_size = 100;
    stats.total_size = 100_000;
    stats
}

fn medium_stats() -> Statistics {
    let mut stats = Statistics::new(100_000.0);
    stats.avg_row_size = 200;
    stats.total_size = 20_000_000;
    stats
}

fn large_stats() -> Statistics {
    let mut stats = Statistics::new(10_000_000.0);
    stats.avg_row_size = 200;
    stats.total_size = 2_000_000_000;
    stats
}

fn equality_filter(table: &str, column: &str, value: i64) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::qualified(table, column))),
        right: Box::new(Expr::Const(Const::Int(value))),
    }
}

fn string_filter(table: &str, column: &str, value: &str) -> Expr {
    Expr::BinOp {
        op: BinOp::Eq,
        left: Box::new(Expr::Column(ColumnRef::qualified(table, column))),
        right: Box::new(Expr::Const(Const::String(value.to_owned()))),
    }
}

// ── FederatedQuery Model Tests ──────────────────────────────

#[test]
fn query_with_single_local_source() {
    let mut sources = HashMap::new();
    sources.insert("users".into(), DataSource::local("users", small_stats()));
    let query = FederatedQuery::new(RelExpr::scan("users"), sources);

    assert!(!query.is_distributed());
    assert!(query.local_sources().contains(&"users"));
    assert!(query.remote_sources().is_empty());
    assert_eq!(query.remote_endpoint_count(), 0);
}

#[test]
fn query_with_single_remote_source() {
    let mut sources = HashMap::new();
    sources.insert(
        "orders".into(),
        DataSource::remote(
            pg_connection(),
            "orders",
            Some(large_stats()),
            QueryCapabilities::full(),
        ),
    );
    let query = FederatedQuery::new(RelExpr::scan("orders"), sources);

    assert!(query.is_distributed());
    assert_eq!(query.remote_sources().len(), 1);
    assert_eq!(query.remote_endpoint_count(), 1);
}

#[test]
fn query_with_multiple_remote_sources() {
    let mut sources = HashMap::new();
    sources.insert(
        "orders".into(),
        DataSource::remote(
            pg_connection(),
            "orders",
            Some(large_stats()),
            QueryCapabilities::full(),
        ),
    );
    sources.insert(
        "products".into(),
        DataSource::remote(
            mysql_connection(),
            "products",
            Some(medium_stats()),
            QueryCapabilities::full(),
        ),
    );
    sources.insert(
        "local_cache".into(),
        DataSource::local("local_cache", small_stats()),
    );
    let query = FederatedQuery::new(RelExpr::scan("orders"), sources);

    assert!(query.is_distributed());
    assert_eq!(query.remote_sources().len(), 2);
    assert_eq!(query.remote_endpoint_count(), 2);
    assert_eq!(query.local_sources().len(), 1);
}

#[test]
fn query_colocated_tables_same_endpoint() {
    let mut sources = HashMap::new();
    sources.insert(
        "orders".into(),
        DataSource::remote(pg_connection(), "orders", None, QueryCapabilities::full()),
    );
    sources.insert(
        "customers".into(),
        DataSource::remote(
            pg_connection(),
            "customers",
            None,
            QueryCapabilities::full(),
        ),
    );
    let query = FederatedQuery::new(RelExpr::scan("orders"), sources);

    assert_eq!(query.remote_endpoint_count(), 1);
}

// ── Database Type Capabilities Tests ────────────────────────

#[test]
fn postgresql_has_full_capabilities() {
    let caps = DatabaseType::PostgreSQL.default_capabilities();
    assert!(caps.supports_filter_pushdown);
    assert!(caps.supports_join_pushdown);
    assert!(caps.supports_aggregate_pushdown);
    assert!(caps.supports_window_pushdown);
    assert!(caps.supports_function("STDDEV"));
    assert!(caps.supports_function("ARRAY_AGG"));
    assert_eq!(caps.max_query_complexity, None);
}

#[test]
fn mysql_capabilities() {
    let caps = DatabaseType::MySQL.default_capabilities();
    assert!(caps.supports_filter_pushdown);
    assert!(caps.supports_join_pushdown);
    assert!(caps.supports_function("GROUP_CONCAT"));
    assert!(!caps.supports_function("ARRAY_AGG"));
}

#[test]
fn sqlite_capabilities() {
    let caps = DatabaseType::SQLite.default_capabilities();
    assert!(caps.supports_filter_pushdown);
    assert!(caps.supports_join_pushdown);
    assert!(caps.supports_function("COUNT"));
}

#[test]
fn generic_jdbc_limited_capabilities() {
    let caps = DatabaseType::GenericJdbc.default_capabilities();
    assert!(caps.supports_filter_pushdown);
    assert!(!caps.supports_join_pushdown);
    assert!(!caps.supports_aggregate_pushdown);
    assert!(!caps.supports_window_pushdown);
    assert_eq!(caps.max_query_complexity, Some(50));
}

#[test]
fn snowflake_cloud_capabilities() {
    let caps = DatabaseType::Snowflake.default_capabilities();
    assert!(caps.supports_filter_pushdown);
    assert!(caps.supports_join_pushdown);
    assert!(caps.supports_function("APPROX_COUNT_DISTINCT"));
}

#[test]
fn duckdb_capabilities() {
    let caps = DatabaseType::DuckDB.default_capabilities();
    assert!(caps.supports_filter_pushdown);
    assert!(caps.supports_function("LIST"));
    assert!(caps.supports_function("APPROX_COUNT_DISTINCT"));
}

// ── Remote Connection Tests ─────────────────────────────────

#[test]
fn transfer_time_fast_connection() {
    let conn = fast_connection();
    // 1 Gbps = 125_000 bytes/ms, 1MB = 1_048_576 bytes
    // Transfer: 1_048_576 / 125_000 = ~8.4ms
    // Plus latency: 1ms
    let time = conn.transfer_time_ms(1_048_576);
    assert!(time > 8.0);
    assert!(time < 12.0);
}

#[test]
fn transfer_time_slow_connection() {
    let conn = slow_connection();
    // 10 Mbps = 1_250 bytes/ms, 1MB = 1_048_576 bytes
    // Transfer: 1_048_576 / 1_250 = ~838ms
    // Plus latency: 200ms
    let time = conn.transfer_time_ms(1_048_576);
    assert!(time > 1000.0);
    assert!(time < 1100.0);
}

#[test]
fn transfer_time_zero_bytes() {
    let conn = pg_connection();
    let time = conn.transfer_time_ms(0);
    // Just the latency
    assert!((time - 10.0).abs() < f64::EPSILON);
}

// ── Cost Model Tests ────────────────────────────────────────

#[test]
fn cost_model_ship_query_small_result() {
    let model = FederatedCostModel::new();
    let conn = pg_connection();
    let stats = large_stats();

    let cost = model.estimate_ship_query(&conn, Some(&stats), 100.0, 200);

    assert!(cost.total_ms > 0.0);
    assert!(cost.remote_exec_ms > 0.0);
    // Small result = small transfer
    assert_eq!(cost.transfer_bytes, 20_000);
    assert_eq!(cost.local_exec_ms, 0.0);
}

#[test]
fn cost_model_ship_data_full_large_table() {
    let model = FederatedCostModel::new();
    let conn = pg_connection();
    let stats = large_stats();

    let cost = model.estimate_ship_data(&conn, Some(&stats), false);

    assert_eq!(cost.rows_transferred, 10_000_000);
    assert!(cost.network_transfer_ms > 0.0);
    assert!(cost.total_ms > cost.network_transfer_ms);
}

#[test]
fn cost_model_filtered_much_cheaper_than_full() {
    let model = FederatedCostModel::new();
    let conn = pg_connection();
    let stats = large_stats();

    let full = model.estimate_ship_data(&conn, Some(&stats), false);
    let filtered = model.estimate_ship_data(&conn, Some(&stats), true);

    // Filtered should transfer ~10% of data
    assert!(filtered.transfer_bytes < full.transfer_bytes / 5);
    assert!(filtered.total_ms < full.total_ms);
}

#[test]
fn cost_model_hybrid_selectivity_impact() {
    let model = FederatedCostModel::new();
    let conn = pg_connection();
    let stats = large_stats();

    let selective = model.estimate_hybrid(&conn, Some(&stats), 0.001, 2.0);
    let unselective = model.estimate_hybrid(&conn, Some(&stats), 0.5, 2.0);

    assert!(selective.transfer_bytes < unselective.transfer_bytes);
    assert!(selective.total_ms < unselective.total_ms);
}

#[test]
fn cost_model_local_no_network() {
    let model = FederatedCostModel::new();
    let stats = medium_stats();

    let cost = model.estimate_local(Some(&stats));

    assert_eq!(cost.remote_exec_ms, 0.0);
    assert_eq!(cost.network_transfer_ms, 0.0);
    assert_eq!(cost.transfer_bytes, 0);
    assert!(cost.local_exec_ms > 0.0);
}

#[test]
fn cost_model_slow_link_favors_pushdown() {
    let model = FederatedCostModel::new();
    let fast = fast_connection();
    let slow = slow_connection();
    let stats = large_stats();

    let fast_full = model.estimate_ship_data(&fast, Some(&stats), false);
    let slow_full = model.estimate_ship_data(&slow, Some(&stats), false);

    // Slow connection = much higher network transfer cost
    assert!(slow_full.network_transfer_ms > fast_full.network_transfer_ms * 5.0);
    // Total cost is also higher
    assert!(slow_full.total_ms > fast_full.total_ms);
}

#[test]
fn cost_model_estimate_output_rows() {
    let model = FederatedCostModel::new();
    let stats = large_stats();

    let scan_rows = model.estimate_output_rows(&RelExpr::scan("t"), Some(&stats));
    assert!((scan_rows - 10_000_000.0).abs() < f64::EPSILON);

    let limit = RelExpr::Limit {
        count: 10,
        offset: 0,
        input: Box::new(RelExpr::scan("t")),
    };
    let limit_rows = model.estimate_output_rows(&limit, Some(&stats));
    assert!((limit_rows - 10.0).abs() < f64::EPSILON);
}

// ── Optimizer Strategy Tests ────────────────────────────────

#[test]
fn optimizer_local_only_query() {
    let optimizer = FederatedOptimizer::new();
    let mut sources = HashMap::new();
    sources.insert("users".into(), DataSource::local("users", medium_stats()));
    let query = FederatedQuery::new(RelExpr::scan("users"), sources);

    let plan = optimizer
        .optimize_federated(&query)
        .expect("should succeed");

    assert!(matches!(plan.location, ExecutionLocation::Local { .. }));
}

#[test]
fn optimizer_empty_sources_error() {
    let optimizer = FederatedOptimizer::new();
    let query = FederatedQuery::new(RelExpr::scan("t"), HashMap::new());

    assert!(optimizer.optimize_federated(&query).is_err());
}

#[test]
fn optimizer_generates_multiple_strategies() {
    let optimizer = FederatedOptimizer::new();
    let mut sources = HashMap::new();
    sources.insert(
        "orders".into(),
        DataSource::remote(
            pg_connection(),
            "orders",
            Some(large_stats()),
            QueryCapabilities::full(),
        ),
    );
    let plan = RelExpr::Filter {
        predicate: string_filter("orders", "status", "ACTIVE"),
        input: Box::new(RelExpr::scan("orders")),
    };
    let query = FederatedQuery::new(plan, sources);

    let strategies = optimizer.enumerate_strategies(&query);
    // Should have at least: local, ship_query, ship_data, ship_data_filtered, hybrid
    assert!(strategies.len() >= 4);
}

#[test]
fn optimizer_picks_cheapest_strategy() {
    let optimizer = FederatedOptimizer::new();
    let mut sources = HashMap::new();
    sources.insert(
        "orders".into(),
        DataSource::remote(
            pg_connection(),
            "orders",
            Some(large_stats()),
            QueryCapabilities::full(),
        ),
    );
    let plan = RelExpr::Filter {
        predicate: string_filter("orders", "status", "ACTIVE"),
        input: Box::new(RelExpr::scan("orders")),
    };
    let query = FederatedQuery::new(plan, sources);

    let result = optimizer
        .optimize_federated(&query)
        .expect("should succeed");

    // Chosen strategy should be cheaper than all alternatives
    for alt in &result.alternatives {
        assert!(result.cost.total_ms <= alt.total_ms);
    }
}

#[test]
fn optimizer_can_ship_simple_scan() {
    let optimizer = FederatedOptimizer::new();
    let caps = QueryCapabilities::full();
    let plan = RelExpr::scan("t");

    assert!(optimizer.can_ship_query(&plan, &caps));
}

#[test]
fn optimizer_can_ship_filter_scan() {
    let optimizer = FederatedOptimizer::new();
    let caps = QueryCapabilities::full();
    let plan = RelExpr::Filter {
        predicate: Expr::Const(Const::Bool(true)),
        input: Box::new(RelExpr::scan("t")),
    };

    assert!(optimizer.can_ship_query(&plan, &caps));
}

#[test]
fn optimizer_cannot_ship_to_limited_remote() {
    let optimizer = FederatedOptimizer::new();
    let caps = QueryCapabilities::minimal();
    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::Const(Const::Bool(true)),
        left: Box::new(RelExpr::scan("a")),
        right: Box::new(RelExpr::scan("b")),
    };

    assert!(!optimizer.can_ship_query(&plan, &caps));
}

#[test]
fn optimizer_cannot_ship_recursive_cte() {
    let optimizer = FederatedOptimizer::new();
    let caps = QueryCapabilities::full();
    let plan = RelExpr::RecursiveCTE {
        name: "r".into(),
        base_case: Box::new(RelExpr::scan("t")),
        recursive_case: Box::new(RelExpr::scan("t")),
        body: Box::new(RelExpr::scan("r")),
        cycle_detection: None,
    };

    assert!(!optimizer.can_ship_query(&plan, &caps));
}

#[test]
fn optimizer_cannot_ship_values() {
    let optimizer = FederatedOptimizer::new();
    let caps = QueryCapabilities::full();
    let plan = RelExpr::Values {
        rows: vec![vec![Expr::Const(Const::Int(1))]],
    };

    assert!(!optimizer.can_ship_query(&plan, &caps));
}

// ── PostgreSQL Scenario Tests ───────────────────────────────

#[test]
fn pg_filter_pushdown_on_large_table() {
    let optimizer = FederatedOptimizer::new();
    let mut sources = HashMap::new();
    sources.insert(
        "events".into(),
        DataSource::remote(
            pg_connection(),
            "events",
            Some(large_stats()),
            DatabaseType::PostgreSQL.default_capabilities(),
        ),
    );

    let plan = RelExpr::Filter {
        predicate: equality_filter("events", "user_id", 42),
        input: Box::new(RelExpr::scan("events")),
    };
    let query = FederatedQuery::new(plan, sources);

    let result = optimizer
        .optimize_federated(&query)
        .expect("should succeed");

    // For a large table with selective filter, should not choose
    // full data shipping
    assert_ne!(result.cost.strategy, "ship_data_full");
    assert!(!result.steps.is_empty());
}

#[test]
fn pg_aggregation_pushdown() {
    let optimizer = FederatedOptimizer::new();
    let mut sources = HashMap::new();
    sources.insert(
        "sales".into(),
        DataSource::remote(
            pg_connection(),
            "sales",
            Some(large_stats()),
            DatabaseType::PostgreSQL.default_capabilities(),
        ),
    );

    let plan = RelExpr::Aggregate {
        group_by: vec![Expr::Column(ColumnRef::new("region"))],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(Expr::Column(ColumnRef::new("amount"))),
            distinct: false,
            alias: Some("total".into()),
        }],
        input: Box::new(RelExpr::scan("sales")),
    };
    let query = FederatedQuery::new(plan, sources);

    let result = optimizer
        .optimize_federated(&query)
        .expect("should succeed");

    assert!(result.cost.total_ms > 0.0);
}

// ── MySQL Scenario Tests ────────────────────────────────────

#[test]
fn mysql_join_with_local() {
    let optimizer = FederatedOptimizer::new();
    let mut sources = HashMap::new();
    sources.insert(
        "local_users".into(),
        DataSource::local("local_users", small_stats()),
    );
    sources.insert(
        "remote_orders".into(),
        DataSource::remote(
            mysql_connection(),
            "remote_orders",
            Some(medium_stats()),
            DatabaseType::MySQL.default_capabilities(),
        ),
    );

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("local_users", "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified(
                "remote_orders",
                "user_id",
            ))),
        },
        left: Box::new(RelExpr::scan("local_users")),
        right: Box::new(RelExpr::scan("remote_orders")),
    };
    let query = FederatedQuery::new(plan, sources);

    let result = optimizer
        .optimize_federated(&query)
        .expect("should succeed");

    assert!(result.cost.total_ms > 0.0);
    assert!(!result.steps.is_empty());
}

// ── SQLite Scenario Tests ───────────────────────────────────

#[test]
fn sqlite_small_table_data_ship() {
    let optimizer = FederatedOptimizer::new();
    let mut sources = HashMap::new();
    sources.insert(
        "config".into(),
        DataSource::remote(
            sqlite_connection(),
            "config",
            Some(small_stats()),
            DatabaseType::SQLite.default_capabilities(),
        ),
    );

    let plan = RelExpr::scan("config");
    let query = FederatedQuery::new(plan, sources);

    let result = optimizer
        .optimize_federated(&query)
        .expect("should succeed");

    assert!(result.cost.total_ms > 0.0);
}

// ── Cross-Database Tests ────────────────────────────────────

#[test]
fn cross_database_pg_and_mysql() {
    let optimizer = FederatedOptimizer::new();
    let mut sources = HashMap::new();
    sources.insert(
        "pg_orders".into(),
        DataSource::remote(
            pg_connection(),
            "pg_orders",
            Some(large_stats()),
            DatabaseType::PostgreSQL.default_capabilities(),
        ),
    );
    sources.insert(
        "mysql_products".into(),
        DataSource::remote(
            mysql_connection(),
            "mysql_products",
            Some(medium_stats()),
            DatabaseType::MySQL.default_capabilities(),
        ),
    );

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified(
                "pg_orders",
                "product_id",
            ))),
            right: Box::new(Expr::Column(ColumnRef::qualified("mysql_products", "id"))),
        },
        left: Box::new(RelExpr::scan("pg_orders")),
        right: Box::new(RelExpr::scan("mysql_products")),
    };
    let query = FederatedQuery::new(plan, sources);

    let result = optimizer
        .optimize_federated(&query)
        .expect("should succeed");

    assert_eq!(query.remote_endpoint_count(), 2);
    assert!(result.cost.total_ms > 0.0);
}

#[test]
fn three_way_federation() {
    let optimizer = FederatedOptimizer::new();
    let mut sources = HashMap::new();
    sources.insert(
        "local_users".into(),
        DataSource::local("local_users", small_stats()),
    );
    sources.insert(
        "pg_orders".into(),
        DataSource::remote(
            pg_connection(),
            "pg_orders",
            Some(large_stats()),
            QueryCapabilities::full(),
        ),
    );
    sources.insert(
        "mysql_inventory".into(),
        DataSource::remote(
            mysql_connection(),
            "mysql_inventory",
            Some(medium_stats()),
            QueryCapabilities::full(),
        ),
    );

    let plan = RelExpr::scan("pg_orders");
    let query = FederatedQuery::new(plan, sources);

    assert_eq!(query.remote_endpoint_count(), 2);
    assert_eq!(query.local_sources().len(), 1);

    let result = optimizer
        .optimize_federated(&query)
        .expect("should succeed");
    assert!(result.cost.total_ms > 0.0);
}

// ── Edge Cases ──────────────────────────────────────────────

#[test]
fn no_statistics_available() {
    let optimizer = FederatedOptimizer::new();
    let mut sources = HashMap::new();
    sources.insert(
        "remote".into(),
        DataSource::remote(pg_connection(), "remote", None, QueryCapabilities::full()),
    );
    let query = FederatedQuery::new(RelExpr::scan("remote"), sources);

    let result = optimizer
        .optimize_federated(&query)
        .expect("should succeed with defaults");
    assert!(result.cost.total_ms > 0.0);
}

#[test]
fn minimal_capabilities_remote() {
    let optimizer = FederatedOptimizer::new();
    let mut sources = HashMap::new();
    sources.insert(
        "legacy".into(),
        DataSource::remote(
            pg_connection(),
            "legacy",
            Some(medium_stats()),
            QueryCapabilities::minimal(),
        ),
    );

    let plan = RelExpr::Join {
        join_type: JoinType::Inner,
        condition: Expr::Const(Const::Bool(true)),
        left: Box::new(RelExpr::scan("legacy")),
        right: Box::new(RelExpr::scan("legacy")),
    };
    let query = FederatedQuery::new(plan, sources);

    // Should not ship query since minimal caps don't support joins
    let strategies = optimizer.enumerate_strategies(&query);
    let has_ship_query = strategies
        .iter()
        .any(|s| matches!(s, ExecutionLocation::ShipQuery { .. }));
    assert!(!has_ship_query);
}

#[test]
fn project_scan_can_be_shipped() {
    let optimizer = FederatedOptimizer::new();
    let caps = QueryCapabilities::full();
    let plan = RelExpr::Project {
        columns: vec![ProjectionColumn {
            expr: Expr::Column(ColumnRef::new("id")),
            alias: None,
        }],
        input: Box::new(RelExpr::scan("t")),
    };

    assert!(optimizer.can_ship_query(&plan, &caps));
}

#[test]
fn aggregate_scan_can_be_shipped() {
    let optimizer = FederatedOptimizer::new();
    let caps = QueryCapabilities::full();
    let plan = RelExpr::Aggregate {
        group_by: vec![],
        aggregates: vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: None,
            distinct: false,
            alias: None,
        }],
        input: Box::new(RelExpr::scan("t")),
    };

    assert!(optimizer.can_ship_query(&plan, &caps));
}

#[test]
fn sort_limit_can_be_shipped() {
    let optimizer = FederatedOptimizer::new();
    let caps = QueryCapabilities::full();
    let plan = RelExpr::Limit {
        count: 10,
        offset: 0,
        input: Box::new(RelExpr::Sort {
            keys: vec![],
            input: Box::new(RelExpr::scan("t")),
        }),
    };

    assert!(optimizer.can_ship_query(&plan, &caps));
}

// ── Analysis Tests ──────────────────────────────────────────

#[test]
fn analysis_produces_plan_with_steps() {
    let optimizer = FederatedOptimizer::new();
    let mut sources = HashMap::new();
    sources.insert(
        "orders".into(),
        DataSource::remote(
            pg_connection(),
            "orders",
            Some(large_stats()),
            QueryCapabilities::full(),
        ),
    );
    let plan = RelExpr::Filter {
        predicate: string_filter("orders", "status", "ACTIVE"),
        input: Box::new(RelExpr::scan("orders")),
    };
    let query = FederatedQuery::new(plan, sources);

    let analysis = optimizer.analyze(&query).expect("should succeed");

    assert!(!analysis.plan.steps.is_empty());
    assert!(analysis.plan.cost.total_ms > 0.0);
}

#[test]
fn analysis_compares_alternatives() {
    let optimizer = FederatedOptimizer::new();
    let mut sources = HashMap::new();
    sources.insert(
        "data".into(),
        DataSource::remote(
            pg_connection(),
            "data",
            Some(large_stats()),
            QueryCapabilities::full(),
        ),
    );
    let plan = RelExpr::Filter {
        predicate: equality_filter("data", "id", 1),
        input: Box::new(RelExpr::scan("data")),
    };
    let query = FederatedQuery::new(plan, sources);

    let analysis = optimizer.analyze(&query).expect("should succeed");

    assert!(!analysis.plan.alternatives.is_empty());
}

// ── Format Helper Tests ─────────────────────────────────────

#[test]
fn format_bytes_display() {
    assert_eq!(format_bytes(0), "0B");
    assert_eq!(format_bytes(100), "100B");
    assert_eq!(format_bytes(1024), "1.0KB");
    assert_eq!(format_bytes(1_048_576), "1.0MB");
    assert_eq!(format_bytes(1_073_741_824), "1.0GB");
    assert_eq!(format_bytes(10_485_760), "10.0MB");
}

// ── Serialization Tests ─────────────────────────────────────

#[test]
fn federated_query_json_roundtrip() {
    let mut sources = HashMap::new();
    sources.insert("t".into(), DataSource::local("t", small_stats()));
    let query = FederatedQuery::new(RelExpr::scan("t"), sources);

    let json = serde_json::to_string(&query).expect("serialize");
    let back: FederatedQuery = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(query, back);
}

#[test]
fn execution_location_json_roundtrip() {
    let loc = ExecutionLocation::Hybrid {
        remote_subquery: Box::new(RelExpr::scan("r")),
        local_operations: Box::new(RelExpr::scan("l")),
        target: pg_connection(),
    };
    let json = serde_json::to_string(&loc).expect("serialize");
    let back: ExecutionLocation = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(loc, back);
}

#[test]
fn remote_connection_json_roundtrip() {
    let conn = mysql_connection();
    let json = serde_json::to_string(&conn).expect("serialize");
    let back: RemoteConnection = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(conn, back);
}

#[test]
fn database_type_all_variants_display() {
    let types = [
        DatabaseType::PostgreSQL,
        DatabaseType::MySQL,
        DatabaseType::SQLite,
        DatabaseType::Snowflake,
        DatabaseType::BigQuery,
        DatabaseType::SparkSQL,
        DatabaseType::DuckDB,
        DatabaseType::GenericJdbc,
    ];
    for db in &types {
        let name = db.to_string();
        assert!(!name.is_empty());
    }
}

// ── Cost Breakdown Tests ────────────────────────────────────

#[test]
fn cost_breakdown_savings_calculation() {
    let cost = ra_core::federated::FederatedCostBreakdown {
        strategy: "hybrid".into(),
        remote_exec_ms: 50.0,
        network_transfer_ms: 200.0,
        transfer_bytes: 10_485_760,
        local_exec_ms: 30.0,
        total_ms: 280.0,
        rows_transferred: 100_000,
    };

    // Compare against 16 seconds for full data ship
    let savings = cost.savings_percent(16_000.0);
    assert!(savings > 98.0);
    assert!(savings < 99.0);
}

#[test]
fn cost_breakdown_no_savings_when_zero_alternative() {
    let cost = ra_core::federated::FederatedCostBreakdown {
        strategy: "test".into(),
        remote_exec_ms: 0.0,
        network_transfer_ms: 0.0,
        transfer_bytes: 0,
        local_exec_ms: 100.0,
        total_ms: 100.0,
        rows_transferred: 0,
    };

    assert!((cost.savings_percent(0.0)).abs() < f64::EPSILON);
}

#[test]
fn federated_plan_best_alternative_picks_cheapest() {
    let plan = ra_core::federated::FederatedPlan {
        location: ExecutionLocation::Local {
            query: RelExpr::scan("t"),
        },
        cost: ra_core::federated::FederatedCostBreakdown {
            strategy: "local".into(),
            remote_exec_ms: 0.0,
            network_transfer_ms: 0.0,
            transfer_bytes: 0,
            local_exec_ms: 50.0,
            total_ms: 50.0,
            rows_transferred: 0,
        },
        alternatives: vec![
            ra_core::federated::FederatedCostBreakdown {
                strategy: "ship_query".into(),
                remote_exec_ms: 100.0,
                network_transfer_ms: 50.0,
                transfer_bytes: 1000,
                local_exec_ms: 0.0,
                total_ms: 150.0,
                rows_transferred: 10,
            },
            ra_core::federated::FederatedCostBreakdown {
                strategy: "ship_data".into(),
                remote_exec_ms: 10.0,
                network_transfer_ms: 500.0,
                transfer_bytes: 100_000,
                local_exec_ms: 20.0,
                total_ms: 530.0,
                rows_transferred: 1000,
            },
        ],
        steps: vec!["test".into()],
    };

    let best = plan.best_alternative().expect("should have alt");
    assert_eq!(best.strategy, "ship_query");
    assert!((best.total_ms - 150.0).abs() < f64::EPSILON);
}
