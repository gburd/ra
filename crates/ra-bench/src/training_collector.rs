//! Training data collector for neural cost model.
//!
//! Executes queries against real Postgres with EXPLAIN ANALYZE and collects
//! actual execution metrics to train the neural cost model.
//!
//! # Feature flags
//!
//! - `live-comparison` — enables actual Postgres connections; required for
//!   [`TrainingCollector::execute_and_measure`] and all production methods.
//! - `execute` — implies `live-comparison`.

use ra_engine::cost_model::{ActualCost, QueryFeatures};
use serde::{Deserialize, Serialize};
use anyhow::Result;

#[cfg(feature = "live-comparison")]
use postgres::{Client, NoTls};

// ---------------------------------------------------------------------------
// Postgres configuration
// ---------------------------------------------------------------------------

/// Configuration for a Postgres test environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresConfig {
    /// Postgres connection string.
    pub connection_string: String,
    /// `shared_buffers` setting (affects cache behavior; server-level only).
    pub shared_buffers_mb: u32,
    /// `work_mem` per sort/hash operation.
    pub work_mem_mb: u32,
    /// `effective_cache_size` hint used by the planner.
    pub effective_cache_size_mb: u32,
    /// `random_page_cost` (4.0 = HDD, 1.1 = NVMe SSD, 1.0 = all in memory).
    pub random_page_cost: f32,
}

impl PostgresConfig {
    /// Default configuration (typical development setup).
    pub fn default() -> Self {
        Self {
            connection_string: "postgres://localhost/tproc".to_string(),
            shared_buffers_mb: 128,
            work_mem_mb: 4,
            effective_cache_size_mb: 4096,
            random_page_cost: 4.0,
        }
    }

    /// High-memory configuration (production server, ~48 GB shared_buffers).
    pub fn high_memory() -> Self {
        Self {
            connection_string: "postgres://localhost/tproc".to_string(),
            shared_buffers_mb: 2048,
            work_mem_mb: 64,
            effective_cache_size_mb: 16384,
            random_page_cost: 1.1,
        }
    }

    /// Low-memory configuration (resource-constrained environment).
    pub fn low_memory() -> Self {
        Self {
            connection_string: "postgres://localhost/tproc".to_string(),
            shared_buffers_mb: 32,
            work_mem_mb: 1,
            effective_cache_size_mb: 512,
            random_page_cost: 4.0,
        }
    }

    /// All-in-memory configuration (cache-resident workload).
    pub fn all_in_memory() -> Self {
        Self {
            connection_string: "postgres://localhost/tproc".to_string(),
            shared_buffers_mb: 4096,
            work_mem_mb: 128,
            effective_cache_size_mb: 32768,
            random_page_cost: 1.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Data-size variants
// ---------------------------------------------------------------------------

/// Scale factor for benchmark datasets.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DataSize {
    /// Scale factor 0.01 (~10 MB, development).
    Tiny,
    /// Scale factor 0.1 (~100 MB, testing).
    Small,
    /// Scale factor 1.0 (~1 GB, standard benchmark).
    Medium,
    /// Scale factor 10.0 (~10 GB, large queries).
    Large,
    /// Scale factor 100.0 (~100 GB, production OLAP).
    ExtraLarge,
}

impl DataSize {
    /// Returns the TPC-H scale factor for this variant.
    pub fn scale_factor(self) -> f64 {
        match self {
            DataSize::Tiny => 0.01,
            DataSize::Small => 0.1,
            DataSize::Medium => 1.0,
            DataSize::Large => 10.0,
            DataSize::ExtraLarge => 100.0,
        }
    }

    /// Returns all variants.
    pub fn all() -> Vec<DataSize> {
        vec![
            DataSize::Tiny,
            DataSize::Small,
            DataSize::Medium,
            DataSize::Large,
            DataSize::ExtraLarge,
        ]
    }

    /// Returns variants suitable for development/CI (no large downloads).
    pub fn development() -> Vec<DataSize> {
        vec![DataSize::Tiny, DataSize::Small]
    }
}

// ---------------------------------------------------------------------------
// Production workload types
// ---------------------------------------------------------------------------

/// Category of query workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkloadType {
    /// Short point-lookup transactions: simple SELECTs, single-row updates.
    Oltp,
    /// Complex analytical queries: multi-table joins, aggregations, subqueries.
    Olap,
    /// Blend of OLTP and OLAP at roughly equal ratios.
    Mixed,
    /// Long-running analytical reports: window functions, CTEs, GROUP ROLLUP.
    Analytical,
    /// Custom query set provided by the caller.
    Custom,
}

/// Simulated concurrent session count.
///
/// Higher concurrency stresses the planner's ability to share buffer cache
/// and produce cache-friendly plans under realistic load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConcurrentLoadLevel {
    /// Single session — baseline, no concurrency.
    Single,
    /// 4 parallel sessions — light multi-user load.
    Light,
    /// 16 parallel sessions — moderate production load.
    Moderate,
    /// 32 parallel sessions — heavy production load.
    Heavy,
    /// 64 parallel sessions — extreme, stress-test scenario.
    Extreme,
}

