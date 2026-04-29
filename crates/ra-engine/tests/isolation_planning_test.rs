//! Integration tests for isolation-aware query planning (RFC 0058).
//!
//! Validates that transaction isolation context influences cost
#![expect(clippy::float_cmp, reason = "test code comparing cost adjustments to zero")]
//! adjustments correctly across `PostgreSQL`, `MySQL`, Oracle, `SQLite`,
//! and `DuckDB` backends.

use ra_core::isolation::{BackendKind, IsolationLevel, MultiXactPressure, TransactionContext};
use ra_engine::isolation_cost::{isolation_cost_adjustment, IsolationCostConfig, PlanEstimates};

// ── Helper ───────────────────────────────────────────────────────

fn default_config() -> IsolationCostConfig {
    IsolationCostConfig::default()
}

fn large_seq_scan() -> PlanEstimates {
    PlanEstimates::seq_scan(3000.0, 3_000_000.0)
}

fn small_index_only() -> PlanEstimates {
    PlanEstimates::index_only_scan(47.0, 1000.0)
}

// ── PostgreSQL SSI: index-only scan preferred ────────────────────

#[test]
fn pg_ssi_favors_index_only_over_seq_scan() {
    let txn = TransactionContext::pg_serializable();
    let config = default_config();

    let seq_adj = isolation_cost_adjustment(Some(&txn), &large_seq_scan(), &config);
    let idx_adj = isolation_cost_adjustment(Some(&txn), &small_index_only(), &config);

    assert!(
        seq_adj.cpu > idx_adj.cpu,
        "SSI should strongly penalize seq scan ({:.4}) over \
         index-only scan ({:.4})",
        seq_adj.cpu,
        idx_adj.cpu,
    );
}

#[test]
fn pg_read_committed_no_lock_penalty() {
    let txn = TransactionContext::pg_read_committed();
    let config = default_config();

    let adj = isolation_cost_adjustment(Some(&txn), &large_seq_scan(), &config);

    // READ COMMITTED has no lock penalty, no snapshot bloat,
    // no subxid overflow, no multixact pressure -> zero
    assert_eq!(adj.cpu, 0.0);
}

// ── Snapshot bloat ───────────────────────────────────────────────

#[test]
fn old_snapshot_penalizes_long_running_plans() {
    let mut txn = TransactionContext::pg_serializable();
    txn.snapshot_age_ms = 10_000; // 10 seconds old

    let config = default_config();
    let slow_plan = PlanEstimates {
        estimated_runtime_ms: 5000.0,
        ..PlanEstimates::seq_scan(500.0, 50_000.0)
    };
    let fast_plan = PlanEstimates {
        estimated_runtime_ms: 50.0,
        ..PlanEstimates::index_only_scan(10.0, 100.0)
    };

    let slow_adj = isolation_cost_adjustment(Some(&txn), &slow_plan, &config);
    let fast_adj = isolation_cost_adjustment(Some(&txn), &fast_plan, &config);

    assert!(
        slow_adj.cpu > fast_adj.cpu,
        "Stale snapshot should penalize slow plan ({:.4}) more \
         than fast plan ({:.4})",
        slow_adj.cpu,
        fast_adj.cpu,
    );
}

#[test]
fn fresh_snapshot_no_bloat_penalty() {
    let mut txn = TransactionContext::pg_serializable();
    txn.snapshot_age_ms = 100; // well under threshold

    let config = default_config();
    let plan = large_seq_scan();

    // Only lock penalty should apply, not bloat
    let adj = isolation_cost_adjustment(Some(&txn), &plan, &config);

    // Lock penalty: lock_base_cost * pages
    let expected_lock = config.lock_base_cost * plan.pages_accessed;
    assert!(
        (adj.cpu - expected_lock).abs() < 1e-10,
        "Fresh snapshot should not add bloat penalty; \
         expected {expected_lock:.4}, got {:.4}",
        adj.cpu,
    );
}

// ── SubXID overflow (deep savepoints) ────────────────────────────

#[test]
fn deep_savepoints_penalize_wide_visibility_checks() {
    let mut txn = TransactionContext::pg_read_committed();
    txn.subtransaction_depth = 200;

    let config = default_config();
    let wide = PlanEstimates::seq_scan(500.0, 500_000.0);
    let narrow = PlanEstimates::index_only_scan(5.0, 50.0);

    let wide_adj = isolation_cost_adjustment(Some(&txn), &wide, &config);
    let narrow_adj = isolation_cost_adjustment(Some(&txn), &narrow, &config);

    // SubXID overflow penalty dominates for wide scans
    assert!(wide_adj.cpu > narrow_adj.cpu * 100.0);
}

