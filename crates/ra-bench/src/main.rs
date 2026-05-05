//! Ra benchmark CLI.
//!
//! Runs the Ra optimizer against a corpus of SQL queries (hand-crafted
//! and/or fuzz-generated), measures parse + optimize time, optionally
//! compares plans against a live Postgres instance, and reports results.

// The workspace lint denies print_stdout; this binary intentionally uses it.
#![allow(clippy::print_stdout)]

mod report;
mod runner;
mod training_collector;

use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use ra_engine::Optimizer;
use ra_grammar_fuzzer::{corpus, generator::SqlGenerator};
use ra_grammar_fuzzer::generator::GeneratorConfig;
use ra_grammar_fuzzer::scoring::ScoringWeights;
use ra_grammar_fuzzer::sql_emitter::SqlEmitter;
use runner::RunnerConfig;

use crate::report::BenchReport;
use crate::runner::{run_query, QueryResult};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
#[command(name = "ra-bench", about = "Ra SQL optimizer benchmark harness")]
struct Cli {
    /// Postgres connection string (enables live plan comparison).
    #[arg(long)]
    db: Option<String>,

    /// Query source: corpus, fuzz, or both.
    #[arg(long, default_value = "corpus")]
    mode: Mode,

    /// Number of fuzz-generated queries to run.
    #[arg(long, default_value_t = 1000)]
    fuzz_count: usize,

    /// Maximum RelExpr depth for fuzzer.
    #[arg(long, default_value_t = 4)]
    fuzz_depth: u32,

    /// Write JSON report to this path.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Write unparseable SQL to this file (one query per line).
    #[arg(long)]
    failures: Option<PathBuf>,

    /// Print only the summary table.
    #[arg(long)]
    quiet: bool,

    /// Override scoring weights as JSON (e.g. '{"structural":0.5}').
    #[arg(long)]
    weights: Option<String>,
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum Mode {
    Corpus,
    Fuzz,
    Both,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let cli = Cli::parse();
    let weights = parse_weights(cli.weights.as_deref())?;

    let config = RunnerConfig {
        weights,
        pg_connection: cli.db.clone(),
    };

    let optimizer = Optimizer::new();

    // Collect queries as (category, sql)
    let mut queries: Vec<(String, String)> = Vec::new();

    if matches!(cli.mode, Mode::Corpus | Mode::Both) {
        for entry in corpus::all_queries() {
            queries.push((entry.category.to_owned(), entry.sql.to_owned()));
        }
    }

    if matches!(cli.mode, Mode::Fuzz | Mode::Both) {
        let emitter = SqlEmitter::new();
        let gen = SqlGenerator::with_config(GeneratorConfig {
            max_depth: cli.fuzz_depth,
            ..GeneratorConfig::default()
        });
        use proptest::strategy::{Strategy, ValueTree};
        use proptest::test_runner::TestRunner;
        let mut runner = TestRunner::default();
        for _ in 0..cli.fuzz_count {
            if let Ok(tree) = gen.strategy().new_tree(&mut runner) {
                let sql = emitter.emit(&tree.current());
                queries.push(("fuzz".to_owned(), sql));
            }
        }
    }

    // Run benchmark
    let total = queries.len();
    let t_total = std::time::Instant::now();
    let mut results: Vec<QueryResult> = Vec::with_capacity(total);
    let mut failures: Vec<(String, String, String)> = Vec::new();

    for (i, (category, sql)) in queries.iter().enumerate() {
        if !cli.quiet && i % 50 == 0 {
            print!("\r  {i}/{total}");
            std::io::stdout().flush().ok();
        }

        let result = run_query(sql, category, &config, &optimizer);
        if !result.success {
            if let Some(ref e) = result.error {
                failures.push((category.clone(), sql.clone(), e.clone()));
            }
        }
        results.push(result);
    }

    if !cli.quiet {
        println!("\r  {total}/{total} done in {:.1}s", t_total.elapsed().as_secs_f64());
    }

    let report = BenchReport::from_results(&results);
    report.print_summary();

    if let Some(ref path) = cli.output {
        report.write_json(path)?;
        if !cli.quiet {
            println!("Report written to {}", path.display());
        }
    }

    if let Some(ref path) = cli.failures {
        write_failures(path, &failures)?;
        if !cli.quiet {
            println!("Failures ({}) written to {}", failures.len(), path.display());
        }
    } else if !failures.is_empty() && !cli.quiet {
        println!("{} queries failed to parse.", failures.len());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_weights(json: Option<&str>) -> Result<ScoringWeights> {
    let defaults = ScoringWeights::default();
    let Some(s) = json else { return Ok(defaults) };

    #[derive(serde::Deserialize, Default)]
    struct Partial {
        structural: Option<f64>,
        cost_accuracy: Option<f64>,
        execution: Option<f64>,
        speed: Option<f64>,
    }

    let partial: Partial = serde_json::from_str(s)?;
    Ok(ScoringWeights {
        structural: partial.structural.unwrap_or(defaults.structural),
        cost_accuracy: partial.cost_accuracy.unwrap_or(defaults.cost_accuracy),
        execution: partial.execution.unwrap_or(defaults.execution),
        speed: partial.speed.unwrap_or(defaults.speed),
    })
}

fn write_failures(
    path: &std::path::Path,
    failures: &[(String, String, String)],
) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    for (category, sql, err) in failures {
        writeln!(f, "-- [{category}] {err}")?;
        writeln!(f, "{sql};")?;
        writeln!(f)?;
    }
    Ok(())
}
