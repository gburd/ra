//! CLI entry point for the RA `PostgreSQL` Monitor.
//!
//! Provides a standalone tool to analyze `PostgreSQL` configurations,
//! simulate monitoring scenarios, and launch the TUI dashboard.

#![allow(clippy::print_stdout, clippy::print_stderr)]
#![allow(clippy::exit)]

use std::process;

use clap::{Parser, Subcommand};
use ra_pg_monitor::bloat_detector::{BloatDetector, TableBloatInput};
use ra_pg_monitor::config_checker::{ConfigChecker, PgConfig};
use ra_pg_monitor::query_monitor::{
    PlanNode, PlanNodeType, QueryMonitor, QueryRecord,
    QuerySeverity,
};
use ra_pg_monitor::recommendations::Advisor;
use ra_pg_monitor::schema_analyzer::{
    ColumnTypeInfo, ForeignKeyInfo, IndexUsage, SchemaAnalyzer,
    TableSchemaInfo,
};
use ra_pg_monitor::stats_staleness::{
    StalenessChecker, TableStatsInput,
};
use ra_pg_monitor::MonitorApp;

#[derive(Parser)]
#[command(
    name = "ra-pg-monitor",
    about = "PostgreSQL monitoring and advisory system",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check `PostgreSQL` configuration against best practices.
    CheckConfig {
        /// Total system RAM in GB.
        #[arg(long, default_value = "16")]
        ram_gb: u64,
        /// Number of CPU cores.
        #[arg(long, default_value = "8")]
        cpu_cores: u32,
        /// Storage is SSD.
        #[arg(long, default_value_t = true)]
        ssd: bool,
        /// `shared_buffers` in MB.
        #[arg(long)]
        shared_buffers_mb: Option<u64>,
        /// `effective_cache_size` in MB.
        #[arg(long)]
        effective_cache_size_mb: Option<u64>,
        /// `work_mem` in MB.
        #[arg(long)]
        work_mem_mb: Option<u64>,
        /// `random_page_cost`.
        #[arg(long)]
        random_page_cost: Option<f64>,
        /// `effective_io_concurrency`.
        #[arg(long)]
        effective_io_concurrency: Option<u32>,
        /// `max_parallel_workers_per_gather`.
        #[arg(long)]
        max_parallel_workers: Option<u32>,
    },
    /// Run a demo with sample data to show monitoring capabilities.
    Demo,
    /// Launch the interactive TUI dashboard with sample data.
    Tui,
    /// Analyze a configuration from JSON on stdin.
    Analyze,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::CheckConfig {
            ram_gb,
            cpu_cores,
            ssd,
            shared_buffers_mb,
            effective_cache_size_mb,
            work_mem_mb,
            random_page_cost,
            effective_io_concurrency,
            max_parallel_workers,
        } => {
            run_check_config(
                ram_gb,
                cpu_cores,
                ssd,
                shared_buffers_mb,
                effective_cache_size_mb,
                work_mem_mb,
                random_page_cost,
                effective_io_concurrency,
                max_parallel_workers,
            );
        }
        Command::Demo => run_demo(),
        Command::Tui => run_tui(),
        Command::Analyze => run_analyze(),
    }
}

const MB: u64 = 1024 * 1024;
const GB: u64 = 1024 * 1024 * 1024;

#[allow(clippy::too_many_arguments)]
fn run_check_config(
    ram_gb: u64,
    cpu_cores: u32,
    ssd: bool,
    shared_buffers_mb: Option<u64>,
    effective_cache_size_mb: Option<u64>,
    work_mem_mb: Option<u64>,
    random_page_cost: Option<f64>,
    effective_io_concurrency: Option<u32>,
    max_parallel_workers: Option<u32>,
) {
    let system_ram = ram_gb * GB;
    let config = PgConfig {
        shared_buffers: shared_buffers_mb
            .map_or(128 * MB, |v| v * MB),
        effective_cache_size: effective_cache_size_mb
            .map_or(4 * GB, |v| v * MB),
        work_mem: work_mem_mb.map_or(4 * MB, |v| v * MB),
        maintenance_work_mem: 64 * MB,
        random_page_cost: random_page_cost.unwrap_or(4.0),
        effective_io_concurrency: effective_io_concurrency
            .unwrap_or(1),
        default_statistics_target: 100,
        max_parallel_workers_per_gather: max_parallel_workers
            .unwrap_or(2),
        parallel_tuple_cost: 0.01,
        system_ram,
        cpu_cores,
        is_ssd: ssd,
    };

    let mut checker = ConfigChecker::new();
    checker.load_config(config);
    checker.analyze();

    let issues = checker.issues();
    if issues.is_empty() {
        println!("Configuration looks good for the given hardware.");
    } else {
        println!(
            "Found {} configuration issue(s):\n",
            issues.len()
        );
        for issue in issues {
            println!("  {issue}");
            println!("    Fix: {}\n", issue.suggestion);
        }
    }
}

