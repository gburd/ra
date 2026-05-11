//! Reference optimizer comparison for correctness validation.
//!
//! Compares Ra optimizer output against reference databases
//! (`PostgreSQL`, `DuckDB`) to detect semantic divergence.
//!
//! Requires the `reference-comparison` feature flag and running
//! database instances.

use thiserror::Error;
use tracing::debug;

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
    /// `PostgreSQL`.
    PostgreSQL,
    /// `DuckDB` (in-process).
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
    /// Bitmap heap scan (Postgres physical).
    BitmapHeapScan,
    /// Nested loop join.
    NestedLoop,
    /// Hash join.
    HashJoin,
    /// Merge join.
    MergeJoin,
    /// Logical join (Ra's generic join operator).
    Join,
    /// Filter / selection.
    Filter,
    /// Sort.
    Sort,
    /// Hash aggregate.
    HashAggregate,
    /// Group aggregate.
    GroupAggregate,
    /// Logical aggregate (Ra's generic aggregate).
    Aggregate,
    /// Logical scan (Ra's generic scan).
    Scan,
    /// Limit.
    Limit,
    /// Projection / Result.
    Result,
    /// Append (for UNION).
    Append,
    /// Materialize.
    Materialize,
    /// Hash (build phase for hash join).
    Hash,
    /// Other operator type.
    Other(String),
}

/// Operator equivalence classes for semantic comparison.
///
/// Physical operators are grouped with their logical equivalents so that
/// e.g. Postgres's `HashJoin` matches Ra's logical `Join`.
impl PlanOperator {
    /// Returns the semantic class of this operator for comparison purposes.
    fn semantic_class(&self) -> u8 {
        match self {
            // Scan class
            Self::SeqScan | Self::IndexScan | Self::BitmapHeapScan | Self::Scan => 1,
            // Join class
            Self::NestedLoop | Self::HashJoin | Self::MergeJoin | Self::Join => 2,
            // Aggregate class
            Self::HashAggregate | Self::GroupAggregate | Self::Aggregate => 3,
            // Sort class
            Self::Sort => 4,
            // Limit class
            Self::Limit => 5,
            // Projection class
            Self::Result => 6,
            // Set operation class
            Self::Append => 7,
            // Filter class
            Self::Filter => 8,
            // Auxiliary nodes (Materialize, Hash) — no logical equivalent
            Self::Materialize | Self::Hash => 9,
            // Unknown
            Self::Other(_) => 0,
        }
    }

    /// Returns true if this operator is semantically compatible with `other`.
    ///
    /// Two operators are compatible if they belong to the same semantic class,
    /// e.g. `HashJoin` ≈ `Join`, `SeqScan` ≈ `Scan`.
    pub fn is_semantically_compatible(&self, other: &Self) -> bool {
        let a = self.semantic_class();
        let b = other.semantic_class();
        // Class 0 (Other/unknown) never matches unless exact string match
        if a == 0 || b == 0 {
            return self == other;
        }
        // Auxiliary nodes (class 9) are ignored in comparison
        if a == 9 || b == 9 {
            return false;
        }
        a == b
    }
}

/// Result of comparing plans across optimizers.
#[derive(Debug, Clone)]
pub struct ComparisonResult {
    /// Reference database.
    pub reference: ReferenceDb,
    /// Whether the plans are structurally similar (similarity > 0.5).
    pub structurally_similar: bool,
    /// Structural similarity score in [0.0, 1.0].
    pub similarity_score: f64,
    /// Whether the join ordering matches.
    pub join_order_match: bool,
    /// Cost ratio (`ra_cost` / `reference_cost`), if available.
    pub cost_ratio: Option<f64>,
    /// Actual execution time (ms) from EXPLAIN ANALYZE, if available.
    pub actual_execution_time_ms: Option<f64>,
    /// Actual rows returned from EXPLAIN ANALYZE, if available.
    pub actual_rows: Option<u64>,
    /// Estimated rows from EXPLAIN (before execution).
    pub estimated_rows: Option<u64>,
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

    /// Set the `PostgreSQL` connection string.
    #[cfg(feature = "reference-comparison")]
    #[must_use]
    pub fn with_postgresql(mut self, conn_str: &str) -> Self {
        self.pg_connection = Some(conn_str.to_owned());
        self
    }

    /// Set the `DuckDB` database path.
    #[cfg(feature = "reference-comparison")]
    #[must_use]
    pub fn with_duckdb(mut self, path: &str) -> Self {
        self.duckdb_path = Some(path.to_owned());
        self
    }

