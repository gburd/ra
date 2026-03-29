//! Cost model for gathering statistics.
//!
//! Estimates the resource consumption of statistics collection
//! operations including CPU, I/O, memory, and query interference.

use serde::{Deserialize, Serialize};

/// Cost to gather statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatheringCost {
    /// CPU time in milliseconds.
    pub cpu_time_ms: u64,
    /// I/O operations (pages read).
    pub io_operations: u64,
    /// Memory usage in bytes.
    pub memory_bytes: u64,
    /// Query interference factor (0.0 to 1.0).
    /// Higher values indicate more interference with concurrent queries.
    pub interference_factor: f64,
    /// Estimated wall-clock time in milliseconds.
    pub wall_time_ms: u64,
}

impl GatheringCost {
    /// Create zero cost.
    pub fn zero() -> Self {
        Self {
            cpu_time_ms: 0,
            io_operations: 0,
            memory_bytes: 0,
            interference_factor: 0.0,
            wall_time_ms: 0,
        }
    }

    /// Add two costs together.
    #[must_use]
    pub fn add(&self, other: &Self) -> Self {
        Self {
            cpu_time_ms: self.cpu_time_ms + other.cpu_time_ms,
            io_operations: self.io_operations + other.io_operations,
            memory_bytes: self.memory_bytes.max(other.memory_bytes),
            interference_factor: self.interference_factor.max(other.interference_factor),
            wall_time_ms: self.wall_time_ms + other.wall_time_ms,
        }
    }

    /// Scale cost by a factor.
    #[must_use]
    pub fn scale(&self, factor: f64) -> Self {
        Self {
            cpu_time_ms: (self.cpu_time_ms as f64 * factor) as u64,
            io_operations: (self.io_operations as f64 * factor) as u64,
            memory_bytes: self.memory_bytes,
            interference_factor: self.interference_factor,
            wall_time_ms: (self.wall_time_ms as f64 * factor) as u64,
        }
    }
}

/// Statistics gathering method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatheringMethod {
    /// Full table scan.
    FullScan,
    /// Block-level sampling.
    BlockSample {
        /// Sample rate as percentage (0-100).
        sample_rate: u32,
    },
    /// Row-level sampling.
    RowSample {
        /// Sample rate as percentage (0-100).
        sample_rate: u32,
    },
    /// Index-only scan.
    IndexScan,
    /// Incremental update.
    Incremental,
    /// Sketch-based (`HyperLogLog`, `Count-Min`).
    Sketch,
}

/// Cost estimator for statistics gathering.
#[derive(Debug, Clone)]
pub struct CostEstimator {
    /// CPU cost per row (microseconds).
    pub cpu_cost_per_row: f64,
    /// I/O cost per page (milliseconds).
    pub io_cost_per_page: f64,
    /// Page size in bytes.
    pub page_size: usize,
    /// Rows per page.
    pub rows_per_page: usize,
    /// Buffer pool hit ratio (0.0 to 1.0).
    pub buffer_hit_ratio: f64,
}

impl Default for CostEstimator {
    fn default() -> Self {
        Self {
            cpu_cost_per_row: 1.0,
            io_cost_per_page: 10.0,
            page_size: 8192,
            rows_per_page: 100,
            buffer_hit_ratio: 0.9,
        }
    }
}

impl CostEstimator {
    /// Estimate cost for a gathering method.
    pub fn estimate(
        &self,
        method: GatheringMethod,
        total_rows: u64,
        total_pages: u64,
    ) -> GatheringCost {
        match method {
            GatheringMethod::FullScan => self.full_scan_cost(total_rows, total_pages),
            GatheringMethod::BlockSample { sample_rate } => {
                self.block_sample_cost(total_rows, total_pages, sample_rate)
            }
            GatheringMethod::RowSample { sample_rate } => {
                self.row_sample_cost(total_rows, total_pages, sample_rate)
            }
            GatheringMethod::IndexScan => self.index_scan_cost(total_rows),
            GatheringMethod::Incremental => self.incremental_cost(total_rows),
            GatheringMethod::Sketch => Self::sketch_cost(total_rows),
        }
    }

    fn full_scan_cost(&self, total_rows: u64, total_pages: u64) -> GatheringCost {
        let cpu_time_ms = (total_rows as f64 * self.cpu_cost_per_row / 1000.0) as u64;
        let io_operations = total_pages;
        let effective_io = (io_operations as f64 * (1.0 - self.buffer_hit_ratio)) as u64;
        let io_time_ms = (effective_io as f64 * self.io_cost_per_page) as u64;
        let wall_time_ms = cpu_time_ms + io_time_ms;

        GatheringCost {
            cpu_time_ms,
            io_operations,
            memory_bytes: self.page_size as u64 * 10,
            interference_factor: 0.8,
            wall_time_ms,
        }
    }

    fn block_sample_cost(
        &self,
        total_rows: u64,
        total_pages: u64,
        sample_rate: u32,
    ) -> GatheringCost {
        let sampled_pages = (total_pages as f64 * f64::from(sample_rate) / 100.0) as u64;
        let sampled_rows = (total_rows as f64 * f64::from(sample_rate) / 100.0) as u64;

        let cpu_time_ms = (sampled_rows as f64 * self.cpu_cost_per_row / 1000.0) as u64;
        let effective_io = (sampled_pages as f64 * (1.0 - self.buffer_hit_ratio)) as u64;
        let io_time_ms = (effective_io as f64 * self.io_cost_per_page) as u64;
        let wall_time_ms = cpu_time_ms + io_time_ms;

        GatheringCost {
            cpu_time_ms,
            io_operations: sampled_pages,
            memory_bytes: self.page_size as u64 * 10,
            interference_factor: 0.3,
            wall_time_ms,
        }
    }

    fn row_sample_cost(
        &self,
        total_rows: u64,
        total_pages: u64,
        sample_rate: u32,
    ) -> GatheringCost {
        let sampled_rows = (total_rows as f64 * f64::from(sample_rate) / 100.0) as u64;
        let io_operations = total_pages;

        let cpu_time_ms = (sampled_rows as f64 * self.cpu_cost_per_row / 1000.0) as u64;
        let effective_io = (io_operations as f64 * (1.0 - self.buffer_hit_ratio)) as u64;
        let io_time_ms = (effective_io as f64 * self.io_cost_per_page) as u64;
        let wall_time_ms = cpu_time_ms + io_time_ms;

        GatheringCost {
            cpu_time_ms,
            io_operations,
            memory_bytes: self.page_size as u64 * 10,
            interference_factor: 0.6,
            wall_time_ms,
        }
    }

    fn index_scan_cost(&self, total_rows: u64) -> GatheringCost {
        let cpu_time_ms = (total_rows as f64 * self.cpu_cost_per_row * 0.5 / 1000.0) as u64;
        let pages_accessed = (total_rows / self.rows_per_page as u64).max(1);
        let effective_io = (pages_accessed as f64 * (1.0 - self.buffer_hit_ratio)) as u64;
        let io_time_ms = (effective_io as f64 * self.io_cost_per_page) as u64;
        let wall_time_ms = cpu_time_ms + io_time_ms;

        GatheringCost {
            cpu_time_ms,
            io_operations: pages_accessed,
            memory_bytes: self.page_size as u64 * 5,
            interference_factor: 0.4,
            wall_time_ms,
        }
    }

    fn incremental_cost(&self, total_rows: u64) -> GatheringCost {
        let modified_rows = total_rows / 100;
        let cpu_time_ms = (modified_rows as f64 * self.cpu_cost_per_row / 1000.0) as u64;

        GatheringCost {
            cpu_time_ms,
            io_operations: modified_rows / self.rows_per_page as u64,
            memory_bytes: self.page_size as u64 * 2,
            interference_factor: 0.1,
            wall_time_ms: cpu_time_ms,
        }
    }

    fn sketch_cost(total_rows: u64) -> GatheringCost {
        let cpu_time_ms = (total_rows as f64 * 0.5 / 1000.0) as u64;

        GatheringCost {
            cpu_time_ms,
            io_operations: 0,
            memory_bytes: 1024 * 1024,
            interference_factor: 0.05,
            wall_time_ms: cpu_time_ms,
        }
    }
}

/// Priority for statistics gathering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum GatheringPriority {
    /// Deferred (idle time only).
    Deferred,
    /// Low priority (background).
    Low,
    /// Normal priority.
    Normal,
    /// High priority.
    High,
    /// Critical (blocks queries).
    Critical,
}

#[cfg(test)]

mod tests {
    use super::*;

    // ---- GatheringCost ----

    #[test]
    fn cost_zero_all_fields() {
        let c = GatheringCost::zero();
        assert_eq!(c.cpu_time_ms, 0);
        assert_eq!(c.io_operations, 0);
        assert_eq!(c.memory_bytes, 0);
        assert_eq!(c.interference_factor, 0.0);
        assert_eq!(c.wall_time_ms, 0);
    }

    #[test]
    fn cost_add_sums_cpu_and_io() {
        let a = GatheringCost {
            cpu_time_ms: 100,
            io_operations: 50,
            memory_bytes: 1000,
            interference_factor: 0.5,
            wall_time_ms: 150,
        };
        let b = GatheringCost {
            cpu_time_ms: 200,
            io_operations: 75,
            memory_bytes: 2000,
            interference_factor: 0.3,
            wall_time_ms: 250,
        };
        let t = a.add(&b);
        assert_eq!(t.cpu_time_ms, 300);
        assert_eq!(t.io_operations, 125);
        assert_eq!(t.wall_time_ms, 400);
    }