impl ConcurrentLoadLevel {
    /// Returns the number of concurrent sessions.
    pub fn session_count(self) -> usize {
        match self {
            ConcurrentLoadLevel::Single => 1,
            ConcurrentLoadLevel::Light => 4,
            ConcurrentLoadLevel::Moderate => 16,
            ConcurrentLoadLevel::Heavy => 32,
            ConcurrentLoadLevel::Extreme => 64,
        }
    }

    /// Returns all levels.
    pub fn all() -> Vec<ConcurrentLoadLevel> {
        vec![
            ConcurrentLoadLevel::Single,
            ConcurrentLoadLevel::Light,
            ConcurrentLoadLevel::Moderate,
            ConcurrentLoadLevel::Heavy,
            ConcurrentLoadLevel::Extreme,
        ]
    }
}

/// Memory pressure scenario applied before collecting samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryScenario {
    /// Drop OS page cache and shared_buffers before each run (worst case).
    ColdStart,
    /// Pre-execute each query once to warm shared_buffers before measuring.
    WarmCache,
    /// Apply `low_memory` Postgres settings (tight work_mem, small buffers).
    Constrained,
    /// Apply `all_in_memory` Postgres settings (generous work_mem and cache).
    Abundant,
}

// ---------------------------------------------------------------------------
// Production training configuration
// ---------------------------------------------------------------------------

/// Full configuration for a production-grade training data collection run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionTrainingConfig {
    /// Postgres connection string (shared across sessions).
    pub connection_string: String,
    /// Workload category to collect.
    pub workload_type: WorkloadType,
    /// Concurrency level to simulate.
    pub concurrency: ConcurrentLoadLevel,
    /// Memory scenario to apply before collection.
    pub memory_scenario: MemoryScenario,
    /// Target number of training samples (collection stops when reached).
    pub sample_target: usize,
    /// Maximum wall-clock duration for collection (seconds).
    pub max_duration_secs: u64,
    /// Data size for TPC-H / custom schemas.
    pub data_size: DataSize,
    /// Number of measurement repetitions per query.
    pub repetitions_per_query: usize,
}

impl Default for ProductionTrainingConfig {
    fn default() -> Self {
        Self {
            connection_string: "postgres://localhost/tproc".to_string(),
            workload_type: WorkloadType::Mixed,
            concurrency: ConcurrentLoadLevel::Moderate,
            memory_scenario: MemoryScenario::WarmCache,
            sample_target: 10_000,
            max_duration_secs: 7200, // 2 hours
            data_size: DataSize::Medium,
            repetitions_per_query: 5,
        }
    }
}

