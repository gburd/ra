//! The `monitor` subcommand.

use anyhow::{Context, Result};
use colored::Colorize;

pub fn cmd_monitor(tui: bool, demo: bool, format: &str, quiet: bool) -> Result<()> {
    use ra_pg_monitor::{
        Advisor, BloatDetector, ConfigChecker, MonitorApp, QueryMonitor, SchemaAnalyzer,
        StalenessChecker,
    };

    let mut query_monitor = QueryMonitor::new(100.0);
    let mut schema_analyzer = SchemaAnalyzer::new();
    let mut config_checker = ConfigChecker::new();
    let mut bloat_detector = BloatDetector::new();
    let mut staleness_checker = StalenessChecker::new();

    if demo {
        load_demo_data(
            &mut query_monitor,
            &mut schema_analyzer,
            &mut config_checker,
            &mut bloat_detector,
            &mut staleness_checker,
        );
    }

    let advisor = Advisor::new(
        query_monitor,
        schema_analyzer,
        config_checker,
        bloat_detector,
        staleness_checker,
    );

    if tui {
        let mut app = MonitorApp::new(advisor);
        app.run().context("TUI monitor failed")?;
        return Ok(());
    }

    let recs = advisor.all_recommendations();
    if recs.is_empty() {
        if !quiet {
            eprintln!("{}", "No issues found.".green().bold());
        }
        return Ok(());
    }

    match format {
        "json" => {
            let json = serde_json::to_string_pretty(&recs)
                .context("failed to serialize recommendations")?;
            eprintln!("{json}");
        }
        _ => {
            for rec in &recs {
                let severity_colored = match rec.severity {
                    ra_pg_monitor::Severity::Info => "INFO".cyan(),
                    ra_pg_monitor::Severity::Warning => "WARN".yellow(),
                    ra_pg_monitor::Severity::Error => "ERROR".red(),
                    ra_pg_monitor::Severity::Critical => "CRIT".red().bold(),
                };
                eprintln!(
                    "[{}] {} {}: {}",
                    severity_colored,
                    rec.category,
                    rec.target.bold(),
                    rec.message,
                );
                eprintln!("      {} {}", "Fix:".dimmed(), rec.suggestion,);
            }
            eprintln!();
            eprintln!("{}: {} recommendation(s)", "Total".bold(), recs.len(),);
        }
    }

    Ok(())
}

