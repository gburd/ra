//! Timeline-based fingerprint configuration system.
//!
//! This module provides a comprehensive timeline configuration format that extends
//! the existing `ra_stats::timeline` with hardware profiles, schema definitions,
//! facts, and test expectations.
//!
//! # Overview
//!
//! A timeline configuration captures complete system state at discrete points in time:
//! - **Hardware profiles**: CPU, memory, GPU capabilities
//! - **Schema snapshots**: Tables, columns, indexes, storage format
//! - **Statistics**: Row counts, NDV, histograms, correlations
//! - **Facts**: Feature flags, configurations, capabilities
//! - **Events**: Schema changes, statistics updates, hardware changes
//! - **Expectations**: Test assertions for deterministic testing
//!
//! # Example
//!
//! ```toml
//! [metadata]
//! name = "Index Addition Scenario"
//! description = "Plan changes when index is added mid-execution"
//! query = "SELECT * FROM orders WHERE customer_id = 42"
//! dialect = "postgresql"
//! duration_seconds = 3600
//!
//! [[hardware_profiles]]
//! name = "laptop"
//! cpu_cores = 4
//! total_memory = 16000000000
//! simd_width = 256
//!
//! [[snapshots]]
//! time_offset = 0
//! label = "Initial state - no index"
//! hardware_profile = "laptop"
//!
//!   [snapshots.schema]
//!     [[snapshots.schema.tables]]
//!     name = "orders"
//!     storage_format = "row_based"
//!
//!       [[snapshots.schema.tables.columns]]
//!       name = "customer_id"
//!       data_type = "integer"
//!
//!   [[snapshots.statistics.tables]]
//!   name = "orders"
//!   row_count = 1000000
//!
//!   [snapshots.facts]
//!   supports_hash_join = true
//!   parallel_workers = 4
//!
//! [[snapshots]]
//! time_offset = 1800
//! label = "After index creation"
//! hardware_profile = "laptop"
//!
//!   [snapshots.schema]
//!     [[snapshots.schema.tables]]
//!     name = "orders"
//!
//!     [[snapshots.schema.tables.indexes]]
//!     name = "idx_orders_customer"
//!     index_type = "btree"
//!     columns = ["customer_id"]
//!
//! [[expectations]]
//! snapshot_index = 0
//! expected_plan_pattern = ".*SeqScan.*"
//! expected_cost_range = [20000.0, 30000.0]
//!
//! [[expectations]]
//! snapshot_index = 1
//! expected_plan_pattern = ".*IndexScan.*idx_orders_customer.*"
//! expected_cost_range = [30.0, 100.0]
//! invalidation_trigger = "Index"
//! ```

use ra_core::facts::{DataType, HardwareProfile, IndexType, StorageFormat};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Errors from timeline configuration parsing and validation.
#[derive(Debug, Error)]
pub enum TimelineConfigError {
    /// TOML parsing failed.
    #[error("failed to parse timeline configuration: {0}")]
    ParseError(String),

    /// Snapshot index out of bounds in expectation.
    #[error("expectation references snapshot {index} but only {total} snapshots exist")]
    InvalidSnapshotIndex {
        /// Referenced snapshot index.
        index: usize,
        /// Total number of snapshots.
        total: usize,
    },

    /// Hardware profile reference not found.
    #[error("hardware profile '{profile}' not found")]
    ProfileNotFound {
        /// Missing profile name.
        profile: String,
    },

    /// Snapshot refers to non-existent base profile.
    #[error("snapshot {index} references base profile '{base}' which does not exist")]
    InvalidBaseProfile {
        /// Snapshot index.
        index: usize,
        /// Referenced base profile name.
        base: String,
    },

    /// Timeline has no snapshots.
    #[error("timeline must have at least one snapshot")]
    EmptyTimeline,

    /// Snapshot time offsets not in ascending order.
    #[error("snapshot offsets not in ascending order at index {0}")]
    UnsortedOffsets(usize),

    /// I/O error reading timeline file.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Invalid pattern in expectation.
    #[error("invalid regex pattern in expectation: {0}")]
    InvalidPattern(String),
}

/// Top-level timeline configuration document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineConfig {
    /// Timeline metadata.
    pub metadata: TimelineMetadata,

    /// Named hardware profiles (reusable across snapshots).
    #[serde(default)]
    pub hardware_profiles: Vec<HardwareProfileDef>,

    /// Ordered fingerprint snapshots.
    pub snapshots: Vec<FingerPrintSnapshot>,

    /// Timeline events (schema changes, statistics updates, etc.).
    #[serde(default)]
    pub events: Vec<TimelineEvent>,

    /// Test expectations for validation.
    #[serde(default)]
    pub expectations: Vec<TestExpectation>,
}

