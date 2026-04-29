//! CPU architecture models and performance characteristics.

use serde::{Deserialize, Serialize};

/// CPU instruction set architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CpuArchitecture {
    /// x86-64 (AMD64) architecture.
    X86_64,
    /// ARM 64-bit architecture (`AArch64`).
    ARM64,
    /// RISC-V 64-bit architecture.
    RISCV64,
    /// PowerPC 64-bit architecture.
    PowerPC64,
}

impl CpuArchitecture {
    /// Returns the typical instruction issue width for this architecture.
    #[must_use]
    pub fn typical_issue_width(self) -> u32 {
        match self {
            Self::ARM64 => 6,
            Self::X86_64 | Self::RISCV64 | Self::PowerPC64 => 4,
        }
    }

    /// Returns whether this architecture has hardware prefetching.
    #[must_use]
    pub fn has_hardware_prefetch(self) -> bool {
        match self {
            Self::X86_64 | Self::ARM64 | Self::PowerPC64 => true,
            Self::RISCV64 => false,
        }
    }
}

/// SIMD capabilities for vectorized operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SimdCapability {
    /// SSE (128-bit vectors, x86-64).
    SSE,
    /// AVX (256-bit vectors, x86-64).
    AVX,
    /// AVX2 (256-bit vectors with FMA, x86-64).
    AVX2,
    /// AVX-512 (512-bit vectors, x86-64).
    AVX512,
    /// NEON (128-bit vectors, ARM64).
    NEON,
    /// SVE (scalable vectors, ARM64).
    SVE,
    /// SVE2 (scalable vectors v2, ARM64).
    SVE2,
    /// Vector extension (RISC-V).
    RVV,
}

impl SimdCapability {
    /// Returns the vector register width in bits.
    #[must_use]
    pub fn vector_width_bits(self) -> u32 {
        match self {
            Self::SSE | Self::NEON => 128,
            Self::AVX512 => 512,
            Self::AVX | Self::AVX2 | Self::SVE | Self::SVE2 | Self::RVV => 256,
        }
    }

    /// Returns the number of 64-bit elements that fit in a vector.
    #[must_use]
    pub fn elements_f64(self) -> u32 {
        self.vector_width_bits() / 64
    }

    /// Returns the number of 32-bit elements that fit in a vector.
    #[must_use]
    pub fn elements_f32(self) -> u32 {
        self.vector_width_bits() / 32
    }

    /// Returns whether this SIMD supports fused multiply-add.
    #[must_use]
    pub fn has_fma(self) -> bool {
        match self {
            Self::SSE | Self::AVX => false,
            Self::AVX2 | Self::AVX512 | Self::NEON | Self::SVE | Self::SVE2 | Self::RVV => true,
        }
    }
}

/// Cache hierarchy for a CPU.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheHierarchy {
    /// L1 data cache size per core (bytes).
    pub l1d_bytes: u64,
    /// L1 data cache access latency (ns).
    pub l1d_latency_ns: f64,
    /// L1 data cache associativity.
    pub l1d_associativity: u32,
    /// L2 cache size per core (bytes).
    pub l2_bytes: u64,
    /// L2 cache access latency (ns).
    pub l2_latency_ns: f64,
    /// L2 cache associativity.
    pub l2_associativity: u32,
    /// L3 cache size shared across cores (bytes).
    pub l3_bytes: u64,
    /// L3 cache access latency (ns).
    pub l3_latency_ns: f64,
    /// L3 cache associativity.
    pub l3_associativity: u32,
    /// Cache line size (bytes).
    pub line_size_bytes: u32,
}

impl CacheHierarchy {
    /// Returns the total cache capacity (L1 + L2 + L3).
    #[must_use]
    pub fn total_capacity(&self) -> u64 {
        self.l1d_bytes + self.l2_bytes + self.l3_bytes
    }

    /// Typical cache hierarchy for Intel Xeon Ice Lake.
    #[must_use]
    pub fn intel_xeon_ice_lake() -> Self {
        Self {
            l1d_bytes: 49_152,
            l1d_latency_ns: 1.2,
            l1d_associativity: 12,
            l2_bytes: 1_310_720,
            l2_latency_ns: 4.0,
            l2_associativity: 20,
            l3_bytes: 58_720_256,
            l3_latency_ns: 20.0,
            l3_associativity: 11,
            line_size_bytes: 64,
        }
    }

    /// Typical cache hierarchy for AMD EPYC Milan.
    #[must_use]
    pub fn amd_epyc_milan() -> Self {
        Self {
            l1d_bytes: 32_768,
            l1d_latency_ns: 1.0,
            l1d_associativity: 8,
            l2_bytes: 524_288,
            l2_latency_ns: 3.5,
            l2_associativity: 8,
            l3_bytes: 268_435_456,
            l3_latency_ns: 18.0,
            l3_associativity: 16,
            line_size_bytes: 64,
        }
    }

