//! Memory architecture models including NUMA configurations.

use serde::{Deserialize, Serialize};

/// NUMA (Non-Uniform Memory Access) configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NumaTopology {
    /// Uniform Memory Access (single memory controller).
    UMA,
    /// 2-node NUMA (dual-socket).
    NUMA2,
    /// 4-node NUMA (quad-socket).
    NUMA4,
    /// 8-node NUMA (8-socket).
    NUMA8,
}

impl NumaTopology {
    /// Returns the number of NUMA nodes.
    #[must_use]
    pub fn node_count(self) -> u32 {
        match self {
            Self::UMA => 1,
            Self::NUMA2 => 2,
            Self::NUMA4 => 4,
            Self::NUMA8 => 8,
        }
    }

    /// Returns the typical NUMA penalty factor (remote/local bandwidth ratio).
    #[must_use]
    pub fn remote_penalty_factor(self) -> f64 {
        match self {
            Self::UMA => 1.0,
            Self::NUMA2 => 0.6,
            Self::NUMA4 => 0.4,
            Self::NUMA8 => 0.3,
        }
    }

    /// Returns the typical NUMA latency overhead (ns).
    #[must_use]
    pub fn remote_latency_overhead_ns(self) -> f64 {
        match self {
            Self::UMA => 0.0,
            Self::NUMA2 => 50.0,
            Self::NUMA4 => 100.0,
            Self::NUMA8 => 150.0,
        }
    }
}

/// Memory technology type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryType {
    /// DDR4 SDRAM.
    DDR4,
    /// DDR5 SDRAM.
    DDR5,
    /// High Bandwidth Memory (`HBM2e`).
    HBM2e,
    /// High Bandwidth Memory 3 (`HBM3`).
    HBM3,
    /// Persistent Memory (Intel Optane).
    PersistentMemory,
}

impl MemoryType {
    /// Returns typical bandwidth per channel (GB/s).
    #[must_use]
    pub fn bandwidth_per_channel_gbps(self) -> f64 {
        match self {
            Self::DDR4 => 25.6,
            Self::DDR5 => 51.2,
            Self::HBM2e => 410.0,
            Self::HBM3 => 614.4,
            Self::PersistentMemory => 8.0,
        }
    }

    /// Returns typical access latency (ns).
    #[must_use]
    pub fn latency_ns(self) -> f64 {
        match self {
            Self::DDR4 => 85.0,
            Self::DDR5 => 75.0,
            Self::HBM2e => 50.0,
            Self::HBM3 => 45.0,
            Self::PersistentMemory => 300.0,
        }
    }

    /// Returns whether this memory is byte-addressable.
    #[must_use]
    pub fn is_byte_addressable(self) -> bool {
        match self {
            Self::DDR4 | Self::DDR5 | Self::HBM2e | Self::HBM3 | Self::PersistentMemory => true,
        }
    }

    /// Returns whether this memory is persistent across reboots.
    #[must_use]
    pub fn is_persistent(self) -> bool {
        matches!(self, Self::PersistentMemory)
    }
}

/// Memory configuration for a system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Human-readable name.
    pub name: String,
    /// Memory technology type.
    pub memory_type: MemoryType,
    /// Total memory capacity (bytes).
    pub capacity_bytes: u64,
    /// Number of memory channels per socket.
    pub channels_per_socket: u32,
    /// Number of sockets.
    pub sockets: u32,
    /// NUMA topology.
    pub numa_topology: NumaTopology,
    /// Local memory bandwidth (GB/s).
    pub local_bandwidth_gbps: f64,
    /// Remote memory bandwidth (GB/s).
    pub remote_bandwidth_gbps: f64,
    /// Memory latency (ns).
    pub latency_ns: f64,
}

impl MemoryConfig {
    /// Returns the total memory bandwidth (GB/s).
    #[must_use]
    pub fn total_bandwidth_gbps(&self) -> f64 {
        self.local_bandwidth_gbps * f64::from(self.sockets)
    }

    /// Returns the effective bandwidth for remote access (GB/s).
    #[must_use]
    pub fn effective_remote_bandwidth_gbps(&self) -> f64 {
        self.remote_bandwidth_gbps
    }

