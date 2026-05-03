//! Vendor-neutral cloud infrastructure profile database.
//!
//! Provides a comprehensive set of representative deployment profiles
//! covering the full spectrum of modern cloud infrastructure. Profiles
//! are abstracted from specific cloud providers (AWS, Azure, GCP) into
//! vendor-neutral categories.

use crate::deployment_profiles::{
    ClusterTopology, ComputeInstance, DeploymentProfile, InstanceClass,
    StorageInstance, StorageTier,
};
use crate::dynamic_facts::DatabaseScenario;
use ra_core::facts::CpuArchitecture;

/// Selects deployment profiles for fuzzing scenarios.
pub trait ProfileSelector {
    /// Select a representative profile for a database scenario.
    fn select_for_scenario(
        scenario: &DatabaseScenario,
    ) -> DeploymentProfile;

    /// Select a random profile from the full catalog.
    fn select_random() -> DeploymentProfile;

    /// Select profiles filtered by CPU architecture.
    fn select_by_architecture(
        arch: CpuArchitecture,
    ) -> Vec<DeploymentProfile>;

    /// Select edge-case profiles that stress optimizer boundaries.
    fn select_edge_cases() -> Vec<DeploymentProfile>;

    /// Return all profiles in the catalog.
    fn all_profiles() -> Vec<DeploymentProfile>;
}

/// Profile selector backed by a comprehensive catalog of cloud
/// infrastructure configurations.
pub struct CloudProfileSelector;

impl CloudProfileSelector {
    // ── Compute instance constructors ──────────────────────────

