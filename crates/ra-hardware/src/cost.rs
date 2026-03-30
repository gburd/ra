//! Hardware-aware cost model that compares CPU vs accelerator
//! execution including data transfer overhead.

use ra_core::{Cost, CostModel, RelExpr, StatisticsProvider};
use serde::{Deserialize, Serialize};

use crate::device::Device;
use crate::profile::HardwareProfile;

/// A cost model that estimates execution cost on different hardware
/// devices and selects the cheapest option.
///
/// For each operator, it computes:
/// - CPU cost (baseline)
/// - GPU cost = `PCIe` transfer + GPU compute
/// - FPGA cost = `PCIe` transfer + FPGA pipeline
///
/// The cheapest option wins. When the data is already resident on a
/// device (cached), the transfer cost is zero.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareCostModel {
    /// The hardware profile describing available devices.
    pub profile: HardwareProfile,
}

/// Convert a non-negative `f64` to `u64`, clamping to zero and
/// saturating at `u64::MAX`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn f64_to_u64(v: f64) -> u64 {
    if v <= 0.0 {
        0
    } else if v >= u64::MAX as f64 {
        u64::MAX
    } else {
        v as u64
    }
}

impl HardwareCostModel {
    /// Create a new hardware-aware cost model.
    #[must_use]
    pub fn new(profile: HardwareProfile) -> Self {
        Self { profile }
    }

    /// Estimate the cost of a scan on the given device.
    ///
    /// Scans are pipelined operators: startup cost is zero (first
    /// row available immediately).
    #[must_use]
    pub fn scan_cost(&self, row_count: f64, avg_row_size: u64, device: Device) -> Cost {
        let data_bytes = row_count * avg_row_size as f64;
        let mem = f64_to_u64(data_bytes);

        match device {
            Device::Cpu => {
                let bw = self.profile.cpu_memory_bandwidth_gbps;
                let cpu_time = data_bytes / (bw * 1e9);
                Cost::new(cpu_time, 0.0, 0.0, mem)
            }
            Device::Gpu => {
                let transfer = data_bytes / (self.profile.pcie_bandwidth_gbps * 1e9);
                let gpu_compute = data_bytes / (self.profile.gpu_memory_bandwidth_gbps * 1e9);
                Cost::new(gpu_compute + transfer, 0.0, 0.0, mem)
            }
            Device::Fpga => {
                let clock_mhz = f64::from(self.profile.fpga_clock_mhz);
                let clock_period = 1.0 / (clock_mhz * 1e6);
                let fpga_time = row_count * clock_period;
                Cost::new(fpga_time, 0.0, 0.0, mem)
            }
        }
    }

    /// Estimate the cost of a hash join on the given device.
    ///
    /// Hash join is a blocking operator on the build side: all
    /// build rows must be consumed before the first probe row is
    /// processed. Startup cost = build cost.
    #[must_use]
    pub fn hash_join_cost(
        &self,
        build_rows: f64,
        probe_rows: f64,
        avg_row_size: u64,
        device: Device,
    ) -> Cost {
        let row_f = avg_row_size as f64;
        let build_bytes = build_rows * row_f;
        let probe_bytes = probe_rows * row_f;
        let total_bytes = build_bytes + probe_bytes;
        let ht_mem = f64_to_u64(build_bytes * 2.0);

        match device {
            Device::Cpu => {
                let build_cost = build_rows * 100e-9;
                let probe_cost = probe_rows * 50e-9;
                Cost::with_startup(
                    build_cost + probe_cost,
                    0.0, 0.0, ht_mem,
                    build_cost, 0.0, 0.0,
                )
            }
            Device::Gpu => {
                let transfer = total_bytes / (self.profile.pcie_bandwidth_gbps * 1e9);
                let sm = f64::from(self.profile.gpu_sm_count);
                let gpu_build = build_rows * 100e-9 / sm;
                let gpu_probe = probe_rows * 50e-9 / sm;
                let build_transfer = build_bytes / (self.profile.pcie_bandwidth_gbps * 1e9);
                Cost::with_startup(
                    gpu_build + gpu_probe + transfer,
                    0.0, 0.0, ht_mem,
                    gpu_build + build_transfer, 0.0, 0.0,
                )
            }
            Device::Fpga => {
                let clock_mhz = f64::from(self.profile.fpga_clock_mhz);
                let clock_period = 1.0 / (clock_mhz * 1e6);
                let fpga_build = build_rows * clock_period * 2.0;
                let fpga_probe = probe_rows * clock_period;
                Cost::with_startup(
                    fpga_build + fpga_probe,
                    0.0, 0.0,
                    self.profile.fpga_bram_bytes,
                    fpga_build, 0.0, 0.0,
                )
            }
        }
    }