/// Timeline metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineMetadata {
    /// Human-readable name.
    pub name: String,

    /// Description of the scenario.
    pub description: String,

    /// Query being optimized (optional).
    #[serde(default)]
    pub query: Option<String>,

    /// SQL dialect (e.g., "postgresql", "mysql").
    #[serde(default)]
    pub dialect: Option<String>,

    /// Total simulated duration in seconds.
    #[serde(default)]
    pub duration_seconds: Option<u64>,

    /// Schema or benchmark name (e.g., "TPC-H", "IMDB").
    #[serde(default)]
    pub schema: Option<String>,

    /// Scale factor for benchmark.
    #[serde(default)]
    pub scale_factor: Option<f64>,
}

/// Named hardware profile definition (reusable).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HardwareProfileDef {
    /// Profile name (e.g., "laptop", "server", "cloud-xlarge").
    pub name: String,

    /// Number of CPU cores.
    pub cpu_cores: u32,

    /// Total memory in bytes.
    pub total_memory: u64,

    /// Available memory in bytes.
    #[serde(default)]
    pub available_memory: Option<u64>,

    /// SIMD width in bits (128=SSE, 256=AVX2, 512=AVX-512).
    #[serde(default = "default_simd_width")]
    pub simd_width: u32,

    /// Whether GPU is available.
    #[serde(default)]
    pub has_gpu: bool,

    /// GPU memory in bytes (if available).
    #[serde(default)]
    pub gpu_memory: Option<u64>,

    /// L1 cache size in bytes.
    #[serde(default = "default_l1_cache")]
    pub l1_cache_size: u64,

    /// L2 cache size in bytes.
    #[serde(default = "default_l2_cache")]
    pub l2_cache_size: u64,

    /// L3 cache size in bytes.
    #[serde(default = "default_l3_cache")]
    pub l3_cache_size: u64,
}

fn default_simd_width() -> u32 {
    256
}

fn default_l1_cache() -> u64 {
    32 * 1024
}

fn default_l2_cache() -> u64 {
    256 * 1024
}

fn default_l3_cache() -> u64 {
    8 * 1024 * 1024
}

impl HardwareProfileDef {
    /// Convert to `HardwareProfile` used by optimizer.
    #[must_use]
    pub fn to_hardware_profile(&self) -> HardwareProfile {
        HardwareProfile {
            cpu_cores: self.cpu_cores,
            available_memory: self.available_memory.unwrap_or(self.total_memory),
            total_memory: self.total_memory,
            simd_width: self.simd_width,
            has_gpu: self.has_gpu,
            gpu_memory: self.gpu_memory,
            l1_cache_size: self.l1_cache_size,
            l2_cache_size: self.l2_cache_size,
            l3_cache_size: self.l3_cache_size,
        }
    }
}

/// Complete fingerprint snapshot at a point in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FingerPrintSnapshot {
    /// Time offset in seconds from timeline start.
    pub time_offset: u64,

    /// Optional label for this snapshot.
    #[serde(default)]
    pub label: Option<String>,

    /// Hardware profile name (references `hardware_profiles`).
    pub hardware_profile: String,

    /// Schema snapshot (tables, columns, indexes).
    pub schema: SchemaSnapshot,

    /// Statistics snapshot.
    #[serde(default)]
    pub statistics: StatisticsSnapshot,

    /// Facts snapshot (feature flags, configurations).
    #[serde(default)]
    pub facts: FactsSnapshot,
}

/// Schema snapshot (tables, columns, indexes, constraints).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SchemaSnapshot {
    /// Table definitions.
    #[serde(default)]
    pub tables: Vec<TableDef>,
}

/// Table definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableDef {
    /// Table name.
    pub name: String,

    /// Storage format.
    #[serde(default)]
    pub storage_format: StorageFormatDef,

    /// Column definitions.
    #[serde(default)]
    pub columns: Vec<ColumnDef>,

    /// Index definitions.
    #[serde(default)]
    pub indexes: Vec<IndexDef>,

    /// Primary key columns.
    #[serde(default)]
    pub primary_key: Vec<String>,

    /// Foreign key constraints.
    #[serde(default)]
    pub foreign_keys: Vec<ForeignKeyDef>,
}

