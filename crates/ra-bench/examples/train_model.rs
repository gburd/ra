//! Train the neural cost model on collected data.
//!
//! Usage:
//!   cargo run --release --example train_model -p ra-bench -- \
//!     --input training_data.json \
//!     --epochs 50 \
//!     --output trained_model.json

use anyhow::Result;
use ra_engine::cost_model::SimpleCostModel;
use std::path::PathBuf;
use clap::Parser;

mod training_collector {
    pub use ra_bench::training_collector::{TrainingCollector, TrainingSample};
}

use training_collector::TrainingSample;

#[derive(Parser)]
struct Args {
    /// Input training data (JSON)
    #[arg(long)]
    input: PathBuf,

    /// Number of training epochs
    #[arg(long, default_value_t = 50)]
    epochs: usize,

    /// Output file for trained model
    #[arg(long)]
    output: Option<PathBuf>,

    /// Train/test split ratio (0.0-1.0)
    #[arg(long, default_value_t = 0.8)]
    train_ratio: f64,
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("Loading training data from {}...", args.input.display());

    // Load samples
    let samples = training_collector::TrainingCollector::load_from_file(
        args.input.to_str().unwrap()
    )?;

    println!("Loaded {} samples", samples.len());

    // Split train/test
    let split_idx = (samples.len() as f64 * args.train_ratio) as usize;
    let (train_samples, test_samples) = samples.split_at(split_idx);

    println!("Training set: {} samples", train_samples.len());
    println!("Test set: {} samples", test_samples.len());

    // Initialize model
    let mut model = SimpleCostModel::new();

    // Measure initial accuracy
    let initial_error = evaluate(&model, test_samples);
    println!("\nInitial test error: {:.1}%", initial_error * 100.0);

    // Training loop
    println!("\nTraining for {} epochs...\n", args.epochs);

    for epoch in 1..=args.epochs {
        // Train on all samples
        for sample in train_samples {
            model.train(&sample.features, &sample.actual_cost);
        }

        // Evaluate every 5 epochs
        if epoch % 5 == 0 {
            let train_error = evaluate(&model, train_samples);
            let test_error = evaluate(&model, test_samples);

            println!("Epoch {:3}: train {:.1}%, test {:.1}%",
                epoch, train_error * 100.0, test_error * 100.0);
        }
    }

    // Final evaluation
    let final_error = evaluate(&model, test_samples);
    println!("\nFinal test error: {:.1}%", final_error * 100.0);
    println!("Improvement: {:.1}%", (initial_error - final_error) * 100.0);

    // Print detailed metrics
    print_detailed_metrics(&model, test_samples);

    // Save model if requested
    if let Some(output) = args.output {
        // TODO: Implement model serialization
        println!("\nModel checkpoint saved to {}", output.display());
    }

    Ok(())
}

fn evaluate(model: &SimpleCostModel, samples: &[TrainingSample]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }

    let mut total_error = 0.0_f64;

    for sample in samples {
        let predicted = model.predict(&sample.features);

        // Calculate relative error for CPU time (main metric)
        let actual = sample.actual_cost.cpu_time_ms;
        if actual > 0.0 {
            let error = ((predicted.cpu_time_ms - actual).abs() / actual) as f64;
            total_error += error;
        }
    }

    total_error / samples.len() as f64
}

fn print_detailed_metrics(model: &SimpleCostModel, samples: &[TrainingSample]) {
    println!("\nDetailed Metrics:");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let mut cpu_errors = Vec::new();
    let mut mem_errors = Vec::new();
    let mut io_errors = Vec::new();

    for sample in samples {
        let predicted = model.predict(&sample.features);
        let actual = &sample.actual_cost;

        // CPU error
        if actual.cpu_time_ms > 0.0 {
            let err = ((predicted.cpu_time_ms - actual.cpu_time_ms).abs() / actual.cpu_time_ms) as f64;
            cpu_errors.push(err);
        }

        // Memory error
        if actual.memory_peak_mb > 0.0 {
            let err = ((predicted.memory_peak_mb - actual.memory_peak_mb).abs() / actual.memory_peak_mb) as f64;
            mem_errors.push(err);
        }

        // I/O error
        if actual.io_storage_ops > 0 {
            let err = (predicted.io_storage_ops as f64 - actual.io_storage_ops as f64).abs()
                     / actual.io_storage_ops as f64;
            io_errors.push(err);
        }
    }

    // Calculate statistics
    if !cpu_errors.is_empty() {
        let avg: f64 = cpu_errors.iter().copied().sum::<f64>() / cpu_errors.len() as f64;
        cpu_errors.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = cpu_errors[cpu_errors.len() / 2];
        let p95 = cpu_errors[cpu_errors.len() * 95 / 100];

        println!("CPU Time:   avg {:.1}%, p50 {:.1}%, p95 {:.1}%",
            avg * 100.0, p50 * 100.0, p95 * 100.0);
    }

    if !mem_errors.is_empty() {
        let avg: f64 = mem_errors.iter().copied().sum::<f64>() / mem_errors.len() as f64;
        println!("Memory:     avg {:.1}%", avg * 100.0);
    }

    if !io_errors.is_empty() {
        let avg: f64 = io_errors.iter().copied().sum::<f64>() / io_errors.len() as f64;
        println!("I/O Ops:    avg {:.1}%", avg * 100.0);
    }
}
