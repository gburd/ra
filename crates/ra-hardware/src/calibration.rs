//! Calibrated cost model parameters derived from hardware benchmarks.
//!
//! Converts raw [`HardwareMeasurements`] into cost model coefficients
//! that the optimizer uses for plan comparison. The calibration
//! normalizes all costs relative to a reference machine so that
//! cost comparisons remain meaningful across hardware.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::benchmark::{
    BenchmarkConfig, HardwareMeasurements, get_measurements,
    run_benchmarks,
};
use crate::profile::HardwareProfile;

/// Cached calibrated cost model, computed once.
static CALIBRATED_MODEL: OnceLock<CalibratedCostModel> = OnceLock::new();

/// Reference machine parameters for normalization.
///
/// All costs are relative to this baseline. A machine faster than
/// the reference produces factors < 1.0; slower produces > 1.0.
const REF_SEQ_IO_MBPS: f64 = 3500.0;
const REF_RAND_IO_MBPS: f64 = 3000.0;
const REF_CPU_TUPLE_NS: f64 = 10.0;
const REF_L3_LATENCY_NS: f64 = 12.0;
const REF_DRAM_LATENCY_NS: f64 = 80.0;

/// Cost model with hardware-calibrated parameters.
///
/// Replaces static cost constants with values derived from actual
/// hardware measurements. Each field is a cost multiplier relative
/// to a reference `NVMe`-equipped server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibratedCostModel {
    /// Sequential I/O cost per MB (relative to reference).
    /// Lower = faster storage.
    pub sequential_io_cost: f64,

    /// Random I/O cost per operation (relative to reference).
    /// Lower = faster random access (`NVMe` vs HDD).
    pub random_io_cost: f64,

    /// Ratio of random to sequential I/O cost.
    /// `NVMe`: ~1.2, SATA SSD: ~1.4, HDD: ~300.
    pub random_io_ratio: f64,

    /// CPU cost per tuple (relative to reference).
    pub cpu_tuple_cost: f64,

    /// L2 cache miss penalty (L2 latency / L1 latency).
    pub l2_miss_penalty: f64,

    /// L3 cache miss penalty (L3 latency / L2 latency).
    pub l3_miss_penalty: f64,

    /// DRAM miss penalty (DRAM latency / L3 latency).
    pub dram_miss_penalty: f64,

    /// Raw measurements used to derive these parameters.
    pub measurements: HardwareMeasurements,
}

impl CalibratedCostModel {
    /// Create a calibrated cost model from hardware measurements.
    #[must_use]
    pub fn from_measurements(m: &HardwareMeasurements) -> Self {
        let seq_mbps = m.sequential_read_mbps.max(1.0);
        let rand_mbps = m.random_read_mbps.max(0.01);
        let cpu_ns = m.cpu_tuple_cost_ns.max(0.1);

        Self {
            sequential_io_cost: REF_SEQ_IO_MBPS / seq_mbps,
            random_io_cost: REF_RAND_IO_MBPS / rand_mbps,
            random_io_ratio: m.random_io_ratio(),
            cpu_tuple_cost: cpu_ns / REF_CPU_TUPLE_NS,
            l2_miss_penalty: m.l2_miss_penalty(),
            l3_miss_penalty: m.l3_miss_penalty(),
            dram_miss_penalty: m.dram_miss_penalty(),
            measurements: m.clone(),
        }
    }

    /// Create from a hardware profile using static values (no
    /// benchmarks). Used when benchmarks are disabled.
    #[must_use]
    pub fn from_profile(hw: &HardwareProfile) -> Self {
        let seq_bw = hw.storage_bandwidth_gbps * 1000.0; // GB/s to MB/s
        let seq_bw = seq_bw.max(1.0);

        // Estimate random I/O from sequential based on storage type
        // heuristic: if storage bandwidth is low, assume HDD-like ratio
        let rand_bw = if hw.storage_bandwidth_gbps < 0.5 {
            seq_bw / 300.0 // HDD
        } else if hw.storage_bandwidth_gbps < 1.0 {
            seq_bw / 1.4 // SATA SSD
        } else {
            seq_bw / 1.2 // `NVMe`
        };

        let m = HardwareMeasurements {
            sequential_read_mbps: seq_bw,
            random_read_mbps: rand_bw.max(0.01),
            cpu_tuple_cost_ns: 10.0,
            l1_latency_ns: 1.0,
            l2_latency_ns: 4.0,
            l3_latency_ns: hw.l3_latency_ns,
            dram_latency_ns: hw.dram_latency_ns,
        };

        Self::from_measurements(&m)
    }

