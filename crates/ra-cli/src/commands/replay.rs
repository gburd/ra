//! The `replay` subcommand and the shared optimization step-view renderer.
//!
//! RA-STEERING §7.2 debugger command. `ra replay <trace.json>` loads an
//! `OptimizationTrace` previously captured by `ra optimize --emit-trace`,
//! re-parses and re-optimizes the stored SQL deterministically, prints the
//! same per-iteration step view used by `ra optimize --step`, and reports
//! whether the replay reproduces the stored trace (same iteration count and
//! final cost). A divergence surfaces non-determinism in the optimizer.
//!
//! The step-view renderer ([`render_step_view`]) is shared with the
//! `optimize --step` path so both show an identical table.

use std::path::Path;

use anyhow::{Context, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};

use ra_engine::{OptimizationTrace, Optimizer};
use ra_parser::sql_to_relexpr;

/// On-disk shape written by `ra optimize --emit-trace` and read by
/// `ra replay`: the source SQL plus the captured optimization trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceCapture {
    /// The SQL query that produced the trace.
    pub sql: String,
    /// The captured optimization trace (per-iteration cost history, etc.).
    pub trace: OptimizationTrace,
}

/// Render the per-iteration step view of an optimization trace.
///
/// Prints a header (iterations, final improvement %, final e-graph nodes,
/// termination reason, wall-clock ms) followed by a table of one row per
/// e-graph iteration: index, cost, delta vs. the previous iteration, and a
/// marker on the optimal stop point. Written to stderr to match the rest of
/// the `optimize` command's diagnostic output.
pub fn render_step_view(trace: &OptimizationTrace, sql: Option<&str>) {
    eprintln!("{}", "Optimization Step Trace".bold());
    if let Some(q) = sql {
        eprintln!("  {}: {q}", "SQL".bold());
    }
    eprintln!("  {}:        {}", "Iterations".bold(), trace.iterations_run);
    eprintln!(
        "  {}:  {:.2}%",
        "Final improvement".bold(),
        trace.final_improvement_pct
    );
    eprintln!(
        "  {}:      {}",
        "E-graph nodes".bold(),
        trace.egraph_nodes_final
    );
    eprintln!("  {}: {}", "Termination".bold(), trace.termination_reason);
    eprintln!(
        "  {}:   {:.3} ms",
        "Optimize time".bold(),
        trace.optimization_time_ms
    );
    eprintln!();

    if trace.cost_per_iteration.is_empty() {
        eprintln!(
            "  {}",
            "(no per-iteration cost history — fast-path or zero-iteration query)".yellow()
        );
        return;
    }

    eprintln!("  {:>4}  {:>16}  {:>16}  note", "iter", "cost", "delta");
    eprintln!(
        "  {:>4}  {:>16}  {:>16}  ----",
        "----", "----------------", "----------------"
    );
    let mut prev: Option<f64> = None;
    for (i, &cost) in trace.cost_per_iteration.iter().enumerate() {
        let delta = match prev {
            Some(p) => format!("{:+.4}", cost - p),
            None => "-".to_string(),
        };
        let note = if i == trace.optimal_stop_point {
            "<- optimal stop".green().to_string()
        } else {
            String::new()
        };
        eprintln!("  {i:>4}  {cost:>16.4}  {delta:>16}  {note}");
        prev = Some(cost);
    }
}