/// Column definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnDef {
    /// Column name.
    pub name: String,

    /// Data type.
    pub data_type: DataTypeDef,

    /// Whether column is nullable.
    #[serde(default = "default_nullable")]
    pub nullable: bool,
}

fn default_nullable() -> bool {
    true
}

/// Data type definition (serialization-friendly).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataTypeDef {
    /// Integer types.
    Integer,
    /// Floating point types.
    Float,
    /// String/text types.
    String,
    /// Boolean type.
    Boolean,
    /// Date/time types.
    Timestamp,
    /// Binary data.
    Binary,
    /// JSON/JSONB.
    Json,
    /// Array types.
    Array(Box<DataTypeDef>),
    /// Other/unknown type.
    Other(String),
}

impl From<DataTypeDef> for DataType {
    fn from(def: DataTypeDef) -> Self {
        match def {
            DataTypeDef::Integer => Self::Integer,
            DataTypeDef::Float => Self::Float,
            DataTypeDef::String => Self::String,
            DataTypeDef::Boolean => Self::Boolean,
            DataTypeDef::Timestamp => Self::Timestamp,
            DataTypeDef::Binary => Self::Binary,
            DataTypeDef::Json => Self::Json,
            DataTypeDef::Array(inner) => Self::Array(Box::new((*inner).into())),
            DataTypeDef::Other(name) => Self::Other(name),
        }
    }
}

/// Index definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexDef {
    /// Index name.
    pub name: String,

    /// Index type.
    #[serde(default)]
    pub index_type: IndexTypeDef,

    /// Indexed columns (key columns).
    pub columns: Vec<String>,

    /// Included (non-key) columns.
    #[serde(default)]
    pub included_columns: Vec<String>,

    /// Whether the index is unique.
    #[serde(default)]
    pub is_unique: bool,
}

/// Index type definition (serialization-friendly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum IndexTypeDef {
    /// B-tree index.
    #[default]
    Btree,
    /// Hash index.
    Hash,
    /// `GiST` (Generalized Search Tree).
    Gist,
    /// GIN (Generalized Inverted Index).
    Gin,
    /// SP-GiST (Space-Partitioned `GiST`).
    SpGist,
    /// BRIN (Block Range Index).
    Brin,
    /// RUM (GIN extension).
    Rum,
    /// Bitmap index.
    Bitmap,
    /// Unknown or unsupported.
    Unknown,
}

impl From<IndexTypeDef> for IndexType {
    fn from(def: IndexTypeDef) -> Self {
        match def {
            IndexTypeDef::Btree => Self::BTree,
            IndexTypeDef::Hash => Self::Hash,
            IndexTypeDef::Gist => Self::Gist,
            IndexTypeDef::Gin => Self::Gin,
            IndexTypeDef::SpGist => Self::SpGist,
            IndexTypeDef::Brin => Self::Brin,
            IndexTypeDef::Rum => Self::Rum,
            IndexTypeDef::Bitmap => Self::Bitmap,
            IndexTypeDef::Unknown => Self::Unknown,
        }
    }
}

/// Foreign key constraint definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForeignKeyDef {
    /// Columns in this table.
    pub columns: Vec<String>,

    /// Referenced table.
    pub referenced_table: String,

    /// Referenced columns.
    pub referenced_columns: Vec<String>,
}

/// Storage format definition (serialization-friendly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum StorageFormatDef {
    /// Row-based storage (heap tables).
    #[default]
    RowBased,
    /// Column-based storage.
    Columnar,
    /// Parquet files.
    Parquet,
    /// ORC files.
    Orc,
    /// Arrow IPC.
    ArrowIpc,
    /// Unknown or mixed format.
    Unknown,
}

impl From<StorageFormatDef> for StorageFormat {
    fn from(def: StorageFormatDef) -> Self {
        match def {
            StorageFormatDef::RowBased => Self::RowBased,
            StorageFormatDef::Columnar => Self::Columnar,
            StorageFormatDef::Parquet => Self::Parquet,
            StorageFormatDef::Orc => Self::Orc,
            StorageFormatDef::ArrowIpc => Self::ArrowIpc,
            StorageFormatDef::Unknown => Self::Unknown,
        }
    }
}

/// Statistics snapshot (table and column statistics).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StatisticsSnapshot {
    /// Per-table statistics.
    #[serde(default)]
    pub tables: Vec<TableStatsDef>,
}

