//! The `model` subcommand — inspect the trained BitNet cost model.
//!
//! RA-STEERING §7.2 debugger command. Loads the committed cost model
//! (`models/cost_model.bitnet.json`, override with `--path`) and reports
//! its sizes, training-sample count, layer dimensions, per-feature
//! normalization table, and a live `predict_all` on a representative
//! feature vector. Thin wrapper over public `BitNetCostModel`
//! accessors — no new model logic here.
#![expect(clippy::print_stdout, reason = "CLI output")]

use anyhow::Result;
use serde::Serialize;

use ra_engine::speculative_router::OptimizationFeatures;
use ra_engine::training_coordinator::bootstrap_model;
use ra_engine::BitNetCostModel;
use ra_parser::sql_to_relexpr;

/// Layer dimensions of the BitNet model: F inputs -> H hidden -> O outputs.
/// These mirror the public `ra_bitnet::{F, H, O}` consts (16 -> 32 -> 16).
const F: usize = OptimizationFeatures::DIM; // 16 input features
const H: usize = 32; // hidden layer width
const O: usize = 16; // output cost dimensions

const DEFAULT_MODEL_PATH: &str = "models/cost_model.bitnet.json";

/// A representative query used to derive a live `predict_all` sample so
/// users see the model producing output, not just static metadata.
const SAMPLE_QUERY: &str = "SELECT * FROM a JOIN b ON a.id = b.id WHERE a.x > 5";

#[derive(Serialize)]
struct ModelReport {
    source: String,
    weights_only_bytes: usize,
    model_size_bytes: usize,
    samples_trained: usize,
    layer_dims: LayerDims,
    scalar_bias: f32,
    scalar_head: Vec<f32>,
    normalization: Vec<FeatureNorm>,
    mape_note: &'static str,
    sample_prediction: SamplePrediction,
}

#[derive(Serialize)]
struct LayerDims {
    input: usize,
    hidden: usize,
    output: usize,
    description: String,
}

#[derive(Serialize)]
struct FeatureNorm {
    feature: usize,
    mean: f32,
    inv_std: f32,
}

#[derive(Serialize)]
struct SamplePrediction {
    query: &'static str,
    features: Vec<f32>,
    cost_vector: Vec<f32>,
    scalar: f64,
}

// MAPE is a training-loop metric; the serialized model file carries no
// MAPE/feedback history (only packed weights, biases, scales, and
// normalization tables — see BitNetCostModel's serde fields). Do NOT
// fabricate a number here (RA-STEERING §3 bans unbacked metrics).
const MAPE_NOTE: &str = "MAPE: not persisted in the model file (training-loop metric only)";

/// Load the model from `path`, falling back to a freshly bootstrapped
/// model if the file is missing so the command still shows live output.
fn load_model(path: &str) -> (BitNetCostModel, String) {
    match BitNetCostModel::load_from_file(path) {
        Ok(m) => (m, path.to_string()),
        Err(e) => (
            bootstrap_model(),
            format!("bootstrap_model() (no file at {path}: {e})"),
        ),
    }
}

pub fn cmd_model(path: Option<&str>, format: &str) -> Result<()> {
    let path = path.unwrap_or(DEFAULT_MODEL_PATH);
    let (model, source) = load_model(path);

    let mean = model.feature_mean();
    let inv_std = model.feature_inv_std();
    let normalization: Vec<FeatureNorm> = (0..F)
        .map(|i| FeatureNorm {
            feature: i,
            mean: mean[i],
            inv_std: inv_std[i],
        })
        .collect();

    // Live sample prediction on a representative parsed query.
    let sample_plan = sql_to_relexpr(SAMPLE_QUERY)
        .map_err(|e| anyhow::anyhow!("sample query parse failed: {e}"))?;
    let sample_feats = OptimizationFeatures::from_expr(&sample_plan).as_array();
    let sample_cost = model.predict_all(&sample_feats);
    let sample_scalar = model.predict_scalar(&sample_feats);

    let layer_desc = format!("{F} -> {H} -> {O}");

    if format.eq_ignore_ascii_case("json") {
        let report = ModelReport {
            source,
            weights_only_bytes: model.weights_only_bytes(),
            model_size_bytes: model.model_size_bytes(),
            samples_trained: model.samples_trained,
            layer_dims: LayerDims {
                input: F,
                hidden: H,
                output: O,
                description: layer_desc,
            },
            scalar_bias: model.scalar_bias(),
            scalar_head: model.scalar_head().to_vec(),
            normalization,
            mape_note: MAPE_NOTE,
            sample_prediction: SamplePrediction {
                query: SAMPLE_QUERY,
                features: sample_feats.to_vec(),
                cost_vector: sample_cost.to_vec(),
                scalar: sample_scalar,
            },
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("BitNet cost model: {source}");
    println!("  weights_only_bytes {}", model.weights_only_bytes());
    println!("  model_size_bytes   {}", model.model_size_bytes());
    println!("  samples_trained    {}", model.samples_trained);
    println!("  layer dims         {layer_desc} (input -> hidden -> output)");
    println!("  scalar_bias        {}", model.scalar_bias());
    println!("  {MAPE_NOTE}");
    println!();
    println!("Scalar head ({} coeffs):", model.scalar_head().len());
    print!("  ");
    for (i, c) in model.scalar_head().iter().enumerate() {
        print!("[{i}]={c:.4} ");
    }
    println!();
    println!();
    println!("Per-feature normalization (mean / inv_std):");
    for n in &normalization {
        println!(
            "  [{:>2}] mean {:>12.4}  inv_std {:>12.6}",
            n.feature, n.mean, n.inv_std
        );
    }
    println!();
    println!("Sample predict_all for: {SAMPLE_QUERY}");
    print!("  features: ");
    for v in &sample_feats {
        print!("{v} ");
    }
    println!();
    println!("  cost vector:");
    for (i, v) in sample_cost.iter().enumerate() {
        println!("    [{i:>2}] {v}");
    }
    println!("  scalar: {sample_scalar:.4}");

    Ok(())
}
