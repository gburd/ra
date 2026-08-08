//! `ra bench --mape` — RA-STEERING Gate 4 (Codeberg #14): honestly measure
//! whether Ra's BitNet cost model earns its place.
//!
//! For each query in a corpus we collect three numbers:
//!   * `actual_ms` — real execution time, median of N `EXPLAIN (ANALYZE)` runs
//!     (a warmup run is discarded first) on live PostgreSQL.
//!   * `ra_pred`   — Ra's BitNet model prediction (`predict_cpu_ms`) on the
//!     16-D optimization features of the *optimized* plan — the exact feature
//!     path the speculative router / `ra model` use in production.
//!   * `pg_cost`   — PostgreSQL's own estimated total cost (`EXPLAIN (FORMAT
//!     JSON)` top-plan "Total Cost").
//!
//! # MAPE methodology (fair to PG)
//!
//! `ra_pred` is already in ~ms, directly comparable to `actual_ms`:
//!   MAPE_ra = mean(|ra_pred_i - actual_i| / actual_i).
//!
//! `pg_cost` is in abstract page-cost units, not ms. To compare it fairly we
//! fit ONE global least-squares scalar k mapping pg_cost -> ms across the whole
//! workload (k = Σ cost·actual / Σ cost²) and then:
//!   MAPE_pg = mean(|k·pg_cost_i - actual_i| / actual_i).
//! This gives PG a free global calibration — the standard "does PG's cost shape
//! predict runtime" baseline — so the comparison does not penalize PG for its
//! units.
//!
//! # Caveat (stated honestly)
//!
//! The default tier-0 corpus runs against tiny, cache-resident tables, so
//! absolute times are small and noisy (§6 retired SF=0.01 for exactly this
//! reason). This MAPE is *indicative, not definitive*. Point `--corpus` at a
//! larger workload (e.g. a full TPC-H load) for a definitive number.

#![expect(clippy::print_stdout, reason = "CLI output")]

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use postgres::{Client, NoTls};
use serde::Serialize;
use serde_json::Value;

use ra_engine::speculative_router::OptimizationFeatures;
use ra_engine::training_coordinator::bootstrap_model;
use ra_engine::{BitNetCostModel, Optimizer};
use ra_parser::sql_to_relexpr;

/// Default tier-0 corpus (relative to repo root).
const TIER0_CORPUS: &str = "benchmarks/planner_comparison/queries";
/// Committed production model; overridable via `RA_MODEL_PATH`.
const DEFAULT_MODEL_PATH: &str = "models/cost_model.bitnet.json";

/// One usable measurement (query parsed, optimized, and ran on PG).
#[derive(Debug, Clone, Serialize)]
struct QueryRow {
    id: String,
    actual_ms: f64,
    ra_pred: f64,
    pg_cost: f64,
    /// k·pg_cost, filled after the global scalar fit.
    pg_pred: f64,
}

/// A query skipped from the MAPE set, with the reason.
#[derive(Debug, Clone, Serialize)]
struct Skipped {
    id: String,
    stage: &'static str,
    error: String,
}

#[derive(Debug, Clone, Serialize)]
struct MapeReport {
    corpus: String,
    model_source: String,
    runs: usize,
    n_used: usize,
    n_skipped: usize,
    /// Least-squares scalar mapping pg_cost -> ms.
    pg_scalar_k: f64,
    mape_ra: f64,
    mape_pg: f64,
    beats_baseline: bool,
    verdict: String,
    caveat: &'static str,
    rows: Vec<QueryRow>,
    skipped: Vec<Skipped>,
}

const CAVEAT: &str = "Tier-0 tables are tiny and cache-resident, so actual_ms is \
    small and noisy; this MAPE is indicative, not definitive. Use --corpus with a \
    larger (e.g. full TPC-H) load for a definitive number.";

/// Fit the global least-squares scalar k mapping cost -> ms:
/// k = Σ(cost·actual) / Σ(cost²). Returns 0.0 when Σcost² is ~0 (no signal).
#[must_use]
fn fit_scalar_k(cost: &[f64], actual: &[f64]) -> f64 {
    let num: f64 = cost.iter().zip(actual).map(|(c, a)| c * a).sum();
    let den: f64 = cost.iter().map(|c| c * c).sum();
    if den.abs() < 1e-12 {
        0.0
    } else {
        num / den
    }
}

