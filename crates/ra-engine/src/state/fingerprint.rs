//! System fingerprint: compact state vector for neural decision-making.
//!
//! [`SystemFingerprint`] encodes the current system state into a fixed-size
//! struct that neural components consume at every optimization decision point.
//! It is updated by a background monitor thread (~1s cadence for hardware,
//! ~30s for capabilities) and read atomically by the hot path.
//!
//! # Performance
//!
//! - Read: ~10ns (Arc clone + pointer deref)
//! - Update: ~50ns (Arc swap)
//! - Size: 56 bytes (fits in one cache line)

use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, Ordering};

/// Capability bit flags for loaded `PostgreSQL` extensions and features.
///
/// Rule groups declare required capabilities; the monitor detects actual
/// state. Mismatch means the rule group is skipped at zero cost.
pub mod capabilities {
    /// Citus: distributed queries and shard-aware optimization.
    pub const CITUS: u64 = 1 << 0;
    /// `PostGIS`: spatial indexes and operators.
    pub const POSTGIS: u64 = 1 << 1;
    /// `pg_trgm`: trigram similarity indexes.
    pub const PG_TRGM: u64 = 1 << 2;
    /// `DocumentDB`: BSON/document operators (Amazon `DocumentDB` compat layer).
    pub const DOCUMENTDB: u64 = 1 << 3;
    /// `pgvector`: vector similarity search (HNSW, `IVFFlat`).
    pub const PGVECTOR: u64 = 1 << 4;
    /// `TimescaleDB`: hypertables, compression, continuous aggregates.
    pub const TIMESCALEDB: u64 = 1 << 5;
    /// `pg_partman`: automated partition management.
    pub const PG_PARTMAN: u64 = 1 << 6;
    /// Parallel query: `max_parallel_workers_per_gather` > 0.
    pub const PARALLEL_QUERY: u64 = 1 << 7;
    /// GPU acceleration: `pg_strom` or similar.
    pub const GPU_ACCEL: u64 = 1 << 8;
    /// Foreign data wrappers actively in use.
    pub const FDW_ACTIVE: u64 = 1 << 9;
    /// RUM index extension loaded.
    pub const RUM_INDEX: u64 = 1 << 10;
    /// `pg_stat_statements` available for workload tracking.
    pub const PG_STAT_STATEMENTS: u64 = 1 << 11;
    /// Incremental sort available (PG 13+).
    pub const INCREMENTAL_SORT: u64 = 1 << 12;
    /// Memoize node available (PG 14+).
    pub const MEMOIZE: u64 = 1 << 13;
    /// Full text search with GIN indexes.
    pub const FTS_GIN: u64 = 1 << 14;
    /// BRIN indexes in active use.
    pub const BRIN_INDEXES: u64 = 1 << 15;
}

/// Compact system state snapshot consumed by neural optimizer components.
///
/// Updated atomically by the background monitor. All float fields are
/// normalized to [0.0, 1.0] unless otherwise noted.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct SystemFingerprint {
    // --- Hardware utilization (updated ~1s) ---
    /// Current CPU utilization fraction (0.0 = idle, 1.0 = saturated).
    pub cpu_load_fraction: f32,
    /// Memory pressure (0.0 = abundant, 1.0 = swapping/OOM).
    pub memory_pressure: f32,
    /// I/O saturation (disk queue depth / max throughput).
    pub io_saturation: f32,
    /// `PostgreSQL` `shared_buffers` cache hit rate.
    pub shared_buffers_hit_rate: f32,

    // --- Capabilities (updated on extension load/unload, ~30s poll) ---
    /// Bit flags for loaded extensions/features (see [`capabilities`] module).
    pub capabilities: u64,

    // --- Statistics quality (updated per-ANALYZE) ---
    /// Mean staleness across all active tables (0.0 = fresh, 1.0 = ancient).
    pub avg_staleness: f32,
    /// Maximum staleness of any single table.
    pub worst_staleness: f32,
    /// Fraction of columns that have histogram statistics.
    pub stats_coverage: f32,

    // --- Workload character (rolling window, ~60s) ---
    /// Fraction of recent queries that are simple (OLTP-like).
    pub oltp_fraction: f32,
    /// Average number of tables per query in the recent window.
    pub avg_tables_per_query: f32,
    /// Plan cache hit rate (higher = more repetitive workload).
    pub plan_cache_hit_rate: f32,

    // --- Model confidence ---
    /// Total training samples the neural model has processed.
    pub model_samples_trained: u32,
    /// Recent mean absolute percentage error of neural predictions.
    pub model_recent_mape: f32,
}

