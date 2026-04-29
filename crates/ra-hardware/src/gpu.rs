//! GPU architecture models and performance characteristics.

use serde::{Deserialize, Serialize};

/// GPU vendor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GpuVendor {
    /// NVIDIA GPUs.
    NVIDIA,
    /// AMD GPUs.
    AMD,
    /// Intel GPUs.
    Intel,
    /// Apple Silicon GPUs.
    Apple,
}

/// GPU compute architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GpuArchitecture {
    /// NVIDIA Ampere (A100, RTX 30-series).
    NvidiaAmpere,
    /// NVIDIA Ada Lovelace (RTX 40-series).
    NvidiaAda,
    /// NVIDIA Hopper (H100, H200).
    NvidiaHopper,
    /// AMD RDNA 2 (RX 6000-series).
    AmdRDNA2,
    /// AMD RDNA 3 (RX 7000-series).
    AmdRDNA3,
    /// AMD CDNA 2 (MI200-series).
    AmdCDNA2,
    /// AMD CDNA 3 (MI300-series).
    AmdCDNA3,
    /// Intel Xe-HPG (Arc A-series).
    IntelXeHPG,
    /// Intel Xe-HPC (Ponte Vecchio).
    IntelXeHPC,
    /// Apple M-series GPU.
    AppleMSeries,
}

impl GpuArchitecture {
    /// Returns the vendor for this architecture.
    #[must_use]
    pub fn vendor(self) -> GpuVendor {
        match self {
            Self::NvidiaAmpere | Self::NvidiaAda | Self::NvidiaHopper => GpuVendor::NVIDIA,
            Self::AmdRDNA2 | Self::AmdRDNA3 | Self::AmdCDNA2 | Self::AmdCDNA3 => GpuVendor::AMD,
            Self::IntelXeHPG | Self::IntelXeHPC => GpuVendor::Intel,
            Self::AppleMSeries => GpuVendor::Apple,
        }
    }

    /// Returns whether this architecture has hardware ray tracing.
    #[must_use]
    pub fn has_ray_tracing(self) -> bool {
        match self {
            Self::NvidiaAmpere
            | Self::NvidiaAda
            | Self::NvidiaHopper
            | Self::AmdRDNA2
            | Self::AmdRDNA3
            | Self::IntelXeHPG => true,
            Self::AmdCDNA2 | Self::AmdCDNA3 | Self::IntelXeHPC | Self::AppleMSeries => false,
        }
    }

    /// Returns whether this is a datacenter/HPC architecture.
    #[must_use]
    pub fn is_datacenter(self) -> bool {
        match self {
            Self::NvidiaHopper | Self::AmdCDNA2 | Self::AmdCDNA3 | Self::IntelXeHPC => true,
            Self::NvidiaAmpere
            | Self::NvidiaAda
            | Self::AmdRDNA2
            | Self::AmdRDNA3
            | Self::IntelXeHPG
            | Self::AppleMSeries => false,
        }
    }
}

/// Memory type for GPU VRAM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GpuMemoryType {
    /// GDDR6 (consumer GPUs).
    GDDR6,
    /// GDDR6X (high-end NVIDIA GPUs).
    GDDR6X,
    /// `HBM2e` (datacenter GPUs).
    HBM2e,
    /// `HBM3` (next-gen datacenter GPUs).
    HBM3,
    /// Unified memory (Apple Silicon).
    Unified,
}

impl GpuMemoryType {
    /// Returns typical bandwidth (GB/s).
    #[must_use]
    pub fn typical_bandwidth_gbps(self) -> f64 {
        match self {
            Self::GDDR6 => 448.0,
            Self::GDDR6X => 1008.0,
            Self::HBM2e => 2039.0,
            Self::HBM3 => 3350.0,
            Self::Unified => 800.0,
        }
    }

    /// Returns typical access latency (ns).
    #[must_use]
    pub fn latency_ns(self) -> f64 {
        match self {
            Self::GDDR6 => 100.0,
            Self::GDDR6X => 90.0,
            Self::HBM2e => 50.0,
            Self::HBM3 => 45.0,
            Self::Unified => 70.0,
        }
    }
}