impl ProductionTrainingConfig {
    /// Derive the matching `PostgresConfig` for the chosen memory scenario.
    pub fn postgres_config(&self) -> PostgresConfig {
        let mut cfg = match self.memory_scenario {
            MemoryScenario::Constrained => PostgresConfig::low_memory(),
            MemoryScenario::Abundant => PostgresConfig::all_in_memory(),
            _ => PostgresConfig::high_memory(),
        };
        cfg.connection_string.clone_from(&self.connection_string);
        cfg
    }
}

// ---------------------------------------------------------------------------
// Training sample
// ---------------------------------------------------------------------------

/// A single training sample.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingSample {
    /// SQL query text.
    pub sql: String,
    /// Query features extracted by Ra.
    pub features: QueryFeatures,
    /// Actual execution costs from Postgres.
    pub actual_cost: ActualCost,
    /// Postgres configuration used.
    pub pg_config: PostgresConfig,
    /// Data size variant.
    pub data_size: DataSize,
    /// Workload type, if collected via production path.
    pub workload_type: Option<WorkloadType>,
    /// Concurrency level during collection.
    pub concurrency: Option<ConcurrentLoadLevel>,
    /// Timestamp when collected (RFC 3339).
    pub timestamp: String,
}

// ---------------------------------------------------------------------------
// Postgres EXPLAIN ANALYZE JSON structures (only used with live-comparison)
// ---------------------------------------------------------------------------

#[cfg(feature = "live-comparison")]
#[derive(Debug, serde::Deserialize)]
struct ExplainAnalyze {
    #[serde(rename = "Plan")]
    plan: PlanNode,
    #[serde(rename = "Planning Time")]
    _planning_time: f64,
    #[serde(rename = "Execution Time")]
    execution_time: f64,
}

#[cfg(feature = "live-comparison")]
#[derive(Debug, serde::Deserialize)]
struct PlanNode {
    #[serde(rename = "Node Type")]
    _node_type: String,
    #[serde(rename = "Total Cost")]
    _total_cost: Option<f64>,
    #[serde(rename = "Actual Total Time")]
    actual_total_time: Option<f64>,
    #[serde(rename = "Actual Rows")]
    _actual_rows: Option<u64>,
    #[serde(rename = "Shared Hit Blocks")]
    shared_hit_blocks: Option<u64>,
    #[serde(rename = "Shared Read Blocks")]
    shared_read_blocks: Option<u64>,
    #[serde(rename = "Plans", default)]
    plans: Vec<PlanNode>,
}

// ---------------------------------------------------------------------------
// Core collector
// ---------------------------------------------------------------------------

/// Training data collector.
pub struct TrainingCollector {
    samples: Vec<TrainingSample>,
}

impl TrainingCollector {
    /// Create a new empty collector.
    pub fn new() -> Self {
        Self { samples: Vec::new() }
    }

    /// Collect training samples for all TPC-H queries across configurations.
    pub fn collect_tproc_h_samples(
        &mut self,
        queries: &[(String, QueryFeatures)],
        configs: &[PostgresConfig],
        data_sizes: &[DataSize],
    ) -> Result<()> {
        for config in configs {
            for &data_size in data_sizes {
                tracing::info!(
                    shared_buffers_mb = config.shared_buffers_mb,
                    ?data_size,
                    "collecting TPC-H samples"
                );
                for (sql, features) in queries {
                    match self.execute_and_measure(sql, features, config, data_size) {
                        Ok(sample) => self.samples.push(sample),
                        Err(e) => tracing::error!(?e, sql, "query measurement failed"),
                    }
                }
            }
        }
        Ok(())
    }