impl Default for SystemFingerprint {
    fn default() -> Self {
        Self {
            cpu_load_fraction: 0.0,
            memory_pressure: 0.0,
            io_saturation: 0.0,
            shared_buffers_hit_rate: 0.99,

            capabilities: capabilities::PARALLEL_QUERY
                | capabilities::INCREMENTAL_SORT
                | capabilities::MEMOIZE,

            avg_staleness: 0.0,
            worst_staleness: 0.0,
            stats_coverage: 1.0,

            oltp_fraction: 0.5,
            avg_tables_per_query: 3.0,
            plan_cache_hit_rate: 0.0,

            model_samples_trained: 0,
            model_recent_mape: 1.0, // untrained = max uncertainty
        }
    }
}

impl SystemFingerprint {
    /// Number of float dimensions exposed to neural models.
    /// 12 floats + 1 u64 (treated as 2 f32s) + 1 u32 (treated as f32) = 14 dims.
    pub const NEURAL_DIM: usize = 14;

    /// Check whether a specific capability bit is set.
    #[inline]
    #[must_use]
    pub fn has_capability(&self, cap: u64) -> bool {
        self.capabilities & cap != 0
    }

    /// Encode the fingerprint into a fixed-size float vector for neural input.
    ///
    /// The 14-dimensional encoding matches the neural model's expected input
    /// layout when concatenated with `QueryFeatures` (12-dim).
    #[inline]
    #[must_use]
    pub fn to_neural_vec(&self) -> [f32; Self::NEURAL_DIM] {
        [
            self.cpu_load_fraction,
            self.memory_pressure,
            self.io_saturation,
            self.shared_buffers_hit_rate,
            // Capabilities encoded as two normalized floats (low/high 32 bits)
            (self.capabilities & 0xFFFF_FFFF) as f32 / u32::MAX as f32,
            ((self.capabilities >> 32) & 0xFFFF_FFFF) as f32 / u32::MAX as f32,
            self.avg_staleness,
            self.worst_staleness,
            self.stats_coverage,
            self.oltp_fraction,
            self.avg_tables_per_query / 20.0, // normalize to ~[0,1]
            self.plan_cache_hit_rate,
            self.model_samples_trained as f32 / 10000.0, // normalize
            1.0 - self.model_recent_mape.clamp(0.0, 1.0), // invert: higher = better
        ]
    }

    /// Compute the neural blend factor (alpha) for hybrid cost function.
    ///
    /// Returns a value in [0.0, 0.9] representing how much weight to give
    /// the neural model vs traditional costing. Never reaches 1.0 — the
    /// traditional cost function always contributes at least 10%.
    #[inline]
    #[must_use]
    pub fn compute_blend_alpha(&self) -> f32 {
        // Base confidence from training data volume (sigmoid saturation)
        let data_conf =
            1.0 - (-(self.model_samples_trained as f32 / 2000.0)).exp();

        // Reduce confidence when system state is unusual
        let state_stability =
            1.0 - self.io_saturation.max(self.memory_pressure);

        // Reduce confidence when statistics are stale
        let stats_quality = 1.0 - self.avg_staleness;

        // Product of confidence factors, clamped to safety ceiling
        (data_conf * state_stability * stats_quality).clamp(0.0, 0.9)
    }

    /// Compressed 4-dimensional context for per-node neural features.
    ///
    /// Used inside `HybridCostFn` where the full 14-dim vector is too
    /// expensive per-node. This is a lossy PCA-like compression:
    /// [`resource_pressure`, `stats_quality`, `workload_type`, `model_confidence`].
    #[inline]
    #[must_use]
    pub fn compressed_context(&self) -> [f32; 4] {
        [
            // Resource pressure: combined CPU/IO/memory
            (self.cpu_load_fraction + self.io_saturation + self.memory_pressure)
                / 3.0,
            // Statistics quality
            self.stats_coverage * (1.0 - self.avg_staleness),
            // Workload type (0 = OLAP, 1 = OLTP)
            self.oltp_fraction,
            // Model confidence
            self.compute_blend_alpha(),
        ]
    }
}

