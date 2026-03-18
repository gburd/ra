//! Predefined hardware profiles combining CPU, memory, storage, and GPU.

use serde::{Deserialize, Serialize};

use crate::cpu::CpuModel;
use crate::gpu::GpuModel;
use crate::memory::{MemoryConfig, MemoryType, NumaTopology};
use crate::storage::StorageDevice;

/// A complete hardware profile combining all components.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompleteHardwareProfile {
    /// Human-readable name.
    pub name: String,
    /// CPU model.
    pub cpu: CpuModel,
    /// Memory configuration.
    pub memory: MemoryConfig,
    /// Primary storage device.
    pub storage: StorageDevice,
    /// Optional GPU accelerator.
    pub gpu: Option<GpuModel>,
}

impl CompleteHardwareProfile {
    /// Desktop workstation: Intel i9-13900K, 64GB DDR5, ``NVMe`` Gen4, RTX 4070.
    #[must_use]
    pub fn desktop_workstation() -> Self {
        Self {
            name: "Desktop Workstation".into(),
            cpu: CpuModel::intel_core_i9_13900k(),
            memory: MemoryConfig::ddr5_single_socket(),
            storage: StorageDevice::nvme_gen4_samsung_990_pro(),
            gpu: Some(GpuModel::nvidia_rtx_4070()),
        }
    }

    /// Desktop enthusiast: AMD Ryzen 9 7950X, 64GB DDR5, ``NVMe`` Gen5, RTX 4090.
    #[must_use]
    pub fn desktop_enthusiast() -> Self {
        Self {
            name: "Desktop Enthusiast".into(),
            cpu: CpuModel::amd_ryzen_9_7950x(),
            memory: MemoryConfig::ddr5_single_socket(),
            storage: StorageDevice::nvme_gen5_consumer(),
            gpu: Some(GpuModel::nvidia_rtx_4090()),
        }
    }

    /// Apple M2 Mac: M2 CPU+GPU, 24GB unified memory, fast SSD.
    #[must_use]
    pub fn apple_mac_m2() -> Self {
        Self {
            name: "Apple Mac M2".into(),
            cpu: CpuModel::apple_m2(),
            memory: MemoryConfig::apple_m2_unified(),
            storage: StorageDevice::nvme_gen4_enterprise(),
            gpu: Some(GpuModel::apple_m2()),
        }
    }

    /// Entry server: Single Xeon, 128GB DDR4, SATA SSD, no GPU.
    #[must_use]
    pub fn entry_server() -> Self {
        Self {
            name: "Entry Server".into(),
            cpu: CpuModel::intel_xeon_8380(),
            memory: MemoryConfig::ddr4_single_socket(),
            storage: StorageDevice::sata_ssd_enterprise(),
            gpu: None,
        }
    }

    /// Dual-socket server: 2x EPYC 7763, 512GB DDR4, ``NVMe`` Gen4, no GPU.
    #[must_use]
    pub fn dual_socket_server() -> Self {
        Self {
            name: "Dual-Socket Server".into(),
            cpu: CpuModel::amd_epyc_7763(),
            memory: MemoryConfig::ddr4_dual_socket(),
            storage: StorageDevice::nvme_gen4_datacenter(),
            gpu: None,
        }
    }

    /// Quad-socket server: 4x EPYC, 2TB DDR4, ``NVMe`` array, no GPU.
    #[must_use]
    pub fn quad_socket_server() -> Self {
        Self {
            name: "Quad-Socket Server".into(),
            cpu: CpuModel::amd_epyc_7763(),
            memory: MemoryConfig::ddr4_quad_socket(),
            storage: StorageDevice::nvme_gen4_datacenter(),
            gpu: None,
        }
    }

    /// GPU server: Dual Xeon, 512GB DDR4, ``NVMe``, NVIDIA A100.
    #[must_use]
    pub fn gpu_server_a100() -> Self {
        Self {
            name: "GPU Server (A100)".into(),
            cpu: CpuModel::intel_xeon_8380(),
            memory: MemoryConfig::ddr4_dual_socket(),
            storage: StorageDevice::nvme_gen4_datacenter(),
            gpu: Some(GpuModel::nvidia_a100_80gb()),
        }
    }