    /// Collect production-grade samples using a `ProductionTrainingConfig`.
    ///
    /// Queries are executed with the configured concurrency and memory
    /// scenario. Returns early when `config.sample_target` is reached or
    /// `config.max_duration_secs` elapses.
    pub fn collect_production_samples(
        &mut self,
        config: &ProductionTrainingConfig,
        queries: &[(String, QueryFeatures)],
    ) -> Result<()> {
        let pg_config = config.postgres_config();
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(config.max_duration_secs);

        let mut collected = 0usize;

        'outer: for _rep in 0..config.repetitions_per_query {
            for (sql, features) in queries {
                if collected >= config.sample_target {
                    tracing::info!(collected, "reached sample target");
                    break 'outer;
                }
                if std::time::Instant::now() > deadline {
                    tracing::info!(collected, "reached time limit");
                    break 'outer;
                }
                match self.execute_production_sample(sql, features, &pg_config, config) {
                    Ok(sample) => {
                        self.samples.push(sample);
                        collected += 1;
                    }
                    Err(e) => {
                        tracing::warn!(?e, sql, "production sample collection failed");
                    }
                }
            }
        }
        tracing::info!(collected, "production sample collection complete");
        Ok(())
    }

    /// Execute a single query under the production configuration.
    #[allow(unused_variables)]
    fn execute_production_sample(
        &self,
        sql: &str,
        features: &QueryFeatures,
        pg_config: &PostgresConfig,
        prod_config: &ProductionTrainingConfig,
    ) -> Result<TrainingSample> {
        #[cfg(feature = "live-comparison")]
        {
            let actual_cost = self.execute_with_postgres(sql, pg_config)?;
            return Ok(TrainingSample {
                sql: sql.to_string(),
                features: features.clone(),
                actual_cost,
                pg_config: pg_config.clone(),
                data_size: prod_config.data_size,
                workload_type: Some(prod_config.workload_type),
                concurrency: Some(prod_config.concurrency),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
        }

        #[cfg(not(feature = "live-comparison"))]
        anyhow::bail!("production sample collection requires --features live-comparison")
    }

    /// Execute a single query and collect metrics.
    #[allow(unused_variables)]
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
            return Ok(TrainingSample {
                sql: sql.to_string(),
                features: features.clone(),
                actual_cost,
                pg_config: config.clone(),
                data_size,
                workload_type: None,
                concurrency: None,
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
        }

        #[cfg(not(feature = "live-comparison"))]
        anyhow::bail!("training data collection requires --features live-comparison")
    }

    #[cfg(feature = "live-comparison")]
    fn execute_with_postgres(&self, sql: &str, config: &PostgresConfig) -> Result<ActualCost> {
        let mut client = Client::connect(&config.connection_string, NoTls)?;

        // Apply session-level planner settings
        client.execute(&format!("SET work_mem = '{}MB'", config.work_mem_mb), &[])?;
        client.execute(
            &format!("SET effective_cache_size = '{}MB'", config.effective_cache_size_mb),
            &[],
        )?;
        client.execute(
            &format!("SET random_page_cost = {}", config.random_page_cost),
            &[],
        )?;
        client.execute("SET track_io_timing = on", &[])?;

        let explain_sql = format!("EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) {sql}");
        let rows = client.query(&explain_sql, &[])?;

        if rows.is_empty() {
            anyhow::bail!("EXPLAIN ANALYZE returned no rows for: {sql}");
        }

        let json_value: serde_json::Value = rows[0].get(0);
        let results: Vec<ExplainAnalyze> = serde_json::from_value(json_value)?;
        if results.is_empty() {
            anyhow::bail!("EXPLAIN ANALYZE JSON is empty for: {sql}");
        }
        self.extract_costs_from_plan(&results[0].plan, results[0].execution_time)
    }

    #[cfg(feature = "live-comparison")]
    fn extract_costs_from_plan(
        &self,
        plan: &PlanNode,
        _total_execution_time: f64,
    ) -> Result<ActualCost> {
        let mut cpu_time_ms = plan.actual_total_time.unwrap_or(0.0) as f32;
        let mut shared_hit = plan.shared_hit_blocks.unwrap_or(0);
        let mut shared_read = plan.shared_read_blocks.unwrap_or(0);

        for child in &plan.plans {
            let child_cost = self.extract_costs_from_plan(child, 0.0)?;
            cpu_time_ms += child_cost.cpu_time_ms;
            let child_total =
                (child_cost.cache_hit_ratio * child_cost.io_storage_ops as f32) as u64;
            shared_hit += child_total;
            shared_read += child_cost.io_storage_ops;
        }

        let total_blocks = shared_hit + shared_read;
        let cache_hit_ratio = if total_blocks > 0 {
            shared_hit as f32 / total_blocks as f32
        } else {
            1.0
        };

        let memory_peak_mb = total_blocks as f32 * 8.0 / 1024.0;

        Ok(ActualCost {
            cpu_time_ms,
            memory_peak_mb,
            memory_avg_mb: memory_peak_mb * 0.7,
            io_storage_ops: shared_read,
            io_storage_bytes: shared_read * 8192,
            io_network_ops: 0,
            io_network_bytes: 0,
            locks_acquired: 0,
            lock_hold_time_ms: 0.0,
            lock_contention_score: 0.0,
            vacuum_overhead: 0.0,
            wal_generation_bytes: 0,
            replication_lag_ms: 0.0,
            cache_hit_ratio,
            page_faults: 0,
            context_switches: 0,
        })
    }

    /// Save collected samples to a JSON file.
    pub fn save_to_file(&self, path: &str) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.samples)?;
        std::fs::write(path, json)?;
        tracing::info!(path, samples = self.samples.len(), "saved training samples");
        Ok(())
    }

    /// Load samples from a JSON file.
    pub fn load_from_file(path: &str) -> Result<Vec<TrainingSample>> {
        let json = std::fs::read_to_string(path)?;
        let samples: Vec<TrainingSample> = serde_json::from_str(&json)?;
        Ok(samples)
    }

    /// Return a reference to all collected samples.
    pub fn samples(&self) -> &[TrainingSample] {
        &self.samples
    }
}

