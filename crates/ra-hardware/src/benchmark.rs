//! Hardware microbenchmarks for cost model calibration.
//!
//! Measures actual hardware performance characteristics at startup
//! to replace static cost constants. Addresses the 100x variance
//! between HDD and `NVMe` that static models cannot capture.

use std::hint::black_box;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::debug;

/// Number of benchmark iterations for statistical stability.
const BENCHMARK_ITERATIONS: usize = 5;

/// Size of the temporary file for I/O benchmarks (bytes).
const IO_BENCHMARK_FILE_SIZE: usize = 100 * 1024 * 1024; // 100 MB

/// Block size for random I/O benchmark (bytes).
const RANDOM_IO_BLOCK_SIZE: usize = 8 * 1024; // 8 KB

/// Number of random read operations per iteration.
const RANDOM_IO_OPERATIONS: usize = 1000;

/// Number of tuples for CPU benchmark.
const CPU_TUPLE_COUNT: usize = 1_000_000;

/// Cached benchmark results, computed once.
static CACHED_MEASUREMENTS: OnceLock<HardwareMeasurements> = OnceLock::new();

/// Raw measurements from hardware microbenchmarks.
///
/// All throughput values are in MB/s, latencies in nanoseconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HardwareMeasurements {
    /// Sequential read throughput in MB/s.
    pub sequential_read_mbps: f64,
    /// Random read throughput in MB/s (8KB blocks).
    pub random_read_mbps: f64,
    /// CPU cost per tuple in nanoseconds.
    pub cpu_tuple_cost_ns: f64,
    /// L1-resident access latency in nanoseconds.
    pub l1_latency_ns: f64,
    /// L2-resident access latency in nanoseconds.
    pub l2_latency_ns: f64,
    /// L3-resident access latency in nanoseconds.
    pub l3_latency_ns: f64,
    /// DRAM access latency in nanoseconds.
    pub dram_latency_ns: f64,
}

impl HardwareMeasurements {
    /// Ratio of random to sequential I/O cost.
    ///
    /// Higher values mean random I/O is relatively more expensive
    /// (typical for HDD: 300:1, `NVMe`: 1.2:1).
    #[must_use]
    pub fn random_io_ratio(&self) -> f64 {
        if self.random_read_mbps <= 0.0 {
            return 300.0; // Fallback: assume HDD
        }
        (self.sequential_read_mbps / self.random_read_mbps).max(1.0)
    }

    /// L2 miss penalty relative to L1.
    #[must_use]
    pub fn l2_miss_penalty(&self) -> f64 {
        if self.l1_latency_ns <= 0.0 {
            return 3.0;
        }
        (self.l2_latency_ns / self.l1_latency_ns).max(1.0)
    }

    /// L3 miss penalty relative to L2.
    #[must_use]
    pub fn l3_miss_penalty(&self) -> f64 {
        if self.l2_latency_ns <= 0.0 {
            return 4.0;
        }
        (self.l3_latency_ns / self.l2_latency_ns).max(1.0)
    }

    /// DRAM miss penalty relative to L3.
    #[must_use]
    pub fn dram_miss_penalty(&self) -> f64 {
        if self.l3_latency_ns <= 0.0 {
            return 8.0;
        }
        (self.dram_latency_ns / self.l3_latency_ns).max(1.0)
    }

    /// Default measurements for a typical `NVMe` SSD server.
    ///
    /// Used as fallback when benchmarks cannot run or are disabled.
    #[must_use]
    pub fn default_nvme() -> Self {
        Self {
            sequential_read_mbps: 3500.0,
            random_read_mbps: 3000.0,
            cpu_tuple_cost_ns: 10.0,
            l1_latency_ns: 1.0,
            l2_latency_ns: 4.0,
            l3_latency_ns: 12.0,
            dram_latency_ns: 80.0,
        }
    }

    /// Default measurements for a typical SATA SSD.
    #[must_use]
    pub fn default_sata_ssd() -> Self {
        Self {
            sequential_read_mbps: 550.0,
            random_read_mbps: 400.0,
            cpu_tuple_cost_ns: 10.0,
            l1_latency_ns: 1.0,
            l2_latency_ns: 4.0,
            l3_latency_ns: 12.0,
            dram_latency_ns: 80.0,
        }
    }