    /// Single-socket DDR4 configuration (128 GB, 8 channels).
    #[must_use]
    pub fn ddr4_single_socket() -> Self {
        Self {
            name: "DDR4-3200 (1x 128GB, 8ch)".into(),
            memory_type: MemoryType::DDR4,
            capacity_bytes: 137_438_953_472,
            channels_per_socket: 8,
            sockets: 1,
            numa_topology: NumaTopology::UMA,
            local_bandwidth_gbps: 204.8,
            remote_bandwidth_gbps: 204.8,
            latency_ns: 85.0,
        }
    }

    /// Dual-socket DDR4 configuration (512 GB, 2x8 channels).
    #[must_use]
    pub fn ddr4_dual_socket() -> Self {
        Self {
            name: "DDR4-3200 (2x 256GB, 16ch)".into(),
            memory_type: MemoryType::DDR4,
            capacity_bytes: 549_755_813_888,
            channels_per_socket: 8,
            sockets: 2,
            numa_topology: NumaTopology::NUMA2,
            local_bandwidth_gbps: 204.8,
            remote_bandwidth_gbps: 122.9,
            latency_ns: 90.0,
        }
    }

    /// Quad-socket DDR4 configuration (2 TB, 4x8 channels).
    #[must_use]
    pub fn ddr4_quad_socket() -> Self {
        Self {
            name: "DDR4-3200 (4x 512GB, 32ch)".into(),
            memory_type: MemoryType::DDR4,
            capacity_bytes: 2_199_023_255_552,
            channels_per_socket: 8,
            sockets: 4,
            numa_topology: NumaTopology::NUMA4,
            local_bandwidth_gbps: 204.8,
            remote_bandwidth_gbps: 81.9,
            latency_ns: 100.0,
        }
    }

    /// DDR5 configuration (256 GB, 8 channels).
    #[must_use]
    pub fn ddr5_single_socket() -> Self {
        Self {
            name: "DDR5-4800 (1x 256GB, 8ch)".into(),
            memory_type: MemoryType::DDR5,
            capacity_bytes: 274_877_906_944,
            channels_per_socket: 8,
            sockets: 1,
            numa_topology: NumaTopology::UMA,
            local_bandwidth_gbps: 409.6,
            remote_bandwidth_gbps: 409.6,
            latency_ns: 75.0,
        }
    }

    /// Dual-socket DDR5 configuration (1 TB, 2x8 channels).
    #[must_use]
    pub fn ddr5_dual_socket() -> Self {
        Self {
            name: "DDR5-4800 (2x 512GB, 16ch)".into(),
            memory_type: MemoryType::DDR5,
            capacity_bytes: 1_099_511_627_776,
            channels_per_socket: 8,
            sockets: 2,
            numa_topology: NumaTopology::NUMA2,
            local_bandwidth_gbps: 409.6,
            remote_bandwidth_gbps: 245.8,
            latency_ns: 80.0,
        }
    }

    /// `HBM2e` configuration (80 GB, 4 stacks).
    #[must_use]
    pub fn hbm2e() -> Self {
        Self {
            name: "HBM2e (80GB, 4 stacks)".into(),
            memory_type: MemoryType::HBM2e,
            capacity_bytes: 85_899_345_920,
            channels_per_socket: 4,
            sockets: 1,
            numa_topology: NumaTopology::UMA,
            local_bandwidth_gbps: 1640.0,
            remote_bandwidth_gbps: 1640.0,
            latency_ns: 50.0,
        }
    }

    /// HBM3 configuration (96 GB, 6 stacks).
    #[must_use]
    pub fn hbm3() -> Self {
        Self {
            name: "HBM3 (96GB, 6 stacks)".into(),
            memory_type: MemoryType::HBM3,
            capacity_bytes: 103_079_215_104,
            channels_per_socket: 6,
            sockets: 1,
            numa_topology: NumaTopology::UMA,
            local_bandwidth_gbps: 3686.4,
            remote_bandwidth_gbps: 3686.4,
            latency_ns: 45.0,
        }
    }

    /// Persistent memory configuration (1.5 TB, App Direct mode).
    #[must_use]
    pub fn persistent_memory() -> Self {
        Self {
            name: "Intel Optane (1.5TB, App Direct)".into(),
            memory_type: MemoryType::PersistentMemory,
            capacity_bytes: 1_649_267_441_664,
            channels_per_socket: 6,
            sockets: 2,
            numa_topology: NumaTopology::NUMA2,
            local_bandwidth_gbps: 48.0,
            remote_bandwidth_gbps: 28.8,
            latency_ns: 300.0,
        }
    }

