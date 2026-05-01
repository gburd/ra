//! Vendor-neutral deployment profile types for comprehensive hardware modeling.
//!
//! This module provides composable types for modeling any cloud infrastructure
//! configuration, enabling the fuzzer to discover corner cases across the full
//! spectrum of modern database deployment scenarios.

use ra_core::facts::{CpuArchitecture, HardwareProfile};

/// Vendor-neutral compute instance classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceClass {
    /// 1 vCPU, 0.5-1 GB RAM (edge/IoT devices)
    Nano,
    /// 1-2 vCPU, 1-2 GB RAM (dev/test)
    Micro,
    /// 2 vCPU, 2-4 GB RAM (small applications)
    Small,
    /// 4 vCPU, 8-16 GB RAM (standard production)
    Medium,
    /// 8-16 vCPU, 32-64 GB RAM (heavy production)
    Large,
    /// 32-64 vCPU, 128-256 GB RAM (enterprise)
    ExtraLarge,
    /// High CPU:memory ratio (compute-bound workloads)
    ComputeOptimal,
    /// High memory:CPU ratio (memory-bound workloads)
    MemoryOptimal,
    /// High local storage bandwidth (I/O-bound workloads)
    StorageOptimal,
    /// Enhanced networking (distributed workloads)
    NetworkOptimal,
    /// Sustained high-frequency CPU (>3.5 GHz)
    HighFrequency,
    /// 32-192 cores (massively parallel workloads)
    ManyCore,
    /// 256 GB - 2 TB+ RAM (in-memory databases)
    HighMemory,
    /// ML training GPUs (A100/H100 class)
    GpuTraining,
    /// ML inference GPUs (T4/L4 class)
    GpuInference,
    /// FPGA acceleration
    FpgaAccel,
}

/// Vendor-neutral storage performance tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageTier {
    /// General-purpose SSD (3000 baseline IOPS, 125 MB/s)
    StandardSsd,
    /// Provisioned high-IOPS SSD (up to 64000 IOPS, 1000 MB/s)
    HighIopsSsd,
    /// Burstable SSD (burst to 3000 IOPS, baseline lower)
    BurstSsd,
    /// Throughput-optimized HDD (500 IOPS, 500 MB/s)
    StandardHdd,
    /// Cold storage HDD (250 IOPS, 250 MB/s)
    ColdHdd,
    /// Instance-attached `NVMe` (1M+ IOPS, 7+ GB/s)
    LocalNvme,
    /// Instance-attached SSD (100K+ IOPS, 2+ GB/s)
    LocalSsd,
    /// Standard object storage (high latency, high throughput)
    ObjectStandard,
    /// Infrequent-access object storage
    ObjectInfrequentAccess,
    /// Archive object storage (minutes-to-hours retrieval)
    ObjectArchive,
    /// Network filesystem (moderate IOPS, shared access)
    NetworkFs,
}

/// Deployment topology for distributed systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterTopology {
    /// Single database instance
    SingleNode,
    /// Small HA cluster (2-8 nodes)
    SmallCluster(u32),
    /// Medium cluster (9-64 nodes)
    MediumCluster(u32),
    /// Large cluster (65-256 nodes)
    LargeCluster(u32),
    /// Massive cluster (257+ nodes)
    MassiveCluster(u32),
    /// Geo-distributed across regions
    MultiRegion {
        /// Number of regions
        regions: u32,
        /// Nodes per region
        nodes_per_region: u32,
    },
}

impl ClusterTopology {
    /// Total number of nodes in the cluster.
    #[must_use]
    pub fn node_count(self) -> u32 {
        match self {
            Self::SingleNode => 1,
            Self::SmallCluster(n)
            | Self::MediumCluster(n)
            | Self::LargeCluster(n)
            | Self::MassiveCluster(n) => n,
            Self::MultiRegion { regions, nodes_per_region } => regions * nodes_per_region,
        }
    }

    /// Whether this topology involves multiple nodes.
    #[must_use]
    pub fn is_distributed(self) -> bool {
        self.node_count() > 1
    }
}

/// A single compute instance specification.
#[derive(Debug, Clone)]
pub struct ComputeInstance {
    /// Instance classification
    pub class: InstanceClass,
    /// CPU architecture
    pub architecture: CpuArchitecture,
    /// Number of vCPUs
    pub cores: u32,
    /// Memory in GB
    pub memory_gb: u64,
    /// Network bandwidth in Gbps
    pub network_bandwidth_gbps: f64,
    /// CPU baseline performance (0.0-1.0, where 1.0 = full sustained)
    pub baseline_performance: f64,
}

/// A single storage volume specification.
#[derive(Debug, Clone)]
pub struct StorageInstance {
    /// Storage performance tier
    pub tier: StorageTier,
    /// Capacity in GB
    pub capacity_gb: u64,
    /// Throughput in MB/s
    pub bandwidth_mbps: u64,
    /// Random I/O operations per second
    pub iops: u64,
    /// Average latency in microseconds
    pub latency_us: u64,
}

