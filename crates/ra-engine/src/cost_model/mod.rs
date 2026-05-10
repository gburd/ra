//! Neural cost model for query optimization.
//!
//! Uses a BitNet 1.58-bit quantized model (ternary weights) for sub-100ns
//! cost prediction. Training uses QAT with Straight-Through Estimator
//! directly in ternary space.
//!
//! # Architecture
//!
//! ```text
//! Query → Feature Extraction → BitNetCostModel → 16 Cost Dimensions
//!                                                      ↓
//!                                         CPU, Memory, I/O, Network, Locks
//! ```
//!
//! The model is trained via execution feedback: observed query costs are
//! fed back to a `BitNetTrainer` which updates latent weights using STE.

mod tokenizer;
pub mod feedback;
mod feature_extractor;

pub use tokenizer::{Tokenizer, TimeBudget};
pub use feature_extractor::{extract_features, extract_features_with_stats};
pub use feedback::{ExecutionFeedback, FeedbackCollector, MapeTracker};
pub use ra_bitnet::{BitNetCostModel, BitNetTrainer, TrainerConfig};

/// Query structural features for neural cost prediction.
///
/// 12-dimensional input vector extracted from a `RelExpr` plan tree.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QueryFeatures {
    pub table_count: f32,
    pub join_count: f32,
    pub filter_count: f32,
    pub aggregate_count: f32,
    pub subquery_count: f32,
    pub cte_count: f32,
    pub window_function_count: f32,
    pub order_by_count: f32,
    pub group_by_count: f32,
    pub distinct_flag: f32,
    pub limit_present: f32,
    pub max_join_cardinality: f32,
}

impl QueryFeatures {
    /// Convert to fixed-size array for model input.
    pub fn to_vec(&self) -> Vec<f32> {
        vec![
            self.table_count,
            self.join_count,
            self.filter_count,
            self.aggregate_count,
            self.subquery_count,
            self.cte_count,
            self.window_function_count,
            self.order_by_count,
            self.group_by_count,
            self.distinct_flag,
            self.limit_present,
            self.max_join_cardinality,
        ]
    }

    /// Convert to fixed-size array for BitNet model input.
    pub fn as_array(&self) -> [f32; Self::FEATURE_DIM] {
        [
            self.table_count,
            self.join_count,
            self.filter_count,
            self.aggregate_count,
            self.subquery_count,
            self.cte_count,
            self.window_function_count,
            self.order_by_count,
            self.group_by_count,
            self.distinct_flag,
            self.limit_present,
            self.max_join_cardinality,
        ]
    }

    /// Number of features.
    pub const FEATURE_DIM: usize = 12;
}

/// Multi-dimensional cost prediction.
#[derive(Debug, Clone, PartialEq)]
pub struct CostVector {
    // Core resources
    pub cpu_time_ms: f32,
    pub memory_peak_mb: f32,
    pub memory_avg_mb: f32,

    // I/O
    pub io_storage_ops: u64,
    pub io_storage_bytes: u64,
    pub io_network_ops: u64,
    pub io_network_bytes: u64,

    // Concurrency
    pub locks_acquired: u32,
    pub lock_hold_time_ms: f32,
    pub lock_contention_score: f32,

    // Postgres-specific
    pub vacuum_overhead: f32,
    pub wal_generation_bytes: u64,
    pub replication_lag_ms: f32,

    // System
    pub cache_hit_ratio: f32,
    pub page_faults: u32,
    pub context_switches: u32,
}

impl Default for CostVector {
    fn default() -> Self {
        Self {
            cpu_time_ms: 0.0,
            memory_peak_mb: 0.0,
            memory_avg_mb: 0.0,
            io_storage_ops: 0,
            io_storage_bytes: 0,
            io_network_ops: 0,
            io_network_bytes: 0,
            locks_acquired: 0,
            lock_hold_time_ms: 0.0,
            lock_contention_score: 0.0,
            vacuum_overhead: 0.0,
            wal_generation_bytes: 0,
            replication_lag_ms: 0.0,
            cache_hit_ratio: 1.0,
            page_faults: 0,
            context_switches: 0,
        }
    }
}

/// Actual observed costs from query execution.
///
/// Used for training the neural cost model via execution feedback.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ActualCost {
    pub cpu_time_ms: f32,
    pub memory_peak_mb: f32,
    pub memory_avg_mb: f32,
    pub io_storage_ops: u64,
    pub io_storage_bytes: u64,
    pub io_network_ops: u64,
    pub io_network_bytes: u64,
    pub locks_acquired: u32,
    pub lock_hold_time_ms: f32,
    pub lock_contention_score: f32,
    pub vacuum_overhead: f32,
    pub wal_generation_bytes: u64,
    pub replication_lag_ms: f32,
    pub cache_hit_ratio: f32,
    pub page_faults: u32,
    pub context_switches: u32,
}