    /// Apple M2 unified memory (24 GB LPDDR5).
    #[must_use]
    pub fn apple_m2_unified() -> Self {
        Self {
            name: "Apple M2 Unified (24GB LPDDR5)".into(),
            memory_type: MemoryType::DDR5,
            capacity_bytes: 25_769_803_776,
            channels_per_socket: 4,
            sockets: 1,
            numa_topology: NumaTopology::UMA,
            local_bandwidth_gbps: 100.0,
            remote_bandwidth_gbps: 100.0,
            latency_ns: 70.0,
        }
    }

    /// Estimate memory access time for sequential scan.
    #[must_use]
    pub fn sequential_access_time_s(&self, bytes: u64) -> f64 {
        let bandwidth_bytes = self.local_bandwidth_gbps * 1e9;
        bytes as f64 / bandwidth_bytes
    }

    /// Estimate memory access time for random access pattern.
    #[must_use]
    pub fn random_access_time_s(&self, accesses: u64) -> f64 {
        let latency_s = self.latency_ns * 1e-9;
        accesses as f64 * latency_s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numa2_node_count() {
        assert_eq!(NumaTopology::NUMA2.node_count(), 2);
    }

    #[test]
    fn numa4_penalty() {
        assert!((NumaTopology::NUMA4.remote_penalty_factor() - 0.4).abs() < 0.01);
    }

    #[test]
    fn ddr4_bandwidth() {
        assert!((MemoryType::DDR4.bandwidth_per_channel_gbps() - 25.6).abs() < 0.1);
    }

    #[test]
    fn hbm3_latency() {
        assert!((MemoryType::HBM3.latency_ns() - 45.0).abs() < 1.0);
    }

    #[test]
    fn persistent_memory_is_persistent() {
        assert!(MemoryType::PersistentMemory.is_persistent());
    }

    #[test]
    fn ddr4_not_persistent() {
        assert!(!MemoryType::DDR4.is_persistent());
    }

    #[test]
    fn ddr4_single_socket_config() {
        let config = MemoryConfig::ddr4_single_socket();
        assert_eq!(config.sockets, 1);
        assert_eq!(config.numa_topology, NumaTopology::UMA);
    }

    #[test]
    fn ddr4_dual_socket_config() {
        let config = MemoryConfig::ddr4_dual_socket();
        assert_eq!(config.sockets, 2);
        assert_eq!(config.numa_topology, NumaTopology::NUMA2);
        assert!(config.remote_bandwidth_gbps < config.local_bandwidth_gbps);
    }

    #[test]
    fn hbm2e_config() {
        let config = MemoryConfig::hbm2e();
        assert_eq!(config.memory_type, MemoryType::HBM2e);
        assert!(config.local_bandwidth_gbps > 1000.0);
    }

    #[test]
    fn hbm3_config() {
        let config = MemoryConfig::hbm3();
        assert_eq!(config.memory_type, MemoryType::HBM3);
        assert!(config.local_bandwidth_gbps > 3000.0);
    }

    #[test]
    fn total_bandwidth() {
        let config = MemoryConfig::ddr4_dual_socket();
        let total = config.total_bandwidth_gbps();
        assert!((total - 409.6).abs() < 0.1);
    }

    #[test]
    fn sequential_access_time() {
        let config = MemoryConfig::ddr4_single_socket();
        let time = config.sequential_access_time_s(1_000_000_000);
        assert!(time > 0.0 && time < 1.0);
    }

    #[test]
    fn random_access_time() {
        let config = MemoryConfig::ddr4_single_socket();
        let time = config.random_access_time_s(10_000);
        assert!(time > 0.0);
    }

    #[test]
    #[expect(
        clippy::expect_used,
        reason = "Test code appropriately uses expect for known-good serialization"
    )]
    fn serialize_roundtrip() {
        let config = MemoryConfig::ddr4_dual_socket();
        let json = serde_json::to_string(&config).expect("serialization should succeed");
        let deserialized: MemoryConfig =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(config, deserialized);
    }
}
