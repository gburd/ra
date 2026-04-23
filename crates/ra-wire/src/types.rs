//! Wire-specific types that augment ra-core types for protocol use.

use ra_core::statistics::Statistics;
use serde::{Deserialize, Serialize};

// ── Execution Hints ─────────────────────────────────────────

/// Result format for row data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ResultFormat {
    /// PostgreSQL binary encoding (DataRow format).
    #[default]
    PgBinary,
    /// Arrow IPC RecordBatch (OLAP).
    ArrowIpc,
    /// PostgreSQL text encoding.
    PgText,
}

/// Execution hints sent alongside a query.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExecutionHints {
    /// Maximum rows to return (0 = unlimited).
    pub row_limit: u64,
    /// Desired result encoding format.
    pub result_format: ResultFormat,
    /// Query timeout in milliseconds (0 = server default).
    pub timeout_ms: u32,
    /// Whether to collect EXPLAIN ANALYZE runtime stats.
    pub collect_runtime_stats: bool,
}

// ── Result Schema ───────────────────────────────────────────

/// Describes the columns of a result set. Sent in the first
/// `RowBatch` of a streaming response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultSchema {
    pub columns: Vec<ResultColumn>,
}

/// A single column in a result set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultColumn {
    pub name: String,
    /// PostgreSQL type OID.
    pub type_oid: u32,
    /// Type modifier (-1 if none).
    pub type_mod: i32,
    /// Wire format length (-1 for variable).
    pub format_len: i16,
}

/// Row data payload — either PG binary or Arrow IPC bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RowData {
    PgBinary(Vec<u8>),
    ArrowIpc(Vec<u8>),
}

// ── Runtime Stats ───────────────────────────────────────────

/// Optional runtime statistics returned after query execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeStats {
    /// Wall-clock execution time in microseconds.
    pub execution_time_us: u64,
    /// Planning time in microseconds.
    pub planning_time_us: u64,
    /// Total rows scanned.
    pub rows_scanned: u64,
    /// Number of shared buffer hits.
    pub shared_hits: u64,
    /// Number of shared buffer reads.
    pub shared_reads: u64,
}

// ── Transaction Isolation ───────────────────────────────────

/// Transaction isolation level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IsolationLevel {
    #[default]
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

// ── COPY Format ─────────────────────────────────────────────

/// COPY protocol data format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopyFormat {
    Text,
    Csv,
    Binary,
}

// ── Facts Wire Types ────────────────────────────────────────

/// What to include in a facts snapshot response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[expect(clippy::struct_excessive_bools)]
pub struct FactsInclude {
    pub table_stats: bool,
    pub column_stats: bool,
    pub index_stats: bool,
    pub foreign_keys: bool,
    pub mvcc_stats: bool,
    pub hardware_profile: bool,
    pub pg_config: bool,
}

impl Default for FactsInclude {
    fn default() -> Self {
        Self {
            table_stats: true,
            column_stats: true,
            index_stats: true,
            foreign_keys: true,
            mvcc_stats: true,
            hardware_profile: true,
            pg_config: true,
        }
    }
}

/// Per-table facts bundle in a snapshot response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableFactsBundle {
    pub schema: String,
    pub table: String,
    pub table_oid: u32,
    pub stats: Statistics,
    pub foreign_keys: Vec<ForeignKeyWire>,
    pub mvcc: Option<MvccStats>,
}

/// Foreign key relationship.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForeignKeyWire {
    pub name: String,
    pub from_columns: Vec<String>,
    pub to_schema: String,
    pub to_table: String,
    pub to_columns: Vec<String>,
}

/// MVCC-specific statistics (PostgreSQL).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MvccStats {
    pub live_tuples: f64,
    pub dead_tuples: f64,
    pub bloat_factor: f64,
    pub last_vacuum: Option<u64>,
    pub last_analyze: Option<u64>,
}

/// Hardware profile transmitted over the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HardwareProfileWire {
    pub cpu_cores: u32,
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub storage_type: StorageType,
    pub l1_cache_bytes: u64,
    pub l2_cache_bytes: u64,
    pub l3_cache_bytes: u64,
}

/// Storage device type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageType {
    Hdd,
    Ssd,
    NvmeSsd,
    Unknown,
}

/// PostgreSQL configuration parameters relevant to optimization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PgConfigWire {
    pub version_major: u32,
    pub shared_buffers_bytes: u64,
    pub work_mem_bytes: u64,
    pub effective_cache_size_bytes: u64,
    pub random_page_cost: f64,
    pub seq_page_cost: f64,
    pub cpu_tuple_cost: f64,
    pub max_parallel_workers_per_gather: u32,
    pub extensions: Vec<String>,
}

// ── Invalidation Types ──────────────────────────────────────

/// Granularity of invalidation subscriptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvalidationGranularity {
    /// Any change to any subscribed table.
    Table,
    /// Per-column statistics changes.
    Column,
    /// Per-index changes.
    Index,
}

/// What was invalidated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InvalidationTarget {
    Table {
        schema: String,
        table: String,
        oid: u32,
    },
    Index {
        schema: String,
        table: String,
        index: String,
    },
    Constraint {
        schema: String,
        table: String,
        name: String,
    },
}

/// Why the invalidation happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvalidationCause {
    Analyze,
    Ddl,
    BulkLoad,
    Vacuum,
    Truncate,
}

// ── Streaming Metrics ───────────────────────────────────────

/// Metrics available for streaming subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreamingMetric {
    ActiveQueries,
    BufferCacheHitRatio,
    TransactionRate,
    TableModificationCounters,
    LockContention,
    WalWriteRate,
}

/// A single metric value in a streaming update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricValue {
    pub metric: StreamingMetric,
    pub value: f64,
}

// ── Facts Catalog Types ─────────────────────────────────────

/// Category of facts available from a backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactCategory {
    pub name: String,
    pub description: String,
    pub resources: Vec<String>,
}

/// A resource identifier for facts queries.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceId(pub String);

/// A fact value returned by a facts query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FactValue {
    Integer(i64),
    Float(f64),
    Text(String),
    Bool(bool),
    Null,
}

// ── SQL Error Types ─────────────────────────────────────────

/// Severity of a notice message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoticeSeverity {
    Warning,
    Notice,
    Debug,
    Info,
    Log,
}
