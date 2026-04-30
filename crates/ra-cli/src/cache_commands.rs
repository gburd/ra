//! CLI commands for plan cache management.

use std::collections::HashMap;

use anyhow::{Context, Result};
use colored::Colorize;

use ra_adaptive::cache_adapter::{AdaptiveCacheConfig, AdaptivePlanCache};
use ra_cache_impl::EvictionPolicy;
use ra_core::cost::StatisticsProvider;
use ra_core::statistics::Statistics;
use ra_engine::Optimizer;

/// In-memory statistics provider for CLI demonstration.
/// In production this would connect to a live catalog.
#[derive(Debug)]
struct CliStatsProvider {
    tables: HashMap<String, Statistics>,
}

impl StatisticsProvider for CliStatsProvider {
    fn get_statistics(&self, table: &str) -> Option<&Statistics> {
        self.tables.get(table)
    }
}

impl CliStatsProvider {
    fn empty() -> Self {
        Self {
            tables: HashMap::new(),
        }
    }
}

/// Show all cached plans.
pub fn cmd_cache_list(verbose: bool, quiet: bool) -> Result<()> {
    let cache = build_demo_cache()?;
    let entries = cache.list().context("listing cache")?;

    if entries.is_empty() {
        if !quiet {
            eprintln!("{}", "Cache is empty.".dimmed());
        }
        return Ok(());
    }

    if !quiet {
        eprintln!();
        eprintln!("{}", "Cached Plans:".bold());
        eprintln!();

        let sql_w = 50;
        eprintln!(
            "  {:<sql_w$}  {:>6}  {:>8}  {:>6}",
            "QUERY".bold(),
            "USES".bold(),
            "REOPT".bold(),
            "TABLES".bold(),
        );
        eprintln!(
            "  {:<sql_w$}  {:>6}  {:>8}  {:>6}",
            "-".repeat(sql_w),
            "-".repeat(6),
            "-".repeat(8),
            "-".repeat(6),
        );

        for (key, plan) in &entries {
            let sql_display = truncate(&key.sql, sql_w);
            let tables = plan.referenced_tables().join(", ");
            eprintln!(
                "  {:<sql_w$}  {:>6}  {:>8}  {}",
                sql_display.cyan(),
                plan.use_count,
                plan.reoptimization_count,
                if verbose {
                    tables
                } else {
                    truncate(&tables, 30).to_owned()
                },
            );
        }

        eprintln!();
        eprintln!("  {} entries cached", entries.len().to_string().bold(),);
    }

    Ok(())
}

/// Show cache performance statistics.
pub fn cmd_cache_stats(quiet: bool) -> Result<()> {
    let cache = build_demo_cache()?;
    let metrics = cache.metrics().context("reading metrics")?;

    if !quiet {
        eprintln!();
        eprintln!("{}", "Cache Statistics:".bold());
        eprintln!();
        eprintln!(
            "  {}: {} / {}",
            "Entries".bold(),
            metrics.current_entries,
            metrics.max_entries,
        );
        eprintln!(
            "  {}: {:.1}%",
            "Utilization".bold(),
            metrics.utilization() * 100.0,
        );
        eprintln!("  {}: {}", "Hits".bold(), metrics.hits,);
        eprintln!("  {}: {}", "Misses".bold(), metrics.misses,);
        eprintln!(
            "  {}: {:.1}%",
            "Hit Rate".bold(),
            metrics.hit_rate() * 100.0,
        );
        eprintln!("  {}: {}", "Evictions".bold(), metrics.evictions,);
        eprintln!("  {}: {}", "Clears".bold(), metrics.clears,);
    }

    Ok(())
}

/// Clear the cache, optionally scoped to a table.
pub fn cmd_cache_clear(table: Option<&str>, quiet: bool) -> Result<()> {
    let cache = build_demo_cache()?;

    if let Some(table_name) = table {
        let removed = cache
            .clear_table(table_name)
            .context("clearing table from cache")?;
        if !quiet {
            eprintln!(
                "Cleared {} entries referencing table '{}'",
                removed.to_string().bold(),
                table_name.cyan(),
            );
        }
    } else {
        cache.clear().context("clearing cache")?;
        if !quiet {
            eprintln!("{}", "Cache cleared.".green().bold());
        }
    }

    Ok(())
}

