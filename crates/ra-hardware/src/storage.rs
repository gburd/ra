//! Storage device models including `NVMe`, SSD, HDD, and cloud storage.

use serde::{Deserialize, Serialize};

/// Storage device technology type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StorageTechnology {
    /// `NVMe` SSD over `PCIe`.
    NVMe,
    /// SATA SSD.
    SataSSD,
    /// SATA HDD (spinning disk).
    SataHDD,
    /// Cloud object storage (S3, GCS, Azure Blob).
    CloudStorage,
}

/// `PCIe` generation for `NVMe` devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PcieGen {
    /// `PCIe` Gen 3 (8 GT/s per lane).
    Gen3,
    /// `PCIe` Gen 4 (16 GT/s per lane).
    Gen4,
    /// `PCIe` Gen 5 (32 GT/s per lane).
    Gen5,
}

impl PcieGen {
    /// Returns bandwidth per lane (GB/s).
    #[must_use]
    pub fn bandwidth_per_lane_gbps(self) -> f64 {
        match self {
            Self::Gen3 => 0.985,
            Self::Gen4 => 1.969,
            Self::Gen5 => 3.938,
        }
    }

    /// Returns total bandwidth for x4 device (GB/s).
    #[must_use]
    pub fn x4_bandwidth_gbps(self) -> f64 {
        self.bandwidth_per_lane_gbps() * 4.0
    }
}

/// HDD spindle speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpindleSpeed {
    /// 5400 RPM (consumer drives).
    RPM5400,
    /// 7200 RPM (desktop drives).
    RPM7200,
    /// 10000 RPM (enterprise drives).
    RPM10K,
    /// 15000 RPM (high-performance enterprise).
    RPM15K,
}

impl SpindleSpeed {
    /// Returns typical sequential read bandwidth (MB/s).
    #[must_use]
    pub fn sequential_bandwidth_mbps(self) -> u32 {
        match self {
            Self::RPM5400 => 120,
            Self::RPM7200 => 160,
            Self::RPM10K => 220,
            Self::RPM15K => 280,
        }
    }

    /// Returns typical random read IOPS.
    #[must_use]
    pub fn random_iops(self) -> u32 {
        match self {
            Self::RPM5400 => 80,
            Self::RPM7200 => 120,
            Self::RPM10K => 180,
            Self::RPM15K => 220,
        }
    }

    /// Returns typical seek latency (ms).
    #[must_use]
    pub fn seek_latency_ms(self) -> f64 {
        match self {
            Self::RPM5400 => 9.0,
            Self::RPM7200 => 7.0,
            Self::RPM10K => 4.5,
            Self::RPM15K => 3.5,
        }
    }
}

/// Cloud storage tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CloudStorageTier {
    /// Standard/Hot tier (frequently accessed).
    Standard,
    /// Infrequent Access tier (monthly access).
    InfrequentAccess,
    /// Archive tier (yearly access, retrieval delay).
    Archive,
    /// Glacier tier (deep archive, hours to retrieve).
    Glacier,
}

impl CloudStorageTier {
    /// Returns typical download bandwidth (MB/s).
    #[must_use]
    pub fn download_bandwidth_mbps(self) -> u32 {
        match self {
            Self::Standard => 800,
            Self::InfrequentAccess => 400,
            Self::Archive => 100,
            Self::Glacier => 50,
        }
    }

    /// Returns typical first-byte latency (ms).
    #[must_use]
    pub fn first_byte_latency_ms(self) -> f64 {
        match self {
            Self::Standard => 10.0,
            Self::InfrequentAccess => 50.0,
            Self::Archive => 5_000.0,
            Self::Glacier => 180_000.0,
        }
    }

    /// Returns cost per GB per month (USD).
    #[must_use]
    pub fn cost_per_gb_per_month(self) -> f64 {
        match self {
            Self::Standard => 0.023,
            Self::InfrequentAccess => 0.0125,
            Self::Archive => 0.004,
            Self::Glacier => 0.00099,
        }
    }
}

/// A storage device model with performance characteristics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageDevice {
    /// Human-readable name.
    pub name: String,
    /// Storage technology.
    pub technology: StorageTechnology,
    /// Storage capacity (bytes).
    pub capacity_bytes: u64,
    /// Sequential read bandwidth (MB/s).
    pub sequential_read_mbps: u32,
    /// Sequential write bandwidth (MB/s).
    pub sequential_write_mbps: u32,
    /// Random read IOPS (4KB blocks).
    pub random_read_iops: u32,
    /// Random write IOPS (4KB blocks).
    pub random_write_iops: u32,
    /// Access latency (microseconds).
    pub latency_us: f64,
}

impl StorageDevice {
    /// Samsung 990 PRO (``NVMe`` Gen 4, 2TB).
    #[must_use]
    pub fn nvme_gen4_samsung_990_pro() -> Self {
        Self {
            name: "Samsung 990 PRO (2TB, PCIe 4.0)".into(),
            technology: StorageTechnology::NVMe,
            capacity_bytes: 2_199_023_255_552,
            sequential_read_mbps: 7_450,
            sequential_write_mbps: 6_900,
            random_read_iops: 1_400_000,
            random_write_iops: 1_550_000,
            latency_us: 20.0,
        }
    }

