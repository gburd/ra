//! Transaction isolation context for isolation-aware query planning.
//!
//! Provides the [`TransactionContext`] fact that captures the current
//! transaction's isolation level, snapshot state, subtransaction depth,
//! and backend-specific flags. The optimizer uses this context to apply
//! cost penalties that favor plans with smaller lock footprints under
//! strict isolation and avoid MVCC bloat under long-running snapshots.
//!
//! See RFC 0058 for the full design rationale.

use serde::{Deserialize, Serialize};

/// SQL standard transaction isolation levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum IsolationLevel {
    /// May see uncommitted changes from other transactions.
    ReadUncommitted,
    /// Sees only committed data; each statement gets a fresh snapshot.
    #[default]
    ReadCommitted,
    /// Snapshot taken at transaction start; no non-repeatable reads.
    RepeatableRead,
    /// Full serializability via SSI or 2PL depending on backend.
    Serializable,
}

impl IsolationLevel {
    /// Whether this level uses predicate locks (`SIRead`) on `PostgreSQL`.
    #[must_use]
    pub fn uses_predicate_locks_pg(&self) -> bool {
        matches!(self, Self::Serializable)
    }

    /// Whether this level holds a single snapshot for the transaction.
    #[must_use]
    pub fn holds_transaction_snapshot(&self) -> bool {
        matches!(self, Self::RepeatableRead | Self::Serializable)
    }

    /// Whether this level uses gap locks on MySQL/InnoDB.
    #[must_use]
    pub fn uses_gap_locks_mysql(&self) -> bool {
        matches!(self, Self::RepeatableRead | Self::Serializable)
    }
}

impl std::fmt::Display for IsolationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadUncommitted => write!(f, "READ UNCOMMITTED"),
            Self::ReadCommitted => write!(f, "READ COMMITTED"),
            Self::RepeatableRead => write!(f, "REPEATABLE READ"),
            Self::Serializable => write!(f, "SERIALIZABLE"),
        }
    }
}

/// Target database backend, determining isolation semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum BackendKind {
    /// `PostgreSQL`: SSI for SERIALIZABLE, MVCC snapshots otherwise.
    #[default]
    PostgreSQL,
    /// MySQL/InnoDB: gap locks for REPEATABLE READ, next-key locks
    /// for SERIALIZABLE (all reads become SELECT ... FOR SHARE).
    MySQLInnoDB,
    /// Oracle: no READ UNCOMMITTED, undo-based read consistency.
    Oracle,
    /// `SQLite`: single-writer, journal or WAL mode.
    SQLite,
    /// `DuckDB`: MVCC with optimistic concurrency; no lock concerns.
    DuckDB,
}

impl std::fmt::Display for BackendKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PostgreSQL => write!(f, "PostgreSQL"),
            Self::MySQLInnoDB => write!(f, "MySQL/InnoDB"),
            Self::Oracle => write!(f, "Oracle"),
            Self::SQLite => write!(f, "SQLite"),
            Self::DuckDB => write!(f, "DuckDB"),
        }
    }
}

/// `MultiXact` pressure indicator for `PostgreSQL`.
///
/// When multiple transactions hold shared locks on the same tuple,
/// `PostgreSQL` stores the lock set in a `MultiXactId`. High pressure
/// can stall vacuuming and degrade performance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum MultiXactPressure {
    /// Few active multi-xacts; no concern.
    #[default]
    Low,
    /// Approaching thresholds that may delay vacuum.
    Medium,
    /// High multi-xact member count; avoid shared tuple locks.
    High,
}

/// Transaction-level metadata that influences plan selection.
///
/// The optimizer uses this context to apply cost penalties for lock
/// footprint, snapshot bloat, subtransaction overhead, and `MultiXact`
/// pressure. When absent (`None` in `OptimizerConfig`), all penalties
/// are zero and the optimizer behaves as before.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionContext {
    /// SQL standard isolation level.
    pub isolation_level: IsolationLevel,
    /// Milliseconds since the transaction's snapshot was acquired.
    pub snapshot_age_ms: u64,
    /// Current subtransaction nesting depth.
    /// `PostgreSQL` `SubXID` cache holds 64 entries; beyond that,
    /// `XidInMVCCSnapshot` degrades to O(n).
    pub subtransaction_depth: u32,
    /// Target database backend.
    pub backend: BackendKind,
    /// Whether SSI (Serializable Snapshot Isolation) is active.
    /// True for `PostgreSQL` SERIALIZABLE since 9.1.
    pub uses_ssi: bool,
    /// Current `MultiXact` pressure level.
    pub multi_xact_pressure: MultiXactPressure,
}

impl TransactionContext {
    /// `PostgreSQL` `SubXID` cache size. Beyond this depth, MVCC
    /// visibility checks degrade from O(1) to O(n).
    pub const PG_SUBXID_CACHE_LIMIT: u32 = 64;

