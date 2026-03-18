//! Integration tests for the ra-metadata crate.
//!
//! These tests verify the metadata gathering logic using mock data.
//! For live database tests, run with Docker:
//! ```bash
//! docker compose --profile test-db up -d
//! cargo test --test metadata_test -- --ignored
//! ```

use std::collections::HashMap;

use ra_metadata::connector::{
    parse_connection_string, ColumnInfo, ConnectionTarget,
    GatheredColumnStats, GatheredIndexStats, GatheredTableStats,
    IndexInfo, SchemaInfo, TableInfo,
};
use ra_metadata::diff::compare_plans;
use ra_metadata::explain::{
    parse_mysql_explain, parse_postgres_explain,
    parse_sqlite_explain, ExplainPlan, PlanNode,
};
use ra_metadata::postgres::{
    build_pg_schema, build_pg_table_stats, PgColumnRow,
    PgIndexStatsRow, PgStatsRow, PgTableRow, PgTableSizeRow,
};
use ra_metadata::mysql::{
    build_mysql_schema, build_mysql_table_stats,
    MySqlCardinalityRow, MySqlColumnRow, MySqlTableRow,
    MySqlTableSizeRow,
};
use ra_metadata::sqlite::{
    build_sqlite_schema, build_sqlite_table_stats, SqliteColumnRow,
    SqliteStat1Row, SqliteTableRow,
};

// ── Connection string parsing ───────────────────────────────

#[test]
fn parse_all_connection_types() {
    let cases = [
        (
            "postgresql://user:pass@localhost:5432/db",
            "PostgreSql",
        ),
        (
            "postgres://user:pass@localhost/db",
            "PostgreSql",
        ),
        (
            "mysql://user:pass@localhost:3306/db",
            "MySql",
        ),
        ("sqlite:///path/to/db.sqlite", "Sqlite"),
        ("sqlite://path/to/db.sqlite", "Sqlite"),
        ("/var/data/app.db", "Sqlite"),
        ("data.sqlite3", "Sqlite"),
    ];

    for (input, expected_type) in cases {
        let result = parse_connection_string(input);
        assert!(
            result.is_ok(),
            "failed to parse: {input}"
        );
        let target = result.expect("should parse");
        let actual_type = match target {
            ConnectionTarget::PostgreSql(_) => "PostgreSql",
            ConnectionTarget::MySql(_) => "MySql",
            ConnectionTarget::Sqlite(_) => "Sqlite",
        };
        assert_eq!(
            actual_type, expected_type,
            "wrong type for: {input}"
        );
    }
}

#[test]
fn reject_invalid_connection_strings() {
    let cases = [
        "ftp://server/db",
        "http://localhost/db",
        "random_string",
        "",
    ];

    for input in cases {
        let result = parse_connection_string(input);
        assert!(
            result.is_err(),
            "should reject: {input}"
        );
    }
}

// ── PostgreSQL schema building ──────────────────────────────