/// Table statistics definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableStatsDef {
    /// Table name.
    pub name: String,

    /// Total row count.
    pub row_count: u64,

    /// Total page count.
    #[serde(default)]
    pub page_count: Option<u64>,

    /// Average row size in bytes.
    #[serde(default)]
    pub avg_row_size: Option<f64>,

    /// Table size in bytes.
    #[serde(default)]
    pub table_size_bytes: Option<u64>,

    /// Per-column statistics.
    #[serde(default)]
    pub columns: Vec<ColumnStatsDef>,
}

/// Column statistics definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnStatsDef {
    /// Column name.
    pub name: String,

    /// Number of distinct values.
    pub ndv: u64,

    /// NULL fraction (0.0 to 1.0).
    #[serde(default)]
    pub null_fraction: f64,

    /// Average width in bytes.
    #[serde(default = "default_avg_width")]
    pub avg_width: f64,

    /// Physical correlation (-1.0 to 1.0).
    #[serde(default)]
    pub correlation: Option<f64>,

    /// Minimum value (for display).
    #[serde(default)]
    pub min_value: Option<String>,

    /// Maximum value (for display).
    #[serde(default)]
    pub max_value: Option<String>,
}

fn default_avg_width() -> f64 {
    8.0
}

/// Facts snapshot (feature flags, configurations, capabilities).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FactsSnapshot {
    /// Whether hash join is supported.
    #[serde(default)]
    pub supports_hash_join: Option<bool>,

    /// Whether parallel scan is supported.
    #[serde(default)]
    pub supports_parallel_scan: Option<bool>,

    /// Number of parallel workers.
    #[serde(default)]
    pub parallel_workers: Option<u32>,

    /// Work memory in bytes.
    #[serde(default)]
    pub work_mem_bytes: Option<u64>,

    /// Additional custom facts (key-value pairs).
    #[serde(flatten)]
    pub custom: HashMap<String, toml::Value>,
}

/// Timeline event (schema change, statistics update, hardware change).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineEvent {
    /// Time offset in seconds.
    pub time_offset: u64,

    /// Event kind.
    pub kind: EventKind,

    /// Affected table (if applicable).
    #[serde(default)]
    pub table: Option<String>,

    /// Event description.
    #[serde(default)]
    pub description: Option<String>,
}

/// Kind of timeline event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Schema change (add/drop table, column, index, etc.).
    SchemaChange,
    /// Statistics update (ANALYZE).
    StatisticsUpdate,
    /// Hardware change (migration, upgrade).
    HardwareChange,
    /// Configuration change.
    ConfigChange,
    /// Data modification (bulk insert/update/delete).
    DataModification,
}

/// Test expectation for deterministic testing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestExpectation {
    /// Snapshot index (0-based).
    pub snapshot_index: usize,

    /// Expected plan pattern (regex).
    #[serde(default)]
    pub expected_plan_pattern: Option<String>,

    /// Expected cost range [min, max].
    #[serde(default)]
    pub expected_cost_range: Option<[f64; 2]>,

    /// Rules that must be applied.
    #[serde(default)]
    pub rules_applied_must_include: Vec<String>,

    /// Rules that must not be applied.
    #[serde(default)]
    pub rules_applied_must_not_include: Vec<String>,

    /// Expected invalidation trigger (e.g., "Index", "Statistics", "Schema").
    #[serde(default)]
    pub invalidation_trigger: Option<String>,

    /// Expected cardinality estimate (within tolerance).
    #[serde(default)]
    pub expected_cardinality: Option<f64>,

    /// Cardinality tolerance (default 0.2 = 20%).
    #[serde(default = "default_tolerance")]
    pub cardinality_tolerance: f64,
}

fn default_tolerance() -> f64 {
    0.2
}

