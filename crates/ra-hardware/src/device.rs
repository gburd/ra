//! Device types for heterogeneous operator placement.

use serde::{Deserialize, Serialize};

/// A processing device that can execute query operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Device {
    /// CPU execution (scalar or SIMD-vectorized).
    Cpu,
    /// GPU execution via CUDA, `OpenCL`, or similar.
    Gpu,
    /// FPGA execution via synthesized hardware pipelines.
    Fpga,
}

impl Device {
    /// Whether this device requires explicit data transfer from host
    /// memory.
    #[must_use]
    pub fn requires_transfer(self) -> bool {
        match self {
            Self::Cpu => false,
            Self::Gpu | Self::Fpga => true,
        }
    }
}

impl std::fmt::Display for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cpu => write!(f, "CPU"),
            Self::Gpu => write!(f, "GPU"),
            Self::Fpga => write!(f, "FPGA"),
        }
    }
}

/// Describes how data moves between two devices.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TransferPath {
    /// Source device.
    pub from: Device,
    /// Destination device.
    pub to: Device,
    /// Bandwidth in GB/s for this path.
    pub bandwidth_gbps: f64,
    /// One-way latency in microseconds.
    pub latency_us: f64,
}

impl TransferPath {
    /// Estimate transfer time in seconds for the given byte count.
    #[must_use]
    pub fn transfer_time_s(self, bytes: u64) -> f64 {
        let bandwidth_bytes = self.bandwidth_gbps * 1e9;
        let latency_s = self.latency_us * 1e-6;
        let byte_time = bytes as f64 / bandwidth_bytes;
        latency_s + byte_time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_no_transfer() {
        assert!(!Device::Cpu.requires_transfer());
    }

    #[test]
    fn gpu_requires_transfer() {
        assert!(Device::Gpu.requires_transfer());
    }

    #[test]
    fn fpga_requires_transfer() {
        assert!(Device::Fpga.requires_transfer());
    }

    #[test]
    fn device_display() {
        assert_eq!(Device::Cpu.to_string(), "CPU");
        assert_eq!(Device::Gpu.to_string(), "GPU");
        assert_eq!(Device::Fpga.to_string(), "FPGA");
    }

    #[test]
    fn transfer_time_calculation() {
        let path = TransferPath {
            from: Device::Cpu,
            to: Device::Gpu,
            bandwidth_gbps: 25.0,
            latency_us: 1.0,
        };
        let time = path.transfer_time_s(25_000_000_000);
        assert!((time - 1.000_001).abs() < 0.001);
    }

    #[test]
    fn transfer_time_zero_bytes() {
        let path = TransferPath {
            from: Device::Cpu,
            to: Device::Gpu,
            bandwidth_gbps: 25.0,
            latency_us: 1.0,
        };
        let time = path.transfer_time_s(0);
        assert!((time - 1e-6).abs() < 1e-9);
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn serialize_roundtrip() {
        let path = TransferPath {
            from: Device::Cpu,
            to: Device::Gpu,
            bandwidth_gbps: 25.0,
            latency_us: 1.0,
        };
        let json = serde_json::to_string(&path).expect("serialization should succeed");
        let deserialized: TransferPath =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(path, deserialized);
    }
}
