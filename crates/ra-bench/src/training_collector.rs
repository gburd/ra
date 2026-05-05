//! Training data collector for neural cost model.
//!
//! Executes queries against real Postgres with EXPLAIN ANALYZE and collects
//! actual execution metrics to train the neural cost model.

use ra_engine::cost_model::{ActualCost, QueryFeatures};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Configuration for a Postgres test environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresConfig {
    /// Postgres connection string
    pub connection_string: String,
    /// shared_buffers setting (affects cache behavior)
    pub shared_buffers_mb: u32,
    /// work_mem setting (affects sort/hash operations)
    pub work_mem_mb: u32,
    /// effective_cache_size hint
    pub effective_cache_size_mb: u32,
    /// random_page_cost (4.0 = HDD, 1.1 = SSD, 1.0 = all in memory)
    pub random_page_cost: f32,
}

impl PostgresConfig {
    /// Default configuration (typical development setup)
    pub fn default() -> Self {
        Self {
            connection_string: "postgres://localhost/tpch".to_string(),
            shared_buffers_mb: 128,
            work_mem_mb: 4,
            effective_cache_size_mb: 4096,
            random_page_cost: 4.0,
        }
    }

    /// High-memory configuration (production server)
    pub fn high_memory() -> Self {
        Self {
            connection_string: "postgres://localhost/tpch".to_string(),
            shared_buffers_mb: 2048,
            work_mem_mb: 64,
            effective_cache_size_mb: 16384,
            random_page_cost: 1.1,
        }
    }

    /// Low-memory configuration (resource-constrained)
    pub fn low_memory() -> Self {
        Self {
            connection_string: "postgres://localhost/tpch".to_string(),
            shared_buffers_mb: 32,
            work_mem_mb: 1,
            effective_cache_size_mb: 512,
            random_page_cost: 4.0,
        }
    }

    /// All-in-memory configuration (cache-resident workload)
    pub fn all_in_memory() -> Self {
        Self {
            connection_string: "postgres://localhost/tpch".to_string(),
            shared_buffers_mb: 4096,
            work_mem_mb: 128,
            effective_cache_size_mb: 32768,
            random_page_cost: 1.0,
        }
    }
}

/// Data size variant for testing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DataSize {
    /// Scale factor 0.01 (~10 MB, development)
    Tiny,
    /// Scale factor 0.1 (~100 MB, testing)
    Small,
    /// Scale factor 1.0 (~1 GB, standard benchmark)
    Medium,
    /// Scale factor 10.0 (~10 GB, large queries)
    Large,
}

impl DataSize {
    pub fn scale_factor(self) -> f64 {
        match self {
            DataSize::Tiny => 0.01,
            DataSize::Small => 0.1,
            DataSize::Medium => 1.0,
            DataSize::Large => 10.0,
        }
    }

    pub fn all() -> Vec<DataSize> {
        vec![
            DataSize::Tiny,
            DataSize::Small,
            DataSize::Medium,
            DataSize::Large,
        ]
    }
}

/// A single training sample.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingSample {
    /// SQL query text
    pub sql: String,
    /// Query features extracted by Ra
    pub features: QueryFeatures,
    /// Actual execution costs from Postgres
    pub actual_cost: ActualCost,
    /// Postgres configuration used
    pub pg_config: PostgresConfig,
    /// Data size variant
    pub data_size: DataSize,
    /// Timestamp when collected
    pub timestamp: String,
}

/// Postgres EXPLAIN ANALYZE output (JSON format).
#[derive(Debug, Deserialize)]
struct ExplainAnalyze {
    #[serde(rename = "Plan")]
    plan: PlanNode,
    #[serde(rename = "Planning Time")]
    planning_time: f64,
    #[serde(rename = "Execution Time")]
    execution_time: f64,
}

#[derive(Debug, Deserialize)]
struct PlanNode {
    #[serde(rename = "Node Type")]
    node_type: String,
    #[serde(rename = "Total Cost")]
    total_cost: Option<f64>,
    #[serde(rename = "Actual Total Time")]
    actual_total_time: Option<f64>,
    #[serde(rename = "Actual Rows")]
    actual_rows: Option<u64>,
    #[serde(rename = "Shared Hit Blocks")]
    shared_hit_blocks: Option<u64>,
    #[serde(rename = "Shared Read Blocks")]
    shared_read_blocks: Option<u64>,
    #[serde(rename = "Plans", default)]
    plans: Vec<PlanNode>,
}

/// Training data collector.
pub struct TrainingCollector {
    samples: Vec<TrainingSample>,
}

impl TrainingCollector {
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
        }
    }

    /// Collect training samples for all TPROC-H queries across configurations.
    pub fn collect_tproc_h_samples(
        &mut self,
        queries: &[(String, QueryFeatures)],
        configs: &[PostgresConfig],
        data_sizes: &[DataSize],
    ) -> Result<(), Box<dyn std::error::Error>> {
        for config in configs {
            for data_size in data_sizes {
                println!(
                    "Collecting samples: config={:?}, size={:?}",
                    config.shared_buffers_mb, data_size
                );

                for (sql, features) in queries {
                    match self.execute_and_measure(sql, features, config, *data_size) {
                        Ok(sample) => {
                            self.samples.push(sample);
                        }
                        Err(e) => {
                            eprintln!("Error executing query: {}", e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Execute a single query and collect metrics.
    fn execute_and_measure(
        &self,
        sql: &str,
        features: &QueryFeatures,
        config: &PostgresConfig,
        data_size: DataSize,
    ) -> Result<TrainingSample, Box<dyn std::error::Error>> {
        // This will be implemented with actual Postgres connection
        // For now, return a placeholder
        let actual_cost = ActualCost {
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
        };

        Ok(TrainingSample {
            sql: sql.to_string(),
            features: features.clone(),
            actual_cost,
            pg_config: config.clone(),
            data_size,
            timestamp: chrono::Utc::now().to_rfc3339(),
        })
    }

    /// Save collected samples to a file.
    pub fn save_to_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(&self.samples)?;
        std::fs::write(path, json)?;
        println!("Saved {} training samples to {}", self.samples.len(), path);
        Ok(())
    }

    /// Load samples from a file.
    pub fn load_from_file(path: &str) -> Result<Vec<TrainingSample>, Box<dyn std::error::Error>> {
        let json = std::fs::read_to_string(path)?;
        let samples: Vec<TrainingSample> = serde_json::from_str(&json)?;
        Ok(samples)
    }

    /// Get collected samples.
    pub fn samples(&self) -> &[TrainingSample] {
        &self.samples
    }
}

impl Default for TrainingCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_postgres_config_variants() {
        let default_cfg = PostgresConfig::default();
        assert_eq!(default_cfg.shared_buffers_mb, 128);

        let high_mem = PostgresConfig::high_memory();
        assert_eq!(high_mem.shared_buffers_mb, 2048);

        let low_mem = PostgresConfig::low_memory();
        assert_eq!(low_mem.shared_buffers_mb, 32);
    }

    #[test]
    fn test_data_size_scale_factors() {
        assert_eq!(DataSize::Tiny.scale_factor(), 0.01);
        assert_eq!(DataSize::Small.scale_factor(), 0.1);
        assert_eq!(DataSize::Medium.scale_factor(), 1.0);
        assert_eq!(DataSize::Large.scale_factor(), 10.0);
    }

    #[test]
    fn test_training_collector_creation() {
        let collector = TrainingCollector::new();
        assert_eq!(collector.samples().len(), 0);
    }
}
