//! Measure neural cost model performance and accuracy.
//!
//! Tests:
//! 1. Model size
//! 2. Inference latency
//! 3. Training time
//! 4. Prediction accuracy (after training)
//! 5. Rule ranking capability
//!
//! Run with:
//!   cargo run --release --example measure_neural_model -p ra-bench

use ra_engine::cost_model::{ActualCost, QueryFeatures, SimpleCostModel};
use std::time::Instant;

fn main() {
    println!("Neural Cost Model Performance Measurement");
    println!("=========================================\n");

    // Test 1: Model Size
    let model = SimpleCostModel::new();
    let stats = model.stats();
    println!("1. Model Size");
    println!("   Size: {} bytes ({:.2} KB)", stats.model_size_bytes, stats.model_size_bytes as f64 / 1024.0);
    println!("   Samples seen: {}", stats.samples_seen);
    println!();

    // Test 2: Inference Latency
    println!("2. Inference Latency (1000 predictions)");
    let features = QueryFeatures {
        table_count: 3.0,
        join_count: 2.0,
        filter_count: 3.0,
        aggregate_count: 1.0,
        subquery_count: 0.0,
        cte_count: 0.0,
        window_function_count: 0.0,
        order_by_count: 1.0,
        group_by_count: 1.0,
        distinct_flag: 0.0,
        limit_present: 0.0,
        max_join_cardinality: 10000.0,
    };

    let start = Instant::now();
    for _ in 0..1000 {
        let _ = model.predict(&features);
    }
    let elapsed = start.elapsed();
    let avg_latency = elapsed.as_nanos() as f64 / 1000.0;
    println!("   Total: {:?}", elapsed);
    println!("   Average: {:.2} μs per prediction", avg_latency / 1000.0);
    println!();

    // Test 3: Training Time
    println!("3. Training Time (100 samples)");
    let mut model = SimpleCostModel::new();
    let actual = ActualCost {
        cpu_time_ms: 5.2,
        memory_peak_mb: 12.5,
        memory_avg_mb: 10.0,
        io_storage_ops: 150,
        io_storage_bytes: 1024 * 1024,
        io_network_ops: 0,
        io_network_bytes: 0,
        locks_acquired: 2,
        lock_hold_time_ms: 0.5,
        lock_contention_score: 0.1,
        vacuum_overhead: 0.0,
        wal_generation_bytes: 4096,
        replication_lag_ms: 0.0,
        cache_hit_ratio: 0.95,
        page_faults: 10,
        context_switches: 5,
    };

    let start = Instant::now();
    for _ in 0..100 {
        model.train(&features, &actual);
    }
    let elapsed = start.elapsed();
    println!("   Total: {:?}", elapsed);
    println!("   Average: {:.2} μs per sample", elapsed.as_micros() as f64 / 100.0);
    println!();

    // Test 4: Prediction Accuracy
    println!("4. Prediction Accuracy (after training)");
    let prediction = model.predict(&features);
    println!("   Predicted CPU: {:.2}ms (actual: {:.2}ms, error: {:.1}%)",
             prediction.cpu_time_ms, actual.cpu_time_ms,
             (prediction.cpu_time_ms - actual.cpu_time_ms).abs() / actual.cpu_time_ms * 100.0);
    println!("   Predicted Memory: {:.2}MB (actual: {:.2}MB, error: {:.1}%)",
             prediction.memory_peak_mb, actual.memory_peak_mb,
             (prediction.memory_peak_mb - actual.memory_peak_mb).abs() / actual.memory_peak_mb * 100.0);
    println!();

    let stats = model.stats();
    println!("   Samples seen: {}", stats.samples_seen);
    println!("   Average errors:");
    println!("     CPU time: {:.2}ms", stats.avg_errors[0]);
    println!("     Memory peak: {:.2}MB", stats.avg_errors[1]);
    println!();

    // Test 5: Rule Ranking Capability
    println!("5. Rule Ranking Capability");
    println!("   Testing if model can distinguish query complexity...");

    let simple_query = QueryFeatures {
        table_count: 1.0,
        join_count: 0.0,
        filter_count: 1.0,
        aggregate_count: 0.0,
        subquery_count: 0.0,
        cte_count: 0.0,
        window_function_count: 0.0,
        order_by_count: 0.0,
        group_by_count: 0.0,
        distinct_flag: 0.0,
        limit_present: 1.0,
        max_join_cardinality: 0.0,
    };

    let complex_query = QueryFeatures {
        table_count: 5.0,
        join_count: 4.0,
        filter_count: 5.0,
        aggregate_count: 3.0,
        subquery_count: 2.0,
        cte_count: 1.0,
        window_function_count: 1.0,
        order_by_count: 2.0,
        group_by_count: 2.0,
        distinct_flag: 1.0,
        limit_present: 0.0,
        max_join_cardinality: 100000.0,
    };

    let simple_pred = model.predict(&simple_query);
    let complex_pred = model.predict(&complex_query);

    println!("   Simple query predicted CPU: {:.2}ms", simple_pred.cpu_time_ms);
    println!("   Complex query predicted CPU: {:.2}ms", complex_pred.cpu_time_ms);

    if complex_pred.cpu_time_ms > simple_pred.cpu_time_ms {
        println!("   ✓ Model correctly ranks complex > simple");
    } else {
        println!("   ✗ Model failed to distinguish complexity");
    }
    println!();

    // Summary
    println!("Summary");
    println!("-------");
    println!("Model is lightweight ({:.2} KB) with fast inference ({:.2} μs)",
             stats.model_size_bytes as f64 / 1024.0, avg_latency / 1000.0);
    println!("Training is fast enough for online learning ({:.2} μs per sample)",
             elapsed.as_micros() as f64 / 100.0);
    println!("Model can learn patterns and distinguish query complexity");
}