    fn nano_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::Nano,
            architecture: CpuArchitecture::X86_64,
            cores: 1,
            memory_gb: 1,
            network_bandwidth_gbps: 0.5,
            baseline_performance: 0.1,
        }
    }

    fn nano_arm() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::Nano,
            architecture: CpuArchitecture::ARM64,
            cores: 1,
            memory_gb: 1,
            network_bandwidth_gbps: 0.5,
            baseline_performance: 0.2,
        }
    }

    fn micro_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::Micro,
            architecture: CpuArchitecture::X86_64,
            cores: 2,
            memory_gb: 2,
            network_bandwidth_gbps: 1.0,
            baseline_performance: 0.2,
        }
    }

    fn micro_arm() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::Micro,
            architecture: CpuArchitecture::ARM64,
            cores: 2,
            memory_gb: 2,
            network_bandwidth_gbps: 1.0,
            baseline_performance: 0.2,
        }
    }

    fn small_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::Small,
            architecture: CpuArchitecture::X86_64,
            cores: 2,
            memory_gb: 4,
            network_bandwidth_gbps: 5.0,
            baseline_performance: 0.4,
        }
    }

    fn small_arm() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::Small,
            architecture: CpuArchitecture::ARM64,
            cores: 2,
            memory_gb: 4,
            network_bandwidth_gbps: 5.0,
            baseline_performance: 1.0,
        }
    }

    fn small_riscv() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::Small,
            architecture: CpuArchitecture::RISCV,
            cores: 4,
            memory_gb: 8,
            network_bandwidth_gbps: 5.0,
            baseline_performance: 0.7,
        }
    }

    fn medium_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::Medium,
            architecture: CpuArchitecture::X86_64,
            cores: 4,
            memory_gb: 16,
            network_bandwidth_gbps: 10.0,
            baseline_performance: 1.0,
        }
    }

    fn medium_arm() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::Medium,
            architecture: CpuArchitecture::ARM64,
            cores: 4,
            memory_gb: 16,
            network_bandwidth_gbps: 10.0,
            baseline_performance: 1.0,
        }
    }

    fn medium_riscv() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::Medium,
            architecture: CpuArchitecture::RISCV,
            cores: 8,
            memory_gb: 16,
            network_bandwidth_gbps: 10.0,
            baseline_performance: 0.8,
        }
    }

    fn large_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::Large,
            architecture: CpuArchitecture::X86_64,
            cores: 8,
            memory_gb: 32,
            network_bandwidth_gbps: 12.0,
            baseline_performance: 1.0,
        }
    }

    fn large_arm() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::Large,
            architecture: CpuArchitecture::ARM64,
            cores: 16,
            memory_gb: 64,
            network_bandwidth_gbps: 25.0,
            baseline_performance: 1.0,
        }
    }

    fn xlarge_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::ExtraLarge,
            architecture: CpuArchitecture::X86_64,
            cores: 32,
            memory_gb: 128,
            network_bandwidth_gbps: 25.0,
            baseline_performance: 1.0,
        }
    }

    fn xlarge_arm() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::ExtraLarge,
            architecture: CpuArchitecture::ARM64,
            cores: 64,
            memory_gb: 256,
            network_bandwidth_gbps: 50.0,
            baseline_performance: 1.0,
        }
    }

    fn compute_opt_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::ComputeOptimal,
            architecture: CpuArchitecture::X86_64,
            cores: 16,
            memory_gb: 32,
            network_bandwidth_gbps: 25.0,
            baseline_performance: 1.0,
        }
    }

    fn compute_opt_arm() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::ComputeOptimal,
            architecture: CpuArchitecture::ARM64,
            cores: 16,
            memory_gb: 32,
            network_bandwidth_gbps: 25.0,
            baseline_performance: 1.0,
        }
    }

    fn memory_opt_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::MemoryOptimal,
            architecture: CpuArchitecture::X86_64,
            cores: 8,
            memory_gb: 128,
            network_bandwidth_gbps: 25.0,
            baseline_performance: 1.0,
        }
    }

    fn memory_opt_arm() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::MemoryOptimal,
            architecture: CpuArchitecture::ARM64,
            cores: 16,
            memory_gb: 256,
            network_bandwidth_gbps: 50.0,
            baseline_performance: 1.0,
        }
    }

    fn storage_opt_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::StorageOptimal,
            architecture: CpuArchitecture::X86_64,
            cores: 16,
            memory_gb: 64,
            network_bandwidth_gbps: 25.0,
            baseline_performance: 1.0,
        }
    }

    fn network_opt_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::NetworkOptimal,
            architecture: CpuArchitecture::X86_64,
            cores: 32,
            memory_gb: 128,
            network_bandwidth_gbps: 100.0,
            baseline_performance: 1.0,
        }
    }

    fn high_freq_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::HighFrequency,
            architecture: CpuArchitecture::X86_64,
            cores: 8,
            memory_gb: 32,
            network_bandwidth_gbps: 25.0,
            baseline_performance: 1.0,
        }
    }

    fn many_core_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::ManyCore,
            architecture: CpuArchitecture::X86_64,
            cores: 96,
            memory_gb: 384,
            network_bandwidth_gbps: 100.0,
            baseline_performance: 1.0,
        }
    }

    fn many_core_arm() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::ManyCore,
            architecture: CpuArchitecture::ARM64,
            cores: 192,
            memory_gb: 512,
            network_bandwidth_gbps: 100.0,
            baseline_performance: 1.0,
        }
    }

    fn high_mem_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::HighMemory,
            architecture: CpuArchitecture::X86_64,
            cores: 48,
            memory_gb: 768,
            network_bandwidth_gbps: 100.0,
            baseline_performance: 1.0,
        }
    }

    fn high_mem_ultra_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::HighMemory,
            architecture: CpuArchitecture::X86_64,
            cores: 128,
            memory_gb: 2048,
            network_bandwidth_gbps: 100.0,
            baseline_performance: 1.0,
        }
    }

    fn gpu_training_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::GpuTraining,
            architecture: CpuArchitecture::X86_64,
            cores: 96,
            memory_gb: 768,
            network_bandwidth_gbps: 100.0,
            baseline_performance: 1.0,
        }
    }

    fn gpu_inference_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::GpuInference,
            architecture: CpuArchitecture::X86_64,
            cores: 8,
            memory_gb: 32,
            network_bandwidth_gbps: 25.0,
            baseline_performance: 1.0,
        }
    }

    fn gpu_inference_arm() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::GpuInference,
            architecture: CpuArchitecture::ARM64,
            cores: 16,
            memory_gb: 64,
            network_bandwidth_gbps: 25.0,
            baseline_performance: 1.0,
        }
    }

    fn riscv_large() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::Large,
            architecture: CpuArchitecture::RISCV,
            cores: 16,
            memory_gb: 64,
            network_bandwidth_gbps: 10.0,
            baseline_performance: 0.6,
        }
    }

    fn fpga_x86() -> ComputeInstance {
        ComputeInstance {
            class: InstanceClass::FpgaAccel,
            architecture: CpuArchitecture::X86_64,
            cores: 8,
            memory_gb: 32,
            network_bandwidth_gbps: 25.0,
            baseline_performance: 1.0,
        }
    }

    // ── Catalog builders (split by category) ─────────────────

    fn ssd(gb: u64) -> StorageInstance {
        StorageInstance::new(StorageTier::StandardSsd, gb)
    }
    fn hi_ssd(gb: u64) -> StorageInstance {
        StorageInstance::new(StorageTier::HighIopsSsd, gb)
    }
    fn nvme(gb: u64) -> StorageInstance {
        StorageInstance::new(StorageTier::LocalNvme, gb)
    }
    fn hdd(gb: u64) -> StorageInstance {
        StorageInstance::new(StorageTier::StandardHdd, gb)
    }

    fn general_purpose_profiles() -> Vec<DeploymentProfile> {
        vec![
            DeploymentProfile::new(Self::nano_x86(), vec![Self::ssd(20)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::nano_arm(), vec![Self::ssd(20)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::micro_x86(), vec![Self::ssd(50)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::micro_arm(), vec![Self::ssd(50)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::small_x86(), vec![Self::ssd(100)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::small_arm(), vec![Self::ssd(100)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::small_riscv(), vec![Self::ssd(100)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::medium_x86(), vec![Self::ssd(500)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::medium_arm(), vec![Self::ssd(500)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::medium_riscv(), vec![Self::ssd(500)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::medium_x86(), vec![Self::hi_ssd(500)], ClusterTopology::SmallCluster(3)),
            DeploymentProfile::new(Self::medium_arm(), vec![Self::hi_ssd(500)], ClusterTopology::SmallCluster(3)),
            DeploymentProfile::new(Self::large_x86(), vec![Self::hi_ssd(1000)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::large_arm(), vec![Self::hi_ssd(1000)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::large_x86(), vec![Self::hi_ssd(1000)], ClusterTopology::SmallCluster(5)),
            DeploymentProfile::new(Self::large_arm(), vec![Self::hi_ssd(2000)], ClusterTopology::SmallCluster(8)),
            DeploymentProfile::new(Self::xlarge_x86(), vec![Self::hi_ssd(2000), Self::nvme(1000)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::xlarge_arm(), vec![Self::hi_ssd(4000), Self::nvme(2000)], ClusterTopology::SmallCluster(4)),
        ]
    }

    fn specialized_profiles() -> Vec<DeploymentProfile> {
        vec![
            // Compute-optimized
            DeploymentProfile::new(Self::compute_opt_x86(), vec![Self::ssd(500)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::compute_opt_arm(), vec![Self::ssd(500)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::compute_opt_x86(), vec![Self::hi_ssd(1000)], ClusterTopology::MediumCluster(16)),
            // Memory-optimized
            DeploymentProfile::new(Self::memory_opt_x86(), vec![Self::hi_ssd(2000)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::memory_opt_arm(), vec![Self::hi_ssd(4000)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::memory_opt_x86(), vec![Self::hi_ssd(2000)], ClusterTopology::SmallCluster(3)),
            // Storage-optimized
            DeploymentProfile::new(Self::storage_opt_x86(), vec![Self::nvme(2000), Self::hdd(10000)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::storage_opt_x86(), vec![Self::nvme(4000), Self::hdd(50000)], ClusterTopology::MediumCluster(16)),
            DeploymentProfile::new(
                Self::storage_opt_x86(),
                vec![Self::nvme(1000), Self::ssd(5000), StorageInstance::new(StorageTier::ColdHdd, 100_000)],
                ClusterTopology::SmallCluster(6),
            ),
            // Network-optimized
            DeploymentProfile::new(Self::network_opt_x86(), vec![Self::hi_ssd(2000)], ClusterTopology::MediumCluster(32)),
            DeploymentProfile::new(Self::network_opt_x86(), vec![Self::hi_ssd(4000)], ClusterTopology::LargeCluster(128)),
            // High-frequency
            DeploymentProfile::new(Self::high_freq_x86(), vec![Self::nvme(500)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::high_freq_x86(), vec![Self::nvme(1000)], ClusterTopology::SmallCluster(3)),
        ]
    }

    fn high_end_profiles() -> Vec<DeploymentProfile> {
        let obj = |gb| StorageInstance::new(StorageTier::ObjectStandard, gb);
        let archive = |gb| StorageInstance::new(StorageTier::ObjectArchive, gb);

        vec![
            // Many-core
            DeploymentProfile::new(Self::many_core_x86(), vec![Self::nvme(2000), Self::hi_ssd(8000)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::many_core_arm(), vec![Self::nvme(4000), Self::hi_ssd(16000)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::many_core_x86(), vec![Self::nvme(2000)], ClusterTopology::MediumCluster(16)),
            // High-memory
            DeploymentProfile::new(Self::high_mem_x86(), vec![Self::nvme(2000), Self::hi_ssd(10000)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::high_mem_ultra_x86(), vec![Self::nvme(4000), Self::hi_ssd(20000)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::high_mem_x86(), vec![Self::nvme(2000)], ClusterTopology::SmallCluster(4)),
            // GPU
            DeploymentProfile::new(Self::gpu_training_x86(), vec![Self::nvme(4000), Self::hi_ssd(16000)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::gpu_inference_x86(), vec![Self::ssd(500)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::gpu_inference_arm(), vec![Self::ssd(500)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::gpu_training_x86(), vec![Self::nvme(4000)], ClusterTopology::SmallCluster(8)),
            // FPGA
            DeploymentProfile::new(Self::fpga_x86(), vec![Self::hi_ssd(1000)], ClusterTopology::SingleNode),
            // RISC-V
            DeploymentProfile::new(Self::riscv_large(), vec![Self::ssd(500)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::riscv_large(), vec![Self::ssd(1000)], ClusterTopology::SmallCluster(3)),
            // Data warehouse topologies
            DeploymentProfile::new(Self::xlarge_x86(), vec![Self::nvme(2000), obj(50000), archive(500_000)], ClusterTopology::MediumCluster(16)),
            DeploymentProfile::new(Self::many_core_x86(), vec![Self::nvme(4000), Self::hi_ssd(20000), obj(200_000)], ClusterTopology::LargeCluster(64)),
            DeploymentProfile::new(Self::many_core_arm(), vec![Self::nvme(8000), Self::hi_ssd(50000), archive(1_000_000)], ClusterTopology::MassiveCluster(512)),
            // Multi-region
            DeploymentProfile::new(Self::large_x86(), vec![Self::hi_ssd(2000)], ClusterTopology::MultiRegion { regions: 3, nodes_per_region: 3 }),
            DeploymentProfile::new(Self::xlarge_arm(), vec![Self::hi_ssd(4000), Self::nvme(1000)], ClusterTopology::MultiRegion { regions: 5, nodes_per_region: 8 }),
            // Miscellaneous
            DeploymentProfile::new(Self::medium_x86(), vec![StorageInstance::new(StorageTier::NetworkFs, 5000)], ClusterTopology::SmallCluster(4)),
            DeploymentProfile::new(Self::small_x86(), vec![Self::hdd(2000)], ClusterTopology::SingleNode),
            DeploymentProfile::new(Self::large_arm(), vec![Self::hi_ssd(2000)], ClusterTopology::MediumCluster(32)),
            DeploymentProfile::new(Self::xlarge_arm(), vec![Self::nvme(2000), Self::hi_ssd(8000)], ClusterTopology::LargeCluster(128)),
            DeploymentProfile::new(Self::many_core_x86(), vec![Self::nvme(4000), Self::hi_ssd(20000)], ClusterTopology::MassiveCluster(1024)),
        ]
    }

    fn build_catalog() -> Vec<DeploymentProfile> {
        let mut catalog = Self::general_purpose_profiles();
        catalog.extend(Self::specialized_profiles());
        catalog.extend(Self::high_end_profiles());
        catalog
    }
}

impl ProfileSelector for CloudProfileSelector {
    fn select_for_scenario(
        scenario: &DatabaseScenario,
    ) -> DeploymentProfile {
        match scenario {
            DatabaseScenario::SmallDev => DeploymentProfile::new(
                Self::small_x86(), vec![Self::ssd(100)], ClusterTopology::SingleNode,
            ),
            DatabaseScenario::MediumProd => DeploymentProfile::new(
                Self::medium_x86(), vec![Self::hi_ssd(500)], ClusterTopology::SmallCluster(3),
            ),
            DatabaseScenario::LargeEnterprise => DeploymentProfile::new(
                Self::xlarge_x86(), vec![Self::hi_ssd(2000), Self::nvme(1000)], ClusterTopology::MediumCluster(16),
            ),
            DatabaseScenario::DataWarehouse => DeploymentProfile::new(
                Self::many_core_x86(),
                vec![Self::nvme(4000), StorageInstance::new(StorageTier::ObjectStandard, 200_000)],
                ClusterTopology::LargeCluster(64),
            ),
            DatabaseScenario::MemoryConstrained => DeploymentProfile::new(
                Self::nano_arm(), vec![Self::ssd(20)], ClusterTopology::SingleNode,
            ),
            DatabaseScenario::HighPerformance => DeploymentProfile::new(
                Self::many_core_arm(), vec![Self::nvme(4000), Self::hi_ssd(16000)], ClusterTopology::SmallCluster(4),
            ),
            DatabaseScenario::StaleStats => DeploymentProfile::new(
                Self::medium_arm(), vec![Self::ssd(500)], ClusterTopology::SingleNode,
            ),
            DatabaseScenario::SkewedData => DeploymentProfile::new(
                Self::large_x86(), vec![Self::hi_ssd(2000)], ClusterTopology::SmallCluster(3),
            ),
        }
    }

    fn select_random() -> DeploymentProfile {
        let mut catalog = Self::build_catalog();
        let idx = fastrand::usize(..catalog.len());
        catalog.swap_remove(idx)
    }

    fn select_by_architecture(
        arch: CpuArchitecture,
    ) -> Vec<DeploymentProfile> {
        Self::build_catalog()
            .into_iter()
            .filter(|p| p.compute.architecture == arch)
            .collect()
    }

    fn select_edge_cases() -> Vec<DeploymentProfile> {
        let cold = |gb| StorageInstance::new(StorageTier::ColdHdd, gb);
        let archive = |gb| StorageInstance::new(StorageTier::ObjectArchive, gb);

        vec![
            // Minimum viable: 1 core, 1 GB, HDD
            DeploymentProfile::new(Self::nano_x86(), vec![cold(10)], ClusterTopology::SingleNode),
            // Maximum single node: 128 cores, 2 TB, NVMe
            DeploymentProfile::new(Self::high_mem_ultra_x86(), vec![Self::nvme(8000)], ClusterTopology::SingleNode),
            // Massive cluster: 1024 nodes
            DeploymentProfile::new(Self::many_core_x86(), vec![Self::nvme(4000)], ClusterTopology::MassiveCluster(1024)),
            // All-archive storage (cold-only analytics)
            DeploymentProfile::new(Self::medium_x86(), vec![archive(1_000_000)], ClusterTopology::SingleNode),
            // Multi-tier storage: 5 tiers
            DeploymentProfile::new(
                Self::xlarge_x86(),
                vec![Self::nvme(500), Self::hi_ssd(2000), Self::ssd(10000), cold(50000), archive(500_000)],
                ClusterTopology::SmallCluster(4),
            ),
            // RISC-V with large cluster
            DeploymentProfile::new(Self::riscv_large(), vec![Self::ssd(1000)], ClusterTopology::MediumCluster(32)),
            // GPU cluster at scale
            DeploymentProfile::new(Self::gpu_training_x86(), vec![Self::nvme(4000)], ClusterTopology::MediumCluster(16)),
            // Geo-distributed: 10 regions
            DeploymentProfile::new(
                Self::large_arm(),
                vec![Self::hi_ssd(2000)],
                ClusterTopology::MultiRegion { regions: 10, nodes_per_region: 5 },
            ),
        ]
    }

    fn all_profiles() -> Vec<DeploymentProfile> {
        Self::build_catalog()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_non_empty() {
        let catalog = CloudProfileSelector::all_profiles();
        assert!(
            catalog.len() >= 50,
            "expected at least 50 profiles, got {}",
            catalog.len()
        );
    }

    #[test]
    fn all_profiles_produce_valid_hardware() {
        for profile in CloudProfileSelector::all_profiles() {
            let hw = profile.to_hardware_profile();
            assert!(hw.cpu_cores > 0);
            assert!(hw.available_memory > 0);
            assert!(hw.simd_width > 0);
        }
    }

    #[test]
    fn select_for_every_scenario() {
        let scenarios = [
            DatabaseScenario::SmallDev,
            DatabaseScenario::MediumProd,
            DatabaseScenario::LargeEnterprise,
            DatabaseScenario::DataWarehouse,
            DatabaseScenario::MemoryConstrained,
            DatabaseScenario::HighPerformance,
            DatabaseScenario::StaleStats,
            DatabaseScenario::SkewedData,
        ];
        for scenario in &scenarios {
            let profile =
                CloudProfileSelector::select_for_scenario(scenario);
            let hw = profile.to_hardware_profile();
            assert!(hw.cpu_cores > 0);
        }
    }

    #[test]
    fn select_random_returns_valid() {
        for _ in 0..20 {
            let profile = CloudProfileSelector::select_random();
            let hw = profile.to_hardware_profile();
            assert!(hw.cpu_cores > 0);
            assert!(hw.available_memory > 0);
        }
    }

    #[test]
    fn architecture_coverage() {
        for arch in [
            CpuArchitecture::X86_64,
            CpuArchitecture::ARM64,
            CpuArchitecture::RISCV,
        ] {
            let profiles =
                CloudProfileSelector::select_by_architecture(arch);
            assert!(
                profiles.len() >= 2,
                "architecture {:?} has only {} profiles",
                arch,
                profiles.len()
            );
            for p in &profiles {
                assert_eq!(p.compute.architecture, arch);
            }
        }
    }

    #[test]
    fn edge_cases_are_non_empty() {
        let edges = CloudProfileSelector::select_edge_cases();
        assert!(edges.len() >= 5);
    }

    #[test]
    fn distributed_profiles_exist() {
        let distributed: Vec<_> = CloudProfileSelector::all_profiles()
            .into_iter()
            .filter(DeploymentProfile::supports_distributed_execution)
            .collect();
        assert!(
            distributed.len() >= 10,
            "expected at least 10 distributed profiles, got {}",
            distributed.len()
        );
    }

    #[test]
    fn tiered_storage_profiles_exist() {
        let tiered: Vec<_> = CloudProfileSelector::all_profiles()
            .into_iter()
            .filter(DeploymentProfile::supports_tiered_storage)
            .collect();
        assert!(
            tiered.len() >= 5,
            "expected at least 5 tiered-storage profiles, got {}",
            tiered.len()
        );
    }

    #[test]
    fn gpu_profiles_exist() {
        let gpu: Vec<_> = CloudProfileSelector::all_profiles()
            .into_iter()
            .filter(|p| p.to_hardware_profile().has_gpu)
            .collect();
        assert!(
            gpu.len() >= 3,
            "expected at least 3 GPU profiles, got {}",
            gpu.len()
        );
    }
}