/// Mean absolute percentage error of `pred` against `actual` (fraction, not %).
/// Skips pairs where actual ~ 0 to avoid divide-by-zero blow-ups.
#[must_use]
fn mape(pred: &[f64], actual: &[f64]) -> f64 {
    let mut sum = 0.0;
    let mut n = 0usize;
    for (p, a) in pred.iter().zip(actual) {
        if a.abs() < 1e-9 {
            continue;
        }
        sum += ((p - a) / a).abs();
        n += 1;
    }
    if n == 0 {
        0.0
    } else {
        sum / n as f64
    }
}

/// Median of a slice (sorts a copy). Empty -> 0.0.
#[must_use]
fn median(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = v.len() / 2;
    if v.len().is_multiple_of(2) {
        f64::midpoint(v[mid - 1], v[mid])
    } else {
        v[mid]
    }
}

/// Load the committed production model, falling back to `bootstrap_model()`
/// if the file is absent so the harness still produces numbers. Returns the
/// model and a human-readable source string for the report.
fn load_production_model() -> (BitNetCostModel, String) {
    let path = std::env::var("RA_MODEL_PATH").unwrap_or_else(|_| DEFAULT_MODEL_PATH.to_string());
    match BitNetCostModel::load_from_file(&path) {
        Ok(m) => (m, format!("committed file {path}")),
        Err(e) => (
            bootstrap_model(),
            format!("bootstrap_model() (no file at {path}: {e})"),
        ),
    }
}

/// Read `pg_cost` (top-plan "Total Cost") from `EXPLAIN (FORMAT JSON)`.
fn pg_total_cost(client: &mut Client, sql: &str) -> Result<f64> {
    let rows = client
        .query(&format!("EXPLAIN (FORMAT JSON) {sql}"), &[])
        .map_err(|e| anyhow!("EXPLAIN failed: {e}"))?;
    let plan: Value = rows
        .first()
        .map(|r| r.get::<_, Value>(0))
        .ok_or_else(|| anyhow!("EXPLAIN returned no rows"))?;
    plan.as_array()
        .and_then(|a| a.first())
        .and_then(|f| f.get("Plan"))
        .and_then(|p| p.get("Total Cost"))
        .and_then(Value::as_f64)
        .ok_or_else(|| anyhow!("EXPLAIN JSON missing Plan.Total Cost"))
}

/// One `EXPLAIN (ANALYZE, FORMAT JSON)` run → "Execution Time" (ms).
fn pg_exec_ms(client: &mut Client, sql: &str) -> Result<f64> {
    let rows = client
        .query(&format!("EXPLAIN (ANALYZE, FORMAT JSON) {sql}"), &[])
        .map_err(|e| anyhow!("EXPLAIN ANALYZE failed: {e}"))?;
    let plan: Value = rows
        .first()
        .map(|r| r.get::<_, Value>(0))
        .ok_or_else(|| anyhow!("EXPLAIN ANALYZE returned no rows"))?;
    plan.as_array()
        .and_then(|a| a.first())
        .and_then(|f| f.get("Execution Time"))
        .and_then(Value::as_f64)
        .ok_or_else(|| anyhow!("EXPLAIN JSON missing Execution Time"))
}