/// Re-run a captured optimization trace and print the step view.
///
/// # Errors
///
/// Returns an error if the trace file cannot be read or parsed, the stored
/// SQL cannot be re-parsed, or re-optimization fails.
pub fn cmd_replay(trace_path: &Path, quiet: bool) -> Result<()> {
    let json = std::fs::read_to_string(trace_path)
        .with_context(|| format!("reading trace file: {}", trace_path.display()))?;
    let capture: TraceCapture = serde_json::from_str(&json)
        .with_context(|| format!("parsing trace JSON: {}", trace_path.display()))?;

    if capture.sql.trim().is_empty() {
        anyhow::bail!(
            "trace file {} has no SQL to replay (empty `sql` field)",
            trace_path.display()
        );
    }

    if !quiet {
        eprintln!(
            "{} {}",
            "Replaying trace:".bold(),
            trace_path.display().to_string().cyan()
        );
        eprintln!();
    }

    let plan = sql_to_relexpr(&capture.sql)
        .with_context(|| format!("re-parsing captured SQL: {}", capture.sql))?;

    let optimizer = Optimizer::new();
    let result = optimizer
        .optimize_with_tracking(&plan)
        .with_context(|| format!("re-optimizing captured SQL: {}", capture.sql))?;

    let replayed = result
        .trace
        .as_ref()
        .context("re-optimization produced no trace (unexpected)")?;

    if !quiet {
        render_step_view(replayed, Some(&capture.sql));
        eprintln!();
    }

    // Reproduction check: iterations + final cost should match the stored trace.
    let stored_final = capture.trace.cost_per_iteration.last().copied();
    let replayed_final = replayed.cost_per_iteration.last().copied();
    let iters_match = replayed.iterations_run == capture.trace.iterations_run;
    let cost_match = match (stored_final, replayed_final) {
        (Some(a), Some(b)) => (a - b).abs() < 1e-6 || (a - b).abs() / a.abs().max(1.0) < 1e-9,
        (None, None) => true,
        _ => false,
    };

    if iters_match && cost_match {
        if !quiet {
            eprintln!(
                "{} iterations={} final_cost={}",
                "Reproduced:".green().bold(),
                replayed.iterations_run,
                replayed_final.map_or_else(|| "n/a".to_string(), |c| format!("{c:.4}"))
            );
        }
        Ok(())
    } else {
        eprintln!(
            "{}",
            "Divergence (optimizer non-determinism?):".red().bold()
        );
        eprintln!(
            "  iterations: stored={} replayed={}{}",
            capture.trace.iterations_run,
            replayed.iterations_run,
            if iters_match { "" } else { "  MISMATCH" }
        );
        eprintln!(
            "  final cost: stored={} replayed={}{}",
            stored_final.map_or_else(|| "n/a".to_string(), |c| format!("{c:.4}")),
            replayed_final.map_or_else(|| "n/a".to_string(), |c| format!("{c:.4}")),
            if cost_match { "" } else { "  MISMATCH" }
        );
        anyhow::bail!("replay did not reproduce the stored trace");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_engine::Optimizer;
    use ra_parser::sql_to_relexpr;

    /// Capture a trace via the tracking optimizer, write it to a temp file,
    /// replay it, and assert the replay reproduces the iteration count.
    #[test]
    fn round_trip_capture_replay_reproduces_iterations() {
        let sql = "SELECT a FROM t WHERE a > 1 AND a < 100";
        let plan = sql_to_relexpr(sql).expect("parse");
        let optimizer = Optimizer::new();
        let mut result = optimizer.optimize_with_tracking(&plan).expect("optimize");
        let mut trace = result.trace.take().expect("trace present");
        trace.sql = sql.to_string();
        let stored_iters = trace.iterations_run;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trace.json");
        let capture = TraceCapture {
            sql: sql.to_string(),
            trace,
        };
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&capture).expect("serialize"),
        )
        .expect("write");

        // cmd_replay returns Ok only when iterations + final cost reproduce.
        cmd_replay(&path, true).expect("replay reproduces");

        // Re-optimize independently to confirm determinism of the iteration count.
        let result2 = optimizer.optimize_with_tracking(&plan).expect("optimize2");
        let trace2 = result2.trace.expect("trace2");
        assert_eq!(trace2.iterations_run, stored_iters);
    }

    #[test]
    fn replay_rejects_empty_sql() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("empty.json");
        let capture = TraceCapture {
            sql: String::new(),
            trace: OptimizationTrace {
                sql: String::new(),
                features: ra_engine::cost_model::extract_features(
                    &sql_to_relexpr("SELECT 1").expect("parse"),
                ),
                iterations_run: 0,
                cost_per_iteration: Vec::new(),
                termination_reason: "test".to_string(),
                final_improvement_pct: 0.0,
                optimal_stop_point: 0,
                egraph_nodes_final: 0,
                optimization_time_ms: 0.0,
            },
        };
        std::fs::write(&path, serde_json::to_string(&capture).expect("serialize")).expect("write");
        assert!(cmd_replay(&path, true).is_err());
    }
}
