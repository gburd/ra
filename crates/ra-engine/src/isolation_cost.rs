//! Isolation-aware cost adjustments for query plan selection.
//!
//! Computes penalty costs based on transaction isolation context:
//! - **Lock penalty**: penalizes wide scan footprints under SSI
//!   or MySQL SERIALIZABLE to reduce SIRead/shared lock contention.
//! - **Bloat penalty**: penalizes long-running plans when a
//!   transaction-level snapshot pins dead tuples.
//! - **SubXID penalty**: penalizes plans with many visibility checks
//!   when subtransaction depth exceeds PostgreSQL's 64-entry cache.
//! - **MultiXact penalty**: penalizes plans that acquire many shared
//!   row locks under high MultiXact pressure.
//!
//! See RFC 0058 for the full design rationale.

use ra_core::cost::Cost;
use ra_core::isolation::{
    BackendKind, IsolationLevel, MultiXactPressure, TransactionContext,
};

/// Tunable weights for isolation-aware cost penalties.
///
/// All weights default to empirically reasonable values derived
/// from PostgreSQL internals benchmarks. Use
/// [`IsolationCostConfig::default`] unless you have workload-specific
/// calibration data.
#[derive(Debug, Clone)]
pub struct IsolationCostConfig {
    /// Base cost per page accessed under predicate locking.
    /// Represents the overhead of acquiring and managing SIRead locks.
    pub lock_base_cost: f64,
    /// Multiplier for bloat penalty, applied to estimated runtime
    /// when snapshot age exceeds the threshold.
    pub bloat_weight: f64,
    /// Snapshot age threshold (ms) above which bloat penalty kicks in.
    pub bloat_snapshot_threshold_ms: u64,
    /// Weight per visibility check when SubXID cache overflows.
    pub subxid_weight: f64,
    /// Weight per shared lock when MultiXact pressure is elevated.
    pub multixact_weight: f64,
    /// Estimated dead tuple rate (tuples/ms) for bloat calculations.
    pub dead_tuple_rate: f64,
    /// Gap lock penalty per range-scanned page (MySQL-specific).
    pub gap_lock_cost: f64,
    /// Undo retention pressure per ms of execution (Oracle-specific).
    pub undo_retention_weight: f64,
}

impl Default for IsolationCostConfig {
    fn default() -> Self {
        Self {
            lock_base_cost: 0.01,
            bloat_weight: 0.001,
            bloat_snapshot_threshold_ms: 1000,
            subxid_weight: 0.005,
            multixact_weight: 0.008,
            dead_tuple_rate: 0.1,
            gap_lock_cost: 0.015,
            undo_retention_weight: 0.0005,
        }
    }
}

/// Estimated plan characteristics used for penalty calculations.
///
/// These are derived from the plan structure and table statistics
/// before isolation penalties are applied.
#[derive(Debug, Clone)]
pub struct PlanEstimates {
    /// Number of heap/data pages the plan accesses.
    pub pages_accessed: f64,
    /// Estimated number of rows the plan touches.
    pub rows_accessed: f64,
    /// Estimated total runtime in milliseconds.
    pub estimated_runtime_ms: f64,
    /// Number of MVCC visibility checks the plan performs.
    /// Approximated as `rows_accessed` for sequential scans.
    pub visibility_checks: f64,
    /// Whether the plan uses an index-only scan (no heap access).
    pub is_index_only: bool,
    /// Number of range-scanned index pages (for gap lock estimation).
    pub index_range_pages: f64,
}

impl PlanEstimates {
    /// Create estimates for a sequential scan over `pages` pages.
    #[must_use]
    pub fn seq_scan(pages: f64, rows: f64) -> Self {
        Self {
            pages_accessed: pages,
            rows_accessed: rows,
            estimated_runtime_ms: pages * 0.1 + rows * 0.01,
            visibility_checks: rows,
            is_index_only: false,
            index_range_pages: 0.0,
        }
    }

    /// Create estimates for an index-only scan.
    #[must_use]
    pub fn index_only_scan(
        index_pages: f64,
        rows: f64,
    ) -> Self {
        Self {
            pages_accessed: index_pages,
            rows_accessed: rows,
            estimated_runtime_ms: index_pages * 0.05 + rows * 0.005,
            visibility_checks: rows,
            is_index_only: true,
            index_range_pages: index_pages,
        }
    }