    /// GPU server next-gen: Dual Xeon, 1TB DDR5, `NVMe`, NVIDIA H100.
    #[must_use]
    pub fn gpu_server_h100() -> Self {
        Self {
            name: "GPU Server (H100)".into(),
            cpu: CpuModel::intel_xeon_8380(),
            memory: MemoryConfig::ddr5_dual_socket(),
            storage: StorageDevice::nvme_gen4_datacenter(),
            gpu: Some(GpuModel::nvidia_h100_80gb()),
        }
    }

    /// AMD GPU server: Dual EPYC, 512GB DDR4, `NVMe`, AMD MI250X.
    #[must_use]
    pub fn gpu_server_amd_mi250x() -> Self {
        Self {
            name: "GPU Server (MI250X)".into(),
            cpu: CpuModel::amd_epyc_7763(),
            memory: MemoryConfig::ddr4_dual_socket(),
            storage: StorageDevice::nvme_gen4_datacenter(),
            gpu: Some(GpuModel::amd_mi250x()),
        }
    }

    /// AMD GPU server next-gen: Dual EPYC, 1TB DDR5, `NVMe`, AMD MI300X.
    #[must_use]
    pub fn gpu_server_amd_mi300x() -> Self {
        Self {
            name: "GPU Server (MI300X)".into(),
            cpu: CpuModel::amd_epyc_7763(),
            memory: MemoryConfig::ddr5_dual_socket(),
            storage: StorageDevice::nvme_gen4_datacenter(),
            gpu: Some(GpuModel::amd_mi300x()),
        }
    }

    /// Intel GPU server: Dual Xeon, 512GB DDR4, `NVMe`, Ponte Vecchio.
    #[must_use]
    pub fn gpu_server_intel() -> Self {
        Self {
            name: "GPU Server (Intel Ponte Vecchio)".into(),
            cpu: CpuModel::intel_xeon_8380(),
            memory: MemoryConfig::ddr4_dual_socket(),
            storage: StorageDevice::nvme_gen4_datacenter(),
            gpu: Some(GpuModel::intel_ponte_vecchio()),
        }
    }

    /// Data warehouse: Quad EPYC, 2TB DDR4, 8x `NVMe`, Optane, no GPU.
    #[must_use]
    pub fn data_warehouse() -> Self {
        Self {
            name: "Data Warehouse".into(),
            cpu: CpuModel::amd_epyc_7763(),
            memory: MemoryConfig::ddr4_quad_socket(),
            storage: StorageDevice::nvme_gen4_intel_optane(),
            gpu: None,
        }
    }

    /// OLTP database: Dual Xeon, 512GB DDR5, Optane SSD, no GPU.
    #[must_use]
    pub fn oltp_database() -> Self {
        Self {
            name: "OLTP Database".into(),
            cpu: CpuModel::intel_xeon_8380(),
            memory: MemoryConfig::ddr5_dual_socket(),
            storage: StorageDevice::nvme_gen4_intel_optane(),
            gpu: None,
        }
    }

    /// OLAP database: Quad EPYC, 2TB DDR4, `NVMe` array, A100.
    #[must_use]
    pub fn olap_database() -> Self {
        Self {
            name: "OLAP Database".into(),
            cpu: CpuModel::amd_epyc_7763(),
            memory: MemoryConfig::ddr4_quad_socket(),
            storage: StorageDevice::nvme_gen4_datacenter(),
            gpu: Some(GpuModel::nvidia_a100_80gb()),
        }
    }

    /// Edge device: ARM Graviton3, 64GB DDR5, `NVMe`, no GPU.
    #[must_use]
    pub fn edge_device() -> Self {
        Self {
            name: "Edge Device (Graviton3)".into(),
            cpu: CpuModel::arm_graviton3(),
            memory: MemoryConfig::ddr5_single_socket(),
            storage: StorageDevice::nvme_gen4_samsung_990_pro(),
            gpu: None,
        }
    }

    /// Cloud VM small: 8 vCPUs, 32GB, cloud storage.
    #[must_use]
    pub fn cloud_vm_small() -> Self {
        Self {
            name: "Cloud VM (m5.2xlarge)".into(),
            cpu: CpuModel::intel_xeon_8380(),
            memory: MemoryConfig::ddr4_single_socket(),
            storage: StorageDevice::cloud_s3_standard(),
            gpu: None,
        }
    }

