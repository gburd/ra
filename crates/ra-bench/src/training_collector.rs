//! Training data collector for neural cost model.
//!
//! Executes queries against real Postgres with EXPLAIN ANALYZE and collects
//! actual execution metrics to train the neural cost model.

use ra_engine::cost_model::{ActualCost, QueryFeatures};
use serde::{Deserialize, Serialize};
use anyhow::{Context, Result};

#[cfg(feature = "live-comparison")]
use postgres::{Client, NoTls};

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
    ) -> Result<()> {
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
    ) -> Result<TrainingSample> {
        #[cfg(feature = "live-comparison")]
        {
            let actual_cost = self.execute_with_postgres(sql, config)?;

            Ok(TrainingSample {
                sql: sql.to_string(),
                features: features.clone(),
                actual_cost,
                pg_config: config.clone(),
                data_size,
                timestamp: chrono::Utc::now().to_rfc3339(),
            })
        }

        #[cfg(not(feature = "live-comparison"))]
        {
            anyhow::bail!("Training data collection requires --features live-comparison")
        }
    }

    #[cfg(feature = "live-comparison")]
    fn execute_with_postgres(
        &self,
        sql: &str,
        config: &PostgresConfig,
    ) -> Result<ActualCost> {
        // Connect to Postgres
        let mut client = Client::connect(&config.connection_string, NoTls)?;

        // Configure session-level Postgres settings
        // Note: shared_buffers cannot be set per-session, only at server start
        client.execute(
            &format!("SET work_mem = '{}MB'", config.work_mem_mb),
            &[],
        )?;
        client.execute(
            &format!("SET effective_cache_size = '{}MB'", config.effective_cache_size_mb),
            &[],
        )?;
        client.execute(
            &format!("SET random_page_cost = {}", config.random_page_cost),
            &[],
        )?;

        // Enable timing and buffer statistics
        client.execute("SET track_io_timing = on", &[])?;

        // Run EXPLAIN ANALYZE with BUFFERS and JSON output
        let explain_query = format!(
            "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) {}",
            sql
        );

        let rows = client.query(&explain_query, &[])?;

        if rows.is_empty() {
            anyhow::bail!("EXPLAIN ANALYZE returned no rows");
        }

        // Parse JSON output
        let json_value: serde_json::Value = rows[0].get(0);
        let explain_results: Vec<ExplainAnalyze> = serde_json::from_value(json_value)?;

        if explain_results.is_empty() {
            anyhow::bail!("EXPLAIN ANALYZE JSON is empty");
        }

        let result = &explain_results[0];

        // Extract actual costs from the plan tree
        let actual_cost = self.extract_costs_from_plan(&result.plan, result.execution_time)?;

        Ok(actual_cost)
    }

    #[cfg(feature = "live-comparison")]
    fn extract_costs_from_plan(
        &self,
        plan: &PlanNode,
        total_execution_time: f64,
    ) -> Result<ActualCost> {
        // Recursively sum costs from all plan nodes
        let mut cpu_time_ms = plan.actual_total_time.unwrap_or(0.0) as f32;
        let mut shared_hit = plan.shared_hit_blocks.unwrap_or(0);
        let mut shared_read = plan.shared_read_blocks.unwrap_or(0);

        // Traverse child nodes
        for child in &plan.plans {
            let child_cost = self.extract_costs_from_plan(child, total_execution_time)?;
            cpu_time_ms += child_cost.cpu_time_ms;
            shared_hit += child_cost.cache_hit_ratio as u64 * child_cost.io_storage_ops;
            shared_read += child_cost.io_storage_ops;
        }

        // Calculate cache hit ratio (Postgres block size is 8KB)
        let total_blocks = shared_hit + shared_read;
        let cache_hit_ratio = if total_blocks > 0 {
            shared_hit as f32 / total_blocks as f32
        } else {
            1.0
        };

        // Estimate memory usage (very rough approximation)
        // In reality, would need to query pg_stat_statements or track work_mem usage
        let memory_peak_mb = (total_blocks as f64 * 8.0 / 1024.0) as f32;

        Ok(ActualCost {
            cpu_time_ms,
            memory_peak_mb,
            memory_avg_mb: memory_peak_mb * 0.7, // Rough estimate
            io_storage_ops: shared_read,
            io_storage_bytes: shared_read * 8192, // 8KB per block
            io_network_ops: 0, // Local execution
            io_network_bytes: 0,
            locks_acquired: 0, // Not exposed in EXPLAIN ANALYZE
            lock_hold_time_ms: 0.0,
            lock_contention_score: 0.0,
            vacuum_overhead: 0.0,
            wal_generation_bytes: 0, // Would need pg_stat_statements
            replication_lag_ms: 0.0,
            cache_hit_ratio,
            page_faults: 0, // Not exposed
            context_switches: 0, // Not exposed
        })
    }

    /// Save collected samples to a file.
    pub fn save_to_file(&self, path: &str) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.samples)?;
        std::fs::write(path, json)?;
        println!("Saved {} training samples to {}", self.samples.len(), path);
        Ok(())
    }

    /// Load samples from a file.
    pub fn load_from_file(path: &str) -> Result<Vec<TrainingSample>> {
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