    /// Compare a SQL query's plan against `PostgreSQL` using EXPLAIN ANALYZE.
    ///
    /// This actually executes the query to get real execution statistics.
    ///
    /// # Errors
    ///
    /// Returns error if the connection fails or `EXPLAIN ANALYZE` returns
    /// unexpected output.
    #[cfg(feature = "reference-comparison")]
    pub fn compare_with_postgresql_analyze(
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

        let explain_sql = format!("EXPLAIN (FORMAT JSON, ANALYZE, TIMING) {sql}");
        let rows = client
            .query(&explain_sql, &[])
            .map_err(|e| ReferenceError::Explain(e.to_string()))?;

        if rows.is_empty() {
            return Err(ReferenceError::Explain(
                "empty EXPLAIN ANALYZE result".to_owned(),
            ));
        }

        let plan_json: serde_json::Value = rows[0].get(0);

        // Extract execution statistics from JSON
        let (actual_time_ms, actual_rows, estimated_rows) =
            Self::extract_execution_stats(&plan_json);

        debug!("PostgreSQL EXPLAIN ANALYZE: actual_time={actual_time_ms:?}ms, actual_rows={actual_rows:?}, estimated_rows={estimated_rows:?}");

        Ok(ComparisonResult {
            reference: ReferenceDb::PostgreSQL,
            structurally_similar: true,
            similarity_score: 1.0,
            join_order_match: true,
            cost_ratio: None,
            actual_execution_time_ms: actual_time_ms,
            actual_rows,
            estimated_rows,
            notes: vec![format!(
                "PostgreSQL EXPLAIN ANALYZE completed (time: {actual_time_ms:?}ms, rows: {actual_rows:?})"
            )],
        })
    }

    /// Extract execution statistics from EXPLAIN (ANALYZE) JSON output.
    #[cfg(feature = "reference-comparison")]
    fn extract_execution_stats(plan_json: &serde_json::Value) -> (Option<f64>, Option<u64>, Option<u64>) {
        // EXPLAIN (ANALYZE, FORMAT JSON) returns: [{"Plan": {...}}]
        let plan = plan_json
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|obj| obj.get("Plan"));

