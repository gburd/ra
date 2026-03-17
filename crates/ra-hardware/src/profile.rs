//! Hardware profile describing available accelerators and their
//! performance characteristics.

use serde::{Deserialize, Serialize};

/// Describes the hardware capabilities of a system for cost-based
/// operator placement decisions.
///
/// All bandwidth values are in GB/s, latencies in nanoseconds unless
/// otherwise noted, and memory sizes in bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct HardwareProfile {
    /// Human-readable name for this profile.
    pub name: String,

    // -- CPU --
    /// Whether a CPU is available (always true in practice).
    pub cpu_available: bool,
    /// Number of physical CPU cores.
    pub cpu_cores: u32,
    /// CPU memory bandwidth in GB/s (per socket).
    pub cpu_memory_bandwidth_gbps: f64,
    /// L2 cache size in bytes (per core).
    pub l2_cache_bytes: u64,
    /// L3 cache size in bytes (shared).
    pub l3_cache_bytes: u64,
    /// L3 cache access latency in nanoseconds.
    pub l3_latency_ns: f64,
    /// DRAM access latency in nanoseconds.
    pub dram_latency_ns: f64,
    /// SIMD register width in bits (128=SSE, 256=AVX2, 512=AVX-512).
    pub simd_width_bits: u32,
    /// Number of NUMA nodes.
    pub numa_nodes: u32,
    /// Hardware memory-level parallelism (outstanding loads).
    pub memory_level_parallelism: u32,

    // -- GPU --
    /// Whether a GPU is available.
    pub gpu_available: bool,
    /// GPU device memory in bytes.
    pub gpu_memory_bytes: u64,
    /// GPU memory bandwidth in GB/s.
    pub gpu_memory_bandwidth_gbps: f64,
    /// Number of GPU streaming multiprocessors (SMs).
    pub gpu_sm_count: u32,
    /// Whether unified memory is supported.
    pub unified_memory_supported: bool,
    /// Whether page migration engine is available.
    pub page_migration_engine_available: bool,
    /// Unified memory page size in bytes.
    pub um_page_size_bytes: u64,
    /// Unified memory fault latency in microseconds.
    pub um_fault_latency_us: f64,
    /// Unified memory migration bandwidth in GB/s.
    pub um_migration_bandwidth_gbps: f64,
    /// Whether chunked (streaming) GPU transfer is supported.
    pub chunked_transfer_enabled: bool,

    // -- FPGA --
    /// Whether an FPGA is available.
    pub fpga_available: bool,
    /// FPGA clock frequency in MHz.
    pub fpga_clock_mhz: u32,
    /// FPGA block RAM capacity in bytes.
    pub fpga_bram_bytes: u64,
    /// Maximum pipeline depth in stages.
    pub fpga_max_pipeline_depth: u32,
    /// Reconfiguration time in milliseconds.
    pub fpga_reconfig_ms: u32,
    /// Whether FPGA is near storage (`SmartSSD`, CSD).
    pub fpga_near_storage: bool,
    /// Available LUTs for logic synthesis.
    pub fpga_available_luts: u64,
    /// Number of parallel regex/NFA engines.
    pub fpga_regex_engines: u32,

    // -- Interconnect --
    /// `PCIe` bandwidth in GB/s (host-to-device).
    pub pcie_bandwidth_gbps: f64,
    /// Storage bandwidth in GB/s.
    pub storage_bandwidth_gbps: f64,
}

impl HardwareProfile {
    /// Estimate available GPU memory after accounting for runtime
    /// overhead (driver, allocator metadata, etc.).
    #[must_use]
    pub fn available_gpu_memory_bytes(&self) -> u64 {
        self.gpu_memory_bytes * 9 / 10
    }

    /// A reasonable default profile for a modern server with a
    /// high-end GPU (e.g., NVIDIA A100 80 GB).
    #[must_use]
    pub fn gpu_server() -> Self {
        Self {
            name: "GPU Server (A100 80GB)".into(),
            cpu_available: true,
            cpu_cores: 64,
            cpu_memory_bandwidth_gbps: 50.0,
            l2_cache_bytes: 1_048_576,
            l3_cache_bytes: 67_108_864,
            l3_latency_ns: 35.0,
            dram_latency_ns: 90.0,
            simd_width_bits: 512,
            numa_nodes: 2,
            memory_level_parallelism: 16,
            gpu_available: true,
            gpu_memory_bytes: 85_899_345_920,
            gpu_memory_bandwidth_gbps: 2039.0,
            gpu_sm_count: 108,
            unified_memory_supported: true,
            page_migration_engine_available: true,
            um_page_size_bytes: 65_536,
            um_fault_latency_us: 20.0,
            um_migration_bandwidth_gbps: 12.0,
            chunked_transfer_enabled: true,
            fpga_available: false,
            fpga_clock_mhz: 0,
            fpga_bram_bytes: 0,
            fpga_max_pipeline_depth: 0,
            fpga_reconfig_ms: 0,
            fpga_near_storage: false,
            fpga_available_luts: 0,
            fpga_regex_engines: 0,
            pcie_bandwidth_gbps: 25.0,
            storage_bandwidth_gbps: 7.0,
        }
    }