    /// Cloud VM large: 64 vCPUs, 512GB, cloud storage.
    #[must_use]
    pub fn cloud_vm_large() -> Self {
        Self {
            name: "Cloud VM (m5.16xlarge)".into(),
            cpu: CpuModel::amd_epyc_7763(),
            memory: MemoryConfig::ddr4_dual_socket(),
            storage: StorageDevice::cloud_s3_standard(),
            gpu: None,
        }
    }

    /// Cloud GPU instance: 96 vCPUs, 1TB, A100, cloud storage.
    #[must_use]
    pub fn cloud_gpu_instance() -> Self {
        Self {
            name: "Cloud GPU Instance (p4d.24xlarge)".into(),
            cpu: CpuModel::intel_xeon_8380(),
            memory: MemoryConfig::ddr5_dual_socket(),
            storage: StorageDevice::cloud_s3_standard(),
            gpu: Some(GpuModel::nvidia_a100_80gb()),
        }
    }

    /// Cloud ARM instance: Graviton3, 64GB, cloud storage.
    #[must_use]
    pub fn cloud_arm_instance() -> Self {
        Self {
            name: "Cloud ARM Instance (c7g.16xlarge)".into(),
            cpu: CpuModel::arm_graviton3(),
            memory: MemoryConfig::ddr5_dual_socket(),
            storage: StorageDevice::cloud_s3_standard(),
            gpu: None,
        }
    }

    /// Persistent memory server: Dual Xeon, 1.5TB Optane PMEM, `NVMe`.
    #[must_use]
    pub fn persistent_memory_server() -> Self {
        Self {
            name: "Persistent Memory Server".into(),
            cpu: CpuModel::intel_xeon_8380(),
            memory: MemoryConfig::persistent_memory(),
            storage: StorageDevice::nvme_gen4_datacenter(),
            gpu: None,
        }
    }

    /// Archive storage: Single CPU, 128GB, HDDs, cloud glacier.
    #[must_use]
    pub fn archive_storage() -> Self {
        Self {
            name: "Archive Storage".into(),
            cpu: CpuModel::intel_xeon_8380(),
            memory: MemoryConfig::ddr4_single_socket(),
            storage: StorageDevice::hdd_7200rpm_enterprise(),
            gpu: None,
        }
    }

    /// All-flash array: Quad EPYC, 2TB DDR5, 24x `NVMe` Gen4.
    #[must_use]
    pub fn all_flash_array() -> Self {
        Self {
            name: "All-Flash Array".into(),
            cpu: CpuModel::amd_epyc_7763(),
            memory: MemoryConfig::ddr5_dual_socket(),
            storage: StorageDevice::nvme_gen4_datacenter(),
            gpu: None,
        }
    }

    /// ML training workstation: Ryzen 9, 64GB, `NVMe` Gen5, 2x RTX 4090.
    #[must_use]
    pub fn ml_training_workstation() -> Self {
        Self {
            name: "ML Training Workstation".into(),
            cpu: CpuModel::amd_ryzen_9_7950x(),
            memory: MemoryConfig::ddr5_single_socket(),
            storage: StorageDevice::nvme_gen5_consumer(),
            gpu: Some(GpuModel::nvidia_rtx_4090()),
        }
    }

    /// ML inference server: Dual Xeon, 512GB DDR5, `NVMe`, H100.
    #[must_use]
    pub fn ml_inference_server() -> Self {
        Self {
            name: "ML Inference Server".into(),
            cpu: CpuModel::intel_xeon_8380(),
            memory: MemoryConfig::ddr5_dual_socket(),
            storage: StorageDevice::nvme_gen4_datacenter(),
            gpu: Some(GpuModel::nvidia_h100_80gb()),
        }
    }

