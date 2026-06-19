//! Ra benchmark CLI.
//!
//! Runs the Ra optimizer against a corpus of SQL queries (hand-crafted
//! and/or fuzz-generated), measures parse + optimize time, optionally
//! compares plans against a live Postgres instance, and reports results.

// The workspace lint denies print_stdout; this binary intentionally uses it.
#![allow(clippy::print_stdout)]

mod compare;
mod report;
mod runner;
pub mod benchmark_harness;
pub mod job_benchmark;
pub mod report_generator;
pub mod statistical_analysis;
pub mod tproc_c;
pub mod training_collector;

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
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, clap::Subcommand)]
enum Commands {
    /// Run benchmark against corpus and/or fuzz-generated queries
    Bench(BenchArgs),
    /// Compare Ra planner vs PG planner on a live database
    Compare(CompareArgs),
    /// Collect training data from Postgres execution
    CollectTraining(CollectTrainingArgs),
    /// Generate comprehensive analysis report from saved benchmark results
    Analyze(AnalyzeArgs),
    /// Run JOB (Join Order Benchmark) offline against Ra optimizer
    BenchmarkJob(BenchmarkJobArgs),
    /// Train or fine-tune the neural cost model from collected data
    Train(TrainArgs),
    /// Run TPROC-C (TPC-C-like) OLTP queries offline against Ra optimizer
    BenchmarkOltp(BenchmarkOltpArgs),
}

#[derive(Debug, Parser)]
struct CompareArgs {
    /// Postgres connection string (required — must have ra_planner extension installed).
    #[arg(long)]
    db: String,

    /// Query source: corpus, fuzz, or both.
    #[arg(long, default_value = "corpus")]
    mode: Mode,

    /// Number of fuzz-generated queries (if mode=fuzz or both).
    #[arg(long, default_value_t = 100)]
    fuzz_count: usize,

    /// Write JSON comparison report to this path.
    #[arg(long, default_value = "comparison_results.json")]
    output: PathBuf,

    /// Number of repetitions per query for statistical stability.
    #[arg(long, default_value_t = 5)]
    repetitions: usize,

    /// Verify result correctness (compare result sets).
    #[arg(long)]
    verify: bool,
}

#[derive(Debug, Parser)]
struct AnalyzeArgs {
    /// Input BenchmarkReport JSON files (one or more).
    #[arg(required = true)]
    inputs: Vec<PathBuf>,

    /// Write Markdown report to this path.
    #[arg(long, default_value = "ra_analysis_report.md")]
    output: PathBuf,

    /// Also write JSON executive summary to this path.
    #[arg(long)]
    json: Option<PathBuf>,
}

#[derive(Debug, Parser)]
struct TrainArgs {
    /// Input training data JSON (produced by `collect-training`).
    #[arg(required = true)]
    input: PathBuf,

    /// Model output path (BitNet format).
    #[arg(long, default_value = "cost_model.bitnet.json")]
    model: PathBuf,

    /// Number of training epochs.
    #[arg(long, default_value_t = 10)]
    epochs: usize,

    /// Training batch size.
    #[arg(long, default_value_t = 64)]
    batch_size: usize,

    /// Fraction of data to hold out for evaluation (0.0–1.0).
    #[arg(long, default_value_t = 0.1)]
    eval_split: f64,
}

#[derive(Debug, Parser)]
struct BenchmarkOltpArgs {
    /// Postgres connection string for live execution timing (optional).
    #[arg(long)]
    db: Option<String>,

    /// Write JSON report to this path.
    #[arg(long, default_value = "oltp_benchmark_results.json")]
    output: PathBuf,

    /// Number of Ra optimizer runs per query.
    #[arg(long, default_value_t = 10)]
    repetitions: usize,
}

#[derive(Debug, Parser)]
struct BenchmarkJobArgs {
    /// Postgres connection string for live execution timing (optional).
    #[arg(long)]
    db: Option<String>,

    /// Write JSON report to this path.
    #[arg(long, default_value = "job_benchmark_results.json")]
    output: PathBuf,

    /// Number of Ra optimizer runs per query.
    #[arg(long, default_value_t = 10)]
    repetitions: usize,

    /// Only run queries with at most this many tables.
    #[arg(long, default_value_t = 20)]
    max_tables: usize,
}

#[derive(Debug, Parser)]
struct BenchArgs {
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

    /// Verify result correctness by comparing optimized query output
    /// against original query output on Postgres (requires --db).
    #[arg(long)]
    verify_results: bool,

    /// Override scoring weights as JSON (e.g. '{"structural":0.5}').
    #[arg(long)]
    weights: Option<String>,
}

