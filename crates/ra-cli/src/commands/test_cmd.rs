//! The `test` subcommand.

use anyhow::{bail, Result};
use colored::Colorize;

use crate::helpers::collect_rra_files;
use crate::output::print_header;
use crate::test_executor::{run_tests, FileResult, TestOutcome, TestResult};

pub fn cmd_test(path: &str, filter: Option<&str>, verbose: bool, quiet: bool) -> Result<()> {
    let files = collect_rra_files(path)?;

    if files.is_empty() {
        bail!("no .rra files found in {path}");
    }

    if !quiet {
        print_header(&format!("Running tests from {} file(s)...", files.len()));
    }

    let (results, summary) = run_tests(&files, filter, verbose)?;

    if !quiet {
        print_file_results(&summary.file_results, verbose);

        if verbose {
            print_individual_results(&results);
        }

        eprintln!();
        print_test_summary(&summary);

        if !summary.slowest.is_empty() && verbose {
            eprintln!();
            eprintln!("{}", "Slowest tests:".bold());
            for (name, dur) in &summary.slowest {
                eprintln!("  {:>6.0}ms  {}", dur.as_secs_f64() * 1000.0, name.dimmed(),);
            }
        }
    }

    if summary.failed > 0 {
        bail!("{} test(s) failed", summary.failed);
    }

    Ok(())
}

fn print_file_results(file_results: &[FileResult], verbose: bool) {
    for fr in file_results {
        if fr.passed == fr.total {
            if verbose {
                eprintln!(
                    "  {} {} ({}/{} passed)",
                    "[PASS]".green().bold(),
                    fr.display_path,
                    fr.passed,
                    fr.total,
                );
            }
        } else {
            eprintln!(
                "  {} {} ({}/{} passed)",
                "[FAIL]".red().bold(),
                fr.display_path,
                fr.passed,
                fr.total,
            );
            for (name, reason) in &fr.failures {
                eprintln!("        - {} {}", name, format!("({reason})").yellow(),);
            }
        }
    }
}

fn print_individual_results(results: &[TestResult]) {
    eprintln!();
    eprintln!("{}", "Individual results:".bold());
    for result in results {
        match &result.outcome {
            TestOutcome::Pass => {
                eprintln!(
                    "  {} {} ({}ms)",
                    "[PASS]".green().bold(),
                    result.name,
                    result.duration.as_millis(),
                );
            }
            TestOutcome::Fail { reason } => {
                eprintln!("  {} {}", "[FAIL]".red().bold(), result.name,);
                eprintln!("        {}", reason.yellow());
            }
            TestOutcome::Skip { reason } => {
                eprintln!(
                    "  {} {} ({})",
                    "[SKIP]".dimmed().bold(),
                    result.name,
                    reason.dimmed(),
                );
            }
            TestOutcome::Error { message } => {
                eprintln!(
                    "  {} {} ({})",
                    "[ERR]".red().bold(),
                    result.name,
                    message.red(),
                );
            }
        }
    }
}

fn print_test_summary(summary: &crate::test_executor::TestSummary) {
    let pass_rate = if summary.total > 0 {
        let rate = summary.passed as f64 / summary.total as f64 * 100.0;
        format!("{rate:.1}%")
    } else {
        "N/A".to_owned()
    };

    let status_line = format!(
        "Summary: {}/{} passed ({pass_rate})",
        summary.passed, summary.total,
    );

    if summary.failed == 0 && summary.errored == 0 {
        eprintln!("{}", status_line.green().bold());
    } else {
        eprintln!("{}", status_line.bold());
    }

    if summary.failed > 0 {
        eprintln!("  {}: {} tests", "Failed".red().bold(), summary.failed,);
    }
    if summary.skipped > 0 {
        eprintln!("  {}: {} tests", "Skipped".dimmed(), summary.skipped,);
    }
    if summary.errored > 0 {
        eprintln!("  {}: {} tests", "Errors".red(), summary.errored,);
    }
    eprintln!(
        "  {}: {:.1}s",
        "Duration".dimmed(),
        summary.duration.as_secs_f64(),
    );
}