    /// Raspberry Pi 4 edge device: 4-core ARM, 4GB LPDDR4, `microSD`.
    #[must_use]
    pub fn raspberry_pi_4() -> Self {
        Self {
            name: "Raspberry Pi 4 (4GB)".into(),
            cpu: CpuModel::raspberry_pi_4(),
            memory: MemoryConfig {
                name: "LPDDR4-3200 (4GB)".into(),
                memory_type: MemoryType::DDR4,
                capacity_bytes: 4_294_967_296,
                channels_per_socket: 1,
                sockets: 1,
                numa_topology: NumaTopology::UMA,
                local_bandwidth_gbps: 4.0,
                remote_bandwidth_gbps: 4.0,
                latency_ns: 120.0,
            },
            storage: StorageDevice::microsd_uhs1(),
            gpu: None,
        }
    }

    /// Tape archive server: Xeon, 128GB, LTO-9 tape library.
    #[must_use]
    pub fn tape_archive() -> Self {
        Self {
            name: "Tape Archive Server".into(),
            cpu: CpuModel::intel_xeon_8380(),
            memory: MemoryConfig::ddr4_single_socket(),
            storage: StorageDevice::tape_lto9(),
            gpu: None,
        }
    }

    /// NAS-backed analytics: EPYC, 256GB, NFS over 10GbE.
    #[must_use]
    pub fn nas_analytics() -> Self {
        Self {
            name: "NAS Analytics Server".into(),
            cpu: CpuModel::amd_epyc_7763(),
            memory: MemoryConfig::ddr4_dual_socket(),
            storage: StorageDevice::nas_nfs_10gbe(),
            gpu: None,
        }
    }

    /// Desktop workstation (budget): Intel i7, 16GB DDR5, `NVMe` Gen4, no GPU.
    #[must_use]
    pub fn desktop_budget() -> Self {
        Self {
            name: "Desktop Budget".into(),
            cpu: CpuModel::intel_core_i7_12700k(),
            memory: MemoryConfig {
                name: "DDR5-4800 (16GB, 2ch)".into(),
                memory_type: MemoryType::DDR5,
                capacity_bytes: 17_179_869_184,
                channels_per_socket: 2,
                sockets: 1,
                numa_topology: NumaTopology::UMA,
                local_bandwidth_gbps: 76.8,
                remote_bandwidth_gbps: 76.8,
                latency_ns: 78.0,
            },
            storage: StorageDevice::nvme_gen4_samsung_990_pro(),
            gpu: None,
        }
    }

    /// Returns a list of all predefined profiles.
    #[must_use]
    pub fn all_profiles() -> Vec<Self> {
        vec![
            Self::desktop_workstation(),
            Self::desktop_enthusiast(),
            Self::desktop_budget(),
            Self::apple_mac_m2(),
            Self::entry_server(),
            Self::dual_socket_server(),
            Self::quad_socket_server(),
            Self::gpu_server_a100(),
            Self::gpu_server_h100(),
            Self::gpu_server_amd_mi250x(),
            Self::gpu_server_amd_mi300x(),
            Self::gpu_server_intel(),
            Self::data_warehouse(),
            Self::oltp_database(),
            Self::olap_database(),
            Self::edge_device(),
            Self::raspberry_pi_4(),
            Self::cloud_vm_small(),
            Self::cloud_vm_large(),
            Self::cloud_gpu_instance(),
            Self::cloud_arm_instance(),
            Self::persistent_memory_server(),
            Self::archive_storage(),
            Self::tape_archive(),
            Self::nas_analytics(),
            Self::all_flash_array(),
            Self::ml_training_workstation(),
            Self::ml_inference_server(),
        ]
    }