    /// Default measurements for a typical 7200 RPM HDD.
    #[must_use]
    pub fn default_hdd() -> Self {
        Self {
            sequential_read_mbps: 150.0,
            random_read_mbps: 0.5,
            cpu_tuple_cost_ns: 10.0,
            l1_latency_ns: 1.0,
            l2_latency_ns: 4.0,
            l3_latency_ns: 12.0,
            dram_latency_ns: 80.0,
        }
    }
}

/// Configuration for hardware benchmarks.
#[derive(Debug, Clone)]
pub struct BenchmarkConfig {
    /// Whether to run benchmarks at all.
    pub enabled: bool,
    /// Maximum time allowed for all benchmarks combined.
    pub timeout: Duration,
    /// Override sequential I/O measurement (MB/s).
    pub override_sequential_io: Option<f64>,
    /// Override random I/O measurement (MB/s).
    pub override_random_io: Option<f64>,
    /// Override CPU tuple cost (ns).
    pub override_cpu_tuple_cost: Option<f64>,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout: Duration::from_secs(10),
            override_sequential_io: None,
            override_random_io: None,
            override_cpu_tuple_cost: None,
        }
    }
}

impl BenchmarkConfig {
    /// Config that disables all benchmarks and uses defaults.
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }
}

/// Get cached benchmark results, running benchmarks on first call.
///
/// Results are cached via `OnceLock` so benchmarks only run once
/// per process lifetime. Uses default config.
#[must_use]
pub fn get_measurements() -> &'static HardwareMeasurements {
    CACHED_MEASUREMENTS.get_or_init(|| {
        run_benchmarks(&BenchmarkConfig::default())
    })
}

/// Run all hardware microbenchmarks with the given config.
///
/// Falls back to default measurements if benchmarks fail or are
/// disabled.
#[must_use]
pub fn run_benchmarks(config: &BenchmarkConfig) -> HardwareMeasurements {
    if !config.enabled {
        debug!("hardware benchmarks disabled, using NVMe defaults");
        return apply_overrides(
            HardwareMeasurements::default_nvme(),
            config,
        );
    }

    let deadline = Instant::now() + config.timeout;

    let sequential_read_mbps = benchmark_with_deadline(
        deadline,
        benchmark_sequential_io,
        3500.0,
    );
    let random_read_mbps = benchmark_with_deadline(
        deadline,
        benchmark_random_io,
        3000.0,
    );
    let cpu_tuple_cost_ns = benchmark_with_deadline(
        deadline,
        benchmark_cpu_tuple,
        10.0,
    );
    let (l1, l2, l3, dram) = benchmark_cache_hierarchy(deadline);

    let measurements = HardwareMeasurements {
        sequential_read_mbps,
        random_read_mbps,
        cpu_tuple_cost_ns,
        l1_latency_ns: l1,
        l2_latency_ns: l2,
        l3_latency_ns: l3,
        dram_latency_ns: dram,
    };

    debug!(
        seq_mbps = measurements.sequential_read_mbps,
        rand_mbps = measurements.random_read_mbps,
        cpu_ns = measurements.cpu_tuple_cost_ns,
        io_ratio = measurements.random_io_ratio(),
        "hardware calibration complete"
    );

    apply_overrides(measurements, config)
}

/// Apply manual overrides from config to measurements.
fn apply_overrides(
    mut m: HardwareMeasurements,
    config: &BenchmarkConfig,
) -> HardwareMeasurements {
    if let Some(v) = config.override_sequential_io {
        m.sequential_read_mbps = v;
    }
    if let Some(v) = config.override_random_io {
        m.random_read_mbps = v;
    }
    if let Some(v) = config.override_cpu_tuple_cost {
        m.cpu_tuple_cost_ns = v;
    }
    m
}

/// Run a benchmark function, returning fallback if deadline exceeded.
fn benchmark_with_deadline<F: FnOnce() -> f64>(
    deadline: Instant,
    f: F,
    fallback: f64,
) -> f64 {
    if Instant::now() >= deadline {
        return fallback;
    }
    f()
}