    /// Typical cache hierarchy for Apple M2 (performance cores).
    #[must_use]
    pub fn apple_m2() -> Self {
        Self {
            l1d_bytes: 131_072,
            l1d_latency_ns: 0.8,
            l1d_associativity: 8,
            l2_bytes: 16_777_216,
            l2_latency_ns: 10.0,
            l2_associativity: 12,
            l3_bytes: 0,
            l3_latency_ns: 0.0,
            l3_associativity: 0,
            line_size_bytes: 128,
        }
    }

    /// Typical cache hierarchy for ARM Neoverse V1.
    #[must_use]
    pub fn arm_neoverse_v1() -> Self {
        Self {
            l1d_bytes: 65_536,
            l1d_latency_ns: 1.5,
            l1d_associativity: 4,
            l2_bytes: 1_048_576,
            l2_latency_ns: 8.0,
            l2_associativity: 8,
            l3_bytes: 33_554_432,
            l3_latency_ns: 25.0,
            l3_associativity: 16,
            line_size_bytes: 64,
        }
    }
}

/// A CPU model with architecture and performance characteristics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpuModel {
    /// Human-readable name.
    pub name: String,
    /// Instruction set architecture.
    pub architecture: CpuArchitecture,
    /// Number of physical cores.
    pub cores: u32,
    /// Hardware threads per physical core (e.g., 2 for SMT/hyperthreading).
    pub threads_per_core: u32,
    /// Base clock frequency (GHz).
    pub base_clock_ghz: f64,
    /// Boost clock frequency (GHz).
    pub boost_clock_ghz: f64,
    /// SIMD capabilities.
    pub simd: SimdCapability,
    /// Cache hierarchy.
    pub cache: CacheHierarchy,
    /// Memory bandwidth per socket (GB/s).
    pub memory_bandwidth_gbps: f64,
    /// DRAM access latency (ns).
    pub dram_latency_ns: f64,
    /// Thermal design power (Watts).
    pub tdp_watts: u32,
}

impl CpuModel {
    /// Returns the total number of hardware threads (cores x `threads_per_core`).
    #[must_use]
    pub fn logical_cores(&self) -> u32 {
        self.cores * self.threads_per_core
    }

    /// Intel Xeon Platinum 8380 (Ice Lake, 40 cores, 2.3 GHz base).
    #[must_use]
    pub fn intel_xeon_8380() -> Self {
        Self {
            name: "Intel Xeon Platinum 8380".into(),
            architecture: CpuArchitecture::X86_64,
            cores: 40,
            threads_per_core: 2,
            base_clock_ghz: 2.3,
            boost_clock_ghz: 3.4,
            simd: SimdCapability::AVX512,
            cache: CacheHierarchy::intel_xeon_ice_lake(),
            memory_bandwidth_gbps: 204.8,
            dram_latency_ns: 90.0,
            tdp_watts: 270,
        }
    }

    /// AMD EPYC 7763 (Milan, 64 cores, 2.45 GHz base).
    #[must_use]
    pub fn amd_epyc_7763() -> Self {
        Self {
            name: "AMD EPYC 7763".into(),
            architecture: CpuArchitecture::X86_64,
            cores: 64,
            threads_per_core: 2,
            base_clock_ghz: 2.45,
            boost_clock_ghz: 3.5,
            simd: SimdCapability::AVX2,
            cache: CacheHierarchy::amd_epyc_milan(),
            memory_bandwidth_gbps: 204.8,
            dram_latency_ns: 85.0,
            tdp_watts: 280,
        }
    }

    /// Apple M2 (4 performance + 4 efficiency cores, 3.5 GHz).
    #[must_use]
    pub fn apple_m2() -> Self {
        Self {
            name: "Apple M2".into(),
            architecture: CpuArchitecture::ARM64,
            cores: 8,
            threads_per_core: 1,
            base_clock_ghz: 3.5,
            boost_clock_ghz: 3.5,
            simd: SimdCapability::NEON,
            cache: CacheHierarchy::apple_m2(),
            memory_bandwidth_gbps: 100.0,
            dram_latency_ns: 70.0,
            tdp_watts: 25,
        }
    }

    /// ARM Graviton3 (64 cores, 2.6 GHz).
    #[must_use]
    pub fn arm_graviton3() -> Self {
        Self {
            name: "AWS Graviton3".into(),
            architecture: CpuArchitecture::ARM64,
            cores: 64,
            threads_per_core: 1,
            base_clock_ghz: 2.6,
            boost_clock_ghz: 2.6,
            simd: SimdCapability::SVE2,
            cache: CacheHierarchy::arm_neoverse_v1(),
            memory_bandwidth_gbps: 307.2,
            dram_latency_ns: 75.0,
            tdp_watts: 200,
        }
    }