    /// Create a default READ COMMITTED context for `PostgreSQL`.
    #[must_use]
    pub fn pg_read_committed() -> Self {
        Self {
            isolation_level: IsolationLevel::ReadCommitted,
            snapshot_age_ms: 0,
            subtransaction_depth: 0,
            backend: BackendKind::PostgreSQL,
            uses_ssi: false,
            multi_xact_pressure: MultiXactPressure::Low,
        }
    }

    /// Create a SERIALIZABLE context for `PostgreSQL` with SSI.
    #[must_use]
    pub fn pg_serializable() -> Self {
        Self {
            isolation_level: IsolationLevel::Serializable,
            snapshot_age_ms: 0,
            subtransaction_depth: 0,
            backend: BackendKind::PostgreSQL,
            uses_ssi: true,
            multi_xact_pressure: MultiXactPressure::Low,
        }
    }

    /// Create a default context for MySQL/InnoDB.
    #[must_use]
    pub fn mysql_default() -> Self {
        Self {
            isolation_level: IsolationLevel::RepeatableRead,
            snapshot_age_ms: 0,
            subtransaction_depth: 0,
            backend: BackendKind::MySQLInnoDB,
            uses_ssi: false,
            multi_xact_pressure: MultiXactPressure::Low,
        }
    }

    /// Whether the subtransaction depth exceeds the `PostgreSQL`
    /// `SubXID` cache, triggering degraded MVCC visibility checks.
    #[must_use]
    pub fn has_subxid_overflow(&self) -> bool {
        self.subtransaction_depth > Self::PG_SUBXID_CACHE_LIMIT
    }
}

impl Default for TransactionContext {
    fn default() -> Self {
        Self::pg_read_committed()
    }
}

#[expect(clippy::expect_used, reason = "test code")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_pg_read_committed() {
        let ctx = TransactionContext::default();
        assert_eq!(ctx.isolation_level, IsolationLevel::ReadCommitted);
        assert_eq!(ctx.backend, BackendKind::PostgreSQL);
        assert!(!ctx.uses_ssi);
        assert!(!ctx.has_subxid_overflow());
    }

    #[test]
    fn pg_serializable_has_ssi() {
        let ctx = TransactionContext::pg_serializable();
        assert_eq!(ctx.isolation_level, IsolationLevel::Serializable);
        assert!(ctx.uses_ssi);
    }

    #[test]
    fn subxid_overflow_at_boundary() {
        let mut ctx = TransactionContext::pg_read_committed();
        ctx.subtransaction_depth = 64;
        assert!(!ctx.has_subxid_overflow());
        ctx.subtransaction_depth = 65;
        assert!(ctx.has_subxid_overflow());
    }

    #[test]
    fn isolation_level_display() {
        assert_eq!(IsolationLevel::Serializable.to_string(), "SERIALIZABLE");
        assert_eq!(IsolationLevel::ReadCommitted.to_string(), "READ COMMITTED");
    }

    #[test]
    fn predicate_locks_only_serializable() {
        assert!(IsolationLevel::Serializable.uses_predicate_locks_pg());
        assert!(!IsolationLevel::RepeatableRead.uses_predicate_locks_pg());
        assert!(!IsolationLevel::ReadCommitted.uses_predicate_locks_pg());
    }

    #[test]
    fn transaction_snapshot_levels() {
        assert!(IsolationLevel::Serializable.holds_transaction_snapshot());
        assert!(IsolationLevel::RepeatableRead.holds_transaction_snapshot());
        assert!(!IsolationLevel::ReadCommitted.holds_transaction_snapshot());
    }

    #[test]
    fn mysql_gap_locks() {
        assert!(IsolationLevel::RepeatableRead.uses_gap_locks_mysql());
        assert!(IsolationLevel::Serializable.uses_gap_locks_mysql());
        assert!(!IsolationLevel::ReadCommitted.uses_gap_locks_mysql());
    }

    #[test]
    fn mysql_default_is_repeatable_read() {
        let ctx = TransactionContext::mysql_default();
        assert_eq!(ctx.isolation_level, IsolationLevel::RepeatableRead);
        assert_eq!(ctx.backend, BackendKind::MySQLInnoDB);
    }

    #[test]
    fn backend_display() {
        assert_eq!(BackendKind::PostgreSQL.to_string(), "PostgreSQL");
        assert_eq!(BackendKind::MySQLInnoDB.to_string(), "MySQL/InnoDB");
    }

    #[test]
    fn serde_roundtrip() {
        let ctx = TransactionContext::pg_serializable();
        let json = serde_json::to_string(&ctx).expect("serialize should succeed");
        let deserialized: TransactionContext =
            serde_json::from_str(&json).expect("deserialize should succeed");
        assert_eq!(ctx, deserialized);
    }
}
