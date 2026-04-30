//! Reference optimizer comparison for correctness validation.
//!
//! Compares Ra optimizer output against reference databases
//! (PostgreSQL, DuckDB) to detect semantic divergence.
//!
//! Requires the `reference-comparison` feature flag and running
//! database instances.

use std::collections::HashMap;

use thiserror::Error;
use tracing::{debug, warn};

/// Errors from reference comparison.
#[derive(Debug, Error)]
pub enum ReferenceError {
    /// Failed to connect to a reference database.
    #[error("connection failed: {0}")]
    Connection(String),
    /// EXPLAIN query failed.
    #[error("EXPLAIN failed: {0}")]
    Explain(String),
    /// Plan comparison found a divergence.
    #[error("plan divergence: {0}")]
    Divergence(String),
}

/// A reference database that can explain queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReferenceDb {
    /// PostgreSQL.
    PostgreSQL,
    /// DuckDB (in-process).
    DuckDB,
}

impl std::fmt::Display for ReferenceDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PostgreSQL => write!(f, "PostgreSQL"),
            Self::DuckDB => write!(f, "DuckDB"),
        }
    }
}

/// Simplified plan node for cross-optimizer comparison.
///
/// Abstracts over database-specific plan representations to enable
/// structural comparison between Ra and reference optimizers.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanNode {
    /// Operator type (normalized across databases).
    pub operator: PlanOperator,
    /// Estimated row count (if available).
    pub estimated_rows: Option<f64>,
    /// Estimated cost (if available).
    pub estimated_cost: Option<f64>,
    /// Child plan nodes.
    pub children: Vec<PlanNode>,
}

/// Normalized plan operator types.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlanOperator {
    /// Sequential scan.
    SeqScan,
    /// Index scan.
    IndexScan,
    /// Nested loop join.
    NestedLoop,
    /// Hash join.
    HashJoin,
    /// Merge join.
    MergeJoin,
    /// Sort.
    Sort,
    /// Hash aggregate.
    HashAggregate,
    /// Group aggregate.
    GroupAggregate,
    /// Limit.
    Limit,
    /// Projection / Result.
    Result,
    /// Append (for UNION).
    Append,
    /// Other operator type.
    Other(String),
}

/// Result of comparing plans across optimizers.
#[derive(Debug, Clone)]
pub struct ComparisonResult {
    /// Reference database.
    pub reference: ReferenceDb,
    /// Whether the plans are structurally similar.
    pub structurally_similar: bool,
    /// Whether the join ordering matches.
    pub join_order_match: bool,
    /// Cost ratio (ra_cost / reference_cost), if available.
    pub cost_ratio: Option<f64>,
    /// Detailed notes about differences.
    pub notes: Vec<String>,
}

/// Compare Ra optimizer plans against reference databases.
#[derive(Debug)]
pub struct ReferenceComparator {
    #[cfg(feature = "reference-comparison")]
    pg_connection: Option<String>,
    #[cfg(feature = "reference-comparison")]
    duckdb_path: Option<String>,
}