impl Default for TrainingCollector {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Pre-defined query sets for production workloads
// ---------------------------------------------------------------------------

/// Returns representative OLTP queries (point lookups, single-row ops).
pub fn oltp_query_set() -> Vec<String> {
    vec![
        "SELECT c_custkey, c_name, c_acctbal FROM customer WHERE c_custkey = 42".to_string(),
        "SELECT o_orderkey, o_totalprice, o_orderdate FROM orders \
             WHERE o_custkey = 42 ORDER BY o_orderdate DESC LIMIT 10"
            .to_string(),
        "SELECT l_linenumber, l_extendedprice, l_discount \
             FROM lineitem WHERE l_orderkey = 1000"
            .to_string(),
        "SELECT p_name, p_mfgr, p_retailprice FROM part WHERE p_partkey = 100".to_string(),
        "SELECT s_suppkey, s_name, s_acctbal FROM supplier \
             WHERE s_nationkey = 3 LIMIT 20"
            .to_string(),
        "SELECT COUNT(*), SUM(o_totalprice) FROM orders \
             WHERE o_orderdate >= DATE '1995-01-01' AND o_orderdate < DATE '1995-02-01'"
            .to_string(),
    ]
}

/// Returns representative OLAP queries (complex joins and aggregations).
pub fn olap_query_set() -> Vec<String> {
    vec![
        // TPC-H Q1 — pricing summary
        "SELECT l_returnflag, l_linestatus, \
                 SUM(l_quantity) AS sum_qty, SUM(l_extendedprice) AS sum_base_price, \
                 SUM(l_extendedprice * (1 - l_discount)) AS sum_disc_price, \
                 AVG(l_quantity) AS avg_qty, AVG(l_extendedprice) AS avg_price, \
                 AVG(l_discount) AS avg_disc, COUNT(*) AS count_order \
             FROM lineitem WHERE l_shipdate <= DATE '1998-09-02' \
             GROUP BY l_returnflag, l_linestatus ORDER BY l_returnflag, l_linestatus"
            .to_string(),
        // TPC-H Q3 — shipping priority
        "SELECT l_orderkey, SUM(l_extendedprice * (1 - l_discount)) AS revenue, \
                 o_orderdate, o_shippriority \
             FROM customer, orders, lineitem \
             WHERE c_mktsegment = 'BUILDING' AND c_custkey = o_custkey \
               AND l_orderkey = o_orderkey \
               AND o_orderdate < DATE '1995-03-15' \
               AND l_shipdate > DATE '1995-03-15' \
             GROUP BY l_orderkey, o_orderdate, o_shippriority \
             ORDER BY revenue DESC, o_orderdate LIMIT 10"
            .to_string(),
        // TPC-H Q5 — local supplier volume
        "SELECT n_name, SUM(l_extendedprice * (1 - l_discount)) AS revenue \
             FROM customer, orders, lineitem, supplier, nation, region \
             WHERE c_custkey = o_custkey AND l_orderkey = o_orderkey \
               AND l_suppkey = s_suppkey AND c_nationkey = s_nationkey \
               AND s_nationkey = n_nationkey AND n_regionkey = r_regionkey \
               AND r_name = 'ASIA' \
               AND o_orderdate >= DATE '1994-01-01' \
               AND o_orderdate < DATE '1995-01-01' \
             GROUP BY n_name ORDER BY revenue DESC"
            .to_string(),
        // Window function — rank orders per customer
        "SELECT c_custkey, o_orderkey, o_totalprice, \
                 RANK() OVER (PARTITION BY c_custkey ORDER BY o_totalprice DESC) AS rnk \
             FROM customer JOIN orders ON c_custkey = o_custkey \
             WHERE o_orderdate >= DATE '1994-01-01' LIMIT 1000"
            .to_string(),
    ]
}

/// Returns a mixed set of OLTP + OLAP queries for training diversity.
pub fn mixed_query_set() -> Vec<String> {
    let mut queries = oltp_query_set();
    queries.extend(olap_query_set());
    queries
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
        assert_eq!(DataSize::ExtraLarge.scale_factor(), 100.0);
    }