#[test]
fn pg_schema_with_full_metadata() {
    let tables = vec![
        PgTableRow {
            schemaname: "public".to_string(),
            tablename: "users".to_string(),
            n_live_tup: Some(10_000),
        },
        PgTableRow {
            schemaname: "public".to_string(),
            tablename: "orders".to_string(),
            n_live_tup: Some(50_000),
        },
    ];

    let mut columns = HashMap::new();
    columns.insert(
        "public.users".to_string(),
        vec![
            PgColumnRow {
                column_name: "id".to_string(),
                data_type: "integer".to_string(),
                nullable: false,
                default_value: Some(
                    "nextval('users_id_seq')".to_string(),
                ),
                ordinal_position: 1,
            },
            PgColumnRow {
                column_name: "email".to_string(),
                data_type: "character varying(255)".to_string(),
                nullable: false,
                default_value: None,
                ordinal_position: 2,
            },
            PgColumnRow {
                column_name: "name".to_string(),
                data_type: "text".to_string(),
                nullable: true,
                default_value: None,
                ordinal_position: 3,
            },
        ],
    );
    columns.insert(
        "public.orders".to_string(),
        vec![
            PgColumnRow {
                column_name: "id".to_string(),
                data_type: "integer".to_string(),
                nullable: false,
                default_value: None,
                ordinal_position: 1,
            },
            PgColumnRow {
                column_name: "user_id".to_string(),
                data_type: "integer".to_string(),
                nullable: false,
                default_value: None,
                ordinal_position: 2,
            },
            PgColumnRow {
                column_name: "total".to_string(),
                data_type: "numeric(10,2)".to_string(),
                nullable: false,
                default_value: None,
                ordinal_position: 3,
            },
        ],
    );

    let schema = build_pg_schema(
        "ecommerce",
        &tables,
        &columns,
        &HashMap::new(),
        &HashMap::new(),
        &[],
        &HashMap::new(),
    );

    assert_eq!(schema.database, "ecommerce");
    assert_eq!(schema.tables.len(), 2);
    assert_eq!(schema.tables[0].columns.len(), 3);
    assert_eq!(schema.tables[1].columns.len(), 3);
    assert_eq!(schema.tables[0].estimated_rows, Some(10_000));
}

#[test]
fn pg_table_stats_negative_ndistinct() {
    let size = PgTableSizeRow {
        total_size: 10_000_000,
        row_count: 100_000,
    };

    let stats = vec![
        PgStatsRow {
            attname: "id".to_string(),
            n_distinct: -1.0,
            null_frac: 0.0,
            avg_width: 4.0,
            correlation: Some(1.0),
        },
        PgStatsRow {
            attname: "status".to_string(),
            n_distinct: 5.0,
            null_frac: 0.02,
            avg_width: 8.0,
            correlation: Some(0.1),
        },
    ];

    let idx_stats = vec![PgIndexStatsRow {
        index_name: "users_pkey".to_string(),
        size_bytes: 500_000,
        scans: Some(10_000),
        tuples_read: Some(50_000),
        tuples_fetched: Some(45_000),
    }];

    let result = build_pg_table_stats(
        "users",
        &size,
        &stats,
        &idx_stats,
    );

    assert_eq!(result.row_count, 100_000);
    assert_eq!(result.total_size_bytes, 10_000_000);

    let id_col = result
        .columns
        .get("id")
        .expect("id column should exist");
    assert_eq!(id_col.distinct_count, 100_000);

    let status_col = result
        .columns
        .get("status")
        .expect("status column should exist");
    assert_eq!(status_col.distinct_count, 5);
}

// ── MySQL schema building ───────────────────────────────────

#[test]
fn mysql_schema_with_tables() {
    let tables = vec![MySqlTableRow {
        table_schema: "shop".to_string(),
        table_name: "products".to_string(),
        table_rows: Some(25_000),
    }];

    let mut columns = HashMap::new();
    columns.insert(
        "shop.products".to_string(),
        vec![
            MySqlColumnRow {
                column_name: "id".to_string(),
                data_type: "int".to_string(),
                is_nullable: "NO".to_string(),
                column_default: None,
                ordinal_position: 1,
            },
            MySqlColumnRow {
                column_name: "name".to_string(),
                data_type: "varchar".to_string(),
                is_nullable: "YES".to_string(),
                column_default: None,
                ordinal_position: 2,
            },
        ],
    );

    let schema = build_mysql_schema(
        "shop",
        &tables,
        &columns,
        &HashMap::new(),
        &HashMap::new(),
        &[],
        &HashMap::new(),
    );

    assert_eq!(schema.tables.len(), 1);
    assert_eq!(schema.tables[0].columns.len(), 2);
    assert!(!schema.tables[0].columns[0].nullable);
    assert!(schema.tables[0].columns[1].nullable);
}