/// Reoptimize stale cached plans.
pub fn cmd_cache_reoptimize(threshold_pct: f64, quiet: bool) -> Result<()> {
    let cache = build_demo_cache()?;
    let optimizer = Optimizer::new();
    let stats = CliStatsProvider::empty();

    let threshold = threshold_pct / 100.0;
    let count = cache
        .reoptimize_with_threshold(&stats, &optimizer, threshold)
        .context("reoptimizing cache")?;

    if !quiet {
        if count == 0 {
            eprintln!("{}", "No stale plans found (all within threshold).".green());
        } else {
            eprintln!(
                "Reoptimized {} plan(s) with drift > {:.0}%",
                count.to_string().bold(),
                threshold_pct,
            );
        }
    }

    Ok(())
}

/// Show statistics drift for cached plans.
pub fn cmd_cache_drift(verbose: bool, quiet: bool) -> Result<()> {
    let cache = build_demo_cache()?;
    let stats = CliStatsProvider::empty();
    let report = cache.check_drift(&stats).context("checking drift")?;

    if !quiet {
        eprintln!();
        eprintln!("{}", "Statistics Drift Report:".bold());
        eprintln!();

        if report.stale_plans.is_empty() {
            eprintln!("  {}", "All cached plans are fresh.".green());
        } else {
            eprintln!(
                "  {} stale plan(s) detected:",
                report.stale_plans.len().to_string().yellow().bold(),
            );
            eprintln!();

            for (key, drift) in &report.stale_plans {
                let sql_display = truncate(&key.sql, 50);
                let status = match drift.status {
                    ra_cache_impl::DriftStatus::Fresh => "fresh".green().to_string(),
                    ra_cache_impl::DriftStatus::Stale => "STALE".red().bold().to_string(),
                    ra_cache_impl::DriftStatus::Unknown => "unknown".yellow().to_string(),
                };

                eprintln!(
                    "  [{status}] {} (max drift: {:.1}%)",
                    sql_display.cyan(),
                    drift.max_drift * 100.0,
                );

                if verbose {
                    for td in &drift.table_drifts {
                        let current_str = td
                            .current_row_count
                            .map_or_else(|| "N/A".to_owned(), |c| format!("{c:.0}"));
                        let drift_str = td
                            .drift_fraction
                            .map_or_else(|| "N/A".to_owned(), |d| format!("{:.1}%", d * 100.0));
                        eprintln!(
                            "    {}: cached={:.0}, \
                             current={}, drift={}",
                            td.table, td.cached_row_count, current_str, drift_str,
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

/// Build a demo cache with some pre-populated entries for
/// illustration. In a real system this would be a persistent
/// singleton.
fn build_demo_cache() -> Result<AdaptivePlanCache> {
    let config = AdaptiveCacheConfig {
        max_entries: 1024,
        eviction_policy: EvictionPolicy::Lru,
        drift_threshold: 0.2,
        ..Default::default()
    };
    let cache = AdaptivePlanCache::with_config(config);
    let optimizer = Optimizer::new();

    let mut tables = HashMap::new();
    tables.insert("users".to_owned(), Statistics::new(10_000.0));
    tables.insert("orders".to_owned(), Statistics::new(50_000.0));
    tables.insert("products".to_owned(), Statistics::new(5_000.0));
    let provider = CliStatsProvider { tables };

    let demo_queries = [
        "SELECT * FROM users WHERE id = 1",
        "SELECT * FROM orders WHERE user_id = 42",
        "SELECT * FROM products WHERE price > 100",
    ];

    for sql in &demo_queries {
        let _ = cache.get_or_optimize(sql, "auto", &provider, &optimizer);
    }

    // Simulate some cache hits
    for sql in &demo_queries[..2] {
        let _ = cache.get_or_optimize(sql, "auto", &provider, &optimizer);
    }

    Ok(cache)
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