#[derive(Debug, Parser)]
struct CollectTrainingArgs {
    /// Postgres connection string (required).
    #[arg(long)]
    db: String,

    /// Postgres configurations to test (comma-separated: default,high-memory,low-memory,all-in-memory).
    #[arg(long, default_value = "default,high-memory")]
    configs: String,

    /// Data sizes to test (comma-separated: tiny,small,medium,large).
    #[arg(long, default_value = "tiny,small")]
    sizes: String,

    /// Output file for training data (JSON).
    #[arg(long, default_value = "training_data.json")]
    output: PathBuf,

    /// Query source: corpus or both (corpus + fuzz).
    #[arg(long, default_value = "corpus")]
    mode: Mode,

    /// Number of fuzz-generated queries (if mode=both).
    #[arg(long, default_value_t = 100)]
    fuzz_count: usize,
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

    match cli.command {
        Commands::Bench(args) => run_bench(args),
        Commands::Compare(args) => run_compare(args),
        Commands::CollectTraining(args) => run_collect_training(args),
        Commands::Analyze(args) => run_analyze(args),
        Commands::BenchmarkJob(args) => run_benchmark_job(args),
        Commands::Train(args) => run_train(args),
        Commands::BenchmarkOltp(args) => run_benchmark_oltp(args),
    }
}