    /// Reference machine calibration (all factors 1.0).
    #[must_use]
    pub fn reference() -> Self {
        let m = HardwareMeasurements {
            sequential_read_mbps: REF_SEQ_IO_MBPS,
            random_read_mbps: REF_RAND_IO_MBPS,
            cpu_tuple_cost_ns: REF_CPU_TUPLE_NS,
            l1_latency_ns: 1.0,
            l2_latency_ns: 4.0,
            l3_latency_ns: REF_L3_LATENCY_NS,
            dram_latency_ns: REF_DRAM_LATENCY_NS,
        };
        Self::from_measurements(&m)
    }

    /// Cost of sequential scan per page.
    ///
    /// Accounts for actual storage throughput relative to reference.
    #[must_use]
    pub fn seq_page_cost(&self) -> f64 {
        self.sequential_io_cost
    }

    /// Cost of random page access (index scan).
    ///
    /// Accounts for actual random I/O throughput. On HDD this will
    /// be ~300x the sequential cost; on `NVMe` ~1.2x.
    #[must_use]
    pub fn rand_page_cost(&self) -> f64 {
        self.random_io_cost
    }

    /// Cost of processing one tuple on CPU.
    #[must_use]
    pub fn tuple_cost(&self) -> f64 {
        self.cpu_tuple_cost
    }

    /// Cache-aware cost multiplier for a hash table of given size.
    ///
    /// Returns a penalty factor based on whether the hash table
    /// fits in L3 cache. Uses measured cache latencies rather than
    /// static assumptions.
    #[must_use]
    pub fn hash_table_cache_factor(
        &self,
        hash_table_bytes: u64,
        l3_cache_bytes: u64,
    ) -> f64 {
        #[expect(clippy::cast_precision_loss)]
        let ht_mb = hash_table_bytes as f64 / (1024.0 * 1024.0);
        #[expect(clippy::cast_precision_loss)]
        let l3_mb = (l3_cache_bytes as f64 / (1024.0 * 1024.0)).max(1.0);

        if ht_mb <= l3_mb {
            // Hash table fits in L3: use L3 latency
            1.0
        } else {
            // Hash table spills to DRAM: apply miss penalty
            let spill_fraction = ((ht_mb - l3_mb) / ht_mb).clamp(0.0, 1.0);
            1.0 + spill_fraction * (self.dram_miss_penalty - 1.0)
        }
    }
}

/// Get the cached calibrated cost model.
///
/// Runs benchmarks on first call, caches the result. Subsequent
/// calls return the cached model.
#[must_use]
pub fn get_calibrated_model() -> &'static CalibratedCostModel {
    CALIBRATED_MODEL.get_or_init(|| {
        CalibratedCostModel::from_measurements(get_measurements())
    })
}

/// Run calibration with custom config.
#[must_use]
pub fn calibrate(config: &BenchmarkConfig) -> CalibratedCostModel {
    let m = run_benchmarks(config);
    CalibratedCostModel::from_measurements(&m)
}