impl StorageInstance {
    /// Create a storage instance with sensible defaults for the given tier.
    #[must_use]
    pub fn new(tier: StorageTier, capacity_gb: u64) -> Self {
        let (bandwidth_mbps, iops, latency_us) = match tier {
            StorageTier::StandardSsd | StorageTier::BurstSsd => (125, 3000, 100),
            StorageTier::HighIopsSsd => (1000, 64000, 50),
            StorageTier::StandardHdd => (500, 500, 5000),
            StorageTier::ColdHdd => (250, 250, 8000),
            StorageTier::LocalNvme => (7000, 1_000_000, 10),
            StorageTier::LocalSsd => (2000, 100_000, 50),
            StorageTier::ObjectStandard => (500, 5000, 2000),
            StorageTier::ObjectInfrequentAccess => (250, 2000, 5000),
            StorageTier::ObjectArchive => (50, 100, 60_000_000), // minutes
            StorageTier::NetworkFs => (500, 12000, 200),
        };
        Self { tier, capacity_gb, bandwidth_mbps, iops, latency_us }
    }
}

/// Complete deployment specification combining compute, storage, and topology.
#[derive(Debug, Clone)]
pub struct DeploymentProfile {
    /// Compute instance specification
    pub compute: ComputeInstance,
    /// Storage volumes (one or more)
    pub storage: Vec<StorageInstance>,
    /// Cluster topology
    pub topology: ClusterTopology,
}

impl DeploymentProfile {
    /// Create a deployment profile with the given components.
    #[must_use]
    pub fn new(
        compute: ComputeInstance,
        storage: Vec<StorageInstance>,
        topology: ClusterTopology,
    ) -> Self {
        Self { compute, storage, topology }
    }

    /// Convert this deployment profile to a `HardwareProfile` for the optimizer.
    #[must_use]
    pub fn to_hardware_profile(&self) -> HardwareProfile {
        let simd_width = match self.compute.architecture {
            CpuArchitecture::X86_64 => match self.compute.class {
                InstanceClass::Nano | InstanceClass::Micro => 128, // SSE
                InstanceClass::HighFrequency | InstanceClass::ManyCore => 512, // AVX-512
                _ => 256, // AVX2 default
            },
            CpuArchitecture::ARM64 | CpuArchitecture::RISCV => 128, // NEON / RVV baseline
        };

        let (has_gpu, gpu_memory) = match self.compute.class {
            InstanceClass::GpuTraining => (true, Some(80 * 1024 * 1024 * 1024)),
            InstanceClass::GpuInference => (true, Some(16 * 1024 * 1024 * 1024)),
            _ => (false, None),
        };

        let (l1, l2, l3) = match self.compute.class {
            InstanceClass::Nano | InstanceClass::Micro => (16 * 1024, 128 * 1024, 2 * 1024 * 1024),
            InstanceClass::Large | InstanceClass::ComputeOptimal => (32 * 1024, 512 * 1024, 16 * 1024 * 1024),
            InstanceClass::ExtraLarge | InstanceClass::MemoryOptimal
            | InstanceClass::GpuTraining | InstanceClass::GpuInference => (64 * 1024, 1024 * 1024, 64 * 1024 * 1024),
            InstanceClass::ManyCore | InstanceClass::HighMemory => (64 * 1024, 2 * 1024 * 1024, 128 * 1024 * 1024),
            InstanceClass::HighFrequency => (64 * 1024, 512 * 1024, 32 * 1024 * 1024),
            _ => (32 * 1024, 256 * 1024, 8 * 1024 * 1024),
        };

        HardwareProfile {
            cpu_cores: self.compute.cores,
            available_memory: self.compute.memory_gb * 1024 * 1024 * 1024,
            total_memory: self.compute.memory_gb * 1024 * 1024 * 1024,
            simd_width,
            has_gpu,
            gpu_memory,
            l1_cache_size: l1,
            l2_cache_size: l2,
            l3_cache_size: l3,
            cpu_architecture: self.compute.architecture,
        }
    }

    /// Whether any storage volume has high random I/O capability (> 10000 IOPS).
    #[must_use]
    pub fn storage_has_high_iops(&self) -> bool {
        self.storage.iter().any(|s| s.iops > 10_000)
    }

    /// Whether any storage volume has high sequential bandwidth (> 1000 MB/s).
    #[must_use]
    pub fn storage_has_high_bandwidth(&self) -> bool {
        self.storage.iter().any(|s| s.bandwidth_mbps > 1000)
    }

    /// Whether this deployment supports distributed execution.
    #[must_use]
    pub fn supports_distributed_execution(&self) -> bool {
        self.topology.is_distributed()
    }

    /// Whether this deployment has multiple storage tiers.
    #[must_use]
    pub fn supports_tiered_storage(&self) -> bool {
        if self.storage.len() < 2 {
            return false;
        }
        let first_tier = self.storage[0].tier;
        self.storage.iter().any(|s| s.tier != first_tier)
    }