/// Host-to-device transfer characteristics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransferCharacteristics {
    /// `PCIe` bandwidth host-to-device (GB/s).
    pub pcie_bandwidth_gbps: f64,
    /// `PCIe` latency (microseconds).
    pub pcie_latency_us: f64,
    /// Whether unified memory is supported.
    pub unified_memory: bool,
    /// Unified memory page size (bytes).
    pub page_size_bytes: u64,
    /// Page migration bandwidth (GB/s).
    pub migration_bandwidth_gbps: f64,
}

impl TransferCharacteristics {
    /// `PCIe` Gen 3 x16 transfer characteristics.
    #[must_use]
    pub fn pcie_gen3_x16() -> Self {
        Self {
            pcie_bandwidth_gbps: 15.75,
            pcie_latency_us: 2.0,
            unified_memory: false,
            page_size_bytes: 0,
            migration_bandwidth_gbps: 0.0,
        }
    }

    /// `PCIe` Gen 4 x16 transfer characteristics.
    #[must_use]
    pub fn pcie_gen4_x16() -> Self {
        Self {
            pcie_bandwidth_gbps: 31.5,
            pcie_latency_us: 1.5,
            unified_memory: true,
            page_size_bytes: 65_536,
            migration_bandwidth_gbps: 12.0,
        }
    }

    /// `PCIe` Gen 5 x16 transfer characteristics.
    #[must_use]
    pub fn pcie_gen5_x16() -> Self {
        Self {
            pcie_bandwidth_gbps: 63.0,
            pcie_latency_us: 1.0,
            unified_memory: true,
            page_size_bytes: 65_536,
            migration_bandwidth_gbps: 20.0,
        }
    }

    /// Apple Silicon unified memory (no `PCIe`).
    #[must_use]
    pub fn unified_memory() -> Self {
        Self {
            pcie_bandwidth_gbps: 0.0,
            pcie_latency_us: 0.0,
            unified_memory: true,
            page_size_bytes: 16_384,
            migration_bandwidth_gbps: 800.0,
        }
    }

    /// Estimate transfer time for bytes from host to device (seconds).
    #[must_use]
    pub fn transfer_time_s(&self, bytes: u64) -> f64 {
        if self.unified_memory {
            0.0
        } else {
            let latency_s = self.pcie_latency_us * 1e-6;
            let bandwidth_bytes = self.pcie_bandwidth_gbps * 1e9;
            latency_s + bytes as f64 / bandwidth_bytes
        }
    }
}

/// A GPU model with architecture and performance characteristics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuModel {
    /// Human-readable name.
    pub name: String,
    /// GPU architecture.
    pub architecture: GpuArchitecture,
    /// GPU memory capacity (bytes).
    pub vram_bytes: u64,
    /// GPU memory type.
    pub memory_type: GpuMemoryType,
    /// GPU memory bandwidth (GB/s).
    pub memory_bandwidth_gbps: f64,
    /// Number of streaming multiprocessors (SMs) or compute units (CUs).
    pub compute_units: u32,
    /// Peak FP32 TFLOPS.
    pub fp32_tflops: f64,
    /// Peak FP64 TFLOPS.
    pub fp64_tflops: f64,
    /// Peak INT8 TOPS (for ML inference).
    pub int8_tops: f64,
    /// Tensor cores available (NVIDIA) or matrix cores (AMD).
    pub has_tensor_cores: bool,
    /// Thermal design power (Watts).
    pub tdp_watts: u32,
    /// Transfer characteristics.
    pub transfer: TransferCharacteristics,
}