/// Benchmark sequential I/O throughput.
///
/// Creates a temporary file and reads it sequentially, measuring
/// throughput in MB/s. Uses multiple iterations and takes the median.
fn benchmark_sequential_io() -> f64 {
    let dir = std::env::temp_dir();
    let path = dir.join("ra_bench_seq_io");

    // Write test file
    if create_benchmark_file(&path, IO_BENCHMARK_FILE_SIZE).is_err() {
        return 3500.0; // Fallback
    }

    let mut results = Vec::with_capacity(BENCHMARK_ITERATIONS);
    let mut buf = vec![0u8; 1024 * 1024]; // 1 MB read buffer

    for _ in 0..BENCHMARK_ITERATIONS {
        let Ok(mut file) = std::fs::File::open(&path) else {
            continue;
        };

        // Warmup: read first MB
        let _ = file.read(&mut buf);
        let _ = file.seek(SeekFrom::Start(0));

        let start = Instant::now();
        let mut total_read = 0usize;
        loop {
            match file.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    black_box(buf[0]);
                    total_read += n;
                }
            }
        }
        let elapsed = start.elapsed();

        if elapsed.as_nanos() > 0 && total_read > 0 {
            #[expect(clippy::cast_precision_loss)]
            let mbps = total_read as f64
                / (1024.0 * 1024.0)
                / elapsed.as_secs_f64();
            results.push(mbps);
        }
    }

    let _ = std::fs::remove_file(&path);
    median(&mut results).unwrap_or(3500.0)
}

/// Benchmark random I/O throughput.
///
/// Reads random 8KB blocks from a temporary file, measuring
/// throughput in MB/s.
fn benchmark_random_io() -> f64 {
    let dir = std::env::temp_dir();
    let path = dir.join("ra_bench_rand_io");

    if create_benchmark_file(&path, IO_BENCHMARK_FILE_SIZE).is_err() {
        return 3000.0; // Fallback
    }

    let mut results = Vec::with_capacity(BENCHMARK_ITERATIONS);
    let mut buf = vec![0u8; RANDOM_IO_BLOCK_SIZE];

    // Simple LCG for deterministic pseudo-random offsets
    let max_offset =
        (IO_BENCHMARK_FILE_SIZE - RANDOM_IO_BLOCK_SIZE) as u64;

    for iter in 0..BENCHMARK_ITERATIONS {
        let Ok(mut file) = std::fs::File::open(&path) else {
            continue;
        };

        let mut rng_state: u64 = 0x0005_DEEC_E66D_u64
            .wrapping_add(iter as u64);

        let start = Instant::now();
        let mut total_read = 0usize;

        for _ in 0..RANDOM_IO_OPERATIONS {
            // LCG step
            rng_state = rng_state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let offset = rng_state % max_offset;

            if file.seek(SeekFrom::Start(offset)).is_err() {
                continue;
            }
            if let Ok(n) = file.read(&mut buf) {
                black_box(buf[0]);
                total_read += n;
            }
        }
        let elapsed = start.elapsed();

        if elapsed.as_nanos() > 0 && total_read > 0 {
            #[expect(clippy::cast_precision_loss)]
            let mbps = total_read as f64
                / (1024.0 * 1024.0)
                / elapsed.as_secs_f64();
            results.push(mbps);
        }
    }

    let _ = std::fs::remove_file(&path);
    median(&mut results).unwrap_or(3000.0)
}

/// Benchmark CPU tuple processing cost.
///
/// Iterates over simulated tuples, measuring nanoseconds per tuple.
fn benchmark_cpu_tuple() -> f64 {
    let mut results = Vec::with_capacity(BENCHMARK_ITERATIONS);

    // Allocate tuples: (id: i64, value: i64, flag: i64)
    #[expect(clippy::cast_possible_wrap)]
    let tuples: Vec<[i64; 3]> = (0..CPU_TUPLE_COUNT as i64)
        .map(|i| [i, i.wrapping_mul(17), i % 7])
        .collect();

    for _ in 0..BENCHMARK_ITERATIONS {
        let start = Instant::now();
        let mut sum: i64 = 0;
        for tuple in &tuples {
            sum = sum.wrapping_add(tuple[0]);
            sum = sum.wrapping_add(tuple[1]);
            if tuple[2] > 3 {
                sum = sum.wrapping_add(1);
            }
        }
        black_box(sum);
        let elapsed = start.elapsed();

        #[expect(clippy::cast_precision_loss)]
        let ns_per_tuple =
            elapsed.as_nanos() as f64 / CPU_TUPLE_COUNT as f64;
        results.push(ns_per_tuple);
    }

    median(&mut results).unwrap_or(10.0)
}