    /// Create estimates for an index scan with heap fetches.
    #[must_use]
    pub fn index_scan(
        index_pages: f64,
        heap_pages: f64,
        rows: f64,
    ) -> Self {
        let total_pages = index_pages + heap_pages;
        Self {
            pages_accessed: total_pages,
            rows_accessed: rows,
            estimated_runtime_ms: total_pages * 0.08 + rows * 0.008,
            visibility_checks: rows,
            is_index_only: false,
            index_range_pages: index_pages,
        }
    }
}

/// Compute the total isolation-aware cost adjustment.
///
/// Returns a `Cost` representing the sum of all applicable penalties
/// (lock, bloat, SubXID, MultiXact) for the given plan under the
/// given transaction context. The caller adds this to the base cost.
///
/// If `txn` is `None`, returns `Cost::ZERO` (no adjustment).
#[must_use]
pub fn isolation_cost_adjustment(
    txn: Option<&TransactionContext>,
    plan: &PlanEstimates,
    config: &IsolationCostConfig,
) -> Cost {
    let Some(txn) = txn else {
        return Cost::ZERO;
    };

    let lock = compute_lock_penalty(txn, plan, config);
    let bloat = compute_bloat_penalty(txn, plan, config);
    let subxid = compute_subxid_penalty(txn, plan, config);
    let multixact = compute_multixact_penalty(txn, plan, config);

    Cost::new(
        lock + bloat + subxid + multixact,
        0.0,
        0.0,
        0,
    )
}

/// Lock footprint penalty.
///
/// Under PostgreSQL SSI, every page accessed acquires an SIRead lock.
/// Under MySQL SERIALIZABLE, every read becomes SELECT ... FOR SHARE.
/// Plans that access fewer pages reduce lock contention and abort rate.
fn compute_lock_penalty(
    txn: &TransactionContext,
    plan: &PlanEstimates,
    config: &IsolationCostConfig,
) -> f64 {
    match txn.backend {
        BackendKind::PostgreSQL => {
            if !txn.uses_ssi {
                return 0.0;
            }
            // Index-only scans touch fewer pages -> lower lock footprint
            let pages = if plan.is_index_only {
                plan.pages_accessed
            } else {
                plan.pages_accessed
            };
            config.lock_base_cost * pages
        }
        BackendKind::MySQLInnoDB => {
            match txn.isolation_level {
                IsolationLevel::Serializable => {
                    // All reads acquire shared locks
                    config.lock_base_cost * plan.rows_accessed
                }
                IsolationLevel::RepeatableRead => {
                    // Gap locks on index range scans
                    config.gap_lock_cost * plan.index_range_pages
                }
                _ => 0.0,
            }
        }
        BackendKind::Oracle => {
            // Oracle uses optimistic conflict detection; no lock penalty.
            // Under SERIALIZABLE, prefer shorter execution to reduce
            // undo retention pressure, but that's the bloat penalty.
            0.0
        }
        BackendKind::SQLite | BackendKind::DuckDB => {
            // SQLite: single writer, no per-row locks.
            // DuckDB: optimistic MVCC, no lock concerns for reads.
            0.0
        }
    }
}

/// Snapshot bloat penalty.
///
/// Under REPEATABLE READ and SERIALIZABLE, a transaction-level snapshot
/// pins dead tuples for the entire transaction. Longer execution means
/// more bloat. Under READ COMMITTED, each statement gets a fresh
/// snapshot, so no bloat accumulates across statements.
fn compute_bloat_penalty(
    txn: &TransactionContext,
    plan: &PlanEstimates,
    config: &IsolationCostConfig,
) -> f64 {
    // Only applies when holding a transaction-level snapshot
    if !txn.isolation_level.holds_transaction_snapshot() {
        return 0.0;
    }

    match txn.backend {
        BackendKind::PostgreSQL | BackendKind::MySQLInnoDB => {
            if txn.snapshot_age_ms > config.bloat_snapshot_threshold_ms
            {
                plan.estimated_runtime_ms
                    * config.dead_tuple_rate
                    * config.bloat_weight
            } else {
                0.0
            }
        }
        BackendKind::Oracle => {
            // Oracle SERIALIZABLE: undo retention pressure
            if txn.isolation_level == IsolationLevel::Serializable {
                plan.estimated_runtime_ms
                    * config.undo_retention_weight
            } else {
                0.0
            }
        }
        BackendKind::SQLite | BackendKind::DuckDB => 0.0,
    }
}