impl GpuModel {
    /// NVIDIA A100 80GB (Ampere, datacenter).
    #[must_use]
    pub fn nvidia_a100_80gb() -> Self {
        Self {
            name: "NVIDIA A100 80GB".into(),
            architecture: GpuArchitecture::NvidiaAmpere,
            vram_bytes: 85_899_345_920,
            memory_type: GpuMemoryType::HBM2e,
            memory_bandwidth_gbps: 2039.0,
            compute_units: 108,
            fp32_tflops: 19.5,
            fp64_tflops: 9.7,
            int8_tops: 624.0,
            has_tensor_cores: true,
            tdp_watts: 400,
            transfer: TransferCharacteristics::pcie_gen4_x16(),
        }
    }

    /// NVIDIA H100 80GB (Hopper, datacenter).
    #[must_use]
    pub fn nvidia_h100_80gb() -> Self {
        Self {
            name: "NVIDIA H100 80GB".into(),
            architecture: GpuArchitecture::NvidiaHopper,
            vram_bytes: 85_899_345_920,
            memory_type: GpuMemoryType::HBM3,
            memory_bandwidth_gbps: 3350.0,
            compute_units: 132,
            fp32_tflops: 51.0,
            fp64_tflops: 26.0,
            int8_tops: 2000.0,
            has_tensor_cores: true,
            tdp_watts: 700,
            transfer: TransferCharacteristics::pcie_gen5_x16(),
        }
    }

    /// NVIDIA RTX 4090 (Ada Lovelace, consumer flagship).
    #[must_use]
    pub fn nvidia_rtx_4090() -> Self {
        Self {
            name: "NVIDIA RTX 4090".into(),
            architecture: GpuArchitecture::NvidiaAda,
            vram_bytes: 25_769_803_776,
            memory_type: GpuMemoryType::GDDR6X,
            memory_bandwidth_gbps: 1008.0,
            compute_units: 128,
            fp32_tflops: 82.6,
            fp64_tflops: 1.29,
            int8_tops: 1321.0,
            has_tensor_cores: true,
            tdp_watts: 450,
            transfer: TransferCharacteristics::pcie_gen4_x16(),
        }
    }

    /// NVIDIA RTX 4070 (Ada Lovelace, mid-range).
    #[must_use]
    pub fn nvidia_rtx_4070() -> Self {
        Self {
            name: "NVIDIA RTX 4070".into(),
            architecture: GpuArchitecture::NvidiaAda,
            vram_bytes: 12_884_901_888,
            memory_type: GpuMemoryType::GDDR6X,
            memory_bandwidth_gbps: 504.2,
            compute_units: 46,
            fp32_tflops: 29.15,
            fp64_tflops: 0.456,
            int8_tops: 466.0,
            has_tensor_cores: true,
            tdp_watts: 200,
            transfer: TransferCharacteristics::pcie_gen4_x16(),
        }
    }

    /// AMD MI250X (CDNA 2, datacenter).
    #[must_use]
    pub fn amd_mi250x() -> Self {
        Self {
            name: "AMD MI250X".into(),
            architecture: GpuArchitecture::AmdCDNA2,
            vram_bytes: 137_438_953_472,
            memory_type: GpuMemoryType::HBM2e,
            memory_bandwidth_gbps: 3277.0,
            compute_units: 220,
            fp32_tflops: 47.9,
            fp64_tflops: 47.9,
            int8_tops: 383.0,
            has_tensor_cores: true,
            tdp_watts: 560,
            transfer: TransferCharacteristics::pcie_gen4_x16(),
        }
    }

    /// AMD MI300X (CDNA 3, datacenter).
    #[must_use]
    pub fn amd_mi300x() -> Self {
        Self {
            name: "AMD MI300X".into(),
            architecture: GpuArchitecture::AmdCDNA3,
            vram_bytes: 206_158_430_208,
            memory_type: GpuMemoryType::HBM3,
            memory_bandwidth_gbps: 5300.0,
            compute_units: 304,
            fp32_tflops: 163.4,
            fp64_tflops: 81.7,
            int8_tops: 1307.0,
            has_tensor_cores: true,
            tdp_watts: 750,
            transfer: TransferCharacteristics::pcie_gen5_x16(),
        }
    }

