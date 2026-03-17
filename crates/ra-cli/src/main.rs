//! Command-line interface for the relational algebra rule system.
#![allow(clippy::print_stderr)]

mod display;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use colored::Colorize;

use ra_engine::Optimizer;
use ra_parser::{
    ParseError, RuleFile, parse_metadata, parse_rule_file, sql_to_relexpr,
    validate_metadata_all,
};

use display::format_plan_tree;

// ── CLI definition ──────────────────────────────────────────

#[derive(Parser)]
#[command(name = "ra-cli")]
#[command(
    about = "Relational Algebra Rule System CLI",
    long_about = None
)]
struct Cli {
    /// Increase output verbosity.
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Suppress all non-error output.
    #[arg(short, long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Validate .rra rule files.
    Validate {
        /// Path to a rule file or directory of rule files.
        path: String,
    },
    /// Run rule test cases.
    Test {
        /// Path to a rule file or directory of rule files.
        path: String,
    },
    /// List available rules.
    List {
        /// Path to the rules directory (defaults to ./rules).
        #[arg(short, long)]
        dir: Option<String>,
    },
    /// Show details for a specific rule.
    Show {
        /// Rule ID to look up.
        rule_id: String,
        /// Path to the rules directory (defaults to ./rules).
        #[arg(short, long)]
        dir: Option<String>,
    },
    /// Explain a SQL query's relational algebra plan.
    Explain {
        /// SQL query to explain.
        query: String,
        /// Hardware profile for cost estimation (edge, mobile, laptop, desktop, server, gpu-server, auto).
        #[arg(long, default_value = "auto")]
        hardware_profile: String,
    },
    /// Optimize a SQL query using rewrite rules.
    Optimize {
        /// SQL query to optimize.
        query: String,
        /// Hardware profile for cost estimation (edge, mobile, laptop, desktop, server, gpu-server, auto).
        #[arg(long, default_value = "auto")]
        hardware_profile: String,
    },
}

// ── Main ────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();

    let filter = if cli.verbose {
        "debug"
    } else if cli.quiet {
        "error"
    } else {
        "info"
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .init();

    match cli.command {
        Commands::Validate { path } => {
            cmd_validate(&path, cli.verbose, cli.quiet)
        }
        Commands::Test { path } => {
            cmd_test(&path, cli.quiet)
        }
        Commands::List { dir } => {
            let dir = dir.as_deref().unwrap_or("rules");
            cmd_list(dir, cli.quiet)
        }
        Commands::Show { rule_id, dir } => {
            let dir = dir.as_deref().unwrap_or("rules");
            cmd_show(&rule_id, dir)
        }
        Commands::Explain { query, hardware_profile } => {
            cmd_explain(&query, &hardware_profile, cli.verbose, cli.quiet)
        }
        Commands::Optimize { query, hardware_profile } => {
            cmd_optimize(&query, &hardware_profile, cli.verbose, cli.quiet)
        }
    }
}

// ── validate ────────────────────────────────────────────────

fn cmd_validate(
    path: &str,
    verbose: bool,
    quiet: bool,
) -> Result<()> {
    let files = collect_rra_files(path)?;

    if files.is_empty() {
        bail!("no .rra files found in {path}");
    }

    if !quiet {
        print_header(&format!(
            "Validating {} file(s)",
            files.len()
        ));
    }

    let mut pass = 0u32;
    let mut fail = 0u32;

    for file in &files {
        let source = std::fs::read_to_string(file)
            .with_context(|| {
                format!("reading {}", file.display())
            })?;

        match parse_rule_file(&source) {
            Ok(rule) => {
                let extra_errors =
                    validate_metadata_all(&rule.metadata);
                if extra_errors.is_empty() {
                    pass += 1;
                    if verbose {
                        print_status(
                            "PASS",
                            &file.display().to_string(),
                            true,
                        );
                    }
                } else {
                    fail += 1;
                    print_status(
                        "FAIL",
                        &file.display().to_string(),
                        false,
                    );
                    for err in &extra_errors {
                        print_detail(&format!("  {err}"));
                    }
                }
            }
            Err(e) => {
                fail += 1;
                print_status(
                    "FAIL",
                    &file.display().to_string(),
                    false,
                );
                print_parse_error(&e, file);
            }
        }
    }

    if !quiet {
        print_summary(pass, fail);
    }

    if fail > 0 {
        bail!("{fail} file(s) failed validation");
    }

    Ok(())
}