#[allow(clippy::too_many_lines)]
fn load_demo_data(
    query_monitor: &mut ra_pg_monitor::QueryMonitor,
    schema_analyzer: &mut ra_pg_monitor::SchemaAnalyzer,
    config_checker: &mut ra_pg_monitor::ConfigChecker,
    bloat_detector: &mut ra_pg_monitor::BloatDetector,
    staleness_checker: &mut ra_pg_monitor::StalenessChecker,
) {
    use ra_pg_monitor::bloat_detector::TableBloatInput;
    use ra_pg_monitor::config_checker::PgConfig;
    use ra_pg_monitor::query_monitor::{PlanNode, PlanNodeType, QueryRecord, QuerySeverity};
    use ra_pg_monitor::schema_analyzer::{
        ColumnTypeInfo, ForeignKeyInfo, IndexUsage, TableSchemaInfo,
    };
    use ra_pg_monitor::stats_staleness::TableStatsInput;

    query_monitor.record(QueryRecord {
        query: "SELECT * FROM orders WHERE status = 'pending'".to_string(),
        duration_ms: 2300.0,
        total_cost: 45000.0,
        root_plan: PlanNodeType::SeqScan,
        plan_nodes: vec![PlanNode {
            node_type: PlanNodeType::SeqScan,
            relation: Some("orders".to_string()),
            estimated_rows: 1_000_000.0,
            actual_rows: Some(50_000.0),
            startup_cost: 0.0,
            total_cost: 45000.0,
        }],
        rows_returned: 50_000,
        shared_hit: 1000,
        shared_read: 9000,
        severity: QuerySeverity::Normal,
        suggestion: String::new(),
        is_regression: false,
    });

    query_monitor.record(QueryRecord {
        query: "SELECT u.name, COUNT(o.id) FROM users u \
                JOIN orders o ON u.id = o.user_id \
                GROUP BY u.name"
            .to_string(),
        duration_ms: 850.0,
        total_cost: 12000.0,
        root_plan: PlanNodeType::HashJoin,
        plan_nodes: vec![
            PlanNode {
                node_type: PlanNodeType::HashJoin,
                relation: None,
                estimated_rows: 50_000.0,
                actual_rows: Some(45_000.0),
                startup_cost: 100.0,
                total_cost: 12000.0,
            },
            PlanNode {
                node_type: PlanNodeType::SeqScan,
                relation: Some("users".to_string()),
                estimated_rows: 100_000.0,
                actual_rows: None,
                startup_cost: 0.0,
                total_cost: 2000.0,
            },
        ],
        rows_returned: 45_000,
        shared_hit: 8000,
        shared_read: 2000,
        severity: QuerySeverity::Normal,
        suggestion: String::new(),
        is_regression: false,
    });

    let orders_table = TableSchemaInfo {
        name: "orders".to_string(),
        columns: vec![
            ColumnTypeInfo {
                name: "id".to_string(),
                pg_type: "integer".to_string(),
                avg_width: 4,
            },
            ColumnTypeInfo {
                name: "user_id".to_string(),
                pg_type: "integer".to_string(),
                avg_width: 4,
            },
            ColumnTypeInfo {
                name: "status".to_string(),
                pg_type: "text".to_string(),
                avg_width: 10,
            },
            ColumnTypeInfo {
                name: "metadata".to_string(),
                pg_type: "jsonb".to_string(),
                avg_width: 500,
            },
        ],
        indexes: vec![
            IndexUsage {
                name: "orders_pkey".to_string(),
                table: "orders".to_string(),
                columns: vec!["id".to_string()],
                index_type: "btree".to_string(),
                scans: 50_000,
                size_bytes: 2_097_152,
                is_unique: true,
                is_primary: true,
            },
            IndexUsage {
                name: "idx_orders_old".to_string(),
                table: "orders".to_string(),
                columns: vec!["status".to_string()],
                index_type: "btree".to_string(),
                scans: 0,
                size_bytes: 1_048_576,
                is_unique: false,
                is_primary: false,
            },
            IndexUsage {
                name: "idx_orders_metadata".to_string(),
                table: "orders".to_string(),
                columns: vec!["metadata".to_string()],
                index_type: "btree".to_string(),
                scans: 100,
                size_bytes: 4_194_304,
                is_unique: false,
                is_primary: false,
            },
        ],
        primary_key: vec!["id".to_string()],
        foreign_keys: vec![ForeignKeyInfo {
            name: "fk_orders_user".to_string(),
            columns: vec!["user_id".to_string()],
            referenced_table: "users".to_string(),
            referenced_columns: vec!["id".to_string()],
        }],
        seq_scan_count: 5000,
        filtered_columns: vec!["status".to_string(), "user_id".to_string()],
        dead_tuples: 200_000,
        live_tuples: 1_000_000,
    };
    schema_analyzer.add_table(orders_table);

    let logs_table = TableSchemaInfo {
        name: "audit_logs".to_string(),
        columns: vec![
            ColumnTypeInfo {
                name: "id".to_string(),
                pg_type: "integer".to_string(),
                avg_width: 4,
            },
            ColumnTypeInfo {
                name: "event".to_string(),
                pg_type: "text".to_string(),
                avg_width: 200,
            },
        ],
        indexes: vec![],
        primary_key: vec![],
        foreign_keys: vec![],
        seq_scan_count: 200,
        filtered_columns: vec![],
        dead_tuples: 0,
        live_tuples: 500_000,
    };
    schema_analyzer.add_table(logs_table);

    schema_analyzer.analyze();

    config_checker.load_config(PgConfig {
        shared_buffers: 128 * 1024 * 1024,
        effective_cache_size: 4 * 1024 * 1024 * 1024,
        work_mem: 4 * 1024 * 1024,
        maintenance_work_mem: 64 * 1024 * 1024,
        random_page_cost: 4.0,
        effective_io_concurrency: 1,
        default_statistics_target: 100,
        max_parallel_workers_per_gather: 2,
        parallel_tuple_cost: 0.01,
        system_ram: 32 * 1024 * 1024 * 1024,
        cpu_cores: 16,
        is_ssd: true,
    });
    config_checker.analyze();

    bloat_detector.analyze_table(&TableBloatInput {
        table: "orders".to_string(),
        live_tuples: 1_000_000,
        dead_tuples: 200_000,
        last_autovacuum: None,
        index_bloat: vec![("idx_orders_old".to_string(), 500_000, 300_000)],
    });

    staleness_checker.analyze_table(&TableStatsInput {
        table: "orders".to_string(),
        live_tuples: 1_000_000,
        modifications_since_analyze: 350_000,
        last_analyze: Some(1_700_000_000),
        last_autoanalyze: None,
        analyze_threshold: 50,
        analyze_scale_factor: 0.1,
    });
    staleness_checker.analyze_table(&TableStatsInput {
        table: "users".to_string(),
        live_tuples: 100_000,
        modifications_since_analyze: 5_000,
        last_analyze: Some(1_710_000_000),
        last_autoanalyze: Some(1_710_000_000),
        analyze_threshold: 50,
        analyze_scale_factor: 0.1,
    });
}
