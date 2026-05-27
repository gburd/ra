#![expect(
    clippy::print_stderr,
    clippy::cast_lossless,
    clippy::doc_markdown,
    clippy::unreadable_literal,
    clippy::unwrap_used,
    clippy::float_cmp,
    reason = "integration test: clarity over lint conformance"
)]
//! Integration test: train BitNet cost model and evaluate predictions.
//!
//! Demonstrates the unified pipeline:
//! 1. Generate synthetic training data (operator features → cost)
//! 2. Train with BitNetTrainer (QAT with STE)
//! 3. Export as BitNetCostModel
//! 4. Evaluate prediction quality (MAPE, correlation)
//! 5. Compare against untrained baseline

use ra_bitnet::{BitNetCostModel, BitNetTrainer, TrainerConfig, F, O};

/// Synthetic cost function that simulates realistic query costs.
///
/// Models: cost ≈ joins * log(cardinality) + filters * 0.3 + sorts * 2.0
fn synthetic_cost(features: &[f32; F]) -> [f32; O] {
    let tables = features[0];
    let joins = features[1];
    let filters = features[2];
    let aggregates = features[3];
    let order_by = features[7];
    let cardinality = features[11];

    let cpu = joins * cardinality.max(1.0).ln() * 0.5
        + filters * 0.3
        + order_by * cardinality.max(1.0).ln() * 0.2
        + aggregates * 1.5
        + tables * 0.1;

    let memory = cpu * 0.4 + cardinality.max(1.0).sqrt() * 0.01;
    let io_ops = (tables * 100.0 + joins * 500.0).max(1.0);

    let mut target = [0.0f32; O];
    target[0] = cpu.max(0.01);           // cpu_time_ms
    target[1] = memory.max(0.01);        // memory_peak_mb
    target[2] = memory * 0.7;            // memory_avg_mb
    target[3] = io_ops;                  // io_storage_ops
    target
}

/// Generate a diverse set of training samples.
fn generate_training_data(n: usize) -> Vec<([f32; F], [f32; O])> {
    let mut samples = Vec::with_capacity(n);
    let mut seed: u64 = 12345;

    for _ in 0..n {
        // Pseudo-random features (deterministic for reproducibility)
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1442695040888963407);
        let mut r = |max: f32| -> f32 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (seed >> 33) as f32 / (u32::MAX >> 1) as f32 * max
        };

        let features: [f32; F] = [
            r(8.0).floor().max(1.0),  // table_count: 1-8
            r(6.0).floor(),           // join_count: 0-6
            r(10.0).floor(),          // filter_count: 0-10
            r(3.0).floor(),           // aggregate_count: 0-3
            r(2.0).floor(),           // subquery_count: 0-2
            r(2.0).floor(),           // cte_count: 0-2
            r(3.0).floor(),           // window_function_count: 0-3
            r(3.0).floor(),           // order_by_count: 0-3
            r(3.0).floor(),           // group_by_count: 0-3
            (r(1.0) > 0.7) as u8 as f32, // distinct_flag
            (r(1.0) > 0.5) as u8 as f32, // limit_present
            (10.0f32).powf(r(5.0) + 1.0), // cardinality: 10 - 1M
            // OptimizationFeatures padding (density / fanout / equi / cross)
            0.0,
            0.0,
            0.0,
            0.0,
        ];

        let target = synthetic_cost(&features);
        samples.push((features, target));
    }
    samples
}

#[test]
fn train_and_evaluate_pipeline() {
    // --- Phase 1: Generate data ---
    let all_data = generate_training_data(500);
    let (train_data, eval_data) = all_data.split_at(400);

    // --- Phase 2: Train with QAT ---
    let mut trainer = BitNetTrainer::new(TrainerConfig {
        lr: 0.005,
        weight_decay: 0.001,
        ..Default::default()
    });

    // Compute normalization from training data
    let feature_samples: Vec<[f32; F]> = train_data.iter().map(|(f, _)| *f).collect();
    let mean = compute_mean(&feature_samples);
    let inv_std = compute_inv_std(&feature_samples, &mean);
    trainer.set_normalization(mean, inv_std);

    // Train for multiple epochs. Post-A4 the model has F=16 inputs
    // (~33% more first-layer weights than the F=12 version this test
    // was originally tuned for); bumped from 20 to 40 epochs so the
    // larger parameter set can still beat the untrained baseline.
    let mut epoch_losses = Vec::new();
    for _epoch in 0..40 {
        trainer.reset_loss();
        for (features, target) in train_data {
            trainer.train_step(features, target);
        }
        epoch_losses.push(trainer.avg_loss());
    }

    // Verify loss decreases
    let first_loss = epoch_losses[0];
    let last_loss = *epoch_losses.last().unwrap();
    assert!(
        last_loss < first_loss * 0.5,
        "Loss should decrease significantly: {first_loss:.4} → {last_loss:.4}"
    );

    // --- Phase 3: Export model ---
    let model = trainer.to_model();
    assert!(model.samples_trained > 0);

    // --- Phase 4: Evaluate ---
    let mut total_mape = 0.0f64;
    let mut predictions = Vec::new();
    let mut actuals = Vec::new();

    for (features, target) in eval_data {
        let pred = model.predict_cpu_ms(features);
        let actual = target[0]; // cpu_time_ms
        if actual > 0.01 {
            total_mape += ((pred - actual) / actual).abs() as f64;
        }
        predictions.push(pred);
        actuals.push(actual);
    }

    let mape = total_mape / eval_data.len() as f64 * 100.0;

    // --- Phase 5: Compare against untrained baseline ---
    let baseline = BitNetCostModel::new_zeros();
    let mut baseline_mape = 0.0f64;
    for (features, target) in eval_data {
        let pred = baseline.predict_cpu_ms(features);
        let actual = target[0];
        if actual > 0.01 {
            baseline_mape += ((pred - actual) / actual).abs() as f64;
        }
    }
    let baseline_mape = baseline_mape / eval_data.len() as f64 * 100.0;

    // Trained model should significantly outperform untrained
    assert!(
        mape < baseline_mape,
        "Trained MAPE ({mape:.1}%) should beat baseline ({baseline_mape:.1}%)"
    );

    // Compute rank correlation (Spearman-like: do predictions preserve ordering?)
    let correlation = rank_correlation(&predictions, &actuals);
    assert!(
        correlation > 0.3,
        "Rank correlation should be positive (got {correlation:.3})"
    );

    // Print results for visibility
    eprintln!("\n=== BitNet Cost Model Evaluation ===");
    eprintln!("Training: {} samples, {} epochs, {} steps",
        train_data.len(), 40, trainer.steps());
    eprintln!("Loss curve: {first_loss:.4} → {last_loss:.4} ({:.0}% reduction)",
        (1.0 - last_loss / first_loss) * 100.0);
    eprintln!("Eval MAPE:  {mape:.1}% (baseline: {baseline_mape:.1}%)");
    eprintln!("Rank corr:  {correlation:.3}");
    eprintln!("Model size: {} bytes (packed)", model.model_size_bytes());
}

#[test]
fn model_save_load_preserves_predictions() {
    let data = generate_training_data(100);
    let mut trainer = BitNetTrainer::new(TrainerConfig::default());

    for (f, t) in &data {
        trainer.train_step(f, t);
    }

    let model = trainer.to_model();
    let path = std::env::temp_dir().join("bitnet_integration_test.json");
    let path_str = path.to_str().unwrap();

    model.save_to_file(path_str).unwrap();
    let loaded = BitNetCostModel::load_from_file(path_str).unwrap();

    // Predictions must be identical after load
    for (f, _) in &data[..10] {
        let a = model.predict_cpu_ms(f);
        let b = loaded.predict_cpu_ms(f);
        assert_eq!(a, b, "Prediction drift after save/load");
    }

    let _ = std::fs::remove_file(path_str);
}

// --- Helpers ---

fn compute_mean(samples: &[[f32; F]]) -> [f32; F] {
    let n = samples.len() as f32;
    let mut mean = [0.0f32; F];
    for s in samples {
        for (i, &x) in s.iter().enumerate() {
            mean[i] += x / n;
        }
    }
    mean
}

fn compute_inv_std(samples: &[[f32; F]], mean: &[f32; F]) -> [f32; F] {
    let n = samples.len() as f32;
    let mut var = [0.0f32; F];
    for s in samples {
        for (i, &x) in s.iter().enumerate() {
            let d = x - mean[i];
            var[i] += d * d / n;
        }
    }
    let mut inv_std = [1.0f32; F];
    for (i, &v) in var.iter().enumerate() {
        let std = v.sqrt();
        inv_std[i] = if std > 1e-6 { 1.0 / std } else { 1.0 };
    }
    inv_std
}

/// Simple rank correlation (Spearman's ρ approximation).
fn rank_correlation(predictions: &[f32], actuals: &[f32]) -> f64 {
    let n = predictions.len();
    if n < 3 {
        return 0.0;
    }

    let pred_ranks = ranks(predictions);
    let actual_ranks = ranks(actuals);

    // Pearson correlation of ranks
    let mean_p: f64 = pred_ranks.iter().sum::<f64>() / n as f64;
    let mean_a: f64 = actual_ranks.iter().sum::<f64>() / n as f64;

    let mut cov = 0.0f64;
    let mut var_p = 0.0f64;
    let mut var_a = 0.0f64;

    for i in 0..n {
        let dp = pred_ranks[i] - mean_p;
        let da = actual_ranks[i] - mean_a;
        cov += dp * da;
        var_p += dp * dp;
        var_a += da * da;
    }

    let denom = (var_p * var_a).sqrt();
    if denom < 1e-10 { 0.0 } else { cov / denom }
}

fn ranks(values: &[f32]) -> Vec<f64> {
    let mut indexed: Vec<(usize, f32)> = values.iter().copied().enumerate().collect();
    indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut result = vec![0.0f64; values.len()];
    for (rank, (orig_idx, _)) in indexed.iter().enumerate() {
        result[*orig_idx] = rank as f64;
    }
    result
}