// ── test ────────────────────────────────────────────────────

fn cmd_test(path: &str, quiet: bool) -> Result<()> {
    let files = collect_rra_files(path)?;

    if files.is_empty() {
        bail!("no .rra files found in {path}");
    }

    if !quiet {
        print_header(&format!(
            "Scanning {} file(s) for test cases",
            files.len()
        ));
    }

    let mut total_tests = 0usize;

    for file in &files {
        let source = std::fs::read_to_string(file)
            .with_context(|| {
                format!("reading {}", file.display())
            })?;

        match parse_rule_file(&source) {
            Ok(rule) => {
                let count = rule.test_cases.len();
                total_tests += count;
                if !quiet && count > 0 {
                    print_status(
                        &format!("{count} test(s)"),
                        &format!(
                            "{} ({})",
                            rule.metadata.id,
                            file.display()
                        ),
                        true,
                    );
                }
            }
            Err(e) => {
                print_status(
                    "SKIP",
                    &file.display().to_string(),
                    false,
                );
                print_parse_error(&e, file);
            }
        }
    }

    if !quiet {
        let msg = format!(
            "\nFound {total_tests} test case(s) across {} file(s).",
            files.len()
        );
        eprintln!("{}", msg.bold());
        eprintln!(
            "{}",
            "(test execution not yet implemented)".dimmed()
        );
    }

    Ok(())
}

// ── list ────────────────────────────────────────────────────

fn cmd_list(dir: &str, quiet: bool) -> Result<()> {
    let rules_dir = Path::new(dir);
    if !rules_dir.is_dir() {
        bail!(
            "rules directory not found: {dir}\n\
             hint: pass --dir <path> or run from the repo root"
        );
    }

    let files = collect_rra_files(dir)?;

    if files.is_empty() {
        if !quiet {
            eprintln!("{}", "No .rra files found.".dimmed());
        }
        return Ok(());
    }

    let mut entries: Vec<(String, String, String, PathBuf)> =
        Vec::new();

    for file in &files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        let Ok(meta) = parse_metadata(&source) else {
            continue;
        };
        entries.push((
            meta.id,
            meta.name,
            meta.category,
            file.clone(),
        ));
    }

    entries.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)));

    if !quiet {
        print_header(&format!("{} rule(s) found", entries.len()));
    }

    let id_w = entries
        .iter()
        .map(|e| e.0.len())
        .max()
        .unwrap_or(2)
        .max(2);
    let name_w = entries
        .iter()
        .map(|e| e.1.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let cat_w = entries
        .iter()
        .map(|e| e.2.len())
        .max()
        .unwrap_or(8)
        .max(8);

    eprintln!(
        "  {:<id_w$}  {:<name_w$}  {:<cat_w$}",
        "ID".bold(),
        "NAME".bold(),
        "CATEGORY".bold(),
    );
    eprintln!(
        "  {:<id_w$}  {:<name_w$}  {:<cat_w$}",
        "-".repeat(id_w),
        "-".repeat(name_w),
        "-".repeat(cat_w),
    );

    for (id, name, category, _path) in &entries {
        eprintln!(
            "  {:<id_w$}  {:<name_w$}  {:<cat_w$}",
            id.cyan(),
            name,
            category.dimmed(),
        );
    }

    Ok(())
}

// ── show ────────────────────────────────────────────────────