/// Recursively collect `*.sql` files under `dir`, sorted by path.
fn collect_sql(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)
            .with_context(|| format!("reading corpus dir {}", d.display()))?
            .filter_map(std::result::Result::ok)
        {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "sql") {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Query id for the report: `<parent>/<stem>`.
fn query_id(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    match path.parent().and_then(Path::file_name) {
        Some(parent) => format!("{}/{stem}", parent.to_string_lossy()),
        None => stem,
    }
}

/// Gate-4 MAPE harness. `runs` is the number of timed EXPLAIN ANALYZE passes
/// per query (a warmup pass is always discarded first); median is taken.
pub fn cmd_bench_mape(db: &str, corpus: Option<&Path>, runs: usize, format: &str) -> Result<()> {
    let corpus_dir = corpus.map_or_else(|| PathBuf::from(TIER0_CORPUS), Path::to_path_buf);
    if !corpus_dir.is_dir() {
        anyhow::bail!(
            "corpus directory not found: {}. Run from the repo root or pass --corpus <dir>.",
            corpus_dir.display()
        );
    }
    let runs = runs.max(1);

    let (model, model_source) = load_production_model();
    // Load the same committed model into the optimizer so optimize() reflects
    // production behavior; predictions use the router feature path directly.
    let mut optimizer = Optimizer::new();
    let _ = optimizer.load_model();

    let mut client =
        Client::connect(db, NoTls).map_err(|e| anyhow!("connect to PostgreSQL failed: {e}"))?;

    let files = collect_sql(&corpus_dir)?;
    let mut rows: Vec<QueryRow> = Vec::new();
    let mut skipped: Vec<Skipped> = Vec::new();

    for path in &files {
        let id = query_id(path);
        let sql_raw =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        // Strip leading comment lines / trailing semicolons for the parser and
        // for a single-statement EXPLAIN.
        let sql = sql_raw.trim().trim_end_matches(';').trim().to_string();
        if sql.is_empty() {
            continue;
        }

        // 1. Parse.
        let expr = match sql_to_relexpr(&sql) {
            Ok(e) => e,
            Err(e) => {
                skipped.push(Skipped {
                    id,
                    stage: "parse",
                    error: e.to_string(),
                });
                continue;
            }
        };
        // 2. Optimize.
        let optimized = match optimizer.optimize(&expr) {
            Ok(o) => o,
            Err(e) => {
                skipped.push(Skipped {
                    id,
                    stage: "optimize",
                    error: e.to_string(),
                });
                continue;
            }
        };
        // 3. pg_cost (also validates the query runs on PG at all).
        let pg_cost = match pg_total_cost(&mut client, &sql) {
            Ok(c) => c,
            Err(e) => {
                skipped.push(Skipped {
                    id,
                    stage: "pg_explain",
                    error: e.to_string(),
                });
                continue;
            }
        };
        // 4. actual_ms: warmup + median of `runs` timed passes.
        let _ = pg_exec_ms(&mut client, &sql); // warmup, discarded
        let mut times = Vec::with_capacity(runs);
        let mut analyze_err = None;
        for _ in 0..runs {
            match pg_exec_ms(&mut client, &sql) {
                Ok(t) => times.push(t),
                Err(e) => {
                    analyze_err = Some(e.to_string());
                    break;
                }
            }
        }
        if let Some(err) = analyze_err {
            skipped.push(Skipped {
                id,
                stage: "pg_analyze",
                error: err,
            });
            continue;
        }
        let actual_ms = median(&times);
        // 5. ra_pred: router feature path -> model.predict_cpu_ms (production).
        let features = OptimizationFeatures::from_expr(&optimized).as_array();
        let ra_pred = f64::from(model.predict_cpu_ms(&features));

        rows.push(QueryRow {
            id,
            actual_ms,
            ra_pred,
            pg_cost,
            pg_pred: 0.0,
        });
    }

    // Fit the global PG scalar and compute both MAPEs.
    let costs: Vec<f64> = rows.iter().map(|r| r.pg_cost).collect();
    let actuals: Vec<f64> = rows.iter().map(|r| r.actual_ms).collect();
    let ra_preds: Vec<f64> = rows.iter().map(|r| r.ra_pred).collect();
    let k = fit_scalar_k(&costs, &actuals);
    for r in &mut rows {
        r.pg_pred = k * r.pg_cost;
    }
    let pg_preds: Vec<f64> = rows.iter().map(|r| r.pg_pred).collect();
    let mape_ra = mape(&ra_preds, &actuals);
    let mape_pg = mape(&pg_preds, &actuals);
    let beats = mape_ra < mape_pg;

    let verdict = format!(
        "RA model MAPE {:.1}% vs PG-cost baseline MAPE {:.1}% over {} queries — model {} the baseline.",
        mape_ra * 100.0,
        mape_pg * 100.0,
        rows.len(),
        if beats { "beats" } else { "does NOT beat" },
    );

    let report = MapeReport {
        corpus: corpus_dir.display().to_string(),
        model_source,
        runs,
        n_used: rows.len(),
        n_skipped: skipped.len(),
        pg_scalar_k: k,
        mape_ra,
        mape_pg,
        beats_baseline: beats,
        verdict: verdict.clone(),
        caveat: CAVEAT,
        rows,
        skipped,
    };

    if format.eq_ignore_ascii_case("json") {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    // Text report.
    println!("Gate 4 — BitNet cost model vs PostgreSQL-cost baseline (Codeberg #14)");
    println!("  corpus:       {}", report.corpus);
    println!("  model:        {}", report.model_source);
    println!(
        "  runs/query:   {} timed EXPLAIN ANALYZE passes (warmup discarded), median taken",
        report.runs
    );
    println!(
        "  queries:      {} used, {} skipped",
        report.n_used, report.n_skipped
    );
    println!();
    println!("Method: MAPE_ra compares ra_pred (~ms) directly to actual_ms.");
    println!(
        "        MAPE_pg fits ONE global least-squares scalar k (= Σcost·actual/Σcost²) mapping"
    );
    println!("        pg_cost -> ms, then compares k·pg_cost to actual_ms — a free global");
    println!("        calibration that is fair to PostgreSQL. k = {k:.6}");
    println!();
    println!(
        "{:<34} {:>11} {:>11} {:>12} {:>12}",
        "query", "actual_ms", "ra_pred", "pg_cost", "k*pg_cost"
    );
    println!("{}", "-".repeat(82));
    for r in &report.rows {
        println!(
            "{:<34} {:>11.3} {:>11.3} {:>12.2} {:>12.3}",
            truncate(&r.id, 34),
            r.actual_ms,
            r.ra_pred,
            r.pg_cost,
            r.pg_pred
        );
    }
    println!("{}", "-".repeat(82));
    println!();
    println!("MAPE_ra (BitNet):        {:.1}%", report.mape_ra * 100.0);
    println!("MAPE_pg (PG-cost + k):   {:.1}%", report.mape_pg * 100.0);
    println!();
    println!("VERDICT: {}", report.verdict);
    println!();
    println!("CAVEAT: {}", report.caveat);
    if !report.skipped.is_empty() {
        println!();
        println!("Skipped queries ({}):", report.skipped.len());
        for s in &report.skipped {
            println!("  [{}] {} — {}", s.stage, s.id, truncate(&s.error, 80));
        }
    }
    Ok(())
}

/// Truncate a string to `max` chars for tabular display.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_k_recovers_known_slope() {
        // actual = 2.0 * cost exactly → least-squares k = 2.0.
        let cost = [1.0, 2.0, 3.0, 4.0];
        let actual = [2.0, 4.0, 6.0, 8.0];
        let k = fit_scalar_k(&cost, &actual);
        assert!((k - 2.0).abs() < 1e-9, "k={k}");
        // With k applied, pg prediction is exact → MAPE 0.
        let pg: Vec<f64> = cost.iter().map(|c| k * c).collect();
        assert!(mape(&pg, &actual) < 1e-9);
    }

    #[test]
    fn scalar_k_least_squares_on_noisy_data() {
        // cost=[1,1], actual=[1,3]: k = (1*1+1*3)/(1+1) = 2.
        let k = fit_scalar_k(&[1.0, 1.0], &[1.0, 3.0]);
        assert!((k - 2.0).abs() < 1e-9, "k={k}");
    }

    #[test]
    fn scalar_k_zero_cost_is_safe() {
        assert!(fit_scalar_k(&[0.0, 0.0], &[1.0, 2.0]).abs() < f64::EPSILON);
    }

    #[test]
    fn mape_zero_on_perfect_prediction() {
        assert!(mape(&[10.0, 20.0], &[10.0, 20.0]) < 1e-12);
    }

    #[test]
    fn mape_reflects_fifty_percent_error() {
        // pred 50% high on every point.
        let m = mape(&[15.0, 30.0], &[10.0, 20.0]);
        assert!((m - 0.5).abs() < 1e-12, "mape={m}");
    }

    #[test]
    fn mape_skips_zero_actual() {
        // second pair has actual 0 → skipped; only the first (0 error) counts.
        let m = mape(&[5.0, 99.0], &[5.0, 0.0]);
        assert!(m < 1e-12, "mape={m}");
    }

    #[test]
    fn median_odd_and_even() {
        assert!((median(&[3.0, 1.0, 2.0]) - 2.0).abs() < 1e-12);
        assert!((median(&[4.0, 1.0, 3.0, 2.0]) - 2.5).abs() < 1e-12);
        assert!(median(&[]).abs() < f64::EPSILON);
    }
}