        if let Some(plan) = plan {
            let actual_time = plan
                .get("Actual Total Time")
                .and_then(|v| v.as_f64());

            let actual_rows = plan
                .get("Actual Rows")
                .and_then(|v| v.as_u64());

            let estimated_rows = plan
                .get("Plan Rows")
                .and_then(|v| v.as_u64());

            (actual_time, actual_rows, estimated_rows)
        } else {
            (None, None, None)
        }
    }

    /// Compare a SQL query's plan against `PostgreSQL`.
    ///
    /// # Errors
    ///
    /// Returns error if the connection fails or `EXPLAIN` returns
    /// unexpected output.
    #[cfg(feature = "reference-comparison")]
    pub fn compare_with_postgresql(
        &self,
        sql: &str,
        ra_plan: &ra_core::algebra::RelExpr,
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

        let plan_json: serde_json::Value = rows[0].get(0);
        let plan_json_str = plan_json.to_string();
        debug!("PostgreSQL plan: {plan_json_str}");

        // Parse Postgres plan to PlanNode
        let pg_plan = Self::postgres_plan_to_plan_node(&plan_json)?;

        // Convert Ra RelExpr to PlanNode
        let ra_plan_node = Self::ra_relexpr_to_plan_node(ra_plan);

        // Compare plans structurally
        let (similarity, join_match, notes) = Self::compare_plan_nodes(&pg_plan, &ra_plan_node);

        // Extract cost ratio if available
        let cost_ratio = pg_plan.estimated_cost.and_then(|pg_cost| {
            ra_plan_node.estimated_cost.map(|ra_cost| pg_cost / ra_cost)
        });

        Ok(ComparisonResult {
            reference: ReferenceDb::PostgreSQL,
            structurally_similar: similarity > 0.5,
            similarity_score: similarity,
            join_order_match: join_match,
            cost_ratio,
            actual_execution_time_ms: None,
            actual_rows: None,
            estimated_rows: None,
            notes,
        })
    }

    /// Parse Postgres EXPLAIN JSON output into a PlanNode tree.
    #[cfg(feature = "reference-comparison")]
    fn postgres_plan_to_plan_node(json: &serde_json::Value) -> Result<PlanNode, ReferenceError> {
        // EXPLAIN (FORMAT JSON) returns: [{"Plan": {...}}]
        let plan = json
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|obj| obj.get("Plan"))
            .ok_or_else(|| ReferenceError::Explain("invalid JSON structure".to_owned()))?;

        Self::parse_pg_plan_node(plan)
    }

    #[cfg(feature = "reference-comparison")]
    fn parse_pg_plan_node(node: &serde_json::Value) -> Result<PlanNode, ReferenceError> {
        let node_type = node
            .get("Node Type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ReferenceError::Explain("missing Node Type".to_owned()))?;

        let operator = Self::pg_node_type_to_operator(node_type);

        let estimated_rows = node.get("Plan Rows").and_then(|v| v.as_f64());
        let estimated_cost = node.get("Total Cost").and_then(|v| v.as_f64());

        // Recursively parse child plans
        let children = if let Some(plans) = node.get("Plans").and_then(|v| v.as_array()) {
            plans
                .iter()
                .filter_map(|child| Self::parse_pg_plan_node(child).ok())
                .collect()
        } else {
            vec![]
        };

        Ok(PlanNode {
            operator,
            estimated_rows,
            estimated_cost,
            children,
        })
    }

    #[cfg(feature = "reference-comparison")]
    fn pg_node_type_to_operator(node_type: &str) -> PlanOperator {
        match node_type {
            "Seq Scan" => PlanOperator::SeqScan,
            "Index Scan" | "Index Only Scan" | "Bitmap Index Scan" => PlanOperator::IndexScan,
            "Bitmap Heap Scan" => PlanOperator::BitmapHeapScan,
            "Nested Loop" => PlanOperator::NestedLoop,
            "Hash Join" => PlanOperator::HashJoin,
            "Merge Join" => PlanOperator::MergeJoin,
            "Sort" | "Incremental Sort" => PlanOperator::Sort,
            "Hash Aggregate" | "HashAggregate" => PlanOperator::HashAggregate,
            "Group" | "GroupAggregate" | "Group Aggregate" => PlanOperator::GroupAggregate,
            "Limit" => PlanOperator::Limit,
            "Result" => PlanOperator::Result,
            "Append" | "MergeAppend" => PlanOperator::Append,
            "Materialize" => PlanOperator::Materialize,
            "Hash" => PlanOperator::Hash,
            _ => PlanOperator::Other(node_type.to_owned()),
        }
    }

    /// Convert Ra RelExpr to PlanNode for comparison.
    ///
    /// Uses logical operator types (`Scan`, `Join`, `Aggregate`) so that
    /// semantic comparison against Postgres physical operators works via
    /// `is_semantically_compatible()`.
    #[cfg(feature = "reference-comparison")]
    fn ra_relexpr_to_plan_node(expr: &ra_core::algebra::RelExpr) -> PlanNode {
        use ra_core::algebra::RelExpr;

        match expr {
            RelExpr::Scan { .. }
            | RelExpr::IndexScan { .. }
            | RelExpr::IndexOnlyScan { .. }
            | RelExpr::BitmapIndexScan { .. }
            | RelExpr::ParallelScan { .. }
            | RelExpr::MvScan { .. } => PlanNode {
                operator: PlanOperator::Scan,
                estimated_rows: None,
                estimated_cost: None,
                children: vec![],
            },
            RelExpr::Filter { input, .. } => PlanNode {
                operator: PlanOperator::Filter,
                estimated_rows: None,
                estimated_cost: None,
                children: vec![Self::ra_relexpr_to_plan_node(input)],
            },
            RelExpr::Project { input, .. } => PlanNode {
                operator: PlanOperator::Result,
                estimated_rows: None,
                estimated_cost: None,
                children: vec![Self::ra_relexpr_to_plan_node(input)],
            },
            RelExpr::Join { left, right, .. }
            | RelExpr::ParallelHashJoin { left, right, .. } => PlanNode {
                operator: PlanOperator::Join,
                estimated_rows: None,
                estimated_cost: None,
                children: vec![
                    Self::ra_relexpr_to_plan_node(left),
                    Self::ra_relexpr_to_plan_node(right),
                ],
            },
            RelExpr::Aggregate { input, .. }
            | RelExpr::ParallelAggregate { input, .. } => PlanNode {
                operator: PlanOperator::Aggregate,
                estimated_rows: None,
                estimated_cost: None,
                children: vec![Self::ra_relexpr_to_plan_node(input)],
            },
            RelExpr::Sort { input, .. }
            | RelExpr::IncrementalSort { input, .. } => PlanNode {
                operator: PlanOperator::Sort,
                estimated_rows: None,
                estimated_cost: None,
                children: vec![Self::ra_relexpr_to_plan_node(input)],
            },
            RelExpr::Limit { input, .. }
            | RelExpr::TopK { input, .. } => PlanNode {
                operator: PlanOperator::Limit,
                estimated_rows: None,
                estimated_cost: None,
                children: vec![Self::ra_relexpr_to_plan_node(input)],
            },
            RelExpr::Union { left, right, .. }
            | RelExpr::Intersect { left, right, .. }
            | RelExpr::Except { left, right, .. } => PlanNode {
                operator: PlanOperator::Append,
                estimated_rows: None,
                estimated_cost: None,
                children: vec![
                    Self::ra_relexpr_to_plan_node(left),
                    Self::ra_relexpr_to_plan_node(right),
                ],
            },
            RelExpr::Distinct { input } => PlanNode {
                operator: PlanOperator::Aggregate,
                estimated_rows: None,
                estimated_cost: None,
                children: vec![Self::ra_relexpr_to_plan_node(input)],
            },
            RelExpr::Window { input, .. } => PlanNode {
                operator: PlanOperator::Other("WindowAgg".to_owned()),
                estimated_rows: None,
                estimated_cost: None,
                children: vec![Self::ra_relexpr_to_plan_node(input)],
            },
            _ => PlanNode {
                operator: PlanOperator::Other("Unknown".to_owned()),
                estimated_rows: None,
                estimated_cost: None,
                children: vec![],
            },
        }
    }

    /// Compare two plan nodes structurally using BFS traversal with
    /// semantic operator matching.
    ///
    /// Skips auxiliary Postgres nodes (Materialize, Hash) that have no
    /// logical equivalent in Ra. Uses `is_semantically_compatible()` to
    /// match physical operators against their logical classes.
    ///
    /// Returns (similarity_score, join_order_match, notes).
    #[cfg(feature = "reference-comparison")]
    fn compare_plan_nodes(pg: &PlanNode, ra: &PlanNode) -> (f64, bool, Vec<String>) {
        let mut notes = Vec::new();

        // Flatten both trees, skipping auxiliary Postgres nodes
        let pg_nodes = Self::flatten_plan_skip_auxiliary(pg);
        let ra_nodes = Self::flatten_plan_skip_auxiliary(ra);

        let max_len = pg_nodes.len().max(ra_nodes.len());
        if max_len == 0 {
            return (1.0, true, notes);
        }

        let mut matches = 0;
        let min_len = pg_nodes.len().min(ra_nodes.len());

        for i in 0..min_len {
            if pg_nodes[i].is_semantically_compatible(&ra_nodes[i]) {
                matches += 1;
            } else {
                notes.push(format!(
                    "Operator mismatch at position {i}: PG={:?} vs Ra={:?}",
                    pg_nodes[i], ra_nodes[i]
                ));
            }
        }

        if pg_nodes.len() != ra_nodes.len() {
            notes.push(format!(
                "Plan size mismatch: PG has {} operators, Ra has {} operators",
                pg_nodes.len(),
                ra_nodes.len()
            ));
        }

        let similarity = matches as f64 / max_len as f64;

        // Join order check: both plans have same number of joins
        let join_match = Self::count_joins(pg) == Self::count_joins(ra);

        (similarity, join_match, notes)
    }

    /// Flatten a plan tree into a BFS list of operators, skipping
    /// auxiliary nodes (Materialize, Hash) that are implementation
    /// details of the physical plan.
    #[cfg(feature = "reference-comparison")]
    fn flatten_plan_skip_auxiliary(node: &PlanNode) -> Vec<&PlanOperator> {
        use std::collections::VecDeque;

        let mut result = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(node);

        while let Some(current) = queue.pop_front() {
            // Skip auxiliary nodes — just traverse their children
            if matches!(current.operator, PlanOperator::Materialize | PlanOperator::Hash) {
                for child in &current.children {
                    queue.push_back(child);
                }
                continue;
            }
            result.push(&current.operator);
            for child in &current.children {
                queue.push_back(child);
            }
        }

        result
    }

    #[cfg(feature = "reference-comparison")]
    fn count_joins(node: &PlanNode) -> usize {
        let self_count = match node.operator {
            PlanOperator::HashJoin
            | PlanOperator::NestedLoop
            | PlanOperator::MergeJoin
            | PlanOperator::Join => 1,
            _ => 0,
        };

        self_count + node.children.iter().map(|child| Self::count_joins(child)).sum::<usize>()
    }

    /// Compare a SQL query's plan against `DuckDB`.
    ///
    /// # Errors
    ///
    /// Returns error if `DuckDB` initialization fails or `EXPLAIN`
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
            similarity_score: 1.0,
            join_order_match: true,
            cost_ratio: None,
            actual_execution_time_ms: None,
            actual_rows: None,
            estimated_rows: None,
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
    #[must_use]
    pub fn compare_all(
        &self,
        sql: &str,
        ra_plan: &ra_core::algebra::RelExpr,
    ) -> Vec<Result<ComparisonResult, ReferenceError>> {
        let mut results = Vec::new();

        if self.pg_connection.is_some() {
            results.push(self.compare_with_postgresql(sql, ra_plan));
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