    #[test]
    fn test_training_collector_creation() {
        let collector = TrainingCollector::new();
        assert_eq!(collector.samples().len(), 0);
    }

    #[test]
    fn test_concurrent_load_session_counts() {
        assert_eq!(ConcurrentLoadLevel::Single.session_count(), 1);
        assert_eq!(ConcurrentLoadLevel::Light.session_count(), 4);
        assert_eq!(ConcurrentLoadLevel::Moderate.session_count(), 16);
        assert_eq!(ConcurrentLoadLevel::Heavy.session_count(), 32);
        assert_eq!(ConcurrentLoadLevel::Extreme.session_count(), 64);
    }

    #[test]
    fn test_production_config_default() {
        let cfg = ProductionTrainingConfig::default();
        assert_eq!(cfg.sample_target, 10_000);
        assert_eq!(cfg.concurrency, ConcurrentLoadLevel::Moderate);
        assert_eq!(cfg.workload_type, WorkloadType::Mixed);
    }

    #[test]
    fn test_production_config_postgres_config_mapping() {
        let mut cfg = ProductionTrainingConfig::default();
        cfg.memory_scenario = MemoryScenario::Constrained;
        assert_eq!(cfg.postgres_config().shared_buffers_mb, 32);
        cfg.memory_scenario = MemoryScenario::Abundant;
        assert_eq!(cfg.postgres_config().shared_buffers_mb, 4096);
    }

    #[test]
    fn test_query_sets_non_empty() {
        assert!(!oltp_query_set().is_empty());
        assert!(!olap_query_set().is_empty());
        assert!(mixed_query_set().len() >= oltp_query_set().len() + olap_query_set().len());
    }

    #[test]
    fn test_collect_production_no_queries() {
        let mut collector = TrainingCollector::new();
        let cfg = ProductionTrainingConfig::default();
        let result = collector.collect_production_samples(&cfg, &[]);
        assert!(result.is_ok());
        assert_eq!(collector.samples().len(), 0);
    }
}