    /// Returns the number of predefined profiles.
    #[must_use]
    pub fn profile_count() -> usize {
        28
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_workstation_profile() {
        let profile = CompleteHardwareProfile::desktop_workstation();
        assert!(profile.gpu.is_some());
        assert_eq!(profile.cpu.cores, 24);
    }

    #[test]
    fn desktop_enthusiast_profile() {
        let profile = CompleteHardwareProfile::desktop_enthusiast();
        assert!(profile.gpu.is_some());
    }

    #[test]
    fn apple_mac_m2_profile() {
        let profile = CompleteHardwareProfile::apple_mac_m2();
        assert!(profile.gpu.is_some());
    }

    #[test]
    fn entry_server_no_gpu() {
        let profile = CompleteHardwareProfile::entry_server();
        assert!(profile.gpu.is_none());
    }

    #[test]
    fn dual_socket_server_profile() {
        let profile = CompleteHardwareProfile::dual_socket_server();
        assert!(profile.gpu.is_none());
        assert_eq!(profile.cpu.cores, 64);
    }

    #[test]
    fn quad_socket_server_profile() {
        let profile = CompleteHardwareProfile::quad_socket_server();
        assert!(profile.gpu.is_none());
    }

    #[test]
    fn gpu_server_a100_profile() {
        let profile = CompleteHardwareProfile::gpu_server_a100();
        assert!(profile.gpu.is_some());
    }

    #[test]
    fn gpu_server_h100_profile() {
        let profile = CompleteHardwareProfile::gpu_server_h100();
        assert!(profile.gpu.is_some());
    }

    #[test]
    fn data_warehouse_profile() {
        let profile = CompleteHardwareProfile::data_warehouse();
        assert!(profile.gpu.is_none());
    }

    #[test]
    fn olap_database_has_gpu() {
        let profile = CompleteHardwareProfile::olap_database();
        assert!(profile.gpu.is_some());
    }

    #[test]
    fn edge_device_profile() {
        let profile = CompleteHardwareProfile::edge_device();
        assert!(profile.gpu.is_none());
    }

    #[test]
    fn cloud_vm_small_profile() {
        let profile = CompleteHardwareProfile::cloud_vm_small();
        assert!(profile.gpu.is_none());
    }

    #[test]
    fn cloud_gpu_instance_profile() {
        let profile = CompleteHardwareProfile::cloud_gpu_instance();
        assert!(profile.gpu.is_some());
    }

    #[test]
    fn persistent_memory_server_profile() {
        let profile = CompleteHardwareProfile::persistent_memory_server();
        assert!(profile.gpu.is_none());
    }

    #[test]
    fn ml_training_workstation_profile() {
        let profile = CompleteHardwareProfile::ml_training_workstation();
        assert!(profile.gpu.is_some());
    }

    #[test]
    fn ml_inference_server_profile() {
        let profile = CompleteHardwareProfile::ml_inference_server();
        assert!(profile.gpu.is_some());
    }

    #[test]
    fn all_profiles_count() {
        let profiles = CompleteHardwareProfile::all_profiles();
        assert_eq!(profiles.len(), CompleteHardwareProfile::profile_count());
        assert_eq!(profiles.len(), 28);
    }

    #[test]
    fn all_profiles_unique_names() {
        let profiles = CompleteHardwareProfile::all_profiles();
        let names: std::collections::HashSet<_> = profiles.iter().map(|p| &p.name).collect();
        assert_eq!(names.len(), profiles.len());
    }

    #[test]
    fn raspberry_pi_4_profile() {
        let profile = CompleteHardwareProfile::raspberry_pi_4();
        assert!(profile.gpu.is_none());
        assert_eq!(profile.cpu.cores, 4);
        assert!(profile.memory.capacity_bytes <= 8_589_934_592);
    }

    #[test]
    fn tape_archive_profile() {
        let profile = CompleteHardwareProfile::tape_archive();
        assert!(profile.gpu.is_none());
        assert_eq!(
            profile.storage.technology,
            crate::storage::StorageTechnology::Tape
        );
    }

    #[test]
    fn nas_analytics_profile() {
        let profile = CompleteHardwareProfile::nas_analytics();
        assert!(profile.gpu.is_none());
        assert_eq!(
            profile.storage.technology,
            crate::storage::StorageTechnology::NAS
        );
    }

    #[test]
    fn desktop_budget_profile() {
        let profile = CompleteHardwareProfile::desktop_budget();
        assert!(profile.gpu.is_none());
        assert_eq!(profile.cpu.cores, 12);
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn serialize_roundtrip() {
        let profile = CompleteHardwareProfile::gpu_server_a100();
        let json = serde_json::to_string(&profile).expect("serialization should succeed");
        let deserialized: CompleteHardwareProfile =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(profile, deserialized);
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn serialize_roundtrip_raspberry_pi() {
        let profile = CompleteHardwareProfile::raspberry_pi_4();
        let json = serde_json::to_string(&profile).expect("serialization should succeed");
        let deserialized: CompleteHardwareProfile =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(profile, deserialized);
    }
}