    /// Intel Core i9-13900K (24 cores: 8P+16E, 3.0 GHz base).
    #[must_use]
    pub fn intel_core_i9_13900k() -> Self {
        Self {
            name: "Intel Core i9-13900K".into(),
            architecture: CpuArchitecture::X86_64,
            cores: 24,
            threads_per_core: 2,
            base_clock_ghz: 3.0,
            boost_clock_ghz: 5.8,
            simd: SimdCapability::AVX512,
            cache: CacheHierarchy::intel_xeon_ice_lake(),
            memory_bandwidth_gbps: 89.6,
            dram_latency_ns: 85.0,
            tdp_watts: 253,
        }
    }

    /// AMD Ryzen 9 7950X (16 cores, 4.5 GHz base).
    #[must_use]
    pub fn amd_ryzen_9_7950x() -> Self {
        Self {
            name: "AMD Ryzen 9 7950X".into(),
            architecture: CpuArchitecture::X86_64,
            cores: 16,
            threads_per_core: 2,
            base_clock_ghz: 4.5,
            boost_clock_ghz: 5.7,
            simd: SimdCapability::AVX2,
            cache: CacheHierarchy::amd_epyc_milan(),
            memory_bandwidth_gbps: 83.2,
            dram_latency_ns: 80.0,
            tdp_watts: 170,
        }
    }

    /// Raspberry Pi 4 (Broadcom BCM2711, 4 cores Cortex-A72, 1.8 GHz).
    #[must_use]
    pub fn raspberry_pi_4() -> Self {
        Self {
            name: "Raspberry Pi 4 (BCM2711)".into(),
            architecture: CpuArchitecture::ARM64,
            cores: 4,
            threads_per_core: 1,
            base_clock_ghz: 1.5,
            boost_clock_ghz: 1.8,
            simd: SimdCapability::NEON,
            cache: CacheHierarchy {
                l1d_bytes: 32_768,
                l1d_latency_ns: 1.5,
                l1d_associativity: 2,
                l2_bytes: 1_048_576,
                l2_latency_ns: 8.0,
                l2_associativity: 16,
                l3_bytes: 0,
                l3_latency_ns: 0.0,
                l3_associativity: 0,
                line_size_bytes: 64,
            },
            memory_bandwidth_gbps: 4.0,
            dram_latency_ns: 120.0,
            tdp_watts: 6,
        }
    }

    /// Intel Core i7-12700K (12 cores: 8P+4E, 3.6 GHz base).
    #[must_use]
    pub fn intel_core_i7_12700k() -> Self {
        Self {
            name: "Intel Core i7-12700K".into(),
            architecture: CpuArchitecture::X86_64,
            cores: 12,
            threads_per_core: 2,
            base_clock_ghz: 3.6,
            boost_clock_ghz: 5.0,
            simd: SimdCapability::AVX2,
            cache: CacheHierarchy {
                l1d_bytes: 49_152,
                l1d_latency_ns: 1.2,
                l1d_associativity: 12,
                l2_bytes: 1_310_720,
                l2_latency_ns: 4.0,
                l2_associativity: 10,
                l3_bytes: 25_165_824,
                l3_latency_ns: 15.0,
                l3_associativity: 12,
                line_size_bytes: 64,
            },
            memory_bandwidth_gbps: 76.8,
            dram_latency_ns: 82.0,
            tdp_watts: 190,
        }
    }

    /// Estimate CPU execution time for scanning n rows.
    #[must_use]
    pub fn scan_time_s(&self, rows: u64, bytes_per_row: u64) -> f64 {
        let total_bytes = rows as f64 * bytes_per_row as f64;
        let bandwidth_bytes = self.memory_bandwidth_gbps * 1e9;
        total_bytes / bandwidth_bytes
    }

