//! Platform calibration benchmarks.
//!
//! This MVP implementation uses simpler proxy benchmarks instead of actual
//! optimizer operations to avoid circular dependencies. The benchmarks still
//! measure relevant performance characteristics that correlate with optimizer
//! performance.

use crate::profile::{CalibrationResults, PlatformInfo, ScaleFactors, TestProfile};
use chrono::Utc;
use std::fs;
use std::time::{Duration, Instant};

/// Run calibration benchmarks and save profile to .ra-test-profile.toml.
///
/// This runs several benchmarks to measure platform performance:
/// - Simple query optimization (2-table join)
/// - Complex query optimization (4-table join)
/// - E-graph saturation iterations
/// - Integer operations per millisecond
/// - Memory bandwidth
///
/// Total calibration time: ~30 seconds for MVP (vs 90s in full RFC)
pub fn calibrate() -> anyhow::Result<TestProfile> {
    println!("Calibrating test expectations for this platform...");
    println!("This will take about 30 seconds.\n");

    // 1. Detect hardware
    let hw = ra_hardware::detect_hardware();
    let platform = PlatformInfo {
        id: format!("{}-{}-{}", hw.os_name, hw.arch, hw.cpu_model.replace(' ', "-")),
        timestamp: Utc::now(),
        os: format!("{} {}", hw.os_name, hw.os_version),
        arch: hw.arch.clone(),
        cpu_model: hw.cpu_model.clone(),
        cpu_cores: hw.cpu_cores,
        total_memory_gb: hw.available_memory / (1024 * 1024 * 1024),
    };

    // 2. Run micro-benchmarks (10 seconds total for MVP)
    print!("  Integer operations... ");
    let int_ops = benchmark_int_ops(Duration::from_secs(3));
    println!("{} ops/ms", int_ops);

    print!("  Memory bandwidth... ");
    let mem_bw = benchmark_memory_bandwidth(Duration::from_secs(3));
    println!("{} MB/s", mem_bw);

    // 3. Run optimizer benchmarks (20 seconds total for MVP)
    print!("  Simple optimization... ");
    let simple_opt = benchmark_simple_optimization(10);
    println!("{:.2}ms", simple_opt);

    print!("  Complex optimization... ");
    let complex_opt = benchmark_complex_optimization(5);
    println!("{:.2}ms", complex_opt);

    print!("  E-graph saturation... ");
    let saturation = benchmark_egraph_saturation(10);
    println!("{} iterations", saturation);

    // 4. Calculate scale factors relative to baseline
    let baseline = TestProfile::baseline();
    let scale_factors = ScaleFactors {
        time_scale: simple_opt / baseline.calibration.simple_optimization_ms,
        iteration_scale: saturation as f64 / baseline.calibration.egraph_saturation_iters as f64,
        memory_scale: platform.total_memory_gb as f64 / baseline.platform.total_memory_gb as f64,
    };

    let profile = TestProfile {
        platform,
        calibration: CalibrationResults {
            simple_optimization_ms: simple_opt,
            complex_optimization_ms: complex_opt,
            egraph_saturation_iters: saturation,
            int_ops_per_ms: int_ops,
            memory_bandwidth_mbps: mem_bw,
        },
        scale_factors: scale_factors.clone(),
    };

    // 5. Write to .ra-test-profile.toml
    let toml = toml::to_string_pretty(&profile)?;
    fs::write(".ra-test-profile.toml", toml)?;

    println!("\nCalibration complete!");
    println!("  Platform: {}", profile.platform.id);
    println!("  Time scale: {:.2}x ({})",
        scale_factors.time_scale,
        if scale_factors.time_scale > 1.5 { "slower" }
        else if scale_factors.time_scale < 0.7 { "faster" }
        else { "similar" }
    );
    println!("  Iteration scale: {:.2}x", scale_factors.iteration_scale);
    println!("  Memory scale: {:.2}x", scale_factors.memory_scale);
    println!("\nProfile saved to .ra-test-profile.toml");

    Ok(profile)
}

/// Benchmark simple optimization proxy (simulates 2-table join).
///
/// For the MVP, we use a computationally similar operation without
/// requiring the full optimizer dependency.
fn benchmark_simple_optimization(iterations: usize) -> f64 {

    // Create test data simulating table rows
    let mut left_data = Vec::with_capacity(1000);
    let mut right_data = Vec::with_capacity(1000);
    for i in 0..1000 {
        left_data.push((i, i * 2, i * 3));
        right_data.push((i, i * 4, i * 5));
    }

    // Warmup
    for _ in 0..5 {
        simulate_join_optimization(&left_data, &right_data);
    }

    // Measure
    let start = Instant::now();
    for _ in 0..iterations {
        simulate_join_optimization(&left_data, &right_data);
    }
    let elapsed = start.elapsed();

    elapsed.as_secs_f64() * 1000.0 / iterations as f64
}