#[test]
fn mysql_table_stats() {
    let size = MySqlTableSizeRow {
        row_count: 5000,
        total_size: 2_000_000,
    };

    let cardinality = vec![
        MySqlCardinalityRow {
            column_name: "id".to_string(),
            cardinality: Some(5000),
            nullable: "NO".to_string(),
        },
        MySqlCardinalityRow {
            column_name: "category".to_string(),
            cardinality: Some(50),
            nullable: "YES".to_string(),
        },
    ];

    let result = build_mysql_table_stats(
        "products",
        &size,
        &cardinality,
        &[],
    );

    assert_eq!(result.row_count, 5000);
    let id_col = result.columns.get("id").expect("id");
    assert_eq!(id_col.distinct_count, 5000);
    assert_eq!(id_col.null_fraction, 0.0);

    let cat_col =
        result.columns.get("category").expect("category");
    assert_eq!(cat_col.distinct_count, 50);
    assert_eq!(cat_col.null_fraction, 0.01);
}

// ── SQLite schema building ──────────────────────────────────

#[test]
fn sqlite_schema_with_pk_and_columns() {
    let tables = vec![SqliteTableRow {
        name: "logs".to_string(),
    }];

    let mut columns = HashMap::new();
    columns.insert(
        "logs".to_string(),
        vec![
            SqliteColumnRow {
                cid: 0,
                name: "id".to_string(),
                col_type: "INTEGER".to_string(),
                notnull: true,
                dflt_value: None,
                pk: true,
            },
            SqliteColumnRow {
                cid: 1,
                name: "message".to_string(),
                col_type: "TEXT".to_string(),
                notnull: false,
                dflt_value: None,
                pk: false,
            },
            SqliteColumnRow {
                cid: 2,
                name: "level".to_string(),
                col_type: "INTEGER".to_string(),
                notnull: true,
                dflt_value: Some("0".to_string()),
                pk: false,
            },
        ],
    );

    let schema = build_sqlite_schema(
        "logs.db",
        &tables,
        &columns,
        &HashMap::new(),
        &HashMap::new(),
        &[],
    );

    assert_eq!(schema.tables.len(), 1);
    let table = &schema.tables[0];
    assert_eq!(table.columns.len(), 3);
    assert_eq!(table.constraints.len(), 1);
    assert!(!table.columns[0].nullable);
    assert!(table.columns[1].nullable);
}

#[test]
fn sqlite_stat1_parsing() {
    let stat1 = vec![
        SqliteStat1Row {
            tbl: "events".to_string(),
            idx: Some("idx_timestamp".to_string()),
            stat: "1000000 10".to_string(),
        },
        SqliteStat1Row {
            tbl: "events".to_string(),
            idx: Some("idx_type_status".to_string()),
            stat: "1000000 5000 100".to_string(),
        },
    ];

    let result = build_sqlite_table_stats(
        "events",
        1_000_000,
        &stat1,
    );

    assert_eq!(result.row_count, 1_000_000);
    assert_eq!(result.indexes.len(), 2);
    assert!(result.indexes.contains_key("idx_timestamp"));
    assert!(
        result.indexes.contains_key("idx_type_status")
    );
}

// ── EXPLAIN parsing ─────────────────────────────────────────