    /// A profile for an FPGA-accelerated storage appliance
    /// (e.g., Xilinx Alveo U280).
    #[must_use]
    pub fn fpga_appliance() -> Self {
        Self {
            name: "FPGA Appliance (Alveo U280)".into(),
            cpu_available: true,
            cpu_cores: 32,
            cpu_memory_bandwidth_gbps: 50.0,
            l2_cache_bytes: 1_048_576,
            l3_cache_bytes: 33_554_432,
            l3_latency_ns: 35.0,
            dram_latency_ns: 90.0,
            simd_width_bits: 256,
            numa_nodes: 1,
            memory_level_parallelism: 12,
            gpu_available: false,
            gpu_memory_bytes: 0,
            gpu_memory_bandwidth_gbps: 0.0,
            gpu_sm_count: 0,
            unified_memory_supported: false,
            page_migration_engine_available: false,
            um_page_size_bytes: 0,
            um_fault_latency_us: 0.0,
            um_migration_bandwidth_gbps: 0.0,
            chunked_transfer_enabled: false,
            fpga_available: true,
            fpga_clock_mhz: 300,
            fpga_bram_bytes: 41_943_040,
            fpga_max_pipeline_depth: 64,
            fpga_reconfig_ms: 50,
            fpga_near_storage: true,
            fpga_available_luts: 1_304_000,
            fpga_regex_engines: 16,
            pcie_bandwidth_gbps: 15.0,
            storage_bandwidth_gbps: 7.0,
        }
    }

    /// A CPU-only profile for a modern analytics server (no
    /// accelerators).
    #[must_use]
    pub fn cpu_only() -> Self {
        Self {
            name: "CPU-Only Server (2x Xeon)".into(),
            cpu_available: true,
            cpu_cores: 64,
            cpu_memory_bandwidth_gbps: 100.0,
            l2_cache_bytes: 2_097_152,
            l3_cache_bytes: 107_374_182_400,
            l3_latency_ns: 40.0,
            dram_latency_ns: 85.0,
            simd_width_bits: 512,
            numa_nodes: 2,
            memory_level_parallelism: 20,
            gpu_available: false,
            gpu_memory_bytes: 0,
            gpu_memory_bandwidth_gbps: 0.0,
            gpu_sm_count: 0,
            unified_memory_supported: false,
            page_migration_engine_available: false,
            um_page_size_bytes: 0,
            um_fault_latency_us: 0.0,
            um_migration_bandwidth_gbps: 0.0,
            chunked_transfer_enabled: false,
            fpga_available: false,
            fpga_clock_mhz: 0,
            fpga_bram_bytes: 0,
            fpga_max_pipeline_depth: 0,
            fpga_reconfig_ms: 0,
            fpga_near_storage: false,
            fpga_available_luts: 0,
            fpga_regex_engines: 0,
            pcie_bandwidth_gbps: 0.0,
            storage_bandwidth_gbps: 7.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_server_profile() {
        let p = HardwareProfile::gpu_server();
        assert!(p.gpu_available);
        assert!(!p.fpga_available);
        assert!(p.gpu_memory_bytes > 0);
        assert!(p.gpu_sm_count > 0);
    }

    #[test]
    fn fpga_appliance_profile() {
        let p = HardwareProfile::fpga_appliance();
        assert!(!p.gpu_available);
        assert!(p.fpga_available);
        assert!(p.fpga_bram_bytes > 0);
        assert!(p.fpga_near_storage);
    }

    #[test]
    fn cpu_only_profile() {
        let p = HardwareProfile::cpu_only();
        assert!(!p.gpu_available);
        assert!(!p.fpga_available);
        assert!(p.cpu_cores > 0);
    }

    #[test]
    fn available_gpu_memory() {
        let p = HardwareProfile::gpu_server();
        let avail = p.available_gpu_memory_bytes();
        assert!(avail < p.gpu_memory_bytes);
        assert!(avail > p.gpu_memory_bytes * 8 / 10);
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn serialize_roundtrip() {
        let profile = HardwareProfile::gpu_server();
        let json = serde_json::to_string(&profile).expect("serialization should succeed");
        let deserialized: HardwareProfile =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(profile, deserialized);
    }
}