    /// AMD RX 7900 XTX (RDNA 3, consumer flagship).
    #[must_use]
    pub fn amd_rx_7900_xtx() -> Self {
        Self {
            name: "AMD RX 7900 XTX".into(),
            architecture: GpuArchitecture::AmdRDNA3,
            vram_bytes: 25_769_803_776,
            memory_type: GpuMemoryType::GDDR6,
            memory_bandwidth_gbps: 960.0,
            compute_units: 96,
            fp32_tflops: 61.4,
            fp64_tflops: 1.92,
            int8_tops: 245.6,
            has_tensor_cores: false,
            tdp_watts: 355,
            transfer: TransferCharacteristics::pcie_gen4_x16(),
        }
    }

    /// Intel Arc A770 (Xe-HPG, consumer).
    #[must_use]
    pub fn intel_arc_a770() -> Self {
        Self {
            name: "Intel Arc A770".into(),
            architecture: GpuArchitecture::IntelXeHPG,
            vram_bytes: 17_179_869_184,
            memory_type: GpuMemoryType::GDDR6,
            memory_bandwidth_gbps: 560.0,
            compute_units: 32,
            fp32_tflops: 19.66,
            fp64_tflops: 0.307,
            int8_tops: 157.0,
            has_tensor_cores: true,
            tdp_watts: 225,
            transfer: TransferCharacteristics::pcie_gen4_x16(),
        }
    }

    /// Intel Ponte Vecchio (Xe-HPC, datacenter).
    #[must_use]
    pub fn intel_ponte_vecchio() -> Self {
        Self {
            name: "Intel Data Center GPU Max 1550".into(),
            architecture: GpuArchitecture::IntelXeHPC,
            vram_bytes: 137_438_953_472,
            memory_type: GpuMemoryType::HBM2e,
            memory_bandwidth_gbps: 3277.0,
            compute_units: 128,
            fp32_tflops: 45.2,
            fp64_tflops: 22.6,
            int8_tops: 362.0,
            has_tensor_cores: true,
            tdp_watts: 600,
            transfer: TransferCharacteristics::pcie_gen5_x16(),
        }
    }

    /// Apple M2 GPU (unified memory).
    #[must_use]
    pub fn apple_m2() -> Self {
        Self {
            name: "Apple M2 GPU (10-core)".into(),
            architecture: GpuArchitecture::AppleMSeries,
            vram_bytes: 25_769_803_776,
            memory_type: GpuMemoryType::Unified,
            memory_bandwidth_gbps: 100.0,
            compute_units: 10,
            fp32_tflops: 3.6,
            fp64_tflops: 0.9,
            int8_tops: 14.4,
            has_tensor_cores: false,
            tdp_watts: 25,
            transfer: TransferCharacteristics::unified_memory(),
        }
    }

    /// Apple M2 Ultra GPU (unified memory, 76-core).
    #[must_use]
    pub fn apple_m2_ultra() -> Self {
        Self {
            name: "Apple M2 Ultra GPU (76-core)".into(),
            architecture: GpuArchitecture::AppleMSeries,
            vram_bytes: 206_158_430_208,
            memory_type: GpuMemoryType::Unified,
            memory_bandwidth_gbps: 800.0,
            compute_units: 76,
            fp32_tflops: 27.2,
            fp64_tflops: 6.8,
            int8_tops: 108.8,
            has_tensor_cores: false,
            tdp_watts: 200,
            transfer: TransferCharacteristics::unified_memory(),
        }
    }

    /// Estimate GPU scan time for n rows (seconds).
    #[must_use]
    pub fn scan_time_s(&self, rows: u64, bytes_per_row: u64) -> f64 {
        let total_bytes = rows as f64 * bytes_per_row as f64;
        let bandwidth_bytes = self.memory_bandwidth_gbps * 1e9;
        total_bytes / bandwidth_bytes
    }

    /// Estimate GPU hash join time (seconds).
    #[must_use]
    pub fn hash_join_time_s(&self, build_rows: u64, probe_rows: u64) -> f64 {
        let parallelism = f64::from(self.compute_units);
        let build_ns = build_rows as f64 * 100.0 / parallelism;
        let probe_ns = probe_rows as f64 * 50.0 / parallelism;
        (build_ns + probe_ns) / 1e9
    }