#[test]
fn savepoint_depth_at_boundary_no_penalty() {
    let mut txn = TransactionContext::pg_read_committed();
    txn.subtransaction_depth = 64; // exactly at limit

    let config = default_config();
    let plan = large_seq_scan();

    let adj = isolation_cost_adjustment(Some(&txn), &plan, &config);
    // READ COMMITTED, depth=64, no overflow -> zero
    assert_eq!(adj.cpu, 0.0);
}

// ── MultiXact pressure ──────────────────────────────────────────

#[test]
fn high_multixact_penalizes_wide_scans() {
    let mut txn = TransactionContext::pg_serializable();
    txn.multi_xact_pressure = MultiXactPressure::High;

    let config = default_config();
    let wide = large_seq_scan();
    let narrow = small_index_only();

    let wide_adj = isolation_cost_adjustment(Some(&txn), &wide, &config);
    let narrow_adj = isolation_cost_adjustment(Some(&txn), &narrow, &config);

    assert!(wide_adj.cpu > narrow_adj.cpu);
}

#[test]
fn low_multixact_no_penalty() {
    let mut txn = TransactionContext::pg_serializable();
    txn.multi_xact_pressure = MultiXactPressure::Low;

    let config = default_config();
    let plan = large_seq_scan();

    // Only lock penalty, no multixact
    let adj = isolation_cost_adjustment(Some(&txn), &plan, &config);
    let expected_lock = config.lock_base_cost * plan.pages_accessed;
    assert!(
        (adj.cpu - expected_lock).abs() < 1e-10,
        "Low MultiXact should not add multixact penalty"
    );
}

// ── MySQL/InnoDB ─────────────────────────────────────────────────

#[test]
fn mysql_serializable_penalizes_all_reads() {
    let mut txn = TransactionContext::mysql_default();
    txn.isolation_level = IsolationLevel::Serializable;

    let config = default_config();
    let plan = PlanEstimates::seq_scan(100.0, 50_000.0);

    let adj = isolation_cost_adjustment(Some(&txn), &plan, &config);
    // MySQL SERIALIZABLE: lock_base_cost * rows
    assert!(adj.cpu > 0.0);
}

#[test]
fn mysql_repeatable_read_gap_lock_on_range_scan() {
    let txn = TransactionContext::mysql_default();
    let config = default_config();

    let range_scan = PlanEstimates::index_scan(50.0, 30.0, 5000.0);
    let point_lookup = PlanEstimates::index_only_scan(1.0, 1.0);

    let range_adj = isolation_cost_adjustment(Some(&txn), &range_scan, &config);
    let point_adj = isolation_cost_adjustment(Some(&txn), &point_lookup, &config);

    assert!(range_adj.cpu > point_adj.cpu);
}

#[test]
fn mysql_read_committed_no_penalty() {
    let mut txn = TransactionContext::mysql_default();
    txn.isolation_level = IsolationLevel::ReadCommitted;

    let config = default_config();
    let plan = large_seq_scan();

    let adj = isolation_cost_adjustment(Some(&txn), &plan, &config);
    assert_eq!(adj.cpu, 0.0);
}

// ── Oracle ───────────────────────────────────────────────────────

#[test]
fn oracle_serializable_undo_pressure() {
    let txn = TransactionContext {
        isolation_level: IsolationLevel::Serializable,
        snapshot_age_ms: 5000,
        subtransaction_depth: 0,
        backend: BackendKind::Oracle,
        uses_ssi: false,
        multi_xact_pressure: MultiXactPressure::Low,
    };
    let config = default_config();
    let plan = PlanEstimates::seq_scan(200.0, 20_000.0);

    let adj = isolation_cost_adjustment(Some(&txn), &plan, &config);
    assert!(
        adj.cpu > 0.0,
        "Oracle SERIALIZABLE should have undo penalty"
    );
}

#[test]
fn oracle_read_committed_no_penalty() {
    let txn = TransactionContext {
        isolation_level: IsolationLevel::ReadCommitted,
        snapshot_age_ms: 0,
        subtransaction_depth: 0,
        backend: BackendKind::Oracle,
        uses_ssi: false,
        multi_xact_pressure: MultiXactPressure::Low,
    };
    let config = default_config();
    let plan = large_seq_scan();

    let adj = isolation_cost_adjustment(Some(&txn), &plan, &config);
    assert_eq!(adj.cpu, 0.0);
}

// ── SQLite ───────────────────────────────────────────────────────

#[test]
fn sqlite_no_isolation_penalty() {
    let txn = TransactionContext {
        isolation_level: IsolationLevel::Serializable,
        snapshot_age_ms: 0,
        subtransaction_depth: 0,
        backend: BackendKind::SQLite,
        uses_ssi: false,
        multi_xact_pressure: MultiXactPressure::Low,
    };
    let config = default_config();
    let plan = large_seq_scan();

    let adj = isolation_cost_adjustment(Some(&txn), &plan, &config);
    assert_eq!(adj.cpu, 0.0);
}