/// Simulate join optimization workload.
fn simulate_join_optimization(left: &[(usize, usize, usize)], right: &[(usize, usize, usize)]) -> usize {
    // Simulate nested loop join with hash build
    let mut hash_map = std::collections::HashMap::new();
    for &(key, val1, val2) in left {
        hash_map.insert(key, (val1, val2));
    }

    let mut result_count = 0;
    for &(key, val1, val2) in right {
        if let Some(&(left_val1, left_val2)) = hash_map.get(&key) {
            // Simulate tuple materialization
            let _tuple = (key, left_val1, left_val2, val1, val2);
            result_count += 1;
        }
    }
    result_count
}

/// Benchmark complex optimization proxy (simulates 4-table join).
fn benchmark_complex_optimization(iterations: usize) -> f64 {

    // Create test data for 4 tables
    let tables: Vec<Vec<(usize, usize, usize)>> = (0..4)
        .map(|t| {
            (0..500)
                .map(|i| (i, i * (t + 1), i * (t + 2)))
                .collect()
        })
        .collect();

    // Warmup
    for _ in 0..3 {
        simulate_complex_join(&tables);
    }

    // Measure
    let start = Instant::now();
    for _ in 0..iterations {
        simulate_complex_join(&tables);
    }
    let elapsed = start.elapsed();

    elapsed.as_secs_f64() * 1000.0 / iterations as f64
}

/// Simulate complex 4-table join workload.
fn simulate_complex_join(tables: &[Vec<(usize, usize, usize)>]) -> usize {
    // Simulate joining 4 tables in pairs then joining results
    let j1 = simulate_join_optimization(&tables[0], &tables[1]);
    let j2 = simulate_join_optimization(&tables[2], &tables[3]);

    // Simulate final join (simplified)
    j1 + j2
}

/// Benchmark e-graph saturation iterations proxy.
///
/// For the MVP, simulates the iterative pattern matching and rewriting
/// workload without requiring the full egg dependency.
fn benchmark_egraph_saturation(iterations: usize) -> u64 {

    // Simulate e-graph saturation with pattern matching iterations
    let mut total_iters = 0u64;

    for _ in 0..iterations {
        let iters = simulate_saturation();
        total_iters += iters;
    }

    total_iters / iterations as u64
}

/// Simulate e-graph saturation workload.
fn simulate_saturation() -> u64 {
    // Simulate pattern matching and rewriting iterations
    let mut nodes = Vec::with_capacity(1000);
    for i in 0..100 {
        nodes.push((i, i % 10, i % 5)); // node id, op type, children count
    }

    let mut iteration_count = 0u64;
    let mut changed = true;

    while changed && iteration_count < 100 {
        changed = false;
        let mut new_nodes = Vec::new();

        // Simulate pattern matching
        for &(id, op, children) in &nodes {
            // Simulate rule application
            if op == 3 && children == 2 {
                // "Rewrite" by adding new node
                new_nodes.push((id + 1000, op + 1, children));
                changed = true;
            }
        }

        nodes.extend(new_nodes);
        iteration_count += 1;

        // Simulate saturation check
        if nodes.len() > 500 {
            break;
        }
    }

    iteration_count
}

/// Benchmark integer operations per millisecond.
fn benchmark_int_ops(duration: Duration) -> u64 {
    let mut count = 0u64;
    let mut x = 1u64;
    let start = Instant::now();

    while start.elapsed() < duration {
        // Simple integer operations that won't be optimized away
        for _ in 0..1000 {
            x = x.wrapping_mul(1234567).wrapping_add(89);
            x = x.rotate_left(7);
            x ^= x >> 13;
            count += 1;
        }
    }

    // Prevent optimization (use the result)
    if x == 0 {
        eprintln!("Unexpected!");
    }

    let elapsed_ms = start.elapsed().as_millis() as u64;
    count / elapsed_ms.max(1)
}

/// Benchmark memory bandwidth.
fn benchmark_memory_bandwidth(duration: Duration) -> u64 {
    const BUFFER_SIZE: usize = 8 * 1024 * 1024; // 8MB buffer
    let mut buffer = vec![0u8; BUFFER_SIZE];
    let mut total_bytes = 0u64;
    let start = Instant::now();

    while start.elapsed() < duration {
        // Write pattern
        for i in 0..BUFFER_SIZE {
            buffer[i] = (i & 0xFF) as u8;
        }

        // Read and modify pattern
        let mut sum = 0u64;
        for i in 0..BUFFER_SIZE {
            sum += buffer[i] as u64;
            buffer[i] = buffer[i].wrapping_add(1);
        }

        // Prevent optimization
        if sum == 0 {
            eprintln!("Unexpected!");
        }

        total_bytes += (BUFFER_SIZE * 2) as u64; // Read + write
    }

    let elapsed_secs = start.elapsed().as_secs_f64();
    ((total_bytes as f64 / elapsed_secs) / (1024.0 * 1024.0)) as u64
}