    #[test]
    fn cost_add_takes_max_memory() {
        let a = GatheringCost {
            memory_bytes: 1000,
            ..GatheringCost::zero()
        };
        let b = GatheringCost {
            memory_bytes: 2000,
            ..GatheringCost::zero()
        };
        assert_eq!(a.add(&b).memory_bytes, 2000);
    }

    #[test]
    fn cost_add_takes_max_interference() {
        let a = GatheringCost {
            interference_factor: 0.5,
            ..GatheringCost::zero()
        };
        let b = GatheringCost {
            interference_factor: 0.3,
            ..GatheringCost::zero()
        };
        assert_eq!(a.add(&b).interference_factor, 0.5);
    }

    #[test]
    fn cost_scale_doubles() {
        let c = GatheringCost {
            cpu_time_ms: 100,
            io_operations: 50,
            memory_bytes: 1000,
            interference_factor: 0.5,
            wall_time_ms: 200,
        };
        let s = c.scale(2.0);
        assert_eq!(s.cpu_time_ms, 200);
        assert_eq!(s.io_operations, 100);
        assert_eq!(s.wall_time_ms, 400);
        assert_eq!(s.memory_bytes, 1000);
        assert_eq!(s.interference_factor, 0.5);
    }

    #[test]
    fn cost_scale_half() {
        let c = GatheringCost {
            cpu_time_ms: 100,
            io_operations: 50,
            memory_bytes: 1000,
            interference_factor: 0.5,
            wall_time_ms: 200,
        };
        let s = c.scale(0.5);
        assert_eq!(s.cpu_time_ms, 50);
        assert_eq!(s.io_operations, 25);
    }

    #[test]
    fn cost_scale_zero() {
        let c = GatheringCost {
            cpu_time_ms: 100,
            io_operations: 50,
            memory_bytes: 1000,
            interference_factor: 0.5,
            wall_time_ms: 200,
        };
        let s = c.scale(0.0);
        assert_eq!(s.cpu_time_ms, 0);
        assert_eq!(s.io_operations, 0);
    }

    // ---- CostEstimator ----

    #[test]
    fn estimator_default_values() {
        let e = CostEstimator::default();
        assert_eq!(e.cpu_cost_per_row, 1.0);
        assert_eq!(e.io_cost_per_page, 10.0);
        assert_eq!(e.page_size, 8192);
        assert_eq!(e.rows_per_page, 100);
        assert_eq!(e.buffer_hit_ratio, 0.9);
    }

    #[test]
    fn full_scan_has_high_interference() {
        let e = CostEstimator::default();
        let c = e.estimate(GatheringMethod::FullScan, 1_000_000, 10_000);
        assert!(c.interference_factor > 0.5);
    }

    #[test]
    fn full_scan_reads_all_pages() {
        let e = CostEstimator::default();
        let c = e.estimate(GatheringMethod::FullScan, 1_000_000, 10_000);
        assert_eq!(c.io_operations, 10_000);
    }

    #[test]
    fn full_scan_cpu_positive() {
        let e = CostEstimator::default();
        let c = e.estimate(GatheringMethod::FullScan, 1_000_000, 10_000);
        assert!(c.cpu_time_ms > 0);
    }

    #[test]
    fn block_sample_fewer_pages_than_full_scan() {
        let e = CostEstimator::default();
        let sample = e.estimate(
            GatheringMethod::BlockSample { sample_rate: 10 },
            1_000_000,
            10_000,
        );
        let full = e.estimate(GatheringMethod::FullScan, 1_000_000, 10_000);
        assert!(sample.io_operations < full.io_operations);
    }

    #[test]
    fn block_sample_less_cpu_than_full_scan() {
        let e = CostEstimator::default();
        let sample = e.estimate(
            GatheringMethod::BlockSample { sample_rate: 10 },
            1_000_000,
            10_000,
        );
        let full = e.estimate(GatheringMethod::FullScan, 1_000_000, 10_000);
        assert!(sample.cpu_time_ms < full.cpu_time_ms);
    }

    #[test]
    fn block_sample_lower_interference() {
        let e = CostEstimator::default();
        let sample = e.estimate(
            GatheringMethod::BlockSample { sample_rate: 10 },
            1_000_000,
            10_000,
        );
        assert!(sample.interference_factor < 0.5);
    }

    #[test]
    fn row_sample_reads_all_pages() {
        let e = CostEstimator::default();
        let c = e.estimate(
            GatheringMethod::RowSample { sample_rate: 10 },
            1_000_000,
            10_000,
        );
        assert_eq!(c.io_operations, 10_000);
    }