// ── DuckDB ───────────────────────────────────────────────────────

#[test]
fn duckdb_no_isolation_penalty() {
    let txn = TransactionContext {
        isolation_level: IsolationLevel::Serializable,
        snapshot_age_ms: 0,
        subtransaction_depth: 0,
        backend: BackendKind::DuckDB,
        uses_ssi: false,
        multi_xact_pressure: MultiXactPressure::Low,
    };
    let config = default_config();
    let plan = large_seq_scan();

    let adj = isolation_cost_adjustment(Some(&txn), &plan, &config);
    assert_eq!(adj.cpu, 0.0);
}

// ── None context fallback ────────────────────────────────────────

#[test]
fn no_transaction_context_zero_adjustment() {
    let config = default_config();
    let plan = large_seq_scan();

    let adj = isolation_cost_adjustment(None, &plan, &config);
    assert_eq!(adj.cpu, 0.0);
    assert_eq!(adj.io, 0.0);
    assert_eq!(adj.network, 0.0);
    assert_eq!(adj.memory, 0);
}

// ── OptimizerConfig integration ──────────────────────────────────

#[test]
fn optimizer_config_default_has_no_transaction_context() {
    let config = ra_engine::OptimizerConfig::default();
    assert!(config.transaction_context.is_none());
}

#[test]
fn optimizer_config_accepts_transaction_context() {
    let config = ra_engine::OptimizerConfig {
        transaction_context: Some(TransactionContext::pg_serializable()),
        ..ra_engine::OptimizerConfig::default()
    };
    assert!(config.transaction_context.is_some());
}

// ── Custom config weights ────────────────────────────────────────

#[test]
fn custom_weights_affect_penalties() {
    let txn = TransactionContext::pg_serializable();

    let aggressive = IsolationCostConfig {
        lock_base_cost: 1.0, // 100x default
        ..IsolationCostConfig::default()
    };
    let conservative = IsolationCostConfig {
        lock_base_cost: 0.001,
        ..IsolationCostConfig::default()
    };

    let plan = PlanEstimates::seq_scan(100.0, 10000.0);

    let agg_adj = isolation_cost_adjustment(Some(&txn), &plan, &aggressive);
    let con_adj = isolation_cost_adjustment(Some(&txn), &plan, &conservative);

    assert!(
        agg_adj.cpu > con_adj.cpu * 10.0,
        "Aggressive weights ({:.4}) should produce much higher \
         penalty than conservative ({:.4})",
        agg_adj.cpu,
        con_adj.cpu,
    );
}

// ── Compound scenario: TPC-C under SERIALIZABLE ──────────────────

#[test]
fn tpcc_serializable_strongly_prefers_index_scan() {
    // Simulates the TPC-C scenario from the RFC motivation:
    // Seq scan on order_line (3M rows) vs index scan (47 pages)
    let mut txn = TransactionContext::pg_serializable();
    txn.multi_xact_pressure = MultiXactPressure::Medium;

    let config = default_config();

    // Sequential scan: 3M rows across ~23000 pages
    let seq = PlanEstimates::seq_scan(23_000.0, 3_000_000.0);
    // Index-only scan: 47 index pages, 1000 matching rows
    let idx = PlanEstimates::index_only_scan(47.0, 1000.0);

    let seq_adj = isolation_cost_adjustment(Some(&txn), &seq, &config);
    let idx_adj = isolation_cost_adjustment(Some(&txn), &idx, &config);

    // The ratio should be substantial (RFC claims 12% -> 0.3% failure rate)
    let ratio = seq_adj.cpu / idx_adj.cpu;
    assert!(
        ratio > 10.0,
        "TPC-C SERIALIZABLE: seq scan penalty ({:.2}) should be \
         >10x index scan penalty ({:.2}), got ratio {ratio:.1}",
        seq_adj.cpu,
        idx_adj.cpu,
    );
}

// ── Compound scenario: Django savepoint storm ────────────────────

#[test]
fn django_savepoint_storm_penalizes_large_scans() {
    // Simulates the Django ORM scenario from the RFC:
    // 200 nested savepoints, sequential scan vs point lookup
    let mut txn = TransactionContext::pg_read_committed();
    txn.subtransaction_depth = 200;

    let config = default_config();

    let large_scan = PlanEstimates::seq_scan(1000.0, 100_000.0);
    let point_lookup = PlanEstimates::index_only_scan(3.0, 1.0);

    let large_adj = isolation_cost_adjustment(Some(&txn), &large_scan, &config);
    let point_adj = isolation_cost_adjustment(Some(&txn), &point_lookup, &config);

    assert!(
        large_adj.cpu > point_adj.cpu * 1000.0,
        "Django savepoint storm: large scan ({:.2}) should be \
         orders of magnitude worse than point lookup ({:.6})",
        large_adj.cpu,
        point_adj.cpu,
    );
}