/// Benchmark cache hierarchy latencies.
///
/// Uses pointer-chasing with different working set sizes to
/// measure L1, L2, L3, and DRAM access latencies.
///
/// Returns (`l1_ns`, `l2_ns`, `l3_ns`, `dram_ns`).
fn benchmark_cache_hierarchy(deadline: Instant) -> (f64, f64, f64, f64) {
    if Instant::now() >= deadline {
        return (1.0, 4.0, 12.0, 80.0);
    }

    // Working set sizes targeting each cache level.
    // L1: 16 KB (fits in 32-64 KB L1d)
    // L2: 256 KB (fits in 256 KB-1 MB L2)
    // L3: 8 MB (fits in 8-32 MB L3)
    // DRAM: 64 MB (exceeds most L3 caches)
    let l1 = cache_latency_ns(16 * 1024);
    let l2 = cache_latency_ns(256 * 1024);
    let l3 = cache_latency_ns(8 * 1024 * 1024);
    let dram = if Instant::now() < deadline {
        cache_latency_ns(64 * 1024 * 1024)
    } else {
        80.0
    };

    // Ensure monotonicity: each level must be >= the previous
    let l2 = l2.max(l1);
    let l3 = l3.max(l2);
    let dram = dram.max(l3);

    (l1, l2, l3, dram)
}

/// Measure average access latency for a given working set size.
///
/// Uses a pointer-chasing pattern that defeats hardware prefetchers,
/// giving a more accurate measurement of cache/memory latency.
fn cache_latency_ns(working_set_bytes: usize) -> f64 {
    let count = (working_set_bytes / std::mem::size_of::<usize>()).max(64);
    let mut array: Vec<usize> = (0..count).collect();

    // Create a random permutation cycle using Fisher-Yates shuffle
    // with a deterministic seed, then link into a single cycle.
    let mut rng_state: u64 = 0xDEAD_BEEF_CAFE_BABE;
    for i in (1..count).rev() {
        rng_state = rng_state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        #[expect(clippy::cast_possible_truncation)]
        let j = (rng_state as usize) % (i + 1);
        array.swap(i, j);
    }

    // Convert permutation to pointer-chase: array[i] = next index
    let mut chase = vec![0usize; count];
    for i in 0..count {
        chase[i] = array[(i + 1) % count];
    }

    // Warmup
    let mut idx = 0;
    for _ in 0..count {
        idx = chase[idx];
    }
    black_box(idx);

    // Measure
    let iterations = 100_000.min(count * 10);
    idx = 0;
    let start = Instant::now();
    for _ in 0..iterations {
        idx = chase[idx];
    }
    black_box(idx);
    let elapsed = start.elapsed();

    #[expect(clippy::cast_precision_loss)]
    let ns = elapsed.as_nanos() as f64 / iterations as f64;
    ns.max(0.5) // Floor at 0.5 ns
}

/// Create a temporary file filled with deterministic data.
fn create_benchmark_file(
    path: &std::path::Path,
    size: usize,
) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    let chunk = vec![0xABu8; 1024 * 1024]; // 1 MB chunks
    let mut written = 0;
    while written < size {
        let to_write = (size - written).min(chunk.len());
        file.write_all(&chunk[..to_write])?;
        written += to_write;
    }
    file.sync_all()?;
    Ok(())
}

/// Compute the median of a mutable slice.
fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        Some(values[mid - 1].midpoint(values[mid]))
    } else {
        Some(values[mid])
    }
}

