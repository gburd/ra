//! CLI commands for federated query analysis.
#![allow(clippy::print_stderr)]

use std::collections::HashMap;

use anyhow::{Context, Result};
use colored::Colorize;

use ra_core::federated::{
    format_bytes, DataSource, DatabaseType, FederatedCostBreakdown, FederatedPlan, FederatedQuery,
    RemoteConnection,
};
use ra_core::statistics::Statistics;
use ra_engine::federated_optimizer::FederatedOptimizer;
use ra_parser::sql_to_relexpr;

/// Run the `federated analyze` command.
pub fn cmd_federated_analyze(
    query_sql: &str,
    remote_db: &str,
    remote_table: &str,
    latency_ms: u64,
    bandwidth_mbps: u64,
    remote_rows: f64,
    avg_row_size: u64,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    let plan =
        sql_to_relexpr(query_sql).with_context(|| format!("failed to parse SQL: {query_sql}"))?;

    let db_type = parse_database_type(remote_db)?;
    let connection = RemoteConnection::new(db_type, remote_db, latency_ms, bandwidth_mbps);
    let capabilities = db_type.default_capabilities();

    let mut remote_stats = Statistics::new(remote_rows);
    remote_stats.avg_row_size = avg_row_size;
    {
        remote_stats.total_size = (remote_rows * avg_row_size as f64) as u64;
    }

    let mut sources = HashMap::new();
    sources.insert(
        remote_table.to_owned(),
        DataSource::remote(connection, remote_table, Some(remote_stats), capabilities),
    );

    // Detect local tables from the query plan
    let referenced = collect_table_names(&plan);
    for table_name in &referenced {
        if !sources.contains_key(table_name.as_str()) {
            sources.insert(
                table_name.clone(),
                DataSource::local(table_name.as_str(), Statistics::new(10_000.0)),
            );
        }
    }

    let federated_query = FederatedQuery::new(plan, sources);
    let optimizer = FederatedOptimizer::new();

    let analysis = optimizer
        .analyze(&federated_query)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if !quiet {
        print_analysis(&analysis.plan, query_sql, remote_db, verbose);
    }

    Ok(())
}

/// Print a federated analysis result.
fn print_analysis(plan: &FederatedPlan, query_sql: &str, remote_db: &str, verbose: bool) {
    eprintln!();
    eprintln!("{}", "Federated Query Analysis".bold());
    eprintln!();
    eprintln!("  {}:", "SQL".bold());
    crate::output::plans::print_formatted_sql(query_sql);
    eprintln!("  {}: {remote_db}", "Remote".bold());
    eprintln!();

    // Strategy
    let strategy_name = &plan.cost.strategy;
    let strategy_display = match strategy_name.as_str() {
        "ship_query" => "SHIP_QUERY",
        "ship_data_full" => "SHIP_DATA (full scan)",
        "ship_data_filtered" => "SHIP_DATA (with filter pushdown)",
        "hybrid" => "HYBRID (filter pushdown + local execution)",
        "local" => "LOCAL",
        other => other,
    };

    eprintln!("{}: {}", "Strategy".bold(), strategy_display.cyan().bold());

    // Execution steps
    for (i, step) in plan.steps.iter().enumerate() {
        eprintln!("  {}. {step}", i + 1);
    }
    eprintln!();

    // Cost breakdown
    print_cost_breakdown(&plan.cost);

    // Alternatives
    if !plan.alternatives.is_empty() {
        eprintln!();
        eprintln!("{}", "Alternatives:".bold());
        for alt in &plan.alternatives {
            let savings = plan.cost.savings_percent(alt.total_ms);
            eprintln!(
                "  {} {}: {:.0}ms (chosen is {:.1}% cheaper)",
                "-".dimmed(),
                alt.strategy,
                alt.total_ms,
                savings,
            );
            if verbose {
                eprintln!(
                    "    Transfer: {} ({} rows)",
                    format_bytes(alt.transfer_bytes),
                    alt.rows_transferred,
                );
            }
        }
    }

    // Best savings
    if let Some(best_alt) = plan.best_alternative() {
        let savings = plan.cost.savings_percent(best_alt.total_ms);
        if savings > 0.0 {
            eprintln!();
            eprintln!(
                "{}: {:.1}% vs next best ({})",
                "Savings".green().bold(),
                savings,
                best_alt.strategy,
            );
        }
    }
}