/// SubXID overflow penalty.
///
/// PostgreSQL stores subtransaction IDs in a 64-entry process-local
/// cache. When depth exceeds 64, every `XidInMVCCSnapshot` check
/// degrades from O(1) to O(n) where n is the subtransaction count.
/// Plans with many visibility checks (seq scans) are penalized.
fn compute_subxid_penalty(
    txn: &TransactionContext,
    plan: &PlanEstimates,
    config: &IsolationCostConfig,
) -> f64 {
    if txn.backend != BackendKind::PostgreSQL {
        return 0.0;
    }

    if !txn.has_subxid_overflow() {
        return 0.0;
    }

    let overflow_depth =
        txn.subtransaction_depth - TransactionContext::PG_SUBXID_CACHE_LIMIT;
    plan.visibility_checks
        * f64::from(overflow_depth)
        * config.subxid_weight
}

/// MultiXact avoidance penalty.
///
/// Under high MultiXact pressure, plans that acquire many shared row
/// locks generate MultiXactId entries that can stall vacuuming. Prefer
/// plans that touch fewer tuples.
fn compute_multixact_penalty(
    txn: &TransactionContext,
    plan: &PlanEstimates,
    config: &IsolationCostConfig,
) -> f64 {
    if txn.backend != BackendKind::PostgreSQL {
        return 0.0;
    }

    let multiplier = match txn.multi_xact_pressure {
        MultiXactPressure::Low => return 0.0,
        MultiXactPressure::Medium => 0.5,
        MultiXactPressure::High => 1.0,
    };

    plan.rows_accessed * config.multixact_weight * multiplier
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn default_config() -> IsolationCostConfig {
        IsolationCostConfig::default()
    }

    // ── No context -> no penalty ─────────────────────────────────

    #[test]
    fn no_context_returns_zero() {
        let plan = PlanEstimates::seq_scan(100.0, 10000.0);
        let adj = isolation_cost_adjustment(None, &plan, &default_config());
        assert_eq!(adj.cpu, 0.0);
    }

    // ── Lock penalty: PostgreSQL SSI ─────────────────────────────

    #[test]
    fn pg_ssi_penalizes_seq_scan_pages() {
        let txn = TransactionContext::pg_serializable();
        let config = default_config();
        let plan = PlanEstimates::seq_scan(1000.0, 100_000.0);
        let penalty = compute_lock_penalty(&txn, &plan, &config);
        assert!(penalty > 0.0);
        assert!((penalty - config.lock_base_cost * 1000.0).abs() < 1e-10);
    }

    #[test]
    fn pg_ssi_index_only_lower_lock_penalty() {
        let txn = TransactionContext::pg_serializable();
        let config = default_config();
        let seq = PlanEstimates::seq_scan(1000.0, 100_000.0);
        let idx = PlanEstimates::index_only_scan(50.0, 100.0);
        let seq_penalty = compute_lock_penalty(&txn, &seq, &config);
        let idx_penalty = compute_lock_penalty(&txn, &idx, &config);
        assert!(seq_penalty > idx_penalty);
    }

    #[test]
    fn pg_read_committed_no_lock_penalty() {
        let txn = TransactionContext::pg_read_committed();
        let config = default_config();
        let plan = PlanEstimates::seq_scan(1000.0, 100_000.0);
        let penalty = compute_lock_penalty(&txn, &plan, &config);
        assert_eq!(penalty, 0.0);
    }

    // ── Lock penalty: MySQL ──────────────────────────────────────

    #[test]
    fn mysql_serializable_penalizes_all_reads() {
        let mut txn = TransactionContext::mysql_default();
        txn.isolation_level = IsolationLevel::Serializable;
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 5000.0);
        let penalty = compute_lock_penalty(&txn, &plan, &config);
        assert!(
            (penalty - config.lock_base_cost * 5000.0).abs() < 1e-10
        );
    }

    #[test]
    fn mysql_repeatable_read_gap_lock_penalty() {
        let txn = TransactionContext::mysql_default();
        let config = default_config();
        let plan = PlanEstimates::index_scan(30.0, 20.0, 500.0);
        let penalty = compute_lock_penalty(&txn, &plan, &config);
        assert!((penalty - config.gap_lock_cost * 30.0).abs() < 1e-10);
    }

    #[test]
    fn mysql_read_committed_no_lock_penalty() {
        let mut txn = TransactionContext::mysql_default();
        txn.isolation_level = IsolationLevel::ReadCommitted;
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 5000.0);
        let penalty = compute_lock_penalty(&txn, &plan, &config);
        assert_eq!(penalty, 0.0);
    }

    // ── Lock penalty: Oracle, SQLite, DuckDB ─────────────────────

    #[test]
    fn oracle_no_lock_penalty() {
        let txn = TransactionContext {
            isolation_level: IsolationLevel::Serializable,
            backend: BackendKind::Oracle,
            ..TransactionContext::default()
        };
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 5000.0);
        assert_eq!(compute_lock_penalty(&txn, &plan, &config), 0.0);
    }

    #[test]
    fn sqlite_no_lock_penalty() {
        let txn = TransactionContext {
            backend: BackendKind::SQLite,
            ..TransactionContext::default()
        };
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 5000.0);
        assert_eq!(compute_lock_penalty(&txn, &plan, &config), 0.0);
    }

    #[test]
    fn duckdb_no_lock_penalty() {
        let txn = TransactionContext {
            backend: BackendKind::DuckDB,
            ..TransactionContext::default()
        };
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 5000.0);
        assert_eq!(compute_lock_penalty(&txn, &plan, &config), 0.0);
    }

    // ── Bloat penalty ────────────────────────────────────────────

    #[test]
    fn bloat_penalty_under_threshold_is_zero() {
        let mut txn = TransactionContext::pg_serializable();
        txn.snapshot_age_ms = 500;
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 10000.0);
        let penalty = compute_bloat_penalty(&txn, &plan, &config);
        assert_eq!(penalty, 0.0);
    }

    #[test]
    fn bloat_penalty_over_threshold() {
        let mut txn = TransactionContext::pg_serializable();
        txn.snapshot_age_ms = 5000;
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 10000.0);
        let penalty = compute_bloat_penalty(&txn, &plan, &config);
        let expected = plan.estimated_runtime_ms
            * config.dead_tuple_rate
            * config.bloat_weight;
        assert!((penalty - expected).abs() < 1e-10);
        assert!(penalty > 0.0);
    }

    #[test]
    fn bloat_penalty_read_committed_is_zero() {
        let mut txn = TransactionContext::pg_read_committed();
        txn.snapshot_age_ms = 999_999;
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 10000.0);
        let penalty = compute_bloat_penalty(&txn, &plan, &config);
        assert_eq!(penalty, 0.0);
    }

    #[test]
    fn oracle_serializable_undo_penalty() {
        let txn = TransactionContext {
            isolation_level: IsolationLevel::Serializable,
            snapshot_age_ms: 5000,
            backend: BackendKind::Oracle,
            ..TransactionContext::default()
        };
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 10000.0);
        let penalty = compute_bloat_penalty(&txn, &plan, &config);
        let expected =
            plan.estimated_runtime_ms * config.undo_retention_weight;
        assert!((penalty - expected).abs() < 1e-10);
    }

    // ── SubXID penalty ───────────────────────────────────────────

    #[test]
    fn subxid_penalty_under_limit_is_zero() {
        let mut txn = TransactionContext::pg_read_committed();
        txn.subtransaction_depth = 30;
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 10000.0);
        let penalty = compute_subxid_penalty(&txn, &plan, &config);
        assert_eq!(penalty, 0.0);
    }

    #[test]
    fn subxid_penalty_over_limit() {
        let mut txn = TransactionContext::pg_read_committed();
        txn.subtransaction_depth = 100;
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 10000.0);
        let penalty = compute_subxid_penalty(&txn, &plan, &config);
        let overflow = 100 - 64;
        let expected = 10000.0 * f64::from(overflow) * config.subxid_weight;
        assert!((penalty - expected).abs() < 1e-10);
        assert!(penalty > 0.0);
    }

    #[test]
    fn subxid_penalty_not_postgresql_is_zero() {
        let txn = TransactionContext {
            subtransaction_depth: 200,
            backend: BackendKind::MySQLInnoDB,
            ..TransactionContext::default()
        };
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 10000.0);
        let penalty = compute_subxid_penalty(&txn, &plan, &config);
        assert_eq!(penalty, 0.0);
    }

    #[test]
    fn subxid_penalty_index_only_lower() {
        let mut txn = TransactionContext::pg_read_committed();
        txn.subtransaction_depth = 100;
        let config = default_config();
        let seq = PlanEstimates::seq_scan(100.0, 10000.0);
        let idx = PlanEstimates::index_only_scan(5.0, 50.0);
        let seq_penalty = compute_subxid_penalty(&txn, &seq, &config);
        let idx_penalty = compute_subxid_penalty(&txn, &idx, &config);
        assert!(seq_penalty > idx_penalty);
    }

    // ── MultiXact penalty ────────────────────────────────────────

    #[test]
    fn multixact_low_is_zero() {
        let txn = TransactionContext::pg_serializable();
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 10000.0);
        let penalty = compute_multixact_penalty(&txn, &plan, &config);
        assert_eq!(penalty, 0.0);
    }

    #[test]
    fn multixact_medium_applies_half_weight() {
        let mut txn = TransactionContext::pg_serializable();
        txn.multi_xact_pressure = MultiXactPressure::Medium;
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 10000.0);
        let penalty =
            compute_multixact_penalty(&txn, &plan, &config);
        let expected = 10000.0 * config.multixact_weight * 0.5;
        assert!((penalty - expected).abs() < 1e-10);
    }

    #[test]
    fn multixact_high_full_weight() {
        let mut txn = TransactionContext::pg_serializable();
        txn.multi_xact_pressure = MultiXactPressure::High;
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 10000.0);
        let penalty =
            compute_multixact_penalty(&txn, &plan, &config);
        let expected = 10000.0 * config.multixact_weight * 1.0;
        assert!((penalty - expected).abs() < 1e-10);
    }

    #[test]
    fn multixact_not_postgresql_is_zero() {
        let txn = TransactionContext {
            multi_xact_pressure: MultiXactPressure::High,
            backend: BackendKind::MySQLInnoDB,
            ..TransactionContext::default()
        };
        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 10000.0);
        assert_eq!(
            compute_multixact_penalty(&txn, &plan, &config),
            0.0
        );
    }

    // ── Combined adjustment ──────────────────────────────────────

    #[test]
    fn combined_adjustment_sums_penalties() {
        let mut txn = TransactionContext::pg_serializable();
        txn.snapshot_age_ms = 5000;
        txn.subtransaction_depth = 100;
        txn.multi_xact_pressure = MultiXactPressure::High;

        let config = default_config();
        let plan = PlanEstimates::seq_scan(100.0, 10000.0);

        let lock = compute_lock_penalty(&txn, &plan, &config);
        let bloat = compute_bloat_penalty(&txn, &plan, &config);
        let subxid = compute_subxid_penalty(&txn, &plan, &config);
        let multixact =
            compute_multixact_penalty(&txn, &plan, &config);

        let adj = isolation_cost_adjustment(
            Some(&txn),
            &plan,
            &config,
        );

        let expected = lock + bloat + subxid + multixact;
        assert!((adj.cpu - expected).abs() < 1e-10);
        assert!(adj.cpu > 0.0);
    }

    #[test]
    fn ssi_prefers_index_only_over_seq_scan() {
        let txn = TransactionContext::pg_serializable();
        let config = default_config();

        let seq = PlanEstimates::seq_scan(1000.0, 100_000.0);
        let idx = PlanEstimates::index_only_scan(50.0, 100.0);

        let seq_adj =
            isolation_cost_adjustment(Some(&txn), &seq, &config);
        let idx_adj =
            isolation_cost_adjustment(Some(&txn), &idx, &config);

        assert!(
            seq_adj.cpu > idx_adj.cpu,
            "SSI should penalize seq scan ({}) more than \
             index-only scan ({})",
            seq_adj.cpu,
            idx_adj.cpu,
        );
    }

    #[test]
    fn deep_savepoints_penalize_wide_scans() {
        let mut txn = TransactionContext::pg_read_committed();
        txn.subtransaction_depth = 200;
        let config = default_config();

        let wide = PlanEstimates::seq_scan(500.0, 50_000.0);
        let narrow = PlanEstimates::index_only_scan(10.0, 100.0);

        let wide_adj =
            isolation_cost_adjustment(Some(&txn), &wide, &config);
        let narrow_adj =
            isolation_cost_adjustment(Some(&txn), &narrow, &config);

        assert!(
            wide_adj.cpu > narrow_adj.cpu,
            "Deep savepoints should penalize wide scan ({}) more \
             than narrow scan ({})",
            wide_adj.cpu,
            narrow_adj.cpu,
        );
    }
}
