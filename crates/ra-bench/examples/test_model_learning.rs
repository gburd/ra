//! Test neural cost model with diverse training samples.
//!
//! Verifies the model can learn from varied query patterns.

use ra_engine::cost_model::{ActualCost, QueryFeatures, SimpleCostModel};

fn main() {
    println!("Testing Neural Cost Model Learning");
    println!("===================================\n");

    let mut model = SimpleCostModel::new();

    // Training set: diverse queries with realistic costs
    let training_data = vec![
        // Simple single-table scans
        (
            QueryFeatures {
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
            },
            ActualCost {
                cpu_time_ms: 0.5,
                memory_peak_mb: 2.0,
                memory_avg_mb: 1.5,
                io_storage_ops: 10,
                io_storage_bytes: 8192,
                io_network_ops: 0,
                io_network_bytes: 0,
                locks_acquired: 1,
                lock_hold_time_ms: 0.1,
                lock_contention_score: 0.0,
                vacuum_overhead: 0.0,
                wal_generation_bytes: 0,
                replication_lag_ms: 0.0,
                cache_hit_ratio: 0.99,
                page_faults: 0,
                context_switches: 1,
            },
        ),
        // Two-table join
        (
            QueryFeatures {
                table_count: 2.0,
                join_count: 1.0,
                filter_count: 2.0,
                aggregate_count: 0.0,
                subquery_count: 0.0,
                cte_count: 0.0,
                window_function_count: 0.0,
                order_by_count: 0.0,
                group_by_count: 0.0,
                distinct_flag: 0.0,
                limit_present: 0.0,
                max_join_cardinality: 1000.0,
            },
            ActualCost {
                cpu_time_ms: 5.0,
                memory_peak_mb: 12.0,
                memory_avg_mb: 8.0,
                io_storage_ops: 150,
                io_storage_bytes: 1024 * 128,
                io_network_ops: 0,
                io_network_bytes: 0,
                locks_acquired: 2,
                lock_hold_time_ms: 0.5,
                lock_contention_score: 0.1,
                vacuum_overhead: 0.0,
                wal_generation_bytes: 4096,
                replication_lag_ms: 0.0,
                cache_hit_ratio: 0.95,
                page_faults: 5,
                context_switches: 3,
            },
        ),
        // Complex aggregation
        (
            QueryFeatures {
                table_count: 3.0,
                join_count: 2.0,
                filter_count: 3.0,
                aggregate_count: 3.0,
                subquery_count: 0.0,
                cte_count: 0.0,
                window_function_count: 0.0,
                order_by_count: 1.0,
                group_by_count: 2.0,
                distinct_flag: 0.0,
                limit_present: 0.0,
                max_join_cardinality: 10000.0,
            },
            ActualCost {
                cpu_time_ms: 25.0,
                memory_peak_mb: 45.0,
                memory_avg_mb: 35.0,
                io_storage_ops: 500,
                io_storage_bytes: 1024 * 1024,
                io_network_ops: 0,
                io_network_bytes: 0,
                locks_acquired: 3,
                lock_hold_time_ms: 1.2,
                lock_contention_score: 0.2,
                vacuum_overhead: 0.0,
                wal_generation_bytes: 8192,
                replication_lag_ms: 0.0,
                cache_hit_ratio: 0.90,
                page_faults: 20,
                context_switches: 10,
            },
        ),
        // Very complex multi-join with window functions
        (
            QueryFeatures {
                table_count: 5.0,
                join_count: 4.0,
                filter_count: 5.0,
                aggregate_count: 2.0,
                subquery_count: 1.0,
                cte_count: 1.0,
                window_function_count: 2.0,
                order_by_count: 2.0,
                group_by_count: 2.0,
                distinct_flag: 1.0,
                limit_present: 0.0,
                max_join_cardinality: 100000.0,
            },
            ActualCost {
                cpu_time_ms: 150.0,
                memory_peak_mb: 256.0,
                memory_avg_mb: 180.0,
                io_storage_ops: 2000,
                io_storage_bytes: 1024 * 1024 * 10,
                io_network_ops: 0,
                io_network_bytes: 0,
                locks_acquired: 5,
                lock_hold_time_ms: 5.0,
                lock_contention_score: 0.4,
                vacuum_overhead: 0.0,
                wal_generation_bytes: 32768,
                replication_lag_ms: 0.0,
                cache_hit_ratio: 0.85,
                page_faults: 100,
                context_switches: 50,
            },
        ),
    ];

    println!("Training on {} diverse query patterns...\n", training_data.len());

    // Train multiple epochs
    for epoch in 1..=20 {
        for (features, actual_cost) in &training_data {
            model.train(features, actual_cost);
        }

        if epoch % 5 == 0 {
            println!("Epoch {}/20", epoch);

            // Test predictions after this epoch
            for (idx, (features, actual_cost)) in training_data.iter().enumerate() {
                let predicted = model.predict(features);
                let cpu_error = ((predicted.cpu_time_ms - actual_cost.cpu_time_ms).abs()
                    / actual_cost.cpu_time_ms * 100.0).min(100.0);
                let mem_error = ((predicted.memory_peak_mb - actual_cost.memory_peak_mb).abs()
                    / actual_cost.memory_peak_mb * 100.0).min(100.0);

                println!("  Query {}: CPU {:.1}ms (actual {:.1}ms, error {:.1}%), Mem {:.1}MB (actual {:.1}MB, error {:.1}%)",
                    idx + 1,
                    predicted.cpu_time_ms,
                    actual_cost.cpu_time_ms,
                    cpu_error,
                    predicted.memory_peak_mb,
                    actual_cost.memory_peak_mb,
                    mem_error
                );
            }
            println!();
        }
    }

    println!("\nFinal Test: Can model distinguish complexity?");

    // Simple query
    let simple = QueryFeatures {
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

    // Complex query
    let complex = QueryFeatures {
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

    let simple_pred = model.predict(&simple);
    let complex_pred = model.predict(&complex);

    println!("  Simple query:  CPU {:.2}ms, Memory {:.2}MB",
        simple_pred.cpu_time_ms, simple_pred.memory_peak_mb);
    println!("  Complex query: CPU {:.2}ms, Memory {:.2}MB",
        complex_pred.cpu_time_ms, complex_pred.memory_peak_mb);

    if complex_pred.cpu_time_ms > simple_pred.cpu_time_ms * 1.5 {
        println!("  ✓ Model correctly distinguishes complexity");
    } else {
        println!("  ✗ Model failed to distinguish complexity");
    }
}