#[test]
fn pg_explain_complex_join() {
    let json = r#"[{
        "Plan": {
            "Node Type": "Sort",
            "Sort Key": ["o.total DESC"],
            "Startup Cost": 150.0,
            "Total Cost": 155.0,
            "Plan Rows": 100,
            "Plan Width": 128,
            "Plans": [
                {
                    "Node Type": "Hash Join",
                    "Join Type": "Inner",
                    "Hash Cond": "(o.user_id = u.id)",
                    "Startup Cost": 50.0,
                    "Total Cost": 150.0,
                    "Plan Rows": 100,
                    "Plan Width": 128,
                    "Plans": [
                        {
                            "Node Type": "Seq Scan",
                            "Relation Name": "orders",
                            "Alias": "o",
                            "Filter": "(total > 100)",
                            "Startup Cost": 0.0,
                            "Total Cost": 100.0,
                            "Plan Rows": 500,
                            "Plan Width": 64
                        },
                        {
                            "Node Type": "Hash",
                            "Startup Cost": 30.0,
                            "Total Cost": 30.0,
                            "Plan Rows": 1000,
                            "Plan Width": 64,
                            "Plans": [
                                {
                                    "Node Type": "Seq Scan",
                                    "Relation Name": "users",
                                    "Alias": "u",
                                    "Startup Cost": 0.0,
                                    "Total Cost": 30.0,
                                    "Plan Rows": 1000,
                                    "Plan Width": 64
                                }
                            ]
                        }
                    ]
                }
            ]
        }
    }]"#;

    let plan = parse_postgres_explain(json, "SELECT * FROM orders o JOIN users u ON o.user_id = u.id WHERE o.total > 100 ORDER BY o.total DESC");
    let plan = plan.expect("should parse");

    assert_eq!(plan.root.node_type, "Sort");
    assert_eq!(plan.root.children.len(), 1);

    let join = &plan.root.children[0];
    assert_eq!(join.node_type, "Hash Join");
    assert_eq!(join.children.len(), 2);

    let types = plan.root.all_node_types();
    assert!(types.contains(&"Sort"));
    assert!(types.contains(&"Hash Join"));
    assert!(types.contains(&"Seq Scan"));
    assert!(types.contains(&"Hash"));
}

#[test]
fn mysql_explain_with_ordering() {
    let json = r#"{
        "query_block": {
            "select_id": 1,
            "cost_info": { "query_cost": "200.00" },
            "ordering_operation": {
                "using_filesort": true,
                "nested_loop": [
                    {
                        "table": {
                            "table_name": "orders",
                            "access_type": "ALL",
                            "rows_examined_per_scan": 1000,
                            "attached_condition": "orders.total > 100"
                        }
                    },
                    {
                        "table": {
                            "table_name": "users",
                            "access_type": "eq_ref",
                            "key": "PRIMARY",
                            "rows_examined_per_scan": 1
                        }
                    }
                ]
            }
        }
    }"#;

    let plan = parse_mysql_explain(
        json,
        "SELECT * FROM orders JOIN users ORDER BY total",
    )
    .expect("should parse");

    assert_eq!(plan.root.node_type, "Sort");
    assert_eq!(plan.root.children.len(), 1);
    assert_eq!(
        plan.root.children[0].node_type,
        "Nested Loop"
    );
}

#[test]
fn sqlite_explain_multi_table() {
    let text = "QUERY PLAN\n\
                |--SCAN orders\n\
                |--SEARCH users USING INDEX sqlite_autoindex_users_1 (id=?)\n\
                |--SEARCH products USING INDEX idx_product_id (id=?)\n\
                `--USE TEMP B-TREE FOR ORDER BY\n";

    let plan = parse_sqlite_explain(text, "complex query")
        .expect("should parse");

    assert_eq!(plan.root.node_type, "Query Plan");
    assert_eq!(plan.root.children.len(), 4);

    let types: Vec<&str> = plan
        .root
        .children
        .iter()
        .map(|c| c.node_type.as_str())
        .collect();
    assert_eq!(
        types,
        vec!["Seq Scan", "Index Scan", "Index Scan", "Sort"]
    );
}

// ── Differential validation ─────────────────────────────────

#[test]
fn diff_simple_scan_agreement() {
    let ra_plan = ra_core::algebra::RelExpr::scan("users");

    let mut explain_root = PlanNode::new("Seq Scan");
    explain_root.relation = Some("users".to_string());

    let explain = ExplainPlan {
        engine: "PostgreSQL".to_string(),
        query: "SELECT * FROM users".to_string(),
        root: explain_root,
        total_cost: Some(35.5),
        total_rows: Some(2550.0),
    };

    let report = compare_plans(&ra_plan, &explain);

    assert_eq!(report.engine, "PostgreSQL");
    assert!(!report.agreements.is_empty());
    assert!(report.confidence > 0.5);
}