    /// Estimate CPU execution time for a hash join.
    #[must_use]
    pub fn hash_join_time_s(&self, build_rows: u64, probe_rows: u64) -> f64 {
        let build_ns = build_rows as f64 * 100.0;
        let probe_ns = probe_rows as f64 * 50.0;
        (build_ns + probe_ns) / 1e9
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x86_64_issue_width() {
        assert_eq!(CpuArchitecture::X86_64.typical_issue_width(), 4);
    }

    #[test]
    fn arm64_issue_width() {
        assert_eq!(CpuArchitecture::ARM64.typical_issue_width(), 6);
    }

    #[test]
    fn avx512_vector_width() {
        assert_eq!(SimdCapability::AVX512.vector_width_bits(), 512);
    }

    #[test]
    fn avx512_f64_elements() {
        assert_eq!(SimdCapability::AVX512.elements_f64(), 8);
    }

    #[test]
    fn avx2_has_fma() {
        assert!(SimdCapability::AVX2.has_fma());
    }

    #[test]
    fn sse_no_fma() {
        assert!(!SimdCapability::SSE.has_fma());
    }

    #[test]
    fn intel_xeon_cache_total() {
        let cache = CacheHierarchy::intel_xeon_ice_lake();
        assert!(cache.total_capacity() > 50_000_000);
    }

    #[test]
    fn intel_xeon_8380_model() {
        let cpu = CpuModel::intel_xeon_8380();
        assert_eq!(cpu.cores, 40);
        assert_eq!(cpu.architecture, CpuArchitecture::X86_64);
        assert_eq!(cpu.simd, SimdCapability::AVX512);
    }

    #[test]
    fn amd_epyc_7763_model() {
        let cpu = CpuModel::amd_epyc_7763();
        assert_eq!(cpu.cores, 64);
        assert_eq!(cpu.simd, SimdCapability::AVX2);
    }

    #[test]
    fn apple_m2_model() {
        let cpu = CpuModel::apple_m2();
        assert_eq!(cpu.architecture, CpuArchitecture::ARM64);
        assert_eq!(cpu.simd, SimdCapability::NEON);
    }

    #[test]
    fn graviton3_model() {
        let cpu = CpuModel::arm_graviton3();
        assert_eq!(cpu.architecture, CpuArchitecture::ARM64);
        assert_eq!(cpu.simd, SimdCapability::SVE2);
    }

    #[test]
    fn scan_time_nonzero() {
        let cpu = CpuModel::intel_xeon_8380();
        let time = cpu.scan_time_s(1_000_000, 100);
        assert!(time > 0.0);
    }

    #[test]
    fn hash_join_time_nonzero() {
        let cpu = CpuModel::amd_epyc_7763();
        let time = cpu.hash_join_time_s(100_000, 1_000_000);
        assert!(time > 0.0);
    }

    #[test]
    fn powerpc64_issue_width() {
        assert_eq!(CpuArchitecture::PowerPC64.typical_issue_width(), 4);
    }

    #[test]
    fn powerpc64_has_prefetch() {
        assert!(CpuArchitecture::PowerPC64.has_hardware_prefetch());
    }

    #[test]
    fn logical_cores_with_smt() {
        let cpu = CpuModel::intel_xeon_8380();
        assert_eq!(cpu.threads_per_core, 2);
        assert_eq!(cpu.logical_cores(), 80);
    }

    #[test]
    fn logical_cores_no_smt() {
        let cpu = CpuModel::apple_m2();
        assert_eq!(cpu.threads_per_core, 1);
        assert_eq!(cpu.logical_cores(), 8);
    }

    #[test]
    fn raspberry_pi_4_model() {
        let cpu = CpuModel::raspberry_pi_4();
        assert_eq!(cpu.architecture, CpuArchitecture::ARM64);
        assert_eq!(cpu.cores, 4);
        assert_eq!(cpu.threads_per_core, 1);
        assert!(cpu.tdp_watts < 10);
    }

    #[test]
    fn intel_i7_12700k_model() {
        let cpu = CpuModel::intel_core_i7_12700k();
        assert_eq!(cpu.cores, 12);
        assert_eq!(cpu.threads_per_core, 2);
        assert_eq!(cpu.logical_cores(), 24);
    }

    #[test]
    fn neon_vector_width() {
        assert_eq!(SimdCapability::NEON.vector_width_bits(), 128);
    }

    #[test]
    fn sve_has_fma() {
        assert!(SimdCapability::SVE.has_fma());
    }

    #[test]
    fn rvv_elements_f32() {
        assert_eq!(SimdCapability::RVV.elements_f32(), 8);
    }

    #[test]
    fn amd_epyc_cache_larger_l3() {
        let cache = CacheHierarchy::amd_epyc_milan();
        assert!(cache.l3_bytes > cache.l2_bytes);
        assert!(cache.l3_bytes > 200_000_000);
    }

    #[test]
    fn apple_m2_no_l3() {
        let cache = CacheHierarchy::apple_m2();
        assert_eq!(cache.l3_bytes, 0);
        assert!(cache.l2_bytes > 10_000_000);
    }

    #[test]
    fn arm_neoverse_cache_hierarchy() {
        let cache = CacheHierarchy::arm_neoverse_v1();
        assert!(cache.l1d_bytes < cache.l2_bytes);
        assert!(cache.l2_bytes < cache.l3_bytes);
    }

    #[test]
    #[expect(
        clippy::expect_used,
        reason = "Test code appropriately uses expect for known-good serialization"
    )]
    fn serialize_roundtrip() {
        let cpu = CpuModel::intel_xeon_8380();
        let json = serde_json::to_string(&cpu).expect("serialization should succeed");
        let deserialized: CpuModel =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(cpu, deserialized);
    }
}