    #[test]
    fn row_sample_less_cpu_than_full() {
        let e = CostEstimator::default();
        let sample = e.estimate(
            GatheringMethod::RowSample { sample_rate: 10 },
            1_000_000,
            10_000,
        );
        let full = e.estimate(GatheringMethod::FullScan, 1_000_000, 10_000);
        assert!(sample.cpu_time_ms < full.cpu_time_ms);
    }

    #[test]
    fn index_scan_lower_interference_than_full() {
        let e = CostEstimator::default();
        let idx = e.estimate(GatheringMethod::IndexScan, 1_000_000, 10_000);
        let full = e.estimate(GatheringMethod::FullScan, 1_000_000, 10_000);
        assert!(idx.interference_factor < full.interference_factor);
    }

    #[test]
    fn incremental_minimal_cost() {
        let e = CostEstimator::default();
        let inc = e.estimate(GatheringMethod::Incremental, 1_000_000, 10_000);
        let full = e.estimate(GatheringMethod::FullScan, 1_000_000, 10_000);
        assert!(inc.cpu_time_ms < full.cpu_time_ms);
        assert!(inc.io_operations < full.io_operations);
    }

    #[test]
    fn incremental_low_interference() {
        let e = CostEstimator::default();
        let inc = e.estimate(GatheringMethod::Incremental, 1_000_000, 10_000);
        assert!(inc.interference_factor <= 0.1);
    }

    #[test]
    fn sketch_no_io() {
        let e = CostEstimator::default();
        let c = e.estimate(GatheringMethod::Sketch, 1_000_000, 10_000);
        assert_eq!(c.io_operations, 0);
    }

    #[test]
    fn sketch_very_low_interference() {
        let e = CostEstimator::default();
        let c = e.estimate(GatheringMethod::Sketch, 1_000_000, 10_000);
        assert!(c.interference_factor < 0.1);
    }

    #[test]
    fn sketch_memory_usage() {
        let e = CostEstimator::default();
        let c = e.estimate(GatheringMethod::Sketch, 1_000_000, 10_000);
        assert_eq!(c.memory_bytes, 1024 * 1024);
    }

    #[test]
    fn higher_sample_rate_higher_cost() {
        let e = CostEstimator::default();
        let low = e.estimate(
            GatheringMethod::BlockSample { sample_rate: 5 },
            1_000_000,
            10_000,
        );
        let high = e.estimate(
            GatheringMethod::BlockSample { sample_rate: 50 },
            1_000_000,
            10_000,
        );
        assert!(high.cpu_time_ms > low.cpu_time_ms);
    }

    #[test]
    fn larger_table_higher_cost() {
        let e = CostEstimator::default();
        let small = e.estimate(GatheringMethod::FullScan, 10_000, 100);
        let large = e.estimate(GatheringMethod::FullScan, 10_000_000, 100_000);
        assert!(large.cpu_time_ms > small.cpu_time_ms);
    }

    #[test]
    fn custom_estimator_params() {
        let e = CostEstimator {
            cpu_cost_per_row: 0.5,
            io_cost_per_page: 5.0,
            page_size: 4096,
            rows_per_page: 50,
            buffer_hit_ratio: 0.95,
        };
        let c = e.estimate(GatheringMethod::FullScan, 100_000, 2_000);
        assert!(c.cpu_time_ms > 0);
    }

    // ---- GatheringPriority ----

    #[test]
    fn priority_ordering() {
        assert!(GatheringPriority::Critical > GatheringPriority::High);
        assert!(GatheringPriority::High > GatheringPriority::Normal);
        assert!(GatheringPriority::Normal > GatheringPriority::Low);
        assert!(GatheringPriority::Low > GatheringPriority::Deferred);
    }

    #[test]
    fn priority_equality() {
        assert_eq!(GatheringPriority::Normal, GatheringPriority::Normal);
    }

    #[test]
    fn gathering_cost_serialize_roundtrip() {
        let c = GatheringCost {
            cpu_time_ms: 100,
            io_operations: 50,
            memory_bytes: 1000,
            interference_factor: 0.5,
            wall_time_ms: 200,
        };
        let json = serde_json::to_string(&c)
            .expect("serialize");
        let d: GatheringCost = serde_json::from_str(&json)
            .expect("deserialize");
        assert_eq!(c, d);
    }

    #[test]
    fn gathering_method_serialize_roundtrip() {
        let m = GatheringMethod::BlockSample { sample_rate: 10 };
        let json = serde_json::to_string(&m)
            .expect("serialize");
        let d: GatheringMethod = serde_json::from_str(&json)
            .expect("deserialize");
        assert_eq!(m, d);
    }
}
