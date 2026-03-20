//! Cost and benefit estimation for index recommendations

use serde::{Deserialize, Serialize};

/// Estimated benefit from creating an index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexBenefit {
    /// Query IDs that would benefit from this index
    pub affected_queries: Vec<String>,
    /// Average speedup factor for affected queries
    pub avg_speedup: f64,
    /// Total cost units saved across all query executions
    pub total_cost_saved: f64,
}

/// Estimated cost of creating and maintaining an index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexCost {
    /// Storage overhead in bytes
    pub storage_bytes: u64,
    /// Write overhead factor (0.0 to 1.0, where 0.05 = 5% slower writes)
    pub write_overhead: f64,
    /// Estimated time to build the index in seconds
    pub build_time_secs: f64,
}

impl IndexBenefit {
    /// Create a new benefit estimation
    pub fn new(affected_queries: Vec<String>, avg_speedup: f64, total_cost_saved: f64) -> Self {
        Self {
            affected_queries,
            avg_speedup,
            total_cost_saved,
        }
    }

    /// Check if this index provides any benefit
    pub fn has_benefit(&self) -> bool {
        !self.affected_queries.is_empty() && self.total_cost_saved > 0.0
    }

    /// Format benefit as human-readable string
    pub fn format_benefit(&self) -> String {
        format!(
            "Affects {} queries, {:.1}x avg speedup, {:.0} cost units saved",
            self.affected_queries.len(),
            self.avg_speedup,
            self.total_cost_saved
        )
    }
}

impl IndexCost {
    /// Create a new cost estimation
    pub fn new(storage_bytes: u64, write_overhead: f64, build_time_secs: f64) -> Self {
        Self {
            storage_bytes,
            write_overhead,
            build_time_secs,
        }
    }

    /// Get total cost as a single normalized value
    pub fn total(&self) -> f64 {
        // Normalize different cost components
        // Storage: 1 cost unit per MB
        let storage_cost = self.storage_bytes as f64 / (1024.0 * 1024.0);

        // Write overhead: 1000 cost units per 1% overhead
        let write_cost = self.write_overhead * 1000.0;

        // Build time: 10 cost units per second
        let build_cost = self.build_time_secs * 10.0;

        storage_cost + write_cost + build_cost
    }

    /// Format storage size as human-readable string
    pub fn format_storage(&self) -> String {
        if self.storage_bytes < 1024 {
            format!("{} B", self.storage_bytes)
        } else if self.storage_bytes < 1024 * 1024 {
            format!("{:.1} KB", self.storage_bytes as f64 / 1024.0)
        } else if self.storage_bytes < 1024 * 1024 * 1024 {
            format!("{:.1} MB", self.storage_bytes as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1} GB", self.storage_bytes as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }

    /// Format write overhead as percentage
    pub fn format_write_overhead(&self) -> String {
        format!("{:.1}%", self.write_overhead * 100.0)
    }

    /// Format build time as human-readable duration
    pub fn format_build_time(&self) -> String {
        if self.build_time_secs < 1.0 {
            format!("{:.0} ms", self.build_time_secs * 1000.0)
        } else if self.build_time_secs < 60.0 {
            format!("{:.1} s", self.build_time_secs)
        } else if self.build_time_secs < 3600.0 {
            format!("{:.1} min", self.build_time_secs / 60.0)
        } else {
            format!("{:.1} h", self.build_time_secs / 3600.0)
        }
    }

    /// Format cost as human-readable string
    pub fn format_cost(&self) -> String {
        format!(
            "{} storage, {} write overhead, {} build time",
            self.format_storage(),
            self.format_write_overhead(),
            self.format_build_time()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_benefit_calculation() {
        let benefit = IndexBenefit::new(
            vec!["q1".into(), "q2".into()],
            5.0,
            1000.0,
        );

        assert!(benefit.has_benefit());
        assert_eq!(benefit.affected_queries.len(), 2);
        assert_eq!(benefit.avg_speedup, 5.0);
    }

    #[test]
    fn test_cost_calculation() {
        let cost = IndexCost::new(
            10 * 1024 * 1024, // 10 MB
            0.05,             // 5% write overhead
            2.5,              // 2.5 seconds build time
        );

        assert_eq!(cost.format_storage(), "10.0 MB");
        assert_eq!(cost.format_write_overhead(), "5.0%");
        assert_eq!(cost.format_build_time(), "2.5 s");

        // Total cost: 10 (storage) + 50 (write) + 25 (build) = 85
        assert!((cost.total() - 85.0).abs() < 0.01);
    }

    #[test]
    fn test_format_sizes() {
        assert_eq!(IndexCost::new(500, 0.0, 0.0).format_storage(), "500 B");
        assert_eq!(IndexCost::new(2048, 0.0, 0.0).format_storage(), "2.0 KB");
        assert_eq!(IndexCost::new(5 * 1024 * 1024, 0.0, 0.0).format_storage(), "5.0 MB");
        assert_eq!(IndexCost::new(2 * 1024 * 1024 * 1024, 0.0, 0.0).format_storage(), "2.0 GB");
    }

    #[test]
    fn test_format_times() {
        assert_eq!(IndexCost::new(0, 0.0, 0.5).format_build_time(), "500 ms");
        assert_eq!(IndexCost::new(0, 0.0, 30.0).format_build_time(), "30.0 s");
        assert_eq!(IndexCost::new(0, 0.0, 120.0).format_build_time(), "2.0 min");
        assert_eq!(IndexCost::new(0, 0.0, 7200.0).format_build_time(), "2.0 h");
    }
}