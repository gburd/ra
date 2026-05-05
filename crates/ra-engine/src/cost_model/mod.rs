//! Neural cost model for query optimization.
//!
//! This module implements a transformer-based cost prediction model that learns
//! multi-dimensional costs (CPU, memory, I/O, network, locks) from query execution
//! feedback. The model uses online learning to continuously improve predictions.
//!
//! # Architecture
//!
//! ```text
//! SQL Query → Lime Tokens → Token Embeddings → Transformer → Cost Heads
//!                                                                 ↓
//!                                                    CPU, Memory, I/O, Network, Locks
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use ra_engine::cost_model::{CostModel, TimeBudget};
//!
//! let model = CostModel::load("cost_model/model.safetensors")?;
//! let costs = model.predict(tokens, TimeBudget::Balanced)?;
//! println!("Predicted CPU time: {:.2}ms", costs.cpu_ms);
//! ```
//!
//! # Features
//!
//! - **Multi-dimensional costs**: 16 separate cost dimensions
//! - **Latency-aware**: Budget tokens encode time constraints
//! - **Online learning**: Updates from real query execution
//! - **Hybrid approach**: Combines learned costs with rule priors
//! - **GPU-accelerated**: WGPU backend for fast inference
//! - **CPU fallback**: ndarray backend when GPU unavailable
//!
//! # Model Files
//!
//! - `model.safetensors`: Binary weights (~2-5 MB)
//! - `model.toml`: Human-readable metadata
//! - `tokenizer.json`: Vocabulary mapping
//! - `training_log.jsonl`: Append-only execution history (optional)
//!
//! # Implementation Status
//!
//! **Phase 1 (In Progress)**: Infrastructure and design documentation
//! - ✅ Model metadata defined (model.toml)
//! - ✅ Tokenizer vocabulary defined (tokenizer.json)
//! - ✅ Cost dimensions specified (16 dimensions)
//! - ⏸️  Transformer implementation (requires burn crate)
//! - ⏸️  Online learning loop (requires burn crate)
//!
//! To implement the full neural cost model, add burn dependencies:
//! ```toml
//! [dependencies]
//! burn = "0.15"  # Check latest stable version
//! burn-ndarray = "0.15"
//! safetensors = "0.4"
//! ```

mod tokenizer;
pub mod simple_model;
// mod transformer;
// mod learner;
// mod cost_extractor;

pub use tokenizer::{Tokenizer, TimeBudget};
pub use simple_model::{SimpleCostModel, QueryFeatures, ModelStats};

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
            cache_hit_ratio: 1.0,  // Assume good cache by default
            page_faults: 0,
            context_switches: 0,
        }
    }
}

/// Actual observed costs from query execution.
///
/// Used for online learning feedback loop.
#[derive(Debug, Clone, PartialEq)]
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