    /// Total storage capacity across all volumes in GB.
    #[must_use]
    pub fn total_storage_capacity_gb(&self) -> u64 {
        self.storage.iter().map(|s| s.capacity_gb).sum()
    }

    /// Primary storage IOPS (from the first/fastest volume).
    #[must_use]
    pub fn primary_storage_iops(&self) -> u64 {
        self.storage.iter().map(|s| s.iops).max().unwrap_or(0)
    }

    /// Primary storage latency in microseconds.
    #[must_use]
    pub fn primary_storage_latency_us(&self) -> u64 {
        self.storage.iter().map(|s| s.latency_us).min().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_compute() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::Medium,
            architecture: CpuArchitecture::X86_64,
            cores: 4,
            memory_gb: 16,
            network_bandwidth_gbps: 10.0,
            baseline_performance: 1.0,
        }
    }

    #[test]
    fn cluster_topology_node_count() {
        assert_eq!(ClusterTopology::SingleNode.node_count(), 1);
        assert_eq!(ClusterTopology::SmallCluster(3).node_count(), 3);
        assert_eq!(ClusterTopology::MediumCluster(32).node_count(), 32);
        assert_eq!(ClusterTopology::LargeCluster(128).node_count(), 128);
        assert_eq!(ClusterTopology::MassiveCluster(1024).node_count(), 1024);
        assert_eq!(
            ClusterTopology::MultiRegion { regions: 3, nodes_per_region: 8 }.node_count(),
            24
        );
    }

    #[test]
    fn cluster_topology_is_distributed() {
        assert!(!ClusterTopology::SingleNode.is_distributed());
        assert!(ClusterTopology::SmallCluster(3).is_distributed());
        assert!(ClusterTopology::MultiRegion { regions: 2, nodes_per_region: 4 }.is_distributed());
    }

    #[test]
    fn storage_instance_defaults() {
        let nvme = StorageInstance::new(StorageTier::LocalNvme, 1000);
        assert_eq!(nvme.iops, 1_000_000);
        assert_eq!(nvme.bandwidth_mbps, 7000);
        assert_eq!(nvme.latency_us, 10);

        let hdd = StorageInstance::new(StorageTier::StandardHdd, 5000);
        assert_eq!(hdd.iops, 500);
        assert_eq!(hdd.latency_us, 5000);
    }

    #[test]
    fn deployment_to_hardware_profile() {
        let profile = DeploymentProfile::new(
            sample_compute(),
            vec![StorageInstance::new(StorageTier::StandardSsd, 500)],
            ClusterTopology::SingleNode,
        );

        let hw = profile.to_hardware_profile();
        assert_eq!(hw.cpu_cores, 4);
        assert_eq!(hw.available_memory, 16 * 1024 * 1024 * 1024);
        assert_eq!(hw.simd_width, 256); // Medium x86_64 = AVX2
        assert!(!hw.has_gpu);
        assert_eq!(hw.cpu_architecture, CpuArchitecture::X86_64);
    }

    #[test]
    fn deployment_gpu_profile() {
        let gpu_compute = ComputeInstance {
            class: InstanceClass::GpuTraining,
            architecture: CpuArchitecture::X86_64,
            cores: 96,
            memory_gb: 768,
            network_bandwidth_gbps: 100.0,
            baseline_performance: 1.0,
        };
        let profile = DeploymentProfile::new(
            gpu_compute,
            vec![StorageInstance::new(StorageTier::LocalNvme, 4000)],
            ClusterTopology::SingleNode,
        );

        let hw = profile.to_hardware_profile();
        assert!(hw.has_gpu);
        assert_eq!(hw.gpu_memory, Some(80 * 1024 * 1024 * 1024));
    }

    #[test]
    fn deployment_storage_capabilities() {
        let profile = DeploymentProfile::new(
            sample_compute(),
            vec![
                StorageInstance::new(StorageTier::LocalNvme, 500),
                StorageInstance::new(StorageTier::StandardSsd, 5000),
                StorageInstance::new(StorageTier::ObjectArchive, 100_000),
            ],
            ClusterTopology::SmallCluster(3),
        );

        assert!(profile.storage_has_high_iops());
        assert!(profile.storage_has_high_bandwidth());
        assert!(profile.supports_distributed_execution());
        assert!(profile.supports_tiered_storage());
        assert_eq!(profile.total_storage_capacity_gb(), 105_500);
    }

    #[test]
    fn arm_architecture_simd() {
        let arm_compute = ComputeInstance {
            class: InstanceClass::Large,
            architecture: CpuArchitecture::ARM64,
            cores: 16,
            memory_gb: 64,
            network_bandwidth_gbps: 25.0,
            baseline_performance: 1.0,
        };
        let profile = DeploymentProfile::new(
            arm_compute,
            vec![StorageInstance::new(StorageTier::StandardSsd, 500)],
            ClusterTopology::SingleNode,
        );

        let hw = profile.to_hardware_profile();
        assert_eq!(hw.simd_width, 128); // ARM NEON
        assert_eq!(hw.cpu_architecture, CpuArchitecture::ARM64);
    }
}