impl TimelineConfig {
    /// Parse a timeline configuration from a TOML string.
    ///
    /// # Errors
    ///
    /// Returns `TimelineConfigError` if parsing fails or validation fails.
    pub fn from_toml(input: &str) -> Result<Self, TimelineConfigError> {
        let config: Self =
            toml::from_str(input).map_err(|e| TimelineConfigError::ParseError(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    /// Load timeline configuration from a file.
    ///
    /// # Errors
    ///
    /// Returns `TimelineConfigError` if file reading or parsing fails.
    pub fn from_file(path: &std::path::Path) -> Result<Self, TimelineConfigError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml(&content)
    }

    /// Validate internal consistency.
    ///
    /// # Errors
    ///
    /// Returns `TimelineConfigError` if:
    /// - Timeline has no snapshots
    /// - Snapshot time offsets are not in ascending order
    /// - Hardware profile references are invalid
    /// - Expectation references non-existent snapshot
    /// - Regex patterns in expectations are invalid
    pub fn validate(&self) -> Result<(), TimelineConfigError> {
        // Check snapshots exist
        if self.snapshots.is_empty() {
            return Err(TimelineConfigError::EmptyTimeline);
        }

        // Check time offsets are sorted
        for i in 1..self.snapshots.len() {
            if self.snapshots[i].time_offset <= self.snapshots[i - 1].time_offset {
                return Err(TimelineConfigError::UnsortedOffsets(i));
            }
        }

        // Build profile name set for validation
        let profile_names: std::collections::HashSet<_> =
            self.hardware_profiles.iter().map(|p| &p.name).collect();

        // Validate hardware profile references
        for snapshot in &self.snapshots {
            if !profile_names.contains(&snapshot.hardware_profile) {
                return Err(TimelineConfigError::ProfileNotFound {
                    profile: snapshot.hardware_profile.clone(),
                });
            }
        }

        // Validate expectations
        for expectation in &self.expectations {
            if expectation.snapshot_index >= self.snapshots.len() {
                return Err(TimelineConfigError::InvalidSnapshotIndex {
                    index: expectation.snapshot_index,
                    total: self.snapshots.len(),
                });
            }

            // Validate regex patterns
            if let Some(pattern) = &expectation.expected_plan_pattern {
                regex::Regex::new(pattern)
                    .map_err(|e| TimelineConfigError::InvalidPattern(e.to_string()))?;
            }
        }

        Ok(())
    }

    /// Get hardware profile by name.
    #[must_use]
    pub fn get_hardware_profile(&self, name: &str) -> Option<&HardwareProfileDef> {
        self.hardware_profiles.iter().find(|p| p.name == name)
    }

    /// Total number of snapshots.
    #[must_use]
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    /// Total number of events.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Total number of expectations.
    #[must_use]
    pub fn expectation_count(&self) -> usize {
        self.expectations.len()
    }

    /// Duration from first to last snapshot.
    #[must_use]
    pub fn time_span(&self) -> u64 {
        if self.snapshots.len() < 2 {
            return 0;
        }
        self.snapshots.last().map_or(0, |l| l.time_offset)
            - self.snapshots.first().map_or(0, |f| f.time_offset)
    }

    /// All table names mentioned in any snapshot.
    #[must_use]
    pub fn table_names(&self) -> Vec<String> {
        let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for snapshot in &self.snapshots {
            for table in &snapshot.schema.tables {
                names.insert(table.name.clone());
            }
        }
        let mut result: Vec<_> = names.into_iter().collect();
        result.sort();
        result
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_timeline() {
        let toml = r#"
            [metadata]
            name = "Minimal Timeline"
            description = "Just a test"

            [[hardware_profiles]]
            name = "default"
            cpu_cores = 4
            total_memory = 8000000000

            [[snapshots]]
            time_offset = 0
            hardware_profile = "default"

            [snapshots.schema]
            tables = []

            [snapshots.statistics]
            tables = []

            [snapshots.facts]
        "#;

        let config = TimelineConfig::from_toml(toml).unwrap();
        assert_eq!(config.metadata.name, "Minimal Timeline");
        assert_eq!(config.snapshot_count(), 1);
        assert_eq!(config.hardware_profiles.len(), 1);
    }

    #[test]
    fn parse_complete_timeline() {
        let toml = r#"
            [metadata]
            name = "Index Addition"
            description = "Plan changes when index is added"
            query = "SELECT * FROM orders WHERE customer_id = 42"
            dialect = "postgresql"
            duration_seconds = 3600

            [[hardware_profiles]]
            name = "laptop"
            cpu_cores = 4
            total_memory = 16000000000
            simd_width = 256
            has_gpu = false

            [[snapshots]]
            time_offset = 0
            label = "Initial state"
            hardware_profile = "laptop"

            [snapshots.schema]
            [[snapshots.schema.tables]]
            name = "orders"
            storage_format = "row_based"

            [[snapshots.schema.tables.columns]]
            name = "customer_id"
            data_type = "integer"

            [[snapshots.statistics.tables]]
            name = "orders"
            row_count = 1000000

            [[snapshots.statistics.tables.columns]]
            name = "customer_id"
            ndv = 50000
            null_fraction = 0.0

            [snapshots.facts]
            supports_hash_join = true
            parallel_workers = 4

            [[snapshots]]
            time_offset = 1800
            label = "After index"
            hardware_profile = "laptop"

            [snapshots.schema]
            [[snapshots.schema.tables]]
            name = "orders"

            [[snapshots.schema.tables.indexes]]
            name = "idx_orders_customer"
            index_type = "btree"
            columns = ["customer_id"]
            is_unique = false

            [[expectations]]
            snapshot_index = 0
            expected_plan_pattern = ".*SeqScan.*"
            expected_cost_range = [20000.0, 30000.0]

            [[expectations]]
            snapshot_index = 1
            expected_plan_pattern = ".*IndexScan.*"
            expected_cost_range = [30.0, 100.0]
        "#;

        let config = TimelineConfig::from_toml(toml).unwrap();
        assert_eq!(config.snapshot_count(), 2);
        assert_eq!(config.expectation_count(), 2);
        assert_eq!(config.time_span(), 1800);

        // Check first snapshot
        let snap0 = &config.snapshots[0];
        assert_eq!(snap0.label.as_deref(), Some("Initial state"));
        assert_eq!(snap0.schema.tables.len(), 1);
        assert_eq!(snap0.schema.tables[0].indexes.len(), 0);

        // Check second snapshot
        let snap1 = &config.snapshots[1];
        assert_eq!(snap1.schema.tables[0].indexes.len(), 1);
        assert_eq!(
            snap1.schema.tables[0].indexes[0].name,
            "idx_orders_customer"
        );
    }

    #[test]
    fn validate_missing_profile() {
        let toml = r#"
            [metadata]
            name = "Bad"
            description = "Missing profile"

            [[hardware_profiles]]
            name = "laptop"
            cpu_cores = 4
            total_memory = 8000000000

            [[snapshots]]
            time_offset = 0
            hardware_profile = "server"

            [snapshots.schema]
            [snapshots.statistics]
            [snapshots.facts]
        "#;

        let err = TimelineConfig::from_toml(toml).unwrap_err();
        assert!(matches!(err, TimelineConfigError::ProfileNotFound { .. }));
    }

    #[test]
    fn validate_invalid_snapshot_index() {
        let toml = r#"
            [metadata]
            name = "Bad"
            description = "Invalid expectation"

            [[hardware_profiles]]
            name = "laptop"
            cpu_cores = 4
            total_memory = 8000000000

            [[snapshots]]
            time_offset = 0
            hardware_profile = "laptop"

            [snapshots.schema]
            [snapshots.statistics]
            [snapshots.facts]

            [[expectations]]
            snapshot_index = 5
        "#;

        let err = TimelineConfig::from_toml(toml).unwrap_err();
        assert!(matches!(
            err,
            TimelineConfigError::InvalidSnapshotIndex { .. }
        ));
    }

    #[test]
    fn validate_unsorted_offsets() {
        let toml = r#"
            [metadata]
            name = "Bad"
            description = "Unsorted offsets"

            [[hardware_profiles]]
            name = "laptop"
            cpu_cores = 4
            total_memory = 8000000000

            [[snapshots]]
            time_offset = 1000
            hardware_profile = "laptop"

            [snapshots.schema]
            [snapshots.statistics]
            [snapshots.facts]

            [[snapshots]]
            time_offset = 500
            hardware_profile = "laptop"

            [snapshots.schema]
            [snapshots.statistics]
            [snapshots.facts]
        "#;

        let err = TimelineConfig::from_toml(toml).unwrap_err();
        assert!(matches!(err, TimelineConfigError::UnsortedOffsets(_)));
    }

    #[test]
    fn hardware_profile_conversion() {
        let profile_def = HardwareProfileDef {
            name: "test".to_string(),
            cpu_cores: 8,
            total_memory: 16_000_000_000,
            available_memory: Some(12_000_000_000),
            simd_width: 512,
            has_gpu: true,
            gpu_memory: Some(8_000_000_000),
            l1_cache_size: 32768,
            l2_cache_size: 262_144,
            l3_cache_size: 8_388_608,
        };

        let hardware = profile_def.to_hardware_profile();
        assert_eq!(hardware.cpu_cores, 8);
        assert_eq!(hardware.available_memory, 12_000_000_000);
        assert_eq!(hardware.simd_width, 512);
        assert!(hardware.has_gpu);
    }

    #[test]
    fn data_type_conversion() {
        let int_type: DataType = DataTypeDef::Integer.into();
        assert_eq!(int_type, DataType::Integer);

        let array_type: DataType = DataTypeDef::Array(Box::new(DataTypeDef::String)).into();
        assert!(matches!(array_type, DataType::Array(_)));
    }
}