    /// Estimate the cost of sorting rows on the given device.
    ///
    /// Uses O(n log n) comparison model with device-specific constants.
    /// Sort is a fully blocking operator: startup cost equals total
    /// cost because all input must be consumed before producing the
    /// first sorted row.
    #[must_use]
    pub fn sort_cost(
        &self,
        row_count: f64,
        avg_row_size: u64,
        device: Device,
    ) -> Cost {
        let data_bytes = row_count * avg_row_size as f64;
        let mem = f64_to_u64(data_bytes);
        let n_log_n = if row_count > 1.0 {
            row_count * row_count.log2()
        } else {
            row_count
        };

        match device {
            Device::Cpu => {
                let cpu_time = n_log_n * 200e-9;
                // Sort is fully blocking: startup = total
                Cost::with_startup(
                    cpu_time, 0.0, 0.0, mem,
                    cpu_time, 0.0, 0.0,
                )
            }
            Device::Gpu => {
                let transfer = data_bytes / (self.profile.pcie_bandwidth_gbps * 1e9);
                let sm = f64::from(self.profile.gpu_sm_count);
                let gpu_time = n_log_n * 200e-9 / sm;
                let total = gpu_time + transfer;
                Cost::with_startup(
                    total, 0.0, 0.0, mem,
                    total, 0.0, 0.0,
                )
            }
            Device::Fpga => {
                Cost::with_startup(
                    f64::INFINITY, 0.0, 0.0, 0,
                    f64::INFINITY, 0.0, 0.0,
                )
            }
        }
    }

    /// Estimate the cost of an aggregation on the given device.
    ///
    /// Hash aggregation is blocking: all input rows must be consumed
    /// before groups can be emitted. Startup cost = total cost.
    #[must_use]
    pub fn aggregation_cost(
        &self,
        input_rows: f64,
        group_count: f64,
        avg_row_size: u64,
        device: Device,
    ) -> Cost {
        let data_bytes = input_rows * avg_row_size as f64;
        let group_mem = f64_to_u64(group_count * 64.0);

        match device {
            Device::Cpu => {
                let cpu_time = input_rows * 80e-9;
                Cost::with_startup(
                    cpu_time, 0.0, 0.0, group_mem,
                    cpu_time, 0.0, 0.0,
                )
            }
            Device::Gpu => {
                let transfer = data_bytes / (self.profile.pcie_bandwidth_gbps * 1e9);
                let sm = f64::from(self.profile.gpu_sm_count);
                let gpu_time = input_rows * 80e-9 / sm + group_count * 100e-9;
                let total = gpu_time + transfer;
                Cost::with_startup(
                    total, 0.0, 0.0, group_mem,
                    total, 0.0, 0.0,
                )
            }
            Device::Fpga => Cost::with_startup(
                f64::INFINITY, 0.0, 0.0, 0,
                f64::INFINITY, 0.0, 0.0,
            ),
        }
    }

    /// Choose the best device for a scan operator.
    #[must_use]
    pub fn best_scan_device(&self, row_count: f64, avg_row_size: u64) -> Device {
        let mut best_device = Device::Cpu;
        let mut best_cost = self.scan_cost(row_count, avg_row_size, Device::Cpu).total();

        if self.profile.gpu_available {
            let gpu_cost = self.scan_cost(row_count, avg_row_size, Device::Gpu).total();
            if gpu_cost < best_cost {
                best_cost = gpu_cost;
                best_device = Device::Gpu;
            }
        }

        if self.profile.fpga_available {
            let fpga_cost = self
                .scan_cost(row_count, avg_row_size, Device::Fpga)
                .total();
            if fpga_cost < best_cost {
                best_device = Device::Fpga;
            }
        }

        best_device
    }
}