fn cmd_show(rule_id: &str, dir: &str) -> Result<()> {
    let files = collect_rra_files(dir)?;

    let Some((rule, path)) = find_rule_by_id(rule_id, &files)
    else {
        bail!(
            "rule '{rule_id}' not found in {dir}\n\
             hint: run 'ra-cli list' to see available rules"
        );
    };

    eprintln!(
        "{}",
        format!("Rule: {}", rule.metadata.id).bold()
    );
    eprintln!("  {}: {}", "Name".bold(), rule.metadata.name);
    eprintln!(
        "  {}: {}",
        "Category".bold(),
        rule.metadata.category
    );
    eprintln!(
        "  {}: {}",
        "Version".bold(),
        rule.metadata.version
    );
    eprintln!("  {}: {}", "File".bold(), path.display());

    if !rule.metadata.databases.is_empty() {
        eprintln!(
            "  {}: {}",
            "Databases".bold(),
            rule.metadata.databases.join(", ")
        );
    }
    if !rule.metadata.authors.is_empty() {
        eprintln!(
            "  {}: {}",
            "Authors".bold(),
            rule.metadata.authors.join(", ")
        );
    }
    if !rule.metadata.tags.is_empty() {
        eprintln!(
            "  {}: {}",
            "Tags".bold(),
            rule.metadata.tags.join(", ")
        );
    }
    if let Some(ref std) = rule.metadata.standard {
        eprintln!("  {}: {std}", "Standard".bold());
    }

    if !rule.description.is_empty() {
        eprintln!();
        eprintln!("{}", "Description:".bold());
        for line in rule.description.lines() {
            eprintln!("  {line}");
        }
    }

    if let Some(ref alg) = rule.algebra_notation {
        eprintln!();
        eprintln!("{}", "Relational Algebra:".bold());
        for line in alg.lines() {
            eprintln!("  {}", line.green());
        }
    }

    if let Some(ref impl_code) = rule.implementation {
        eprintln!();
        eprintln!("{}", "Implementation:".bold());
        for line in impl_code.lines() {
            eprintln!("  {line}");
        }
    }

    if !rule.test_cases.is_empty() {
        eprintln!();
        eprintln!(
            "{}",
            format!(
                "Test Cases: {} block(s)",
                rule.test_cases.len()
            )
            .bold()
        );
    }

    if !rule.references.is_empty() {
        eprintln!();
        eprintln!("{}", "References:".bold());
        for r in &rule.references {
            eprintln!("  - {r}");
        }
    }

    Ok(())
}

// ── explain ─────────────────────────────────────────────────

fn cmd_explain(query: &str, hardware_profile_name: &str, verbose: bool, quiet: bool) -> Result<()> {
    let plan = sql_to_relexpr(query)
        .with_context(|| format!("failed to parse SQL: {query}"))?;

    let hardware = load_hardware_profile(hardware_profile_name)?;

    if !quiet {
        print_header("Query Plan Explanation");
        eprintln!("  {}: {query}", "SQL".bold());

        if verbose {
            eprintln!("  {}: {} ({} cores, {} MB L3 cache, {}-bit SIMD)",
                "Hardware".bold(),
                hardware.name,
                hardware.cpu_cores,
                hardware.l3_cache_bytes / (1024 * 1024),
                hardware.simd_width_bits
            );
        }

        eprintln!();
        eprintln!("{}", "Plan:".bold());
        eprintln!("{}", format_plan_tree(&plan));
    }

    Ok(())
}

// ── optimize ────────────────────────────────────────────────