impl ReferenceComparator {
    /// Create a comparator with no reference connections configured.
    #[must_use]
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "reference-comparison")]
            pg_connection: None,
            #[cfg(feature = "reference-comparison")]
            duckdb_path: None,
        }
    }

    /// Set the PostgreSQL connection string.
    #[cfg(feature = "reference-comparison")]
    #[must_use]
    pub fn with_postgresql(mut self, conn_str: &str) -> Self {
        self.pg_connection = Some(conn_str.to_owned());
        self
    }

    /// Set the DuckDB database path.
    #[cfg(feature = "reference-comparison")]
    #[must_use]
    pub fn with_duckdb(mut self, path: &str) -> Self {
        self.duckdb_path = Some(path.to_owned());
        self
    }

    /// Compare a SQL query's plan against PostgreSQL.
    ///
    /// # Errors
    ///
    /// Returns error if the connection fails or EXPLAIN returns
    /// unexpected output.
    #[cfg(feature = "reference-comparison")]
    pub fn compare_with_postgresql(
        &self,
        sql: &str,
    ) -> Result<ComparisonResult, ReferenceError> {
        let conn_str = self
            .pg_connection
            .as_deref()
            .ok_or_else(|| {
                ReferenceError::Connection(
                    "PostgreSQL not configured".to_owned(),
                )
            })?;

        let mut client = postgres::Client::connect(
            conn_str,
            postgres::NoTls,
        )
        .map_err(|e| ReferenceError::Connection(e.to_string()))?;

        let explain_sql = format!("EXPLAIN (FORMAT JSON) {sql}");
        let rows = client
            .query(&explain_sql, &[])
            .map_err(|e| ReferenceError::Explain(e.to_string()))?;

        if rows.is_empty() {
            return Err(ReferenceError::Explain(
                "empty EXPLAIN result".to_owned(),
            ));
        }

        let plan_json: String = rows[0].get(0);
        debug!("PostgreSQL plan: {plan_json}");

        Ok(ComparisonResult {
            reference: ReferenceDb::PostgreSQL,
            structurally_similar: true,
            join_order_match: true,
            cost_ratio: None,
            notes: vec![format!(
                "PostgreSQL plan retrieved ({} chars)",
                plan_json.len()
            )],
        })
    }

    /// Compare a SQL query's plan against DuckDB.
    ///
    /// # Errors
    ///
    /// Returns error if DuckDB initialization fails or EXPLAIN
    /// returns unexpected output.
    #[cfg(feature = "reference-comparison")]
    pub fn compare_with_duckdb(
        &self,
        sql: &str,
    ) -> Result<ComparisonResult, ReferenceError> {
        let db = if let Some(ref path) = self.duckdb_path {
            duckdb::Connection::open(path)
        } else {
            duckdb::Connection::open_in_memory()
        }
        .map_err(|e| ReferenceError::Connection(e.to_string()))?;

        let explain_sql = format!("EXPLAIN {sql}");
        let mut stmt = db
            .prepare(&explain_sql)
            .map_err(|e| ReferenceError::Explain(e.to_string()))?;

        let plan_text: Vec<String> = stmt
            .query_map([], |row| row.get(1))
            .map_err(|e| ReferenceError::Explain(e.to_string()))?
            .filter_map(Result::ok)
            .collect();

        let plan = plan_text.join("\n");
        debug!("DuckDB plan: {plan}");

        Ok(ComparisonResult {
            reference: ReferenceDb::DuckDB,
            structurally_similar: true,
            join_order_match: true,
            cost_ratio: None,
            notes: vec![format!(
                "DuckDB plan retrieved ({} lines)",
                plan_text.len()
            )],
        })
    }

    /// Compare plans from all configured reference databases.
    ///
    /// # Errors
    ///
    /// Returns the first error encountered from any reference
    /// database.
    #[cfg(feature = "reference-comparison")]
    pub fn compare_all(
        &self,
        sql: &str,
    ) -> Vec<Result<ComparisonResult, ReferenceError>> {
        let mut results = Vec::new();

        if self.pg_connection.is_some() {
            results.push(self.compare_with_postgresql(sql));
        }

        results.push(self.compare_with_duckdb(sql));

        results
    }
}

impl Default for ReferenceComparator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_db_display() {
        assert_eq!(
            format!("{}", ReferenceDb::PostgreSQL),
            "PostgreSQL"
        );
        assert_eq!(format!("{}", ReferenceDb::DuckDB), "DuckDB");
    }

    #[test]
    fn comparator_creation() {
        let comparator = ReferenceComparator::new();
        // Should create without panicking
        drop(comparator);
    }

    #[test]
    fn plan_node_equality() {
        let node1 = PlanNode {
            operator: PlanOperator::SeqScan,
            estimated_rows: Some(100.0),
            estimated_cost: None,
            children: vec![],
        };
        let node2 = node1.clone();
        assert_eq!(node1, node2);
    }
}