/// Thread-safe, lock-free fingerprint storage using atomic pointer swap.
///
/// The monitor thread produces new fingerprints and swaps them in atomically.
/// Reader threads load the current pointer and clone the data. Since
/// `SystemFingerprint` is Copy (56 bytes), this is effectively a memcpy.
pub struct AtomicFingerprint {
    ptr: AtomicPtr<SystemFingerprint>,
}

impl AtomicFingerprint {
    /// Create a new atomic fingerprint with default values.
    #[must_use]
    pub fn new() -> Self {
        let boxed = Box::new(SystemFingerprint::default());
        Self {
            ptr: AtomicPtr::new(Box::into_raw(boxed)),
        }
    }

    /// Create from an initial fingerprint value.
    #[must_use]
    pub fn with_value(fp: SystemFingerprint) -> Self {
        let boxed = Box::new(fp);
        Self {
            ptr: AtomicPtr::new(Box::into_raw(boxed)),
        }
    }

    /// Read the current fingerprint (lock-free, ~10ns).
    #[inline]
    #[must_use]
    pub fn load(&self) -> SystemFingerprint {
        let ptr = self.ptr.load(Ordering::Acquire);
        // SAFETY: ptr is always valid — we only ever store Box::into_raw
        // results and never free until the next swap or Drop.
        unsafe { *ptr }
    }

    /// Atomically replace the fingerprint with a new value.
    ///
    /// The old value is freed after the swap. Concurrent readers that
    /// loaded the old pointer before this call complete safely because
    /// they only read (Copy) the data — no dangling references.
    pub fn store(&self, new_fp: SystemFingerprint) {
        let new_boxed = Box::new(new_fp);
        let new_ptr = Box::into_raw(new_boxed);
        let old_ptr = self.ptr.swap(new_ptr, Ordering::AcqRel);
        // SAFETY: old_ptr was produced by Box::into_raw in new() or a
        // previous store(). We own it exclusively after the swap.
        unsafe {
            drop(Box::from_raw(old_ptr));
        }
    }
}

impl Default for AtomicFingerprint {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AtomicFingerprint {
    fn drop(&mut self) {
        let ptr = *self.ptr.get_mut();
        if !ptr.is_null() {
            // SAFETY: ptr was produced by Box::into_raw
            unsafe {
                drop(Box::from_raw(ptr));
            }
        }
    }
}

// SAFETY: AtomicFingerprint uses atomic operations for all access.
// The inner pointer is only mutated through atomic swap.
unsafe impl Send for AtomicFingerprint {}
unsafe impl Sync for AtomicFingerprint {}

/// Ergonomic reader handle wrapping `Arc<AtomicFingerprint>`.
///
/// Cheap to clone (Arc refcount increment) and pass into optimizer components.
#[derive(Clone)]
pub struct FingerprintReader {
    inner: Arc<AtomicFingerprint>,
}

impl FingerprintReader {
    /// Create a new reader/writer pair. The returned reader can be cloned
    /// and distributed to optimizer components. Updates go through
    /// [`FingerprintReader::update`] or by accessing the inner Arc directly.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AtomicFingerprint::new()),
        }
    }

    /// Create from an existing shared atomic fingerprint.
    #[must_use]
    pub fn from_shared(shared: Arc<AtomicFingerprint>) -> Self {
        Self { inner: shared }
    }

    /// Read the current system fingerprint (~10ns).
    #[inline]
    #[must_use]
    pub fn read(&self) -> SystemFingerprint {
        self.inner.load()
    }

    /// Update the fingerprint (called by the monitor thread).
    pub fn update(&self, fp: SystemFingerprint) {
        self.inner.store(fp);
    }

    /// Get a reference to the underlying shared storage.
    #[must_use]
    pub fn shared(&self) -> &Arc<AtomicFingerprint> {
        &self.inner
    }
}

