//! The `cost` subcommand — inspect the cost model's view of a query.
//!
//! RA-STEERING §7.2 debugger command. Extracts the 16-D
//! `OptimizationFeatures` from a parsed query, runs the trained BitNet
//! cost model's `predict_all` to expose the full 16-dim cost vector,
//! and reports the scalar plan cost — the model's own scalar-head
//! prediction for the parsed plan vs the scalar cost the bounded
//! optimizer assigns the plan it extracts. Thin wrapper over engine
//! internals — no new cost logic here.
#![expect(clippy::print_stdout, reason = "CLI output")]

use anyhow::Result;
use serde::Serialize;

use ra_engine::speculative_router::OptimizationFeatures;
use ra_engine::training_coordinator::bootstrap_model;
use ra_engine::{BitNetCostModel, Optimizer};
use ra_parser::sql_to_relexpr;

use crate::output::errors::format_sql_error;

/// Labels for the 16 output dimensions of `BitNetCostModel::predict_all`.
/// Source: `docs/NEURAL_COST_MODEL.md` "Cost Dimensions (16 total)".
/// All 16 are cost dimensions — the model's output is a full cost vector.
const COST_DIM_NAMES: [&str; 16] = [
    "cpu_time_ms",
    "memory_peak_mb",
    "memory_avg_mb",
    "io_storage_ops",
    "io_storage_bytes",
    "io_network_ops",
    "io_network_bytes",
    "locks_acquired",
    "lock_hold_time_ms",
    "lock_contention_score",
    "vacuum_overhead",
    "wal_generation_bytes",
    "replication_lag_ms",
    "cache_hit_ratio",
    "page_faults",
    "context_switches",
];

const DEFAULT_MODEL_PATH: &str = "models/cost_model.bitnet.json";

#[derive(Serialize)]
struct CostReport {
    query: String,
    model_source: String,
    samples_trained: usize,
    cost_vector: Vec<CostDim>,
    /// Model scalar-head prediction for the parsed (unoptimized) plan.
    original_scalar_cost: f64,
    /// Scalar cost the bounded optimizer assigns the plan it extracts.
    optimized_plan_cost: Option<f64>,
}

#[derive(Serialize)]
struct CostDim {
    dim: usize,
    name: &'static str,
    value: f32,
}

/// Load the committed model the same way the optimizer's `load_model`
/// does (`models/cost_model.bitnet.json`); fall back to a freshly
/// bootstrapped model if the file is missing so the command still
/// produces live output. Returns the model and a human-readable source.
fn load_cost_model() -> (BitNetCostModel, String) {
    match BitNetCostModel::load_from_file(DEFAULT_MODEL_PATH) {
        Ok(m) => (m, DEFAULT_MODEL_PATH.to_string()),
        Err(e) => (
            bootstrap_model(),
            format!("bootstrap_model() (no file at {DEFAULT_MODEL_PATH}: {e})"),
        ),
    }
}

pub fn cmd_cost(query: &str, format: &str) -> Result<()> {
    let plan = sql_to_relexpr(query).map_err(|e| format_sql_error(&e, query))?;

    let (model, model_source) = load_cost_model();

    let features = OptimizationFeatures::from_expr(&plan);
    let feats = features.as_array();
    let prediction = model.predict_all(&feats);
    let original_scalar = model.predict_scalar(&feats);

    // Scalar cost the bounded optimizer assigns the plan it extracts.
    // Best-effort: an OptRoute::Skip path may return the original plan
    // with a computed cost of 0.
    let optimizer = Optimizer::new();
    let optimized_cost = optimizer.optimize_bounded(&plan).ok().map(|r| r.cost);

    if format.eq_ignore_ascii_case("json") {
        let report = CostReport {
            query: query.to_string(),
            model_source,
            samples_trained: model.samples_trained,
            cost_vector: COST_DIM_NAMES
                .iter()
                .enumerate()
                .zip(prediction.iter())
                .map(|((dim, &name), &value)| CostDim { dim, name, value })
                .collect(),
            original_scalar_cost: original_scalar,
            optimized_plan_cost: optimized_cost,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("Cost model: {model_source}");
    println!("  samples_trained {}", model.samples_trained);
    println!();
    println!("Predicted cost vector (predict_all, 16 dims):");
    for ((dim, name), value) in COST_DIM_NAMES.iter().enumerate().zip(prediction.iter()) {
        println!("  [{dim:>2}] {name:<22} {value}");
    }
    println!();
    println!("Original plan scalar cost (predict_scalar): {original_scalar:.4}");
    match optimized_cost {
        Some(c) => println!("Optimized plan cost (optimize_bounded):     {c:.4}"),
        None => println!("Optimized plan cost (optimize_bounded):     (no result)"),
    }

    Ok(())
}