impl CostModel for HardwareCostModel {
    fn estimate(&self, expr: &RelExpr, statistics: &dyn StatisticsProvider) -> Cost {
        match expr {
            RelExpr::Scan { table, .. } => {
                let stats = statistics.get_statistics(table);
                let row_count = stats.map_or(1000.0, |s| s.row_count);
                let avg_row_size = stats.map_or(100, |s| s.avg_row_size);
                let device = self.best_scan_device(row_count, avg_row_size);
                self.scan_cost(row_count, avg_row_size, device)
            }
            _ => Cost::new(1.0, 0.0, 0.0, 0),
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::profile::HardwareProfile;

    #[test]
    fn scan_cpu_cost() {
        let model = HardwareCostModel::new(HardwareProfile::cpu_only());
        let cost = model.scan_cost(1_000_000.0, 100, Device::Cpu);
        assert!(cost.cpu > 0.0);
        assert_eq!(cost.io, 0.0);
    }

    #[test]
    fn scan_gpu_includes_transfer() {
        let model = HardwareCostModel::new(HardwareProfile::gpu_server());
        let gpu_cost = model.scan_cost(1_000_000.0, 100, Device::Gpu);
        let cpu_only_compute =
            1_000_000.0 * 100.0 / (model.profile.gpu_memory_bandwidth_gbps * 1e9);
        // Total should exceed pure GPU compute (transfer added)
        assert!(gpu_cost.cpu > cpu_only_compute);
    }

    #[test]
    fn gpu_scan_slower_with_transfer_for_bandwidth_bound() {
        // For pure bandwidth-bound scans, PCIe transfer overhead
        // makes GPU slower than CPU (PCIe 25 GB/s < DDR 50 GB/s).
        let model = HardwareCostModel::new(HardwareProfile::gpu_server());
        let cpu_cost = model.scan_cost(100_000_000.0, 100, Device::Cpu);
        let gpu_cost = model.scan_cost(100_000_000.0, 100, Device::Gpu);
        assert!(
            cpu_cost.total() < gpu_cost.total(),
            "CPU scan should win over GPU when PCIe transfer \
             is the bottleneck"
        );
    }

    #[test]
    fn cpu_scan_faster_for_small_data() {
        let model = HardwareCostModel::new(HardwareProfile::gpu_server());
        let cpu_cost = model.scan_cost(100.0, 100, Device::Cpu);
        let gpu_cost = model.scan_cost(100.0, 100, Device::Gpu);
        assert!(cpu_cost.total() < gpu_cost.total());
    }

    #[test]
    fn best_scan_device_small_data() {
        let model = HardwareCostModel::new(HardwareProfile::gpu_server());
        let device = model.best_scan_device(100.0, 100);
        assert_eq!(device, Device::Cpu);
    }

    #[test]
    fn best_scan_device_prefers_cpu_when_transfer_dominates() {
        // For scan-only workloads, CPU wins because PCIe is slower
        // than CPU memory bandwidth. GPU acceleration pays off for
        // compute-heavy operations (joins, aggregations, predicates).
        let model = HardwareCostModel::new(HardwareProfile::gpu_server());
        let device = model.best_scan_device(100_000_000.0, 100);
        assert_eq!(device, Device::Cpu);
    }

    #[test]
    fn best_scan_device_no_gpu() {
        let model = HardwareCostModel::new(HardwareProfile::cpu_only());
        let device = model.best_scan_device(100_000_000.0, 100);
        assert_eq!(device, Device::Cpu);
    }

    #[test]
    fn hash_join_gpu_faster_for_large() {
        let model = HardwareCostModel::new(HardwareProfile::gpu_server());
        let cpu = model.hash_join_cost(1_000_000.0, 100_000_000.0, 100, Device::Cpu);
        let gpu = model.hash_join_cost(1_000_000.0, 100_000_000.0, 100, Device::Gpu);
        assert!(gpu.total() < cpu.total());
    }

    #[test]
    fn aggregation_gpu_faster_for_large() {
        let model = HardwareCostModel::new(HardwareProfile::gpu_server());
        let cpu = model.aggregation_cost(100_000_000.0, 100.0, 100, Device::Cpu);
        let gpu = model.aggregation_cost(100_000_000.0, 100.0, 100, Device::Gpu);
        assert!(gpu.total() < cpu.total());
    }

    #[test]
    fn fpga_aggregation_unsupported() {
        let model = HardwareCostModel::new(HardwareProfile::fpga_appliance());
        let cost = model.aggregation_cost(1_000_000.0, 1000.0, 100, Device::Fpga);
        assert!(cost.cpu.is_infinite());
    }

    #[test]
    fn sort_cost_cpu() {
        let model = HardwareCostModel::new(HardwareProfile::cpu_only());
        let cost = model.sort_cost(1_000_000.0, 100, Device::Cpu);
        assert!(cost.cpu > 0.0);
        assert_eq!(cost.io, 0.0);
    }

    #[test]
    fn sort_cost_gpu_faster_for_large() {
        let model = HardwareCostModel::new(HardwareProfile::gpu_server());
        let cpu = model.sort_cost(100_000_000.0, 100, Device::Cpu);
        let gpu = model.sort_cost(100_000_000.0, 100, Device::Gpu);
        assert!(gpu.total() < cpu.total());
    }

    #[test]
    fn sort_cost_fpga_unsupported() {
        let model = HardwareCostModel::new(HardwareProfile::fpga_appliance());
        let cost = model.sort_cost(1_000_000.0, 100, Device::Fpga);
        assert!(cost.cpu.is_infinite());
    }

    #[test]
    fn sort_cost_scales_superlinearly() {
        let model = HardwareCostModel::new(HardwareProfile::cpu_only());
        let small = model.sort_cost(1_000.0, 100, Device::Cpu);
        let large = model.sort_cost(1_000_000.0, 100, Device::Cpu);
        // Cost should scale more than linearly (n log n)
        assert!(large.cpu / small.cpu > 1000.0);
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn serialize_roundtrip() {
        let model = HardwareCostModel::new(HardwareProfile::gpu_server());
        let json = serde_json::to_string(&model).expect("serialization should succeed");
        let deserialized: HardwareCostModel =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(model.profile, deserialized.profile);
    }
}
