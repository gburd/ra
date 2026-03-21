//! Regression detection CLI commands.

use anyhow::{Context, Result, bail};
use colored::Colorize;
use datafusion::prelude::*;
use ra_core::{RelExpr, expr_to_sql};
use ra_engine::Optimizer;
use ra_hardware::HardwareProfile;
use ra_parser::sql_to_relexpr;
use ra_regression::{
    CostHistory, PlanFingerprint, QueryEntry, RegressionConfig, RegressionDetector,
    RegressionReport, RegressionSeverity, Storage, SqliteStorage, TomlStorage,
};
use std::path::{Path, PathBuf};

/// Establish a baseline for a query.
pub fn cmd_regression_baseline(
    query_file: &Path,
    query_id: Option<&str>,
    storage_type: &str,
    storage_path: &Path,
    hardware_profile: &str,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    // Read query from file
    let sql = std::fs::read_to_string(query_file)
        .with_context(|| format!("Failed to read query file: {}", query_file.display()))?;

    // Use filename as default query_id
    let query_id = query_id.unwrap_or_else(|| {
        query_file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
    });

    // Create storage backend
    let storage: Box<dyn Storage> = match storage_type {
        "sqlite" => Box::new(SqliteStorage::new(storage_path)),
        "toml" => Box::new(TomlStorage::new(storage_path)),
        _ => bail!("Unknown storage type: {}", storage_type),
    };

    // Parse and optimize query
    let ctx = SessionContext::new();
    let plan = ctx
        .sql(&sql)
        .await?
        .into_unoptimized_plan();

    // Convert to RelExpr
    let rel_expr = sql_to_relexpr(&sql)
        .map_err(|e| anyhow::anyhow!("Failed to parse SQL: {}", e))?;

    // Get hardware profile
    let hw_profile = HardwareProfile::from_preset(hardware_profile)
        .unwrap_or_else(|| HardwareProfile::auto_detect());

    // Optimize with cost estimation
    let optimizer = Optimizer::new(hw_profile);
    let (optimized, _egraph) = optimizer.optimize_with_egraph(rel_expr)?;

    // Get plan fingerprint
    let fingerprint = PlanFingerprint::from_plan(&plan);

    // TODO: Extract actual cost from egraph
    let cost = 0.0;

    // Create query entry
    let entry = QueryEntry::new(
        query_id.to_string(),
        sql.clone(),
        fingerprint.as_str().to_string(),
        cost,
    );

    // Store baseline
    storage.add_entry(entry.clone())?;

    if !quiet {
        println!("{}", "Baseline established:".green().bold());
        println!("  Query ID: {}", query_id);
        println!("  Plan hash: {}", fingerprint);
        println!("  Cost: {:.2}", cost);
        println!("  Stored in: {}", storage_path.display());
    }

    if verbose {
        println!("\n{}", "Query plan:".bold());
        println!("{:#?}", optimized);
    }

    Ok(())
}

/// Check for regressions in a query.
pub fn cmd_regression_check(
    query_file: &Path,
    query_id: Option<&str>,
    storage_type: &str,
    storage_path: &Path,
    hardware_profile: &str,
    warn_threshold: Option<f64>,
    error_threshold: Option<f64>,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    // Read query from file
    let sql = std::fs::read_to_string(query_file)
        .with_context(|| format!("Failed to read query file: {}", query_file.display()))?;

    // Use filename as default query_id
    let query_id = query_id.unwrap_or_else(|| {
        query_file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
    });

    // Create storage backend
    let storage: Box<dyn Storage> = match storage_type {
        "sqlite" => Box::new(SqliteStorage::new(storage_path)),
        "toml" => Box::new(TomlStorage::new(storage_path)),
        _ => bail!("Unknown storage type: {}", storage_type),
    };

    // Load history
    let history = storage.load()?;

    // Parse and optimize query
    let ctx = SessionContext::new();
    let plan = ctx
        .sql(&sql)
        .await?
        .into_unoptimized_plan();

    // Convert to RelExpr
    let rel_expr = sql_to_relexpr(&sql)
        .map_err(|e| anyhow::anyhow!("Failed to parse SQL: {}", e))?;

    // Get hardware profile
    let hw_profile = HardwareProfile::from_preset(hardware_profile)
        .unwrap_or_else(|| HardwareProfile::auto_detect());

    // Optimize with cost estimation
    let optimizer = Optimizer::new(hw_profile);
    let optimized = optimizer.optimize(rel_expr)?;

    // Get plan fingerprint
    let fingerprint = PlanFingerprint::from_plan(&plan);

    // Configure detector
    let mut config = RegressionConfig::default();
    if let Some(threshold) = warn_threshold {
        config.warn_threshold = threshold;
    }
    if let Some(threshold) = error_threshold {
        config.error_threshold = threshold;
    }

    // Detect regression
    let detector = RegressionDetector::with_config(config);
    let report = detector.detect(
        query_id,
        optimized.cost(),
        &fingerprint,
        &history,
    );

    // Store new entry
    let entry = QueryEntry::new(
        query_id.to_string(),
        sql.clone(),
        fingerprint.as_str().to_string(),
        optimized.cost(),
    );
    storage.add_entry(entry)?;

    // Display report
    if !quiet {
        match report.severity {
            RegressionSeverity::Error => {
                println!("{}", format!("{}", report).red().bold());
            }
            RegressionSeverity::Warning => {
                println!("{}", format!("{}", report).yellow().bold());
            }
            RegressionSeverity::Info => {
                println!("{}", format!("{}", report).blue());
            }
            RegressionSeverity::None => {
                println!("{}", format!("{}", report).green());
            }
        }
    }

    if verbose {
        println!("\n{}", "Details:".bold());
        println!("  Current cost: {:.2}", report.current_cost);
        if let Some(baseline) = report.baseline_cost {
            println!("  Baseline cost: {:.2}", baseline);
        }
        if let Some(avg) = report.historical_avg {
            println!("  Historical avg: {:.2}", avg);
        }
        if let Some(ratio) = report.cost_ratio {
            println!("  Cost ratio: {:.2}x", ratio);
        }
        if report.plan_changed {
            println!("  {} Plan structure changed", "⚠".yellow());
        }
    }

    // Exit with error code if regression detected
    if report.is_regression() && report.severity == RegressionSeverity::Error {
        std::process::exit(1);
    }

    Ok(())
}