#[test]
fn diff_table_mismatch() {
    let ra_plan = ra_core::algebra::RelExpr::scan("orders");

    let mut explain_root = PlanNode::new("Seq Scan");
    explain_root.relation = Some("users".to_string());

    let explain = ExplainPlan {
        engine: "MySQL".to_string(),
        query: "SELECT * FROM users".to_string(),
        root: explain_root,
        total_cost: None,
        total_rows: None,
    };

    let report = compare_plans(&ra_plan, &explain);

    assert!(!report.disagreements.is_empty());
    assert!(report.confidence < 0.9);
}

#[test]
fn diff_with_filter_agreement() {
    let ra_plan = ra_core::algebra::RelExpr::scan("users").filter(
        ra_core::expr::Expr::Const(ra_core::expr::Const::Bool(
            true,
        )),
    );

    let mut explain_root = PlanNode::new("Seq Scan");
    explain_root.relation = Some("users".to_string());
    explain_root.filter =
        Some("(active = true)".to_string());

    let explain = ExplainPlan {
        engine: "PostgreSQL".to_string(),
        query: "SELECT * FROM users WHERE active".to_string(),
        root: explain_root,
        total_cost: Some(20.0),
        total_rows: Some(100.0),
    };

    let report = compare_plans(&ra_plan, &explain);

    let filter_points: Vec<_> = report
        .agreements
        .iter()
        .filter(|p| {
            p.aspect
                == ra_metadata::diff::DiffAspect::FilterPlacement
        })
        .collect();

    assert_eq!(filter_points.len(), 1);
    assert!(filter_points[0].agrees);
}

// ── Schema serialization roundtrip ──────────────────────────

#[test]
fn schema_json_roundtrip() {
    let schema = SchemaInfo {
        database: "test".to_string(),
        tables: vec![TableInfo {
            schema: "public".to_string(),
            name: "users".to_string(),
            columns: vec![ColumnInfo {
                name: "id".to_string(),
                data_type: "integer".to_string(),
                nullable: false,
                default_value: None,
                ordinal_position: 1,
            }],
            constraints: vec![],
            indexes: vec![IndexInfo {
                name: "users_pkey".to_string(),
                columns: vec!["id".to_string()],
                unique: true,
                index_type: "btree".to_string(),
                primary: true,
            }],
            estimated_rows: Some(1000),
        }],
        views: vec![],
    };

    let json = serde_json::to_string_pretty(&schema)
        .expect("should serialize");
    let roundtrip: SchemaInfo =
        serde_json::from_str(&json)
            .expect("should deserialize");

    assert_eq!(schema, roundtrip);
    assert_eq!(roundtrip.tables[0].indexes[0].name, "users_pkey");
}

#[test]
fn gathered_stats_json_roundtrip() {
    let mut columns = HashMap::new();
    columns.insert(
        "email".to_string(),
        GatheredColumnStats {
            distinct_count: 9500,
            null_fraction: 0.0,
            avg_width: 32.0,
            correlation: Some(0.85),
            most_common_values: None,
        },
    );

    let mut indexes = HashMap::new();
    indexes.insert(
        "users_pkey".to_string(),
        GatheredIndexStats {
            size_bytes: 200_000,
            scans: Some(5000),
            tuples_read: Some(10_000),
            tuples_fetched: Some(9000),
        },
    );

    let stats = GatheredTableStats {
        table: "users".to_string(),
        row_count: 10_000,
        total_size_bytes: 5_000_000,
        columns,
        indexes,
    };

    let json = serde_json::to_string(&stats)
        .expect("should serialize");
    let roundtrip: GatheredTableStats =
        serde_json::from_str(&json)
            .expect("should deserialize");

    assert_eq!(stats, roundtrip);
}