fn cmd_optimize(query: &str, hardware_profile_name: &str, verbose: bool, quiet: bool) -> Result<()> {
    let plan = sql_to_relexpr(query)
        .with_context(|| format!("failed to parse SQL: {query}"))?;

    let hardware = load_hardware_profile(hardware_profile_name)?;

    let mut optimizer = Optimizer::new();
    optimizer.set_hardware_profile(hardware.clone());

    let optimized = optimizer
        .optimize(&plan)
        .with_context(|| format!("failed to optimize query: {query}"))?;

    if !quiet {
        print_header("Query Optimization");
        eprintln!("  {}: {query}", "SQL".bold());

        if verbose {
            eprintln!("  {}: {} ({} cores, {} MB L3 cache, {}-bit SIMD)",
                "Hardware".bold(),
                hardware.name,
                hardware.cpu_cores,
                hardware.l3_cache_bytes / (1024 * 1024),
                hardware.simd_width_bits
            );
        }

        eprintln!();

        eprintln!("{}", "Original Plan:".bold());
        eprintln!("{}", format_plan_tree(&plan));
        eprintln!();

        eprintln!("{}", "Optimized Plan:".bold());
        eprintln!("{}", format_plan_tree(&optimized));
    }

    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────

/// Load a hardware profile by name.
fn load_hardware_profile(name: &str) -> Result<ra_hardware::HardwareProfile> {
    let profile = match name.to_lowercase().as_str() {
        "auto" => ra_hardware::detect_hardware(),
        "cpu-only" => ra_hardware::HardwareProfile::cpu_only(),
        "gpu-server" => ra_hardware::HardwareProfile::gpu_server(),
        "fpga" => ra_hardware::HardwareProfile::fpga_appliance(),
        _ => bail!("unknown hardware profile: {name}. Valid options: auto, cpu-only, gpu-server, fpga"),
    };

    Ok(profile)
}

/// Collect all `.rra` files under a path (file or directory).
fn collect_rra_files(path: &str) -> Result<Vec<PathBuf>> {
    let p = Path::new(path);
    if p.is_file() {
        return Ok(vec![p.to_path_buf()]);
    }
    if !p.is_dir() {
        bail!("path not found: {path}");
    }
    let mut files = Vec::new();
    walk_dir(p, &mut files)?;
    files.sort();
    Ok(files)
}

/// Recursively walk a directory for `.rra` files.
fn walk_dir(
    dir: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, out)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext == "rra")
        {
            out.push(path);
        }
    }
    Ok(())
}

/// Search for a rule by ID across a set of files.
fn find_rule_by_id(
    rule_id: &str,
    files: &[PathBuf],
) -> Option<(RuleFile, PathBuf)> {
    for file in files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        if let Ok(rule) = parse_rule_file(&source) {
            if rule.metadata.id == rule_id {
                return Some((rule, file.clone()));
            }
        }
    }
    None
}

// ── Output formatting ───────────────────────────────────────

fn print_header(msg: &str) {
    eprintln!();
    eprintln!("{}", msg.bold());
    eprintln!();
}

fn print_status(label: &str, detail: &str, ok: bool) {
    if ok {
        eprintln!(
            "  {} {detail}",
            format!("[{label}]").green().bold(),
        );
    } else {
        eprintln!(
            "  {} {detail}",
            format!("[{label}]").red().bold(),
        );
    }
}

fn print_detail(msg: &str) {
    eprintln!("        {}", msg.yellow());
}

fn print_parse_error(err: &ParseError, path: &Path) {
    match err {
        ParseError::MissingFrontmatter => {
            print_detail(&format!(
                "{}: missing YAML frontmatter (---)",
                path.display()
            ));
        }
        ParseError::InvalidYaml { line, source } => {
            print_detail(&format!(
                "{}:{line}: {source}",
                path.display()
            ));
        }
        ParseError::Validation(v) => {
            print_detail(&format!(
                "{}: {v}",
                path.display()
            ));
        }
        ParseError::Other(msg) => {
            print_detail(&format!(
                "{}: {msg}",
                path.display()
            ));
        }
    }
}

fn print_summary(pass: u32, fail: u32) {
    eprintln!();
    let total = pass + fail;
    if fail == 0 {
        eprintln!(
            "{}",
            format!("All {total} file(s) passed validation.")
                .green()
                .bold()
        );
    } else {
        eprintln!(
            "{}: {pass} passed, {fail} failed out of {total}",
            "Summary".bold(),
        );
    }
}