/// Print cost breakdown for a strategy.
fn print_cost_breakdown(cost: &FederatedCostBreakdown) {
    eprintln!("{}", "Cost breakdown:".bold());
    if cost.remote_exec_ms > 0.0 {
        eprintln!(
            "  {}: {:.0}ms",
            "Remote execution".bold(),
            cost.remote_exec_ms,
        );
    }
    if cost.network_transfer_ms > 0.0 {
        eprintln!(
            "  {}: {:.0}ms ({} @ {})",
            "Network transfer".bold(),
            cost.network_transfer_ms,
            format_bytes(cost.transfer_bytes),
            format!("{} rows", cost.rows_transferred).dimmed(),
        );
    }
    if cost.local_exec_ms > 0.0 {
        eprintln!(
            "  {}: {:.0}ms",
            "Local execution".bold(),
            cost.local_exec_ms,
        );
    }
    eprintln!("  {}: {:.0}ms", "Total".bold().underline(), cost.total_ms);
}

/// Parse a database type from a connection string or type name.
fn parse_database_type(s: &str) -> Result<DatabaseType> {
    let lower = s.to_lowercase();
    if lower.starts_with("postgres") || lower.contains("5432") {
        Ok(DatabaseType::PostgreSQL)
    } else if lower.starts_with("mysql") || lower.contains("3306") {
        Ok(DatabaseType::MySQL)
    } else if lower.contains("sqlite") {
        Ok(DatabaseType::SQLite)
    } else if lower.contains("snowflake") {
        Ok(DatabaseType::Snowflake)
    } else if lower.contains("bigquery") {
        Ok(DatabaseType::BigQuery)
    } else if lower.contains("spark") {
        Ok(DatabaseType::SparkSQL)
    } else if lower.contains("duckdb") {
        Ok(DatabaseType::DuckDB)
    } else {
        Ok(DatabaseType::GenericJdbc)
    }
}

/// Collect table names from a relational expression.
fn collect_table_names(expr: &ra_core::algebra::RelExpr) -> Vec<String> {
    let mut names = Vec::new();
    collect_tables_recursive(expr, &mut names);
    names.sort();
    names.dedup();
    names
}

fn collect_tables_recursive(expr: &ra_core::algebra::RelExpr, out: &mut Vec<String>) {
    match expr {
        ra_core::algebra::RelExpr::Scan { table, .. }
        | ra_core::algebra::RelExpr::IndexScan { table, .. }
        | ra_core::algebra::RelExpr::IndexOnlyScan { table, .. }
        | ra_core::algebra::RelExpr::BitmapIndexScan { table, .. }
        | ra_core::algebra::RelExpr::BitmapHeapScan { table, .. }
        | ra_core::algebra::RelExpr::ParallelScan { table, .. } => {
            out.push(table.clone());
        }
        ra_core::algebra::RelExpr::Filter { input, .. }
        | ra_core::algebra::RelExpr::Project { input, .. }
        | ra_core::algebra::RelExpr::Aggregate { input, .. }
        | ra_core::algebra::RelExpr::Sort { input, .. }
        | ra_core::algebra::RelExpr::IncrementalSort { input, .. }
        | ra_core::algebra::RelExpr::Limit { input, .. }
        | ra_core::algebra::RelExpr::Window { input, .. }
        | ra_core::algebra::RelExpr::Distinct { input, .. }
        | ra_core::algebra::RelExpr::ParallelAggregate { input, .. }
        | ra_core::algebra::RelExpr::Gather { input, .. }
        | ra_core::algebra::RelExpr::TopK { input, .. }
        | ra_core::algebra::RelExpr::VectorFilter { input, .. } => {
            collect_tables_recursive(input, out);
        }
        ra_core::algebra::RelExpr::Join { left, right, .. }
        | ra_core::algebra::RelExpr::Union { left, right, .. }
        | ra_core::algebra::RelExpr::Intersect { left, right, .. }
        | ra_core::algebra::RelExpr::Except { left, right, .. }
        | ra_core::algebra::RelExpr::ParallelHashJoin { left, right, .. } => {
            collect_tables_recursive(left, out);
            collect_tables_recursive(right, out);
        }
        ra_core::algebra::RelExpr::BitmapAnd { inputs }
        | ra_core::algebra::RelExpr::BitmapOr { inputs } => {
            for inp in inputs {
                collect_tables_recursive(inp, out);
            }
        }
        ra_core::algebra::RelExpr::CTE {
            definition, body, ..
        } => {
            collect_tables_recursive(definition, out);
            collect_tables_recursive(body, out);
        }
        ra_core::algebra::RelExpr::RecursiveCTE {
            base_case,
            recursive_case,
            body,
            ..
        } => {
            collect_tables_recursive(base_case, out);
            collect_tables_recursive(recursive_case, out);
            collect_tables_recursive(body, out);
        }
        ra_core::algebra::RelExpr::Values { .. }
        | ra_core::algebra::RelExpr::Unnest { .. }
        | ra_core::algebra::RelExpr::MultiUnnest { .. }
        | ra_core::algebra::RelExpr::TableFunction { .. }
        | ra_core::algebra::RelExpr::RowPattern { .. }
        | ra_core::algebra::RelExpr::MvScan { .. } => {}
    }
}
