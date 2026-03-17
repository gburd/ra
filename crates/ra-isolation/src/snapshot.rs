//! Snapshot visibility queries for verifying isolation levels.
//!
//! Different isolation levels provide different visibility guarantees
//! for concurrent modifications. This module provides queries that
//! probe what each session can see at different points in time,
//! enabling automated detection of isolation anomalies.

use serde::{Deserialize, Serialize};

use crate::adapter::{AdapterError, QueryResult};
use crate::session::Session;

/// A snapshot visibility query that checks what a session can see.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotQuery {
    /// Description of what this query is checking.
    pub description: String,
    /// The SQL to execute.
    pub sql: String,
    /// Expected result for READ COMMITTED isolation.
    pub expected_read_committed: Option<ExpectedResult>,
    /// Expected result for REPEATABLE READ isolation.
    pub expected_repeatable_read: Option<ExpectedResult>,
    /// Expected result for SERIALIZABLE isolation.
    pub expected_serializable: Option<ExpectedResult>,
}

/// Expected result for a snapshot visibility query.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExpectedResult {
    /// Whether the query should return rows.
    pub has_rows: bool,
    /// Expected number of rows (if known).
    pub row_count: Option<usize>,
    /// Expected column values in the first row (if known).
    pub first_row: Option<Vec<String>>,
}

/// The isolation level being tested.
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash,
)]
pub enum IsolationLevel {
    /// Read uncommitted (may see dirty reads).
    ReadUncommitted,
    /// Read committed (sees only committed data).
    ReadCommitted,
    /// Repeatable read (snapshot at transaction start).
    RepeatableRead,
    /// Serializable (strictest isolation).
    Serializable,
}

impl IsolationLevel {
    /// Return the SQL SET TRANSACTION statement for this level.
    #[must_use]
    pub fn set_transaction_sql(&self) -> &'static str {
        match self {
            Self::ReadUncommitted => {
                "SET TRANSACTION ISOLATION LEVEL READ UNCOMMITTED"
            }
            Self::ReadCommitted => {
                "SET TRANSACTION ISOLATION LEVEL READ COMMITTED"
            }
            Self::RepeatableRead => {
                "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ"
            }
            Self::Serializable => {
                "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE"
            }
        }
    }

    /// Return a human-readable name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::ReadUncommitted => "READ UNCOMMITTED",
            Self::ReadCommitted => "READ COMMITTED",
            Self::RepeatableRead => "REPEATABLE READ",
            Self::Serializable => "SERIALIZABLE",
        }
    }
}

/// Result of checking snapshot visibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisibilityResult {
    /// The query that was executed.
    pub query: SnapshotQuery,
    /// The actual result from the database.
    pub actual: QueryResult,
    /// Whether the result matched expectations for the isolation level.
    pub matches_expected: Option<bool>,
    /// The isolation level under which this was tested.
    pub isolation_level: IsolationLevel,
}

/// Execute a snapshot visibility query against a session.
///
/// # Errors
///
/// Returns `AdapterError` if the query fails.
pub fn check_visibility(
    session: &mut Session,
    query: &SnapshotQuery,
    level: IsolationLevel,
) -> Result<VisibilityResult, AdapterError> {
    let actual = session.execute_sql(&query.sql)?;

    let expected = match level {
        IsolationLevel::ReadUncommitted
        | IsolationLevel::ReadCommitted => {
            query.expected_read_committed.as_ref()
        }
        IsolationLevel::RepeatableRead => {
            query.expected_repeatable_read.as_ref()
        }
        IsolationLevel::Serializable => {
            query.expected_serializable.as_ref()
        }
    };

    let matches_expected = expected.map(|exp| {
        let row_count_ok = exp
            .row_count
            .map_or(true, |expected_count| {
                actual.rows.len() == expected_count
            });
        let has_rows_ok = exp.has_rows != actual.rows.is_empty();
        let first_row_ok =
            exp.first_row.as_ref().map_or(true, |expected_row| {
                actual.rows.first() == Some(expected_row)
            });
        row_count_ok && has_rows_ok && first_row_ok
    });

    Ok(VisibilityResult {
        query: query.clone(),
        actual,
        matches_expected,
        isolation_level: level,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolation_level_sql() {
        assert!(IsolationLevel::ReadCommitted
            .set_transaction_sql()
            .contains("READ COMMITTED"));
        assert!(IsolationLevel::Serializable
            .set_transaction_sql()
            .contains("SERIALIZABLE"));
    }

    #[test]
    fn isolation_level_names() {
        assert_eq!(
            IsolationLevel::RepeatableRead.name(),
            "REPEATABLE READ"
        );
    }
}