    /// Intel Optane P5800X (``NVMe`` Gen 4, 1.6TB, ultra-low latency).
    #[must_use]
    pub fn nvme_gen4_intel_optane() -> Self {
        Self {
            name: "Intel Optane P5800X (1.6TB, PCIe 4.0)".into(),
            technology: StorageTechnology::NVMe,
            capacity_bytes: 1_759_218_604_441,
            sequential_read_mbps: 7_200,
            sequential_write_mbps: 6_200,
            random_read_iops: 1_500_000,
            random_write_iops: 200_000,
            latency_us: 8.0,
        }
    }

    /// Samsung PM9A3 (``NVMe`` Gen 4, enterprise, 7.68TB).
    #[must_use]
    pub fn nvme_gen4_enterprise() -> Self {
        Self {
            name: "Samsung PM9A3 (7.68TB, PCIe 4.0)".into(),
            technology: StorageTechnology::NVMe,
            capacity_bytes: 8_444_249_301_319,
            sequential_read_mbps: 7_000,
            sequential_write_mbps: 4_000,
            random_read_iops: 1_000_000,
            random_write_iops: 180_000,
            latency_us: 50.0,
        }
    }

    /// Micron 7450 PRO (``NVMe`` Gen 4, 15.36TB, data center).
    #[must_use]
    pub fn nvme_gen4_datacenter() -> Self {
        Self {
            name: "Micron 7450 PRO (15.36TB, PCIe 4.0)".into(),
            technology: StorageTechnology::NVMe,
            capacity_bytes: 16_888_498_602_639,
            sequential_read_mbps: 6_800,
            sequential_write_mbps: 5_300,
            random_read_iops: 1_500_000,
            random_write_iops: 400_000,
            latency_us: 40.0,
        }
    }

    /// Generic `PCIe` Gen 5 ``NVMe`` (4TB, next-gen).
    #[must_use]
    pub fn nvme_gen5_consumer() -> Self {
        Self {
            name: "`NVMe` Gen5 Consumer (4TB, PCIe 5.0)".into(),
            technology: StorageTechnology::NVMe,
            capacity_bytes: 4_398_046_511_104,
            sequential_read_mbps: 12_000,
            sequential_write_mbps: 10_000,
            random_read_iops: 1_800_000,
            random_write_iops: 2_000_000,
            latency_us: 15.0,
        }
    }

    /// Samsung 870 EVO (SATA SSD, 4TB).
    #[must_use]
    pub fn sata_ssd_consumer() -> Self {
        Self {
            name: "Samsung 870 EVO (4TB, SATA)".into(),
            technology: StorageTechnology::SataSSD,
            capacity_bytes: 4_398_046_511_104,
            sequential_read_mbps: 560,
            sequential_write_mbps: 530,
            random_read_iops: 98_000,
            random_write_iops: 88_000,
            latency_us: 100.0,
        }
    }

    /// Enterprise SATA SSD (7.68TB).
    #[must_use]
    pub fn sata_ssd_enterprise() -> Self {
        Self {
            name: "Enterprise SATA SSD (7.68TB)".into(),
            technology: StorageTechnology::SataSSD,
            capacity_bytes: 8_444_249_301_319,
            sequential_read_mbps: 540,
            sequential_write_mbps: 520,
            random_read_iops: 95_000,
            random_write_iops: 80_000,
            latency_us: 80.0,
        }
    }

    /// Western Digital Gold (7200 RPM, 18TB enterprise HDD).
    #[must_use]
    pub fn hdd_7200rpm_enterprise() -> Self {
        Self {
            name: "WD Gold (18TB, 7200 RPM)".into(),
            technology: StorageTechnology::SataHDD,
            capacity_bytes: 19_791_209_299_968,
            sequential_read_mbps: 268,
            sequential_write_mbps: 268,
            random_read_iops: 180,
            random_write_iops: 180,
            latency_us: 7_000.0,
        }
    }

    /// Seagate Exos (7200 RPM, 20TB enterprise HDD).
    #[must_use]
    pub fn hdd_7200rpm_exos() -> Self {
        Self {
            name: "Seagate Exos (20TB, 7200 RPM)".into(),
            technology: StorageTechnology::SataHDD,
            capacity_bytes: 21_990_232_555_520,
            sequential_read_mbps: 285,
            sequential_write_mbps: 285,
            random_read_iops: 170,
            random_write_iops: 170,
            latency_us: 7_000.0,
        }
    }

    /// AWS S3 Standard (cloud object storage).
    #[must_use]
    pub fn cloud_s3_standard() -> Self {
        Self {
            name: "AWS S3 Standard".into(),
            technology: StorageTechnology::CloudStorage,
            capacity_bytes: u64::MAX,
            sequential_read_mbps: 800,
            sequential_write_mbps: 400,
            random_read_iops: 5_500,
            random_write_iops: 3_500,
            latency_us: 10_000.0,
        }
    }