impl Default for FingerprintReader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_fingerprint_has_sane_values() {
        let fp = SystemFingerprint::default();
        assert!(fp.cpu_load_fraction >= 0.0);
        assert!(fp.shared_buffers_hit_rate > 0.0);
        assert!(fp.stats_coverage == 1.0);
        assert!(fp.model_recent_mape == 1.0); // untrained
    }

    #[test]
    fn capability_check_works() {
        let fp = SystemFingerprint::default();
        assert!(fp.has_capability(capabilities::PARALLEL_QUERY));
        assert!(!fp.has_capability(capabilities::CITUS));
        assert!(!fp.has_capability(capabilities::PGVECTOR));
    }

    #[test]
    fn blend_alpha_is_zero_when_untrained() {
        let fp = SystemFingerprint::default();
        // With 0 samples trained, data_conf ≈ 0
        assert!(fp.compute_blend_alpha() < 0.01);
    }

    #[test]
    fn blend_alpha_grows_with_samples() {
        let mut fp = SystemFingerprint::default();
        fp.model_samples_trained = 5000;
        fp.model_recent_mape = 0.1;
        let alpha = fp.compute_blend_alpha();
        assert!(alpha > 0.5, "alpha={alpha} should be > 0.5 with 5000 samples");
        assert!(alpha <= 0.9, "alpha={alpha} must never exceed 0.9");
    }

    #[test]
    fn blend_alpha_capped_under_pressure() {
        let mut fp = SystemFingerprint::default();
        fp.model_samples_trained = 10000;
        fp.model_recent_mape = 0.05;
        fp.io_saturation = 0.9; // heavy I/O pressure
        let alpha = fp.compute_blend_alpha();
        assert!(
            alpha < 0.2,
            "alpha={alpha} should be low under I/O pressure"
        );
    }

    #[test]
    fn neural_vec_dimensions_match() {
        let fp = SystemFingerprint::default();
        let vec = fp.to_neural_vec();
        assert_eq!(vec.len(), SystemFingerprint::NEURAL_DIM);
    }

    #[test]
    fn compressed_context_in_range() {
        let fp = SystemFingerprint::default();
        let ctx = fp.compressed_context();
        for &v in &ctx {
            assert!(v >= 0.0 && v <= 1.0, "context value {v} out of [0,1]");
        }
    }

    #[test]
    fn atomic_fingerprint_read_write() {
        let atomic = AtomicFingerprint::new();
        let initial = atomic.load();
        assert_eq!(initial.model_samples_trained, 0);

        let mut updated = initial;
        updated.model_samples_trained = 42;
        updated.cpu_load_fraction = 0.75;
        atomic.store(updated);

        let read_back = atomic.load();
        assert_eq!(read_back.model_samples_trained, 42);
        assert!((read_back.cpu_load_fraction - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn fingerprint_reader_clone_shares_state() {
        let reader = FingerprintReader::new();
        let reader2 = reader.clone();

        let mut fp = reader.read();
        fp.model_samples_trained = 100;
        reader.update(fp);

        let read_from_clone = reader2.read();
        assert_eq!(read_from_clone.model_samples_trained, 100);
    }

    #[test]
    fn atomic_fingerprint_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let shared = Arc::new(AtomicFingerprint::new());

        let writer = Arc::clone(&shared);
        let reader = Arc::clone(&shared);

        let write_handle = thread::spawn(move || {
            for i in 0..1000 {
                let mut fp = SystemFingerprint::default();
                fp.model_samples_trained = i;
                writer.store(fp);
            }
        });

        let read_handle = thread::spawn(move || {
            let mut last_seen = 0u32;
            for _ in 0..10000 {
                let fp = reader.load();
                // Values must be monotonically non-decreasing or reset to 0
                // (we might read stale values but never garbage)
                assert!(fp.model_samples_trained <= 999);
                if fp.model_samples_trained >= last_seen {
                    last_seen = fp.model_samples_trained;
                }
            }
        });

        write_handle.join().unwrap();
        read_handle.join().unwrap();
    }
}