fn run_demo() {
    let advisor = build_demo_advisor();
    let recs = advisor.all_recommendations();

    println!(
        "RA PostgreSQL Monitor - Demo Analysis\n\
         =====================================\n"
    );

    if recs.is_empty() {
        println!("No issues found.");
        return;
    }

    println!("Found {} recommendation(s):\n", recs.len());
    for rec in &recs {
        println!("  {rec}");
    }
    println!();

    println!("Query Monitor:");
    for q in advisor.query_monitor().slow_queries() {
        println!(
            "  [{severity}] {duration:.1}ms - {query}",
            severity = q.severity,
            duration = q.duration_ms,
            query = if q.query.len() > 60 {
                format!("{}...", &q.query[..57])
            } else {
                q.query.clone()
            },
        );
        if !q.suggestion.is_empty() {
            println!("    -> {}", q.suggestion);
        }
    }
    println!();

    println!("Schema Issues:");
    for issue in advisor.schema_analyzer().issues() {
        println!("  {issue}");
    }
    println!();

    println!("Bloat Findings:");
    for info in advisor.bloat_detector().findings() {
        println!("  {info}");
    }
    println!();

    println!("Statistics Staleness:");
    for info in advisor.staleness_checker().stale_tables() {
        println!("  {info}");
    }
}

fn run_tui() {
    let advisor = build_demo_advisor();
    let mut app = MonitorApp::new(advisor);
    if let Err(e) = app.run() {
        eprintln!("TUI error: {e}");
        process::exit(1);
    }
}

fn run_analyze() {
    let input = match std::io::read_to_string(std::io::stdin()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read stdin: {e}");
            process::exit(1);
        }
    };

    let config: PgConfig = match serde_json::from_str(&input) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Invalid JSON: {e}");
            eprintln!(
                "Expected PgConfig JSON. \
                 Example: {{\"shared_buffers\": 134217728, \
                 \"system_ram\": 17179869184, ...}}"
            );
            process::exit(1);
        }
    };

    let mut checker = ConfigChecker::new();
    checker.load_config(config);
    checker.analyze();

    let issues = checker.issues();
    if issues.is_empty() {
        println!("No configuration issues found.");
    } else {
        for issue in issues {
            println!("{issue}");
        }
    }
}