/// Show regression report for all queries.
pub fn cmd_regression_report(
    storage_type: &str,
    storage_path: &Path,
    format: &str,
    only_regressions: bool,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    // Create storage backend
    let storage: Box<dyn Storage> = match storage_type {
        "sqlite" => Box::new(SqliteStorage::new(storage_path)),
        "toml" => Box::new(TomlStorage::new(storage_path)),
        _ => bail!("Unknown storage type: {}", storage_type),
    };

    // Load history
    let history = storage.load()?;

    // Create detector
    let detector = RegressionDetector::new();

    // Collect reports for all queries
    let mut reports = Vec::new();
    for query_id in history.query_ids() {
        if let Some(latest) = history.get_latest(&query_id) {
            // Create dummy fingerprint from stored hash
            let ctx = SessionContext::new();
            let plan = ctx
                .sql(&latest.sql)
                .await?
                .into_unoptimized_plan();
            let fingerprint = PlanFingerprint::from_plan(&plan);

            let report = detector.detect(
                &query_id,
                latest.cost,
                &fingerprint,
                &history,
            );

            if !only_regressions || report.is_regression() {
                reports.push(report);
            }
        }
    }

    // Sort by severity
    reports.sort_by_key(|r| std::cmp::Reverse(r.severity));

    // Display reports
    match format {
        "json" => {
            let json_reports: Vec<_> = reports
                .into_iter()
                .map(|r| {
                    serde_json::json!({
                        "query_id": r.query_id,
                        "severity": format!("{}", r.severity),
                        "current_cost": r.current_cost,
                        "baseline_cost": r.baseline_cost,
                        "cost_ratio": r.cost_ratio,
                        "plan_changed": r.plan_changed,
                        "description": r.description,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&json_reports)?);
        }
        _ => {
            if reports.is_empty() {
                if !quiet {
                    println!("{}", "No regressions detected.".green());
                }
            } else {
                if !quiet {
                    println!("{}", "Regression Report".bold().underline());
                    println!();
                }

                for report in reports {
                    match report.severity {
                        RegressionSeverity::Error => {
                            println!("{}", format!("{}", report).red().bold());
                        }
                        RegressionSeverity::Warning => {
                            println!("{}", format!("{}", report).yellow().bold());
                        }
                        RegressionSeverity::Info => {
                            println!("{}", format!("{}", report).blue());
                        }
                        RegressionSeverity::None => {
                            if verbose {
                                println!("{}", format!("{}", report).green());
                            }
                        }
                    }
                }

                if !quiet {
                    println!();
                    let error_count = reports
                        .iter()
                        .filter(|r| r.severity == RegressionSeverity::Error)
                        .count();
                    let warning_count = reports
                        .iter()
                        .filter(|r| r.severity == RegressionSeverity::Warning)
                        .count();

                    println!(
                        "Summary: {} errors, {} warnings",
                        error_count.to_string().red().bold(),
                        warning_count.to_string().yellow().bold()
                    );
                }
            }
        }
    }

    Ok(())
}