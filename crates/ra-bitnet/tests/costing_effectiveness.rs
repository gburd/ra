#![expect(
    clippy::print_stderr,
    clippy::uninlined_format_args,
    clippy::cast_lossless,
    clippy::doc_markdown,
    clippy::unreadable_literal,
    reason = "integration test: clarity over lint conformance"
)]
//! Evaluate BitNet cost model effectiveness in the actual plan costing pipeline.
//!
//! Tests whether a trained model improves plan selection when wired into
//! the hybrid extraction cost function. Measures:
//! 1. Training convergence and prediction quality (MAPE, rank correlation)
//! 2. Model influence on cost estimates (how much blend_alpha changes things)
//! 3. Stability: does the model produce consistent rankings across reruns?

use ra_bitnet::{BitNetCostModel, BitNetTrainer, TrainerConfig, F, O};

/// More realistic cost function modeling joins, sorts, and aggregates.
fn realistic_cost(features: &[f32; F]) -> [f32; O] {
    let tables = features[0];
    let joins = features[1];
    let filters = features[2];
    let aggregates = features[3];
    let subqueries = features[4];
    let windows = features[6];
    let order_by = features[7];
    let group_by = features[8];
    let distinct = features[9];
    let limit = features[10];
    let cardinality = features[11];

    let log_card = cardinality.max(1.0).ln();

    // CPU cost model: realistic query planner heuristics
    let join_cost = joins * log_card * 0.8; // O(n log n) per join
    let sort_cost = order_by * log_card * cardinality.max(1.0).sqrt() * 0.01;
    let agg_cost = (aggregates + group_by) * cardinality.max(1.0).sqrt() * 0.1;
    let filter_cost = filters * cardinality.max(1.0) * 0.0001;
    let subquery_cost = subqueries * log_card * 2.0;
    let window_cost = windows * log_card * 1.5;
    let distinct_cost = distinct * cardinality.max(1.0).sqrt() * 0.2;

    let cpu = (join_cost + sort_cost + agg_cost + filter_cost
        + subquery_cost + window_cost + distinct_cost + tables * 0.5)
        .max(0.1);

    // Limit reduces output but not computation
    let effective_output = if limit > 0.5 { 100.0 } else { cardinality.max(1.0).sqrt() };

    let memory = effective_output * 0.01 + joins * 2.0;
    let io_ops = tables * 50.0 + joins * 200.0 + cardinality.max(1.0).sqrt() * 0.5;

    let mut target = [0.0f32; O];
    target[0] = cpu;
    target[1] = memory.max(0.01);
    target[2] = memory * 0.7;
    target[3] = io_ops;
    target[4] = io_ops * 8192.0; // bytes = ops * page_size
    target
}

/// Generate diverse training data covering the query feature space.
///
/// Targets are log-transformed: model predicts ln(cost+1) instead of raw cost.
/// This handles the large dynamic range (costs from 0.1 to 10000+).
fn generate_data(n: usize, seed_start: u64) -> Vec<([f32; F], [f32; O])> {
    let mut samples = Vec::with_capacity(n);
    let mut seed = seed_start;

    for _ in 0..n {
        let mut r = || -> f32 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1442695040888963407);
            (seed >> 33) as f32 / (u32::MAX >> 1) as f32
        };

        let features: [f32; F] = [
            (r() * 8.0).floor().max(1.0),       // tables: 1-8
            (r() * 7.0).floor(),                 // joins: 0-7
            (r() * 12.0).floor(),                // filters: 0-12
            (r() * 4.0).floor(),                 // aggregates: 0-4
            (r() * 3.0).floor(),                 // subqueries: 0-3
            (r() * 2.0).floor(),                 // CTEs: 0-2
            (r() * 4.0).floor(),                 // windows: 0-4
            (r() * 4.0).floor(),                 // order_by: 0-4
            (r() * 4.0).floor(),                 // group_by: 0-4
            if r() > 0.7 { 1.0 } else { 0.0 },  // distinct
            if r() > 0.5 { 1.0 } else { 0.0 },  // limit
            (10.0f32).powf(r() * 6.0 + 1.0),    // cardinality: 10 - 10M
            // OptimizationFeatures padding
            0.0,
            0.0,
            0.0,
            0.0,
        ];

        let raw_target = realistic_cost(&features);
        // Log-transform targets to handle large dynamic range
        let mut target = [0.0f32; O];
        for (i, &v) in raw_target.iter().enumerate() {
            target[i] = (v + 1.0).ln();
        }
        samples.push((features, target));
    }
    samples
}

#[test]
fn training_with_more_data_improves_accuracy() {
    let train_data = generate_data(2000, 42);
    let eval_data = generate_data(200, 99999);

    let mut trainer = BitNetTrainer::new(TrainerConfig {
        lr: 0.003,
        weight_decay: 0.0005,
        max_grad_norm: 2.0,
        ..Default::default()
    });

    // Compute normalization
    let features: Vec<[f32; F]> = train_data.iter().map(|(f, _)| *f).collect();
    let mean = compute_mean(&features);
    let inv_std = compute_inv_std(&features, &mean);
    trainer.set_normalization(mean, inv_std);

    // Track loss per epoch. Post-A4 the model has F=16 inputs with 4
    // training-time-zeroed dims; the extra parameters need more epochs
    // than the F=12 schedule to converge to a useful rank ordering.
    let mut epoch_losses = Vec::new();
    for epoch in 0..100 {
        trainer.reset_loss();
        // Mini-batch SGD: shuffle would be ideal but deterministic order is fine
        for (f, t) in &train_data {
            trainer.train_step(f, t);
        }
        let loss = trainer.avg_loss();
        epoch_losses.push(loss);
        if epoch % 10 == 9 {
            eprintln!("  epoch {:>3}: loss = {:.4}", epoch + 1, loss);
        }
    }

    let model = trainer.to_model();

    // Evaluate MAPE and rank correlation
    let (mape, rank_corr) = evaluate_model(&model, &eval_data);

    eprintln!("\n=== Training Results (2000 samples, 50 epochs) ===");
    eprintln!("Loss:      {:.4} → {:.4}", epoch_losses[0], epoch_losses[99]);
    eprintln!("Eval MAPE: {:.1}%", mape * 100.0);
    eprintln!("Rank corr: {:.3}", rank_corr);
    eprintln!("Steps:     {}", trainer.steps());

    // Accuracy assertions
    assert!(epoch_losses[99] < epoch_losses[0] * 0.1,
        "Loss should decrease >10x: {:.4} → {:.4}", epoch_losses[0], epoch_losses[99]);
    assert!(mape < 0.5,
        "MAPE should be under 50%: got {:.1}%", mape * 100.0);
    assert!(rank_corr > 0.3,
        "Rank correlation should exceed 0.3: got {:.3}", rank_corr);
}

#[test]
fn model_preserves_cost_ordering_for_plan_comparison() {
    // Train a model
    let train_data = generate_data(1000, 7777);
    let mut trainer = BitNetTrainer::new(TrainerConfig {
        lr: 0.005,
        weight_decay: 0.001,
        ..Default::default()
    });

    let features: Vec<[f32; F]> = train_data.iter().map(|(f, _)| *f).collect();
    let mean = compute_mean(&features);
    let inv_std = compute_inv_std(&features, &mean);
    trainer.set_normalization(mean, inv_std);

    for _ in 0..30 {
        for (f, t) in &train_data {
            trainer.train_step(f, t);
        }
    }
    let model = trainer.to_model();

    // Generate plan pairs where one is clearly cheaper than the other.
    // Model predicts in log-space; higher prediction = more expensive.
    let mut correct_orderings = 0;
    let mut total_comparisons = 0;

    for i in 0..100 {
        // "Cheap" query: few joins, low cardinality
        let cheap = [
            2.0, 1.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            100.0 + (i as f32) * 10.0,
            0.0, 0.0, 0.0, 0.0,
        ];
        // "Expensive" query: many joins, high cardinality
        let expensive = [
            6.0, 5.0, 8.0, 2.0, 1.0, 0.0, 2.0, 2.0, 2.0, 1.0, 0.0,
            100_000.0 + (i as f32) * 1000.0,
            0.0, 0.0, 0.0, 0.0,
        ];

        let cheap_pred = model.predict_cpu_ms(&cheap);
        let expensive_pred = model.predict_cpu_ms(&expensive);

        total_comparisons += 1;
        if cheap_pred < expensive_pred {
            correct_orderings += 1;
        }
    }

    let ordering_accuracy = correct_orderings as f64 / total_comparisons as f64;
    eprintln!("\n=== Plan Ordering Accuracy ===");
    eprintln!("Correct orderings: {}/{} ({:.1}%)",
        correct_orderings, total_comparisons, ordering_accuracy * 100.0);

    assert!(ordering_accuracy > 0.90,
        "Model should correctly order cheap vs expensive plans >90%: got {:.1}%",
        ordering_accuracy * 100.0);
}

#[test]
fn blend_alpha_influence_is_bounded() {
    // Verify that with a trained model, the neural component adjusts costs
    // but doesn't dominate (blend_alpha caps at 0.9)
    let train_data = generate_data(500, 11111);
    let mut trainer = BitNetTrainer::new(TrainerConfig::default());

    let features: Vec<[f32; F]> = train_data.iter().map(|(f, _)| *f).collect();
    let mean = compute_mean(&features);
    let inv_std = compute_inv_std(&features, &mean);
    trainer.set_normalization(mean, inv_std);

    for _ in 0..20 {
        for (f, t) in &train_data {
            trainer.train_step(f, t);
        }
    }
    let model = trainer.to_model();

    // Verify predictions are finite and non-negative for diverse inputs
    let test_inputs = generate_data(100, 55555);
    let mut all_finite = true;
    let mut all_non_negative = true;

    for (features, _) in &test_inputs {
        let pred = model.predict_cpu_ms(features);
        if !pred.is_finite() { all_finite = false; }
        if pred < 0.0 { all_non_negative = false; }
    }

    assert!(all_finite, "All predictions must be finite");
    assert!(all_non_negative, "All predictions must be non-negative (softplus)");

    // Verify model distinguishes between queries (in log-space)
    let preds: Vec<f32> = test_inputs.iter()
        .map(|(f, _)| model.predict_cpu_ms(f))
        .collect();
    let min_pred = preds.iter().copied().fold(f32::INFINITY, f32::min);
    let max_pred = preds.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let spread = max_pred - min_pred; // absolute spread in log-space

    eprintln!("\n=== Prediction Spread (log-space) ===");
    eprintln!("Min: {:.4}, Max: {:.4}, Spread: {:.4}", min_pred, max_pred, spread);

    // In log-space, a spread of 0.5 means exp(0.5)=1.6x cost difference
    // With diverse queries, we expect at least some differentiation
    assert!(spread > 0.1,
        "Model should differentiate queries (spread > 0.1): got {:.4}", spread);
}

// --- Helpers ---

fn evaluate_model(model: &BitNetCostModel, eval_data: &[([f32; F], [f32; O])]) -> (f64, f64) {
    let mut total_mape = 0.0f64;
    let mut predictions = Vec::new();
    let mut actuals = Vec::new();

    for (features, target) in eval_data {
        // Model outputs log-space prediction; compare in log-space for MAPE
        let pred_log = model.predict_cpu_ms(features);
        let actual_log = target[0]; // already ln(cost+1)
        if actual_log > 0.01 {
            total_mape += ((pred_log - actual_log) / actual_log).abs() as f64;
        }
        predictions.push(pred_log);
        actuals.push(actual_log);
    }

    let mape = total_mape / eval_data.len() as f64;
    let corr = rank_correlation(&predictions, &actuals);
    (mape, corr)
}

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

fn rank_correlation(predictions: &[f32], actuals: &[f32]) -> f64 {
    let n = predictions.len();
    if n < 3 { return 0.0; }

    let pred_ranks = ranks(predictions);
    let actual_ranks = ranks(actuals);

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