    /// AWS S3 Infrequent Access.
    #[must_use]
    pub fn cloud_s3_ia() -> Self {
        Self {
            name: "AWS S3 Infrequent Access".into(),
            technology: StorageTechnology::CloudStorage,
            capacity_bytes: u64::MAX,
            sequential_read_mbps: 400,
            sequential_write_mbps: 200,
            random_read_iops: 2_000,
            random_write_iops: 1_000,
            latency_us: 50_000.0,
        }
    }

    /// AWS S3 Glacier (deep archive).
    #[must_use]
    pub fn cloud_s3_glacier() -> Self {
        Self {
            name: "AWS S3 Glacier".into(),
            technology: StorageTechnology::CloudStorage,
            capacity_bytes: u64::MAX,
            sequential_read_mbps: 50,
            sequential_write_mbps: 25,
            random_read_iops: 10,
            random_write_iops: 5,
            latency_us: 180_000_000.0,
        }
    }

    /// Estimate sequential read time (seconds).
    #[allow(clippy::cast_precision_loss)]
    #[must_use]
    pub fn sequential_read_time_s(&self, bytes: u64) -> f64 {
        let bandwidth_bytes = f64::from(self.sequential_read_mbps) * 1e6;
        let latency_s = self.latency_us * 1e-6;
        latency_s + bytes as f64 / bandwidth_bytes
    }

    /// Estimate random read time for multiple operations (seconds).
    #[allow(clippy::cast_precision_loss)]
    #[must_use]
    pub fn random_read_time_s(&self, operations: u64) -> f64 {
        let latency_s = self.latency_us * 1e-6;
        operations as f64 * latency_s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcie_gen4_bandwidth() {
        assert!((PcieGen::Gen4.x4_bandwidth_gbps() - 7.876).abs() < 0.01);
    }

    #[test]
    fn pcie_gen5_faster_than_gen4() {
        assert!(PcieGen::Gen5.x4_bandwidth_gbps() > PcieGen::Gen4.x4_bandwidth_gbps());
    }

    #[test]
    fn hdd_7200_bandwidth() {
        assert_eq!(SpindleSpeed::RPM7200.sequential_bandwidth_mbps(), 160);
    }

    #[test]
    fn hdd_15k_faster_seek() {
        assert!(SpindleSpeed::RPM15K.seek_latency_ms() < SpindleSpeed::RPM7200.seek_latency_ms());
    }

    #[test]
    fn cloud_standard_cost() {
        assert!((CloudStorageTier::Standard.cost_per_gb_per_month() - 0.023).abs() < 0.001);
    }

    #[test]
    fn cloud_glacier_lower_cost() {
        assert!(
            CloudStorageTier::Glacier.cost_per_gb_per_month()
                < CloudStorageTier::Standard.cost_per_gb_per_month()
        );
    }

    #[test]
    fn nvme_gen4_samsung() {
        let device = StorageDevice::nvme_gen4_samsung_990_pro();
        assert_eq!(device.technology, StorageTechnology::NVMe);
        assert!(device.sequential_read_mbps > 7_000);
    }

    #[test]
    fn nvme_optane_low_latency() {
        let device = StorageDevice::nvme_gen4_intel_optane();
        assert!(device.latency_us < 10.0);
    }

    #[test]
    fn nvme_gen5_fastest() {
        let gen5 = StorageDevice::nvme_gen5_consumer();
        let gen4 = StorageDevice::nvme_gen4_samsung_990_pro();
        assert!(gen5.sequential_read_mbps > gen4.sequential_read_mbps);
    }

    #[test]
    fn sata_ssd_slower_than_nvme() {
        let sata = StorageDevice::sata_ssd_consumer();
        let nvme = StorageDevice::nvme_gen4_samsung_990_pro();
        assert!(sata.sequential_read_mbps < nvme.sequential_read_mbps);
    }

    #[test]
    fn hdd_high_latency() {
        let hdd = StorageDevice::hdd_7200rpm_enterprise();
        assert!(hdd.latency_us > 5_000.0);
    }

    #[test]
    fn cloud_s3_standard() {
        let s3 = StorageDevice::cloud_s3_standard();
        assert_eq!(s3.technology, StorageTechnology::CloudStorage);
        assert!(s3.sequential_read_mbps > 0);
    }

    #[test]
    fn sequential_read_time() {
        let device = StorageDevice::nvme_gen4_samsung_990_pro();
        let time = device.sequential_read_time_s(1_000_000_000);
        assert!(time > 0.0 && time < 1.0);
    }

    #[test]
    fn random_read_time() {
        let device = StorageDevice::nvme_gen4_samsung_990_pro();
        let time = device.random_read_time_s(10_000);
        assert!(time > 0.0);
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn serialize_roundtrip() {
        let device = StorageDevice::nvme_gen4_samsung_990_pro();
        let json = serde_json::to_string(&device).expect("serialization should succeed");
        let deserialized: StorageDevice =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(device, deserialized);
    }
}