#[cfg(test)]
#[expect(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn reference_model_unity_factors() {
        let model = CalibratedCostModel::reference();
        assert!((model.sequential_io_cost - 1.0).abs() < 0.01);
        assert!((model.random_io_cost - 1.0).abs() < 0.01);
        assert!((model.cpu_tuple_cost - 1.0).abs() < 0.01);
    }

    #[test]
    fn nvme_calibration() {
        let m = HardwareMeasurements::default_nvme();
        let model = CalibratedCostModel::from_measurements(&m);

        // `NVMe`: sequential and random close to reference
        assert!(model.sequential_io_cost < 1.5);
        assert!(model.random_io_cost < 1.5);
        // Low random/sequential ratio
        assert!(model.random_io_ratio < 2.0);
    }

    #[test]
    fn hdd_calibration_high_random_cost() {
        let m = HardwareMeasurements::default_hdd();
        let model = CalibratedCostModel::from_measurements(&m);

        // HDD: sequential is ~23x slower than reference `NVMe`
        assert!(model.sequential_io_cost > 20.0);
        // HDD: random is ~6000x slower than reference `NVMe`
        assert!(model.random_io_cost > 5000.0);
        // Very high ratio
        assert!(model.random_io_ratio > 100.0);
    }

    #[test]
    fn sata_ssd_calibration_moderate() {
        let m = HardwareMeasurements::default_sata_ssd();
        let model = CalibratedCostModel::from_measurements(&m);

        // SATA: ~6x slower sequential than `NVMe`
        assert!(model.sequential_io_cost > 5.0);
        assert!(model.sequential_io_cost < 10.0);
        // Moderate random/sequential ratio
        assert!(model.random_io_ratio > 1.0);
        assert!(model.random_io_ratio < 5.0);
    }

    #[test]
    fn hdd_strongly_prefers_sequential() {
        let hdd = CalibratedCostModel::from_measurements(
            &HardwareMeasurements::default_hdd(),
        );
        // For a 1000-page table, compare sequential vs index scan cost
        let seq_cost = 1000.0 * hdd.seq_page_cost();
        // 100 random lookups (10% selectivity)
        let idx_cost = 100.0 * hdd.rand_page_cost();
        // On HDD, sequential scan of 1000 pages should be cheaper
        // than 100 random lookups
        assert!(
            seq_cost < idx_cost,
            "HDD should prefer seq scan: {seq_cost} vs {idx_cost}"
        );
    }

    #[test]
    fn nvme_can_prefer_index_scan() {
        let nvme = CalibratedCostModel::from_measurements(
            &HardwareMeasurements::default_nvme(),
        );
        // For a 1000-page table, compare sequential vs index scan
        let seq_cost = 1000.0 * nvme.seq_page_cost();
        // 10 random lookups (1% selectivity)
        let idx_cost = 10.0 * nvme.rand_page_cost();
        // On `NVMe`, 10 random lookups should be cheaper than
        // scanning 1000 pages
        assert!(
            idx_cost < seq_cost,
            "NVMe should prefer index scan: {idx_cost} vs {seq_cost}"
        );
    }

    #[test]
    fn cache_factor_fits_in_l3() {
        let model = CalibratedCostModel::reference();
        let factor =
            model.hash_table_cache_factor(1024 * 1024, 8 * 1024 * 1024);
        assert!((factor - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cache_factor_exceeds_l3() {
        let model = CalibratedCostModel::reference();
        let factor = model
            .hash_table_cache_factor(100 * 1024 * 1024, 8 * 1024 * 1024);
        assert!(factor > 1.0);
    }

    #[test]
    fn from_profile_nvme_like() {
        let hw = HardwareProfile::cpu_only(); // 7 GB/s storage
        let model = CalibratedCostModel::from_profile(&hw);
        assert!(model.sequential_io_cost < 1.0); // Faster than ref
        assert!(model.random_io_ratio < 2.0); // `NVMe`-like
    }

    #[test]
    fn calibrate_disabled_uses_defaults() {
        let config = BenchmarkConfig::disabled();
        let model = calibrate(&config);
        assert!((model.sequential_io_cost - 1.0).abs() < 0.01);
    }

    #[test]
    fn calibration_serialization_roundtrip() {
        let model = CalibratedCostModel::reference();
        let json = serde_json::to_string(&model)
            .expect("serialization should succeed");
        let deserialized: CalibratedCostModel =
            serde_json::from_str(&json)
                .expect("deserialization should succeed");
        assert_eq!(model, deserialized);
    }

    #[test]
    fn all_three_profiles_differ() {
        let nvme = CalibratedCostModel::from_measurements(
            &HardwareMeasurements::default_nvme(),
        );
        let sata = CalibratedCostModel::from_measurements(
            &HardwareMeasurements::default_sata_ssd(),
        );
        let hdd = CalibratedCostModel::from_measurements(
            &HardwareMeasurements::default_hdd(),
        );

        // Sequential cost should increase: `NVMe` < SATA < HDD
        assert!(nvme.sequential_io_cost < sata.sequential_io_cost);
        assert!(sata.sequential_io_cost < hdd.sequential_io_cost);

        // Random cost should increase even more dramatically
        assert!(nvme.random_io_cost < sata.random_io_cost);
        assert!(sata.random_io_cost < hdd.random_io_cost);
    }
}