fn run_bench(args: BenchArgs) -> Result<()> {
    let weights = parse_weights(args.weights.as_deref())?;

    let config = RunnerConfig {
        weights,
        pg_connection: args.db.clone(),
        verify_results: args.verify_results,
    };

    let optimizer = Optimizer::new();

    // Collect queries as (category, sql)
    let mut queries: Vec<(String, String)> = Vec::new();

    if matches!(args.mode, Mode::Corpus | Mode::Both) {
        for entry in corpus::all_queries() {
            queries.push((entry.category.to_owned(), entry.sql.to_owned()));
        }
    }

    if matches!(args.mode, Mode::Fuzz | Mode::Both) {
        let emitter = SqlEmitter::new();
        let gen = SqlGenerator::with_config(GeneratorConfig {
            max_depth: args.fuzz_depth,
            ..GeneratorConfig::default()
        });
        use proptest::strategy::{Strategy, ValueTree};
        use proptest::test_runner::TestRunner;
        let mut runner = TestRunner::default();
        for _ in 0..args.fuzz_count {
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
        if !args.quiet && i % 50 == 0 {
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

    if !args.quiet {
        println!("\r  {total}/{total} done in {:.1}s", t_total.elapsed().as_secs_f64());
    }

    let report = BenchReport::from_results(&results);
    report.print_summary();

    if let Some(ref path) = args.output {
        report.write_json(path)?;
        if !args.quiet {
            println!("Report written to {}", path.display());
        }
    }

    if let Some(ref path) = args.failures {
        write_failures(path, &failures)?;
        if !args.quiet {
            println!("Failures ({}) written to {}", failures.len(), path.display());
        }
    } else if !failures.is_empty() && !args.quiet {
        println!("{} queries failed to parse.", failures.len());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// compare subcommand
// ---------------------------------------------------------------------------

#[cfg(feature = "live-comparison")]
fn run_compare(args: CompareArgs) -> Result<()> {
    println!("Ra vs PostgreSQL planner comparison");
    println!("Database: {}", args.db);
    println!("Mode: {:?}", args.mode);
    println!("Repetitions per query: {}", args.repetitions);
    println!();

    // Collect queries
    let mut queries: Vec<(String, String)> = Vec::new();

    if matches!(args.mode, Mode::Corpus | Mode::Both) {
        for entry in corpus::all_queries() {
            queries.push((entry.category.to_owned(), entry.sql.to_owned()));
        }
    }

    if matches!(args.mode, Mode::Fuzz | Mode::Both) {
        let emitter = SqlEmitter::new();
        let gen = SqlGenerator::with_config(GeneratorConfig {
            max_depth: 4,
            ..GeneratorConfig::default()
        });
        use proptest::strategy::{Strategy, ValueTree};
        use proptest::test_runner::TestRunner;
        let mut runner = TestRunner::default();
        for _ in 0..args.fuzz_count {
            if let Ok(tree) = gen.strategy().new_tree(&mut runner) {
                let sql = emitter.emit(&tree.current());
                queries.push(("fuzz".to_owned(), sql));
            }
        }
    }

    println!("Queries to compare: {}", queries.len());
    println!();

    let config = compare::CompareConfig {
        db_url: args.db,
        repetitions: args.repetitions,
        verify_results: args.verify,
    };

    let results = compare::run_comparison(&queries, &config)?;

    // Print summary
    compare::print_summary(&results);

    // Write JSON report
    let json = serde_json::to_string_pretty(&results)?;
    std::fs::write(&args.output, json)?;
    println!("\nReport written to {}", args.output.display());

    Ok(())
}

#[cfg(not(feature = "live-comparison"))]
fn run_compare(_args: CompareArgs) -> Result<()> {
    anyhow::bail!(
        "The 'compare' subcommand requires a live Postgres connection.\n\
         Build with: cargo build -p ra-bench --features live-comparison"
    );
}

#[cfg(feature = "live-comparison")]
fn run_collect_training(args: CollectTrainingArgs) -> Result<()> {
    use training_collector::{DataSize, PostgresConfig, TrainingCollector};

    println!("Collecting training data from Postgres execution...");
    println!("Database: {}", args.db);
    println!("Configs: {}", args.configs);
    println!("Sizes: {}", args.sizes);

    // Parse configurations
    let configs: Vec<PostgresConfig> = args
        .configs
        .split(',')
        .filter_map(|s| match s.trim() {
            "default" => Some(PostgresConfig::default()),
            "high-memory" => Some(PostgresConfig::high_memory()),
            "low-memory" => Some(PostgresConfig::low_memory()),
            "all-in-memory" => Some(PostgresConfig::all_in_memory()),
            _ => {
                eprintln!("Warning: Unknown config '{}', skipping", s);
                None
            }
        })
        .collect();

    // Parse data sizes
    let sizes: Vec<DataSize> = args
        .sizes
        .split(',')
        .filter_map(|s| match s.trim() {
            "tiny" => Some(DataSize::Tiny),
            "small" => Some(DataSize::Small),
            "medium" => Some(DataSize::Medium),
            "large" => Some(DataSize::Large),
            _ => {
                eprintln!("Warning: Unknown size '{}', skipping", s);
                None
            }
        })
        .collect();

    // Collect queries with features
    let mut queries_with_features: Vec<(String, ra_engine::cost_model::QueryFeatures)> = Vec::new();

    if matches!(args.mode, Mode::Corpus | Mode::Both) {
        for entry in corpus::all_queries() {
            // Parse SQL to get RelExpr
            let parsed = match ra_parser::lime_parser::parse_sql(&entry.sql) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Failed to parse {} query: {:?}", entry.category, e);
                    continue;
                }
            };

            // Extract real features from parsed RelExpr
            let features = ra_engine::cost_model::extract_features(&parsed);
            queries_with_features.push((entry.sql.to_owned(), features));
        }
    }

    if matches!(args.mode, Mode::Both) {
        let emitter = SqlEmitter::new();
        let gen = SqlGenerator::with_config(GeneratorConfig {
            max_depth: 4,
            ..GeneratorConfig::default()
        });
        use proptest::strategy::{Strategy, ValueTree};
        use proptest::test_runner::TestRunner;
        let mut runner = TestRunner::default();
        for _ in 0..args.fuzz_count {
            if let Ok(tree) = gen.strategy().new_tree(&mut runner) {
                let sql = emitter.emit(&tree.current());

                // Parse SQL to get RelExpr
                let parsed = match ra_parser::lime_parser::parse_sql(&sql) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("Failed to parse fuzz-generated query: {:?}", e);
                        continue;
                    }
                };

                // Extract real features from parsed RelExpr
                let features = ra_engine::cost_model::extract_features(&parsed);
                queries_with_features.push((sql, features));
            }
        }
    }

    println!("Total queries: {}", queries_with_features.len());
    println!("Expected samples: {} (queries × configs × sizes)",
        queries_with_features.len() * configs.len() * sizes.len());

    // Collect training samples
    let mut collector = TrainingCollector::new();
    collector.collect_tproc_h_samples(&queries_with_features, &configs, &sizes)?;

    // Save to file
    collector.save_to_file(args.output.to_str().unwrap())?;

    println!("Training data collection complete!");
    println!("Samples collected: {}", collector.samples().len());

    Ok(())
}

#[cfg(not(feature = "live-comparison"))]
fn run_collect_training(_args: CollectTrainingArgs) -> Result<()> {
    anyhow::bail!("Training data collection requires --features live-comparison");
}

// ---------------------------------------------------------------------------
// train subcommand
// ---------------------------------------------------------------------------

fn run_train(args: TrainArgs) -> Result<()> {
    use ra_engine::cost_model::{BitNetTrainer, TrainerConfig};
    use crate::training_collector::TrainingCollector;

    println!("Loading training data from: {}", args.input.display());
    let input_str = args.input.to_str()
        .ok_or_else(|| anyhow::anyhow!("input path is not valid UTF-8"))?;
    let samples = TrainingCollector::load_from_file(input_str)?;
    if samples.is_empty() {
        anyhow::bail!("No training samples found in {}", args.input.display());
    }
    println!("Loaded {} samples", samples.len());

    // Train/eval split
    let split = (samples.len() as f64 * (1.0 - args.eval_split)) as usize;
    let (train_samples, eval_samples) = samples.split_at(split.min(samples.len()));
    println!("Train: {} | Eval: {}", train_samples.len(), eval_samples.len());

    // Initialize trainer (QAT with STE — trains directly in ternary space)
    let mut trainer = BitNetTrainer::new(TrainerConfig {
        lr: 0.005,
        weight_decay: 0.001,
        ..Default::default()
    });

    // Compute normalization from training features
    let feature_arrays: Vec<[f32; 16]> = train_samples
        .iter()
        .map(|s| s.features.as_array())
        .collect();
    let mean = compute_feature_mean(&feature_arrays);
    let inv_std = compute_feature_inv_std(&feature_arrays, &mean);
    trainer.set_normalization(mean, inv_std);

    // Convert training data to (features, target) pairs
    let train_pairs: Vec<([f32; 16], [f32; 16])> = train_samples
        .iter()
        .map(|s| {
            let features = s.features.as_array();
            let target = actual_cost_to_array(&s.actual_cost);
            (features, target)
        })
        .collect();

    println!("\nTraining ({} epochs, {} samples):", args.epochs, train_pairs.len());
    println!("{:>6}  {:>12}", "Epoch", "Avg Loss");
    println!("{:>6}  {:>12}", "-----", "--------");

    for epoch in 0..args.epochs {
        trainer.reset_loss();
        trainer.train_batch(&train_pairs);
        let loss = trainer.avg_loss();
        println!("{:>6}  {:>12.4}", epoch + 1, loss);
    }

    // Export trained model
    let model = trainer.to_model();

    // Evaluate on held-out set
    if !eval_samples.is_empty() {
        let mut total_err = 0.0f64;
        for s in eval_samples {
            let pred = model.predict_cpu_ms(&s.features.as_array());
            let actual = s.actual_cost.cpu_time_ms;
            let err = ((pred - actual) as f64).abs() / (actual as f64 + 1.0);
            total_err += err;
        }
        let mape = total_err / eval_samples.len() as f64 * 100.0;
        println!("\nEval MAPE (CPU): {:.2}%", mape);
    }

    // Save model
    let model_path = args.model.to_str().unwrap_or("cost_model.bitnet.json");
    model.save_to_file(model_path)?;
    println!("\nModel saved to: {}", args.model.display());
    println!("  Steps trained: {}", trainer.steps());
    println!("  Model size:    {} bytes", model.model_size_bytes());

    Ok(())
}

fn compute_feature_mean(samples: &[[f32; 16]]) -> [f32; 16] {
    let n = samples.len() as f32;
    let mut mean = [0.0f32; 16];
    for s in samples {
        for (i, &x) in s.iter().enumerate() {
            mean[i] += x / n;
        }
    }
    mean
}

fn compute_feature_inv_std(samples: &[[f32; 16]], mean: &[f32; 16]) -> [f32; 16] {
    let n = samples.len() as f32;
    let mut var = [0.0f32; 16];
    for s in samples {
        for (i, &x) in s.iter().enumerate() {
            let d = x - mean[i];
            var[i] += d * d / n;
        }
    }
    let mut inv_std = [1.0f32; 16];
    for (i, &v) in var.iter().enumerate() {
        let std = v.sqrt();
        inv_std[i] = if std > 1e-6 { 1.0 / std } else { 1.0 };
    }
    inv_std
}

fn actual_cost_to_array(cost: &ra_engine::cost_model::ActualCost) -> [f32; 16] {
    [
        cost.cpu_time_ms,
        cost.memory_peak_mb,
        cost.memory_avg_mb,
        cost.io_storage_ops as f32,
        cost.io_storage_bytes as f32,
        cost.io_network_ops as f32,
        cost.io_network_bytes as f32,
        cost.locks_acquired as f32,
        cost.lock_hold_time_ms,
        cost.lock_contention_score,
        cost.vacuum_overhead,
        cost.wal_generation_bytes as f32,
        cost.replication_lag_ms,
        cost.cache_hit_ratio,
        cost.page_faults as f32,
        cost.context_switches as f32,
    ]
}

// ---------------------------------------------------------------------------
// benchmark-oltp subcommand
// ---------------------------------------------------------------------------

fn run_benchmark_oltp(args: BenchmarkOltpArgs) -> Result<()> {
    use crate::benchmark_harness::{BenchmarkHarness, WorkloadConfig};
    use crate::tproc_c::tproc_c_queries;

    let queries = tproc_c_queries();
    println!("TPROC-C Benchmark: {} queries", queries.len());

    let config = WorkloadConfig {
        ra_repetitions: args.repetitions,
        min_samples: args.repetitions.min(5),
        ..Default::default()
    };

    let mut harness = BenchmarkHarness::new(config);
    for (i, q) in queries.iter().enumerate() {
        print!("  [{:2}/{:2}] {} ... ", i + 1, queries.len(), q.id);
        let timing = harness.add_query(&format!("TPCC_{}", q.id), q.sql, &[]);
        if timing.ra_success_count > 0 {
            println!(
                "ok  parse={:.2}ms  opt={:.2}ms",
                timing.mean_parse_ms, timing.mean_optimize_ms,
            );
        } else {
            println!("PARSE FAILED");
        }
    }

    let report = harness.analyze("tproc_c");
    let successful = report.query_timings.iter().filter(|t| t.ra_success_count > 0).count();
    println!(
        "\nResults: {}/{} queries optimized",
        successful, queries.len()
    );

    let out = args.output.to_str().unwrap_or("oltp_benchmark_results.json");
    crate::benchmark_harness::BenchmarkHarness::save_report(&report, out)?;
    println!("Results written to: {out}");

    Ok(())
}

// ---------------------------------------------------------------------------
// analyze subcommand
// ---------------------------------------------------------------------------

fn run_analyze(args: AnalyzeArgs) -> Result<()> {
    use crate::report_generator::ReportGenerator;

    let mut gen = ReportGenerator::new();
    for path in &args.inputs {
        let path_str = path.to_str().unwrap_or_default();
        gen.add_report_file(path_str)?;
        println!("Loaded: {path_str}");
    }

    let summary = gen.executive_summary();
    println!("\nExecutive Summary");
    println!("=================");
    println!(
        "Overall improvement: {:.1}% (95% CI: [{:.1}%, {:.1}%])",
        summary.overall_improvement_pct, summary.ci_lower, summary.ci_upper,
    );
    println!(
        "Significantly improved: {:.1}% of queries",
        summary.pct_significantly_improved,
    );
    println!("Regressions: {}", summary.total_regressions);
    println!("Recommendation: {}", summary.recommendation.as_str());

    let out = args.output.to_str().unwrap_or("ra_analysis_report.md");
    gen.save_markdown(out)?;
    println!("\nMarkdown report written to: {out}");

    if let Some(json_path) = &args.json {
        let json_str = gen.executive_summary_json()?;
        std::fs::write(json_path, json_str)?;
        println!("JSON summary written to: {}", json_path.display());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// benchmark-job subcommand
// ---------------------------------------------------------------------------

fn run_benchmark_job(args: BenchmarkJobArgs) -> Result<()> {
    use crate::benchmark_harness::{BenchmarkHarness, WorkloadConfig};
    use crate::job_benchmark::job_queries;

    let queries = job_queries();
    let filtered: Vec<_> = queries.iter()
        .filter(|q| q.table_count <= args.max_tables)
        .collect();

    println!("JOB Benchmark: {} queries (max {} tables)", filtered.len(), args.max_tables);

    let config = WorkloadConfig {
        ra_repetitions: args.repetitions,
        min_samples: args.repetitions.min(5),
        ..Default::default()
    };

    let mut harness = BenchmarkHarness::new(config);
    for (i, q) in filtered.iter().enumerate() {
        print!("  [{:2}/{:2}] {} ({} tables) ... ",
            i + 1, filtered.len(), q.id, q.table_count);
        let timing = harness.add_query(
            &format!("JOB_{}", q.id),
            q.sql,
            &[], // no PG baseline in offline mode
        );
        if timing.ra_success_count > 0 {
            println!(
                "ok  parse={:.2}ms  opt={:.2}ms",
                timing.mean_parse_ms, timing.mean_optimize_ms,
            );
        } else {
            println!("PARSE FAILED");
        }
    }

    let report = harness.analyze("job_benchmark");
    let successful = report.query_timings.iter().filter(|t| t.ra_success_count > 0).count();
    println!(
        "\nResults: {}/{} queries optimized successfully",
        successful, filtered.len()
    );

    let out = args.output.to_str().unwrap_or("job_benchmark_results.json");
    BenchmarkHarness::save_report(&report, out)?;
    println!("Results written to: {out}");

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
