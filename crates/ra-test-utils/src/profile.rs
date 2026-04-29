//! Test profile definitions and scaling methods.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

/// Hardware-adaptive test profile containing platform info and scale factors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestProfile {
    /// Platform information
    pub platform: PlatformInfo,
    /// Calibration benchmark results
    pub calibration: CalibrationResults,
    /// Scale factors relative to baseline
    pub scale_factors: ScaleFactors,
}

/// Platform hardware and OS information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformInfo {
    /// Unique platform identifier
    pub id: String,
    /// When this profile was created
    pub timestamp: DateTime<Utc>,
    /// Operating system name and version
    pub os: String,
    /// CPU architecture (`x86_64`, `aarch64`, `riscv64`, etc.)
    pub arch: String,
    /// CPU model name
    pub cpu_model: String,
    /// Number of CPU cores
    pub cpu_cores: u32,
    /// Total system memory in GB
    pub total_memory_gb: u64,
}

/// Calibration benchmark results for this platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationResults {
    /// Time to optimize simple 2-table join query (ms)
    pub simple_optimization_ms: f64,
    /// Time to optimize complex 4-table join query (ms)
    pub complex_optimization_ms: f64,
    /// E-graph saturation iterations for depth-2 expression
    pub egraph_saturation_iters: u64,
    /// Integer operations per millisecond
    pub int_ops_per_ms: u64,
    /// Memory bandwidth (MB/s)
    pub memory_bandwidth_mbps: u64,
}

/// Scale factors relative to baseline platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleFactors {
    /// Time scale relative to baseline (1.0 = same speed)
    pub time_scale: f64,
    /// Iteration scale relative to baseline
    pub iteration_scale: f64,
    /// Memory scale relative to baseline
    pub memory_scale: f64,
}

/// Cached current profile loaded once per test run.
#[expect(
    clippy::print_stderr,
    reason = "Profile loading status needs user feedback"
)]
static CURRENT_PROFILE: LazyLock<TestProfile> = LazyLock::new(|| {
    // Try to load from .ra-test-profile.toml, fall back to baseline
    if Path::new(".ra-test-profile.toml").exists() {
        match fs::read_to_string(".ra-test-profile.toml") {
            Ok(content) => match toml::from_str::<TestProfile>(&content) {
                Ok(profile) => {
                    eprintln!(
                        "Loaded test profile: {} (scale: {:.2}x)",
                        profile.platform.id, profile.scale_factors.time_scale
                    );
                    return profile;
                }
                Err(e) => {
                    eprintln!("Failed to parse .ra-test-profile.toml: {e}");
                }
            },
            Err(e) => {
                eprintln!("Failed to read .ra-test-profile.toml: {e}");
            }
        }
    }

    eprintln!("Using baseline test profile (AWS c7i.xlarge)");
    TestProfile::baseline()
});

impl TestProfile {
    /// Load the current test profile from .ra-test-profile.toml or return baseline.
    ///
    /// The profile is loaded once and cached for the entire test run.
    #[must_use]
    pub fn current() -> &'static Self {
        &CURRENT_PROFILE
    }

    /// Scale a time expectation (milliseconds) based on platform performance.
    ///
    /// # Example
    /// ```
    /// let profile = TestProfile::current();
    /// let expected_ms = profile.scale_time_ms(1000.0);
    /// assert!(duration.as_millis() < expected_ms as u128);
    /// ```
    #[must_use]
    pub fn scale_time_ms(&self, baseline_ms: f64) -> f64 {
        baseline_ms * self.scale_factors.time_scale
    }

    /// Scale an iteration count based on platform performance.
    ///
    /// # Example
    /// ```
    /// let profile = TestProfile::current();
    /// let max_iters = profile.scale_iterations(50);
    /// runner.with_iter_limit(max_iters);
    /// ```
    #[must_use]
    pub fn scale_iterations(&self, baseline: usize) -> usize {
        ((baseline as f64) * self.scale_factors.iteration_scale).ceil() as usize
    }

    /// Scale a memory limit (bytes) based on available memory.
    ///
    /// # Example
    /// ```
    /// let profile = TestProfile::current();
    /// let memory_limit = profile.scale_memory(1_000_000_000);
    /// config.set_memory_limit(memory_limit);
    /// ```
    #[must_use]
    pub fn scale_memory(&self, baseline_bytes: u64) -> u64 {
        ((baseline_bytes as f64) * self.scale_factors.memory_scale).ceil() as u64
    }

    /// Return the baseline profile for AWS c7i.xlarge (4 vCPU, 8GB RAM).
    ///
    /// This is our reference platform against which all other platforms are scaled.
    #[must_use]
    pub fn baseline() -> Self {
        TestProfile {
            platform: PlatformInfo {
                id: "aws-c7i-xlarge".to_string(),
                timestamp: Utc::now(),
                os: "Linux 6.1.0".to_string(),
                arch: "x86_64".to_string(),
                cpu_model: "Intel Xeon Sapphire Rapids".to_string(),
                cpu_cores: 4,
                total_memory_gb: 8,
            },
            calibration: CalibrationResults {
                simple_optimization_ms: 2.5,
                complex_optimization_ms: 6.4,
                egraph_saturation_iters: 50,
                int_ops_per_ms: 1_200_000,
                memory_bandwidth_mbps: 6400,
            },
            scale_factors: ScaleFactors {
                time_scale: 1.0,
                iteration_scale: 1.0,
                memory_scale: 1.0,
            },
        }
    }
}