#[allow(clippy::too_many_lines)]
fn build_demo_advisor() -> Advisor {
    let mut query_monitor = QueryMonitor::new(100.0);

    query_monitor.record(QueryRecord {
        query: "SELECT * FROM orders WHERE amount > 1000"
            .to_string(),
        duration_ms: 2300.0,
        total_cost: 15000.0,
        root_plan: PlanNodeType::SeqScan,
        plan_nodes: vec![PlanNode {
            node_type: PlanNodeType::SeqScan,
            relation: Some("orders".to_string()),
            estimated_rows: 1_000_000.0,
            actual_rows: Some(50_000.0),
            startup_cost: 0.0,
            total_cost: 15000.0,
        }],
        rows_returned: 50_000,
        shared_hit: 200,
        shared_read: 800,
        severity: QuerySeverity::Normal,
        suggestion: String::new(),
        is_regression: false,
    });

    query_monitor.record(QueryRecord {
        query: "SELECT u.name, COUNT(o.id) FROM users u \
                JOIN orders o ON u.id = o.user_id \
                GROUP BY u.name"
            .to_string(),
        duration_ms: 450.0,
        total_cost: 8000.0,
        root_plan: PlanNodeType::HashJoin,
        plan_nodes: vec![
            PlanNode {
                node_type: PlanNodeType::SeqScan,
                relation: Some("users".to_string()),
                estimated_rows: 50_000.0,
                actual_rows: Some(50_000.0),
                startup_cost: 0.0,
                total_cost: 1000.0,
            },
            PlanNode {
                node_type: PlanNodeType::SeqScan,
                relation: Some("orders".to_string()),
                estimated_rows: 500_000.0,
                actual_rows: Some(500_000.0),
                startup_cost: 0.0,
                total_cost: 5000.0,
            },
        ],
        rows_returned: 45_000,
        shared_hit: 500,
        shared_read: 500,
        severity: QuerySeverity::Normal,
        suggestion: String::new(),
        is_regression: false,
    });

    query_monitor.record(QueryRecord {
        query: "SELECT 1".to_string(),
        duration_ms: 0.5,
        total_cost: 0.01,
        root_plan: PlanNodeType::Other,
        plan_nodes: vec![],
        rows_returned: 1,
        shared_hit: 1,
        shared_read: 0,
        severity: QuerySeverity::Normal,
        suggestion: String::new(),
        is_regression: false,
    });

    let mut schema_analyzer = SchemaAnalyzer::new();
    schema_analyzer.add_table(TableSchemaInfo {
        name: "orders".to_string(),
        columns: vec![
            ColumnTypeInfo {
                name: "id".to_string(),
                pg_type: "integer".to_string(),
                avg_width: 4,
            },
            ColumnTypeInfo {
                name: "amount".to_string(),
                pg_type: "numeric".to_string(),
                avg_width: 8,
            },
            ColumnTypeInfo {
                name: "user_id".to_string(),
                pg_type: "integer".to_string(),
                avg_width: 4,
            },
            ColumnTypeInfo {
                name: "payload".to_string(),
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
                size_bytes: 65536,
                is_unique: true,
                is_primary: true,
            },
            IndexUsage {
                name: "idx_orders_old".to_string(),
                table: "orders".to_string(),
                columns: vec!["amount".to_string()],
                index_type: "btree".to_string(),
                scans: 0,
                size_bytes: 32768,
                is_unique: false,
                is_primary: false,
            },
            IndexUsage {
                name: "idx_orders_payload".to_string(),
                table: "orders".to_string(),
                columns: vec!["payload".to_string()],
                index_type: "btree".to_string(),
                scans: 100,
                size_bytes: 131_072,
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
        seq_scan_count: 500,
        filtered_columns: vec!["user_id".to_string()],
        dead_tuples: 50_000,
        live_tuples: 1_000_000,
    });
    schema_analyzer.analyze();

    let mut config_checker = ConfigChecker::new();
    config_checker.load_config(PgConfig {
        shared_buffers: 128 * MB,
        effective_cache_size: 4 * GB,
        work_mem: 4 * MB,
        maintenance_work_mem: 64 * MB,
        random_page_cost: 4.0,
        effective_io_concurrency: 1,
        default_statistics_target: 100,
        max_parallel_workers_per_gather: 2,
        parallel_tuple_cost: 0.01,
        system_ram: 16 * GB,
        cpu_cores: 8,
        is_ssd: true,
    });
    config_checker.analyze();

    let mut bloat_detector = BloatDetector::new();
    bloat_detector.analyze_table(&TableBloatInput {
        table: "orders".to_string(),
        live_tuples: 1_000_000,
        dead_tuples: 200_000,
        last_autovacuum: None,
        index_bloat: vec![(
            "idx_orders_old".to_string(),
            30_000,
            20_000,
        )],
    });

    let mut staleness_checker = StalenessChecker::new();
    staleness_checker.analyze_table(&TableStatsInput {
        table: "orders".to_string(),
        live_tuples: 1_000_000,
        modifications_since_analyze: 350_000,
        last_analyze: Some(1_700_000_000),
        last_autoanalyze: Some(1_700_000_000),
        analyze_threshold: 50,
        analyze_scale_factor: 0.1,
    });

    Advisor::new(
        query_monitor,
        schema_analyzer,
        config_checker,
        bloat_detector,
        staleness_checker,
    )
}