    /// Estimate available VRAM after runtime overhead (bytes).
    #[must_use]
    pub fn available_vram(&self) -> u64 {
        self.vram_bytes * 9 / 10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_ampere_vendor() {
        assert_eq!(GpuArchitecture::NvidiaAmpere.vendor(), GpuVendor::NVIDIA);
    }

    #[test]
    fn hopper_is_datacenter() {
        assert!(GpuArchitecture::NvidiaHopper.is_datacenter());
    }

    #[test]
    fn ada_not_datacenter() {
        assert!(!GpuArchitecture::NvidiaAda.is_datacenter());
    }

    #[test]
    fn hbm3_bandwidth() {
        assert!(GpuMemoryType::HBM3.typical_bandwidth_gbps() > 3000.0);
    }

    #[test]
    fn hbm3_lower_latency_than_gddr6() {
        assert!(GpuMemoryType::HBM3.latency_ns() < GpuMemoryType::GDDR6.latency_ns());
    }

    #[test]
    fn pcie_gen5_faster_than_gen4() {
        let gen5 = TransferCharacteristics::pcie_gen5_x16();
        let gen4 = TransferCharacteristics::pcie_gen4_x16();
        assert!(gen5.pcie_bandwidth_gbps > gen4.pcie_bandwidth_gbps);
    }

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "Exact float equality needed for unified memory test"
    )]
    fn unified_memory_no_transfer() {
        let unified = TransferCharacteristics::unified_memory();
        assert!(unified.unified_memory);
        assert_eq!(unified.transfer_time_s(1_000_000), 0.0);
    }

    #[test]
    fn nvidia_a100_model() {
        let gpu = GpuModel::nvidia_a100_80gb();
        assert_eq!(gpu.architecture, GpuArchitecture::NvidiaAmpere);
        assert!(gpu.has_tensor_cores);
        assert_eq!(gpu.compute_units, 108);
    }

    #[test]
    fn nvidia_h100_model() {
        let gpu = GpuModel::nvidia_h100_80gb();
        assert_eq!(gpu.architecture, GpuArchitecture::NvidiaHopper);
        assert!(gpu.fp32_tflops > 50.0);
    }

    #[test]
    fn amd_mi300x_model() {
        let gpu = GpuModel::amd_mi300x();
        assert_eq!(gpu.architecture, GpuArchitecture::AmdCDNA3);
        assert!(gpu.memory_bandwidth_gbps > 5000.0);
    }

    #[test]
    fn apple_m2_unified() {
        let gpu = GpuModel::apple_m2();
        assert_eq!(gpu.architecture, GpuArchitecture::AppleMSeries);
        assert_eq!(gpu.memory_type, GpuMemoryType::Unified);
        assert!(gpu.transfer.unified_memory);
    }

    #[test]
    fn scan_time_nonzero() {
        let gpu = GpuModel::nvidia_a100_80gb();
        let time = gpu.scan_time_s(1_000_000, 100);
        assert!(time > 0.0);
    }

    #[test]
    fn hash_join_time_nonzero() {
        let gpu = GpuModel::nvidia_h100_80gb();
        let time = gpu.hash_join_time_s(100_000, 1_000_000);
        assert!(time > 0.0);
    }

    #[test]
    fn available_vram_less_than_total() {
        let gpu = GpuModel::nvidia_a100_80gb();
        assert!(gpu.available_vram() < gpu.vram_bytes);
    }

    #[test]
    #[expect(
        clippy::expect_used,
        reason = "Test code appropriately uses expect for known-good serialization"
    )]
    fn serialize_roundtrip() {
        let gpu = GpuModel::nvidia_a100_80gb();
        let json = serde_json::to_string(&gpu).expect("serialization should succeed");
        let deserialized: GpuModel =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(gpu, deserialized);
    }
}