#[cfg(test)]
#[expect(clippy::float_cmp)]
#[expect(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn default_nvme_measurements_reasonable() {
        let m = HardwareMeasurements::default_nvme();
        assert!(m.sequential_read_mbps > 1000.0);
        assert!(m.random_read_mbps > 1000.0);
        assert!(m.cpu_tuple_cost_ns > 0.0);
        assert!(m.random_io_ratio() < 2.0);
    }

    #[test]
    fn default_hdd_measurements_high_ratio() {
        let m = HardwareMeasurements::default_hdd();
        assert!(m.random_io_ratio() > 100.0);
    }

    #[test]
    fn default_sata_ssd_measurements_moderate_ratio() {
        let m = HardwareMeasurements::default_sata_ssd();
        let ratio = m.random_io_ratio();
        assert!(ratio > 1.0);
        assert!(ratio < 5.0);
    }

    #[test]
    fn cache_miss_penalties_increase() {
        let m = HardwareMeasurements::default_nvme();
        assert!(m.l2_miss_penalty() >= 1.0);
        assert!(m.l3_miss_penalty() >= 1.0);
        assert!(m.dram_miss_penalty() >= 1.0);
    }

    #[test]
    fn disabled_config_returns_defaults() {
        let config = BenchmarkConfig::disabled();
        let m = run_benchmarks(&config);
        assert_eq!(m.sequential_read_mbps, 3500.0);
        assert_eq!(m.random_read_mbps, 3000.0);
    }

    #[test]
    fn overrides_applied() {
        let config = BenchmarkConfig {
            enabled: false,
            override_sequential_io: Some(999.0),
            override_random_io: Some(888.0),
            override_cpu_tuple_cost: Some(42.0),
            ..BenchmarkConfig::default()
        };
        let m = run_benchmarks(&config);
        assert!((m.sequential_read_mbps - 999.0).abs() < f64::EPSILON);
        assert!((m.random_read_mbps - 888.0).abs() < f64::EPSILON);
        assert!((m.cpu_tuple_cost_ns - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn median_odd() {
        let mut v = vec![5.0, 1.0, 3.0];
        assert_eq!(median(&mut v), Some(3.0));
    }

    #[test]
    fn median_even() {
        let mut v = vec![4.0, 1.0, 3.0, 2.0];
        assert_eq!(median(&mut v), Some(2.5));
    }

    #[test]
    fn median_empty() {
        let mut v: Vec<f64> = vec![];
        assert_eq!(median(&mut v), None);
    }

    #[test]
    fn median_single() {
        let mut v = vec![42.0];
        assert_eq!(median(&mut v), Some(42.0));
    }

    #[test]
    fn cpu_tuple_benchmark_runs() {
        let ns = benchmark_cpu_tuple();
        assert!(ns > 0.0);
        assert!(ns < 10_000.0); // Should be well under 10 us/tuple
    }

    #[test]
    fn cache_latency_l1_faster_than_dram() {
        let l1 = cache_latency_ns(16 * 1024);
        let dram = cache_latency_ns(64 * 1024 * 1024);
        assert!(l1 < dram);
    }

    #[test]
    fn benchmark_config_default_enabled() {
        let config = BenchmarkConfig::default();
        assert!(config.enabled);
        assert_eq!(config.timeout, Duration::from_secs(10));
    }

    // Synthetic hardware profile tests
    #[test]
    fn nvme_profile_prefers_index_scan() {
        let m = HardwareMeasurements::default_nvme();
        // On NVMe, random I/O is almost as fast as sequential
        // so index scans should be favored
        assert!(m.random_io_ratio() < 2.0);
    }

    #[test]
    fn hdd_profile_prefers_sequential_scan() {
        let m = HardwareMeasurements::default_hdd();
        // On HDD, random I/O is 300x slower than sequential
        // so sequential scans should be strongly favored
        assert!(m.random_io_ratio() > 100.0);
    }

    #[test]
    fn sata_ssd_profile_moderate_preference() {
        let m = HardwareMeasurements::default_sata_ssd();
        let ratio = m.random_io_ratio();
        // SATA SSD has a small preference for sequential
        assert!(ratio > 1.0);
        assert!(ratio < 10.0);
    }

    #[test]
    fn measurements_serialization_roundtrip() {
        let m = HardwareMeasurements::default_nvme();
        let json = serde_json::to_string(&m)
            .expect("serialization should succeed");
        let deserialized: HardwareMeasurements =
            serde_json::from_str(&json)
                .expect("deserialization should succeed");
        assert_eq!(m, deserialized);
    }
}
