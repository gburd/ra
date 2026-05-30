//! EXPLAIN plan types and parsers for `PostgreSQL`, `MySQL`, and `SQLite`.
//!
//! Each database produces EXPLAIN output in a different format:
//! - `PostgreSQL`: `EXPLAIN (FORMAT JSON)` returns a JSON array of plan nodes.
//! - `MySQL`: `EXPLAIN FORMAT=JSON` returns a JSON object with `query_block`.
//! - `SQLite`: `EXPLAIN QUERY PLAN` returns rows with `(id, parent, notused, detail)`.
//!
//! This module provides a unified [`ExplainPlan`] tree and parsers that
//! convert each format into it.

use serde::{Deserialize, Serialize};
use std::fmt::Write;

use crate::error::MetadataError;

/// A parsed EXPLAIN plan tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExplainPlan {
    /// Root node of the plan tree.
    pub root: ExplainNode,
    /// Original SQL query (if available).
    pub query: Option<String>,
    /// Total estimated cost (if available).
    pub total_cost: Option<f64>,
    /// Total estimated rows returned.
    pub total_rows: Option<f64>,
}

/// A single node in the EXPLAIN plan tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExplainNode {
    /// Operator type for this node.
    pub node_type: NodeType,
    /// Join strategy (only present for join nodes).
    pub join_type: Option<JoinType>,
    /// Table or relation name.
    pub relation: Option<String>,
    /// Index used (if any).
    pub index_name: Option<String>,
    /// Estimated startup cost.
    pub startup_cost: Option<f64>,
    /// Estimated total cost.
    pub total_cost: Option<f64>,
    /// Estimated number of output rows.
    pub estimated_rows: Option<f64>,
    /// Estimated average row width in bytes.
    pub estimated_width: Option<u32>,
    /// Filter condition applied at this node.
    pub filter: Option<String>,
    /// Scan direction (Forward, Backward).
    pub scan_direction: Option<String>,
    /// Raw detail string from the database.
    pub raw_detail: Option<String>,
    /// Child plan nodes.
    pub children: Vec<ExplainNode>,
}

/// Operator types found in query plans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeType {
    /// Sequential (full table) scan.
    SeqScan,
    /// Index scan (traverses index, fetches heap rows).
    IndexScan,
    /// Index-only scan (no heap access needed).
    IndexOnlyScan,
    /// Bitmap index scan (builds bitmap of matching pages).
    BitmapIndexScan,
    /// Bitmap heap scan (reads pages from bitmap).
    BitmapHeapScan,
    /// Nested-loop join.
    NestedLoop,
    /// Hash join.
    HashJoin,
    /// Merge (sort-merge) join.
    MergeJoin,
    /// Hash aggregate.
    HashAggregate,
    /// Group aggregate (streaming).
    GroupAggregate,
    /// Sort operator.
    Sort,
    /// Limit operator.
    Limit,
    /// Materialize (buffer intermediate results).
    Materialize,
    /// Append (UNION ALL).
    Append,
    /// Merge append (UNION ALL with pre-sorted inputs).
    MergeAppend,
    /// Subquery scan.
    SubqueryScan,
    /// Function scan (e.g., `generate_series`).
    FunctionScan,
    /// CTE scan (WITH clause).
    CteScan,
    /// Values scan (VALUES clause).
    ValuesScan,
    /// Result node (single-row, no table).
    Result,
    /// Unique (deduplication).
    Unique,
    /// Gather (parallel query root).
    Gather,
    /// Gather merge (parallel, preserving order).
    GatherMerge,
    /// Hash (build side of hash join).
    Hash,
    /// Window aggregate.
    WindowAgg,
    /// Set operation (INTERSECT, EXCEPT).
    SetOp,
    /// Foreign scan (FDW).
    ForeignScan,
    /// An operator not explicitly modeled.
    Other,
}

/// Join strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JoinType {
    /// Inner join.
    Inner,
    /// Left outer join.
    Left,
    /// Right outer join.
    Right,
    /// Full outer join.
    Full,
    /// Semi-join (EXISTS).
    Semi,
    /// Anti-join (NOT EXISTS).
    Anti,
    /// Cross join.
    Cross,
}

// ---- Parsers ----

/// Parse `PostgreSQL` `EXPLAIN (FORMAT JSON)` output.
///
/// `PostgreSQL` returns a JSON array with a single element containing the
/// top-level plan node under the key `"Plan"`.
///
/// # Errors
///
/// Returns [`MetadataError::ExplainParse`] if the input is not valid JSON
/// or does not match the expected `PostgreSQL` EXPLAIN format.
pub fn parse_postgres_explain(json: &str) -> Result<ExplainPlan, MetadataError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| MetadataError::ExplainParse {
            message: format!("invalid JSON: {e}"),
        })?;

    let plans = value
        .as_array()
        .ok_or_else(|| MetadataError::ExplainParse {
            message: "expected JSON array".to_string(),
        })?;

    let first = plans.first().ok_or_else(|| MetadataError::ExplainParse {
        message: "empty plan array".to_string(),
    })?;

    let plan_obj = first
        .get("Plan")
        .ok_or_else(|| MetadataError::ExplainParse {
            message: "missing 'Plan' key".to_string(),
        })?;

    let root = parse_pg_node(plan_obj)?;
    let total_cost = root.total_cost;
    let total_rows = root.estimated_rows;

    Ok(ExplainPlan {
        root,
        query: None,
        total_cost,
        total_rows,
    })
}

fn parse_pg_node(value: &serde_json::Value) -> Result<ExplainNode, MetadataError> {
    let node_type_str = value
        .get("Node Type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Other");

    let join_type_str = value.get("Join Type").and_then(serde_json::Value::as_str);

    let children_val = value.get("Plans").and_then(serde_json::Value::as_array);
    let mut children = Vec::new();
    if let Some(plans) = children_val {
        for child in plans {
            children.push(parse_pg_node(child)?);
        }
    }

    Ok(ExplainNode {
        node_type: parse_pg_node_type(node_type_str),
        join_type: join_type_str.map(parse_join_type),
        relation: value
            .get("Relation Name")
            .and_then(serde_json::Value::as_str)
            .map(String::from),
        index_name: value
            .get("Index Name")
            .and_then(serde_json::Value::as_str)
            .map(String::from),
        startup_cost: value
            .get("Startup Cost")
            .and_then(serde_json::Value::as_f64),
        total_cost: value.get("Total Cost").and_then(serde_json::Value::as_f64),
        estimated_rows: value.get("Plan Rows").and_then(serde_json::Value::as_f64),
        estimated_width: value
            .get("Plan Width")
            .and_then(serde_json::Value::as_u64)
            .map(|w| w as u32),
        filter: value
            .get("Filter")
            .and_then(serde_json::Value::as_str)
            .map(String::from),
        scan_direction: value
            .get("Scan Direction")
            .and_then(serde_json::Value::as_str)
            .map(String::from),
        raw_detail: None,
        children,
    })
}

fn parse_pg_node_type(s: &str) -> NodeType {
    match s {
        "Seq Scan" => NodeType::SeqScan,
        "Index Scan" => NodeType::IndexScan,
        "Index Only Scan" => NodeType::IndexOnlyScan,
        "Bitmap Index Scan" => NodeType::BitmapIndexScan,
        "Bitmap Heap Scan" => NodeType::BitmapHeapScan,
        "Nested Loop" => NodeType::NestedLoop,
        "Hash Join" => NodeType::HashJoin,
        "Merge Join" => NodeType::MergeJoin,
        "Hash Aggregate" | "HashAggregate" => NodeType::HashAggregate,
        "Group Aggregate" | "GroupAggregate" => NodeType::GroupAggregate,
        "Sort" => NodeType::Sort,
        "Limit" => NodeType::Limit,
        "Materialize" => NodeType::Materialize,
        "Append" => NodeType::Append,
        "Merge Append" => NodeType::MergeAppend,
        "Subquery Scan" => NodeType::SubqueryScan,
        "Function Scan" => NodeType::FunctionScan,
        "CTE Scan" => NodeType::CteScan,
        "Values Scan" => NodeType::ValuesScan,
        "Result" => NodeType::Result,
        "Unique" => NodeType::Unique,
        "Gather" => NodeType::Gather,
        "Gather Merge" => NodeType::GatherMerge,
        "Hash" => NodeType::Hash,
        "WindowAgg" => NodeType::WindowAgg,
        "SetOp" => NodeType::SetOp,
        "Foreign Scan" => NodeType::ForeignScan,
        _ => NodeType::Other,
    }
}

fn parse_join_type(s: &str) -> JoinType {
    match s {
        "Left" => JoinType::Left,
        "Right" => JoinType::Right,
        "Full" => JoinType::Full,
        "Semi" => JoinType::Semi,
        "Anti" => JoinType::Anti,
        "Cross" | "Cross Join" => JoinType::Cross,
        // "Inner" and unrecognized types default to Inner.
        _ => JoinType::Inner,
    }
}

/// Parse `MySQL` `EXPLAIN FORMAT=JSON` output.
///
/// `MySQL` wraps the plan in `{"query_block": {...}}`. The structure uses
/// `"table"` for access info and `"nested_loop"` for joins.
///
/// # Errors
///
/// Returns [`MetadataError::ExplainParse`] if the input is not valid JSON
/// or does not match the expected `MySQL` EXPLAIN format.
pub fn parse_mysql_explain(json: &str) -> Result<ExplainPlan, MetadataError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| MetadataError::ExplainParse {
            message: format!("invalid JSON: {e}"),
        })?;

    let query_block = value
        .get("query_block")
        .ok_or_else(|| MetadataError::ExplainParse {
            message: "missing 'query_block' key".to_string(),
        })?;

    let root = parse_mysql_query_block(query_block)?;
    let total_cost = query_block
        .get("cost_info")
        .and_then(|ci| ci.get("query_cost"))
        .and_then(serde_json::Value::as_str)
        .and_then(|s| s.parse::<f64>().ok());
    let total_rows = root.estimated_rows;

    Ok(ExplainPlan {
        root,
        query: None,
        total_cost,
        total_rows,
    })
}

fn parse_mysql_query_block(block: &serde_json::Value) -> Result<ExplainNode, MetadataError> {
    if let Some(nested_loop) = block
        .get("nested_loop")
        .and_then(serde_json::Value::as_array)
    {
        let mut children = Vec::new();
        for item in nested_loop {
            if let Some(table) = item.get("table") {
                children.push(parse_mysql_table_node(table));
            }
        }

        if children.len() == 1 {
            return Ok(children.into_iter().next().unwrap_or_else(unreachable_node));
        }

        return Ok(ExplainNode {
            node_type: NodeType::NestedLoop,
            join_type: Some(JoinType::Inner),
            relation: None,
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: None,
            children,
        });
    }

    if let Some(table) = block.get("table") {
        return Ok(parse_mysql_table_node(table));
    }

    if let Some(ordering) = block.get("ordering_operation") {
        let mut node = parse_mysql_query_block(ordering)?;
        node.node_type = NodeType::Sort;
        return Ok(node);
    }

    if let Some(grouping) = block.get("grouping_operation") {
        let mut node = parse_mysql_query_block(grouping)?;
        node.node_type = NodeType::HashAggregate;
        return Ok(node);
    }

    Ok(ExplainNode {
        node_type: NodeType::Result,
        join_type: None,
        relation: None,
        index_name: None,
        startup_cost: None,
        total_cost: None,
        estimated_rows: None,
        estimated_width: None,
        filter: None,
        scan_direction: None,
        raw_detail: Some("unknown MySQL plan structure".to_string()),
        children: Vec::new(),
    })
}

fn parse_mysql_table_node(table: &serde_json::Value) -> ExplainNode {
    let access_type = table
        .get("access_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("ALL");

    let node_type = match access_type {
        "index" => NodeType::IndexOnlyScan,
        "range" | "ref" | "eq_ref" | "const" | "ref_or_null" | "fulltext" => NodeType::IndexScan,
        "index_merge" => NodeType::BitmapIndexScan,
        // "ALL" and unrecognized types default to sequential scan.
        _ => NodeType::SeqScan,
    };

    let estimated_rows = table
        .get("rows_examined_per_scan")
        .or_else(|| table.get("rows_produced_per_join"))
        .and_then(serde_json::Value::as_f64);

    let total_cost = table
        .get("cost_info")
        .and_then(|ci| ci.get("read_cost").or_else(|| ci.get("eval_cost")))
        .and_then(serde_json::Value::as_str)
        .and_then(|s| s.parse::<f64>().ok());

    ExplainNode {
        node_type,
        join_type: None,
        relation: table
            .get("table_name")
            .and_then(serde_json::Value::as_str)
            .map(String::from),
        index_name: table
            .get("key")
            .and_then(serde_json::Value::as_str)
            .map(String::from),
        startup_cost: None,
        total_cost,
        estimated_rows,
        estimated_width: None,
        filter: table
            .get("attached_condition")
            .and_then(serde_json::Value::as_str)
            .map(String::from),
        scan_direction: None,
        raw_detail: None,
        children: Vec::new(),
    }
}

/// Parse `SQLite` `EXPLAIN QUERY PLAN` output.
///
/// `SQLite` returns rows of `(id, parent, notused, detail)`. The detail
/// string describes each operator textually.
///
/// Each line should be formatted as `id|parent|notused|detail`.
///
/// # Errors
///
/// Returns [`MetadataError::ExplainParse`] if the input is empty or
/// contains malformed rows.
pub fn parse_sqlite_explain(text: &str) -> Result<ExplainPlan, MetadataError> {
    let mut nodes: Vec<(i64, i64, String)> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(4, '|').collect();
        if parts.len() < 4 {
            continue;
        }

        let id: i64 = parts[0]
            .trim()
            .parse()
            .map_err(|_| MetadataError::ExplainParse {
                message: format!("invalid id: {}", parts[0]),
            })?;
        let parent: i64 = parts[1]
            .trim()
            .parse()
            .map_err(|_| MetadataError::ExplainParse {
                message: format!("invalid parent: {}", parts[1]),
            })?;
        let detail = parts[3].trim().to_string();

        nodes.push((id, parent, detail));
    }

    if nodes.is_empty() {
        return Err(MetadataError::ExplainParse {
            message: "empty EXPLAIN QUERY PLAN output".to_string(),
        });
    }

    let root = build_sqlite_tree(&nodes, 0)?;
    let total_rows = root.estimated_rows;

    Ok(ExplainPlan {
        root,
        query: None,
        total_cost: None,
        total_rows,
    })
}

fn build_sqlite_tree(
    nodes: &[(i64, i64, String)],
    parent_id: i64,
) -> Result<ExplainNode, MetadataError> {
    let children_data: Vec<&(i64, i64, String)> =
        nodes.iter().filter(|(_, p, _)| *p == parent_id).collect();

    if children_data.is_empty() {
        return Err(MetadataError::ExplainParse {
            message: format!("no node with parent {parent_id}"),
        });
    }

    let (_, _, ref detail) = children_data[0];
    let mut node = parse_sqlite_detail(detail);

    for child in &children_data[1..] {
        let child_node = parse_sqlite_detail(&child.2);
        node.children.push(child_node);
    }

    for child_data in children_data {
        let grandchildren: Vec<&(i64, i64, String)> = nodes
            .iter()
            .filter(|(_, p, _)| *p == child_data.0)
            .collect();
        for gc in grandchildren {
            let gc_tree = build_sqlite_tree(nodes, gc.1)?;
            node.children.push(gc_tree);
        }
    }

    Ok(node)
}

fn parse_sqlite_detail(detail: &str) -> ExplainNode {
    let upper = detail.to_uppercase();

    let node_type = if upper.contains("SEARCH") {
        NodeType::IndexScan
    } else if upper.contains("SCAN") {
        NodeType::SeqScan
    } else if upper.contains("USE TEMP B-TREE FOR ORDER BY") || upper.contains("SORT") {
        NodeType::Sort
    } else if upper.contains("COMPOUND SUBQUERY") || upper.contains("CO-ROUTINE") {
        NodeType::SubqueryScan
    } else if upper.contains("GROUP BY") || upper.contains("AGGREGATE") {
        NodeType::HashAggregate
    } else if upper.contains("MERGE") {
        NodeType::MergeAppend
    } else if upper.contains("UNION") || upper.contains("COMPOUND") {
        NodeType::Append
    } else if upper.contains("NESTED LOOP") {
        NodeType::NestedLoop
    } else {
        NodeType::Other
    };

    let relation = extract_sqlite_table(detail);
    let index_name = extract_sqlite_index(detail);

    ExplainNode {
        node_type,
        join_type: None,
        relation,
        index_name,
        startup_cost: None,
        total_cost: None,
        estimated_rows: None,
        estimated_width: None,
        filter: None,
        scan_direction: None,
        raw_detail: Some(detail.to_string()),
        children: Vec::new(),
    }
}

fn extract_sqlite_table(detail: &str) -> Option<String> {
    // Patterns: "SCAN TABLE foo", "SEARCH TABLE foo",
    // "SCAN foo", "SEARCH foo" (newer SQLite versions)
    for keyword in &["SCAN TABLE ", "SEARCH TABLE ", "SCAN ", "SEARCH "] {
        if let Some(pos) = detail.find(keyword) {
            let rest = &detail[pos + keyword.len()..];
            let table = rest
                .split(|c: char| c.is_whitespace() || c == '(')
                .next()
                .unwrap_or("");
            if !table.is_empty()
                && !table.eq_ignore_ascii_case("USING")
                && !table.eq_ignore_ascii_case("TABLE")
            {
                return Some(table.to_string());
            }
        }
    }
    None
}

fn extract_sqlite_index(detail: &str) -> Option<String> {
    // Pattern: "USING INDEX idx_name" or "USING COVERING INDEX idx_name"
    for keyword in &["USING COVERING INDEX ", "USING INDEX "] {
        if let Some(pos) = detail.find(keyword) {
            let rest = &detail[pos + keyword.len()..];
            let idx = rest
                .split(|c: char| c.is_whitespace() || c == '(')
                .next()
                .unwrap_or("");
            if !idx.is_empty() {
                return Some(idx.to_string());
            }
        }
    }
    None
}

fn unreachable_node() -> ExplainNode {
    ExplainNode {
        node_type: NodeType::Result,
        join_type: None,
        relation: None,
        index_name: None,
        startup_cost: None,
        total_cost: None,
        estimated_rows: None,
        estimated_width: None,
        filter: None,
        scan_direction: None,
        raw_detail: None,
        children: Vec::new(),
    }
}

// ---- Display ----

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::SeqScan => "Seq Scan",
            Self::IndexScan => "Index Scan",
            Self::IndexOnlyScan => "Index Only Scan",
            Self::BitmapIndexScan => "Bitmap Index Scan",
            Self::BitmapHeapScan => "Bitmap Heap Scan",
            Self::NestedLoop => "Nested Loop",
            Self::HashJoin => "Hash Join",
            Self::MergeJoin => "Merge Join",
            Self::HashAggregate => "Hash Aggregate",
            Self::GroupAggregate => "Group Aggregate",
            Self::Sort => "Sort",
            Self::Limit => "Limit",
            Self::Materialize => "Materialize",
            Self::Append => "Append",
            Self::MergeAppend => "Merge Append",
            Self::SubqueryScan => "Subquery Scan",
            Self::FunctionScan => "Function Scan",
            Self::CteScan => "CTE Scan",
            Self::ValuesScan => "Values Scan",
            Self::Result => "Result",
            Self::Unique => "Unique",
            Self::Gather => "Gather",
            Self::GatherMerge => "Gather Merge",
            Self::Hash => "Hash",
            Self::WindowAgg => "WindowAgg",
            Self::SetOp => "SetOp",
            Self::ForeignScan => "Foreign Scan",
            Self::Other => "Other",
        };
        write!(f, "{s}")
    }
}

impl std::fmt::Display for JoinType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Inner => "Inner",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Full => "Full",
            Self::Semi => "Semi",
            Self::Anti => "Anti",
            Self::Cross => "Cross",
        };
        write!(f, "{s}")
    }
}

impl ExplainNode {
    /// Count total nodes in this subtree (including self).
    pub fn node_count(&self) -> usize {
        1 + self.children.iter().map(Self::node_count).sum::<usize>()
    }

    /// Find all leaf nodes (no children).
    pub fn leaves(&self) -> Vec<&Self> {
        if self.children.is_empty() {
            return vec![self];
        }
        self.children.iter().flat_map(Self::leaves).collect()
    }

    /// Find all nodes matching a predicate.
    pub fn find_nodes<F>(&self, predicate: &F) -> Vec<&Self>
    where
        F: Fn(&Self) -> bool,
    {
        let mut result = Vec::new();
        if predicate(self) {
            result.push(self);
        }
        for child in &self.children {
            result.extend(child.find_nodes(predicate));
        }
        result
    }

    /// Maximum depth of the plan tree.
    pub fn depth(&self) -> usize {
        if self.children.is_empty() {
            return 1;
        }
        1 + self.children.iter().map(Self::depth).max().unwrap_or(0)
    }
}

#[expect(clippy::expect_used, reason = "test code")]
#[cfg(test)]
mod tests {
    use super::*;

    // ---- NodeType Display ----

    #[test]
    fn node_type_display_seq_scan() {
        assert_eq!(NodeType::SeqScan.to_string(), "Seq Scan");
    }

    #[test]
    fn node_type_display_hash_join() {
        assert_eq!(NodeType::HashJoin.to_string(), "Hash Join");
    }

    #[test]
    fn node_type_display_other() {
        assert_eq!(NodeType::Other.to_string(), "Other");
    }

    // ---- JoinType Display ----

    #[test]
    fn join_type_display_inner() {
        assert_eq!(JoinType::Inner.to_string(), "Inner");
    }

    #[test]
    fn join_type_display_left() {
        assert_eq!(JoinType::Left.to_string(), "Left");
    }

    #[test]
    fn join_type_display_full() {
        assert_eq!(JoinType::Full.to_string(), "Full");
    }

    #[test]
    fn join_type_display_semi() {
        assert_eq!(JoinType::Semi.to_string(), "Semi");
    }

    #[test]
    fn join_type_display_anti() {
        assert_eq!(JoinType::Anti.to_string(), "Anti");
    }

    // ---- ExplainNode helpers ----

    fn leaf_node(node_type: NodeType, relation: Option<&str>) -> ExplainNode {
        ExplainNode {
            node_type,
            join_type: None,
            relation: relation.map(String::from),
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: None,
            children: Vec::new(),
        }
    }

    #[test]
    fn node_count_leaf() {
        let n = leaf_node(NodeType::SeqScan, Some("t"));
        assert_eq!(n.node_count(), 1);
    }

    #[test]
    fn node_count_tree() {
        let n = ExplainNode {
            node_type: NodeType::NestedLoop,
            children: vec![
                leaf_node(NodeType::SeqScan, Some("a")),
                leaf_node(NodeType::IndexScan, Some("b")),
            ],
            ..leaf_node(NodeType::NestedLoop, None)
        };
        assert_eq!(n.node_count(), 3);
    }

    #[test]
    fn leaves_single() {
        let n = leaf_node(NodeType::SeqScan, Some("t"));
        assert_eq!(n.leaves().len(), 1);
    }

    #[test]
    fn leaves_tree() {
        let n = ExplainNode {
            node_type: NodeType::HashJoin,
            children: vec![
                leaf_node(NodeType::SeqScan, Some("a")),
                ExplainNode {
                    node_type: NodeType::Sort,
                    children: vec![leaf_node(NodeType::IndexScan, Some("b"))],
                    ..leaf_node(NodeType::Sort, None)
                },
            ],
            ..leaf_node(NodeType::HashJoin, None)
        };
        let leaves = n.leaves();
        assert_eq!(leaves.len(), 2);
    }

    #[test]
    fn find_nodes_by_type() {
        let n = ExplainNode {
            node_type: NodeType::HashJoin,
            children: vec![
                leaf_node(NodeType::SeqScan, Some("a")),
                leaf_node(NodeType::SeqScan, Some("b")),
            ],
            ..leaf_node(NodeType::HashJoin, None)
        };
        let scans = n.find_nodes(&|n| n.node_type == NodeType::SeqScan);
        assert_eq!(scans.len(), 2);
    }

    #[test]
    fn depth_leaf() {
        assert_eq!(leaf_node(NodeType::SeqScan, None).depth(), 1);
    }

    #[test]
    fn depth_tree() {
        let n = ExplainNode {
            node_type: NodeType::HashJoin,
            children: vec![
                leaf_node(NodeType::SeqScan, Some("a")),
                ExplainNode {
                    node_type: NodeType::Sort,
                    children: vec![leaf_node(NodeType::IndexScan, Some("b"))],
                    ..leaf_node(NodeType::Sort, None)
                },
            ],
            ..leaf_node(NodeType::HashJoin, None)
        };
        assert_eq!(n.depth(), 3);
    }

    // ---- PostgreSQL parser ----

    fn pg_simple_json() -> &'static str {
        r#"[
          {
            "Plan": {
              "Node Type": "Seq Scan",
              "Relation Name": "users",
              "Startup Cost": 0.0,
              "Total Cost": 35.5,
              "Plan Rows": 2550,
              "Plan Width": 64
            }
          }
        ]"#
    }

    fn pg_join_json() -> &'static str {
        r#"[
          {
            "Plan": {
              "Node Type": "Hash Join",
              "Join Type": "Inner",
              "Startup Cost": 1.05,
              "Total Cost": 50.0,
              "Plan Rows": 100,
              "Plan Width": 128,
              "Filter": "(a.id = b.a_id)",
              "Plans": [
                {
                  "Node Type": "Seq Scan",
                  "Relation Name": "a",
                  "Startup Cost": 0.0,
                  "Total Cost": 10.0,
                  "Plan Rows": 1000,
                  "Plan Width": 64
                },
                {
                  "Node Type": "Hash",
                  "Startup Cost": 0.5,
                  "Total Cost": 0.5,
                  "Plan Rows": 100,
                  "Plan Width": 64,
                  "Plans": [
                    {
                      "Node Type": "Index Scan",
                      "Relation Name": "b",
                      "Index Name": "b_pkey",
                      "Scan Direction": "Forward",
                      "Startup Cost": 0.28,
                      "Total Cost": 0.5,
                      "Plan Rows": 100,
                      "Plan Width": 64
                    }
                  ]
                }
              ]
            }
          }
        ]"#
    }

    #[test]
    fn pg_parse_simple_scan() {
        let plan = parse_postgres_explain(pg_simple_json()).expect("parse");
        assert_eq!(plan.root.node_type, NodeType::SeqScan);
        assert_eq!(plan.root.relation.as_deref(), Some("users"));
        assert_eq!(plan.root.estimated_rows, Some(2550.0));
        assert_eq!(plan.root.estimated_width, Some(64));
        assert_eq!(plan.root.startup_cost, Some(0.0));
        assert_eq!(plan.root.total_cost, Some(35.5));
        assert!(plan.root.children.is_empty());
    }

    #[test]
    fn pg_plan_total_cost() {
        let plan = parse_postgres_explain(pg_simple_json()).expect("parse");
        assert_eq!(plan.total_cost, Some(35.5));
        assert_eq!(plan.total_rows, Some(2550.0));
    }

    #[test]
    fn pg_parse_join_plan() {
        let plan = parse_postgres_explain(pg_join_json()).expect("parse");
        assert_eq!(plan.root.node_type, NodeType::HashJoin);
        assert_eq!(plan.root.join_type, Some(JoinType::Inner));
        assert_eq!(plan.root.children.len(), 2);
        assert_eq!(plan.root.filter.as_deref(), Some("(a.id = b.a_id)"));
    }

    #[test]
    fn pg_parse_join_children() {
        let plan = parse_postgres_explain(pg_join_json()).expect("parse");
        let left = &plan.root.children[0];
        assert_eq!(left.node_type, NodeType::SeqScan);
        assert_eq!(left.relation.as_deref(), Some("a"));

        let right = &plan.root.children[1];
        assert_eq!(right.node_type, NodeType::Hash);
        assert_eq!(right.children.len(), 1);

        let idx = &right.children[0];
        assert_eq!(idx.node_type, NodeType::IndexScan);
        assert_eq!(idx.relation.as_deref(), Some("b"));
        assert_eq!(idx.index_name.as_deref(), Some("b_pkey"));
        assert_eq!(idx.scan_direction.as_deref(), Some("Forward"));
    }

    #[test]
    fn pg_parse_invalid_json() {
        let result = parse_postgres_explain("not json");
        assert!(result.is_err());
        let err = result.expect_err("should be an error");
        assert!(matches!(err, MetadataError::ExplainParse { .. }));
    }

    #[test]
    fn pg_parse_empty_array() {
        let result = parse_postgres_explain("[]");
        assert!(result.is_err());
    }

    #[test]
    fn pg_parse_missing_plan_key() {
        let result = parse_postgres_explain(r#"[{"foo": "bar"}]"#);
        assert!(result.is_err());
    }

    #[test]
    fn pg_parse_not_array() {
        let result = parse_postgres_explain(r#"{"Plan": {}}"#);
        assert!(result.is_err());
    }

    #[test]
    fn pg_node_type_mapping() {
        assert_eq!(parse_pg_node_type("Seq Scan"), NodeType::SeqScan);
        assert_eq!(parse_pg_node_type("Index Scan"), NodeType::IndexScan);
        assert_eq!(
            parse_pg_node_type("Index Only Scan"),
            NodeType::IndexOnlyScan
        );
        assert_eq!(
            parse_pg_node_type("Bitmap Index Scan"),
            NodeType::BitmapIndexScan
        );
        assert_eq!(
            parse_pg_node_type("Bitmap Heap Scan"),
            NodeType::BitmapHeapScan
        );
        assert_eq!(parse_pg_node_type("Nested Loop"), NodeType::NestedLoop);
        assert_eq!(parse_pg_node_type("Hash Join"), NodeType::HashJoin);
        assert_eq!(parse_pg_node_type("Merge Join"), NodeType::MergeJoin);
        assert_eq!(parse_pg_node_type("Sort"), NodeType::Sort);
        assert_eq!(parse_pg_node_type("Limit"), NodeType::Limit);
        assert_eq!(parse_pg_node_type("Materialize"), NodeType::Materialize);
        assert_eq!(parse_pg_node_type("Result"), NodeType::Result);
        assert_eq!(parse_pg_node_type("Unique"), NodeType::Unique);
        assert_eq!(parse_pg_node_type("Gather"), NodeType::Gather);
        assert_eq!(parse_pg_node_type("Gather Merge"), NodeType::GatherMerge);
        assert_eq!(parse_pg_node_type("Hash"), NodeType::Hash);
        assert_eq!(parse_pg_node_type("WindowAgg"), NodeType::WindowAgg);
        assert_eq!(parse_pg_node_type("SetOp"), NodeType::SetOp);
        assert_eq!(parse_pg_node_type("Foreign Scan"), NodeType::ForeignScan);
        assert_eq!(parse_pg_node_type("Unknown"), NodeType::Other);
    }

    #[test]
    fn pg_join_type_mapping() {
        assert_eq!(parse_join_type("Inner"), JoinType::Inner);
        assert_eq!(parse_join_type("Left"), JoinType::Left);
        assert_eq!(parse_join_type("Right"), JoinType::Right);
        assert_eq!(parse_join_type("Full"), JoinType::Full);
        assert_eq!(parse_join_type("Semi"), JoinType::Semi);
        assert_eq!(parse_join_type("Anti"), JoinType::Anti);
        assert_eq!(parse_join_type("Cross"), JoinType::Cross);
        assert_eq!(parse_join_type("Cross Join"), JoinType::Cross);
        assert_eq!(parse_join_type("Unknown"), JoinType::Inner);
    }

    // ---- MySQL parser ----

    fn mysql_simple_json() -> &'static str {
        r#"{
          "query_block": {
            "select_id": 1,
            "cost_info": { "query_cost": "10.50" },
            "table": {
              "table_name": "users",
              "access_type": "ALL",
              "rows_examined_per_scan": 1000,
              "rows_produced_per_join": 1000,
              "cost_info": { "read_cost": "8.25" }
            }
          }
        }"#
    }

    fn mysql_join_json() -> &'static str {
        r#"{
          "query_block": {
            "select_id": 1,
            "cost_info": { "query_cost": "25.00" },
            "nested_loop": [
              {
                "table": {
                  "table_name": "orders",
                  "access_type": "ALL",
                  "rows_examined_per_scan": 5000
                }
              },
              {
                "table": {
                  "table_name": "users",
                  "access_type": "eq_ref",
                  "key": "users_pkey",
                  "rows_examined_per_scan": 1,
                  "attached_condition": "users.id = orders.user_id"
                }
              }
            ]
          }
        }"#
    }

    fn mysql_ordering_json() -> &'static str {
        r#"{
          "query_block": {
            "select_id": 1,
            "ordering_operation": {
              "table": {
                "table_name": "products",
                "access_type": "range",
                "key": "idx_price",
                "rows_examined_per_scan": 200
              }
            }
          }
        }"#
    }

    #[test]
    fn mysql_parse_simple_scan() {
        let plan = parse_mysql_explain(mysql_simple_json()).expect("parse");
        assert_eq!(plan.root.node_type, NodeType::SeqScan);
        assert_eq!(plan.root.relation.as_deref(), Some("users"));
        assert_eq!(plan.root.estimated_rows, Some(1000.0));
    }

    #[test]
    fn mysql_plan_total_cost() {
        let plan = parse_mysql_explain(mysql_simple_json()).expect("parse");
        assert_eq!(plan.total_cost, Some(10.50));
    }

    #[test]
    fn mysql_parse_nested_loop() {
        let plan = parse_mysql_explain(mysql_join_json()).expect("parse");
        assert_eq!(plan.root.node_type, NodeType::NestedLoop);
        assert_eq!(plan.root.children.len(), 2);
    }

    #[test]
    fn mysql_parse_join_children() {
        let plan = parse_mysql_explain(mysql_join_json()).expect("parse");
        let left = &plan.root.children[0];
        assert_eq!(left.node_type, NodeType::SeqScan);
        assert_eq!(left.relation.as_deref(), Some("orders"));

        let right = &plan.root.children[1];
        assert_eq!(right.node_type, NodeType::IndexScan);
        assert_eq!(right.index_name.as_deref(), Some("users_pkey"));
        assert_eq!(right.filter.as_deref(), Some("users.id = orders.user_id"));
    }

    #[test]
    fn mysql_parse_ordering() {
        let plan = parse_mysql_explain(mysql_ordering_json()).expect("parse");
        assert_eq!(plan.root.node_type, NodeType::Sort);
        assert_eq!(plan.root.relation.as_deref(), Some("products"));
        assert_eq!(plan.root.index_name.as_deref(), Some("idx_price"));
    }

    #[test]
    fn mysql_parse_invalid_json() {
        let result = parse_mysql_explain("not json");
        assert!(result.is_err());
    }

    #[test]
    fn mysql_parse_missing_query_block() {
        let result = parse_mysql_explain(r#"{"foo": "bar"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn mysql_access_type_all_is_seq_scan() {
        let node = parse_mysql_table_node(&serde_json::json!({
            "table_name": "t",
            "access_type": "ALL"
        }));
        assert_eq!(node.node_type, NodeType::SeqScan);
    }

    #[test]
    fn mysql_access_type_index_is_index_only() {
        let node = parse_mysql_table_node(&serde_json::json!({
            "table_name": "t",
            "access_type": "index"
        }));
        assert_eq!(node.node_type, NodeType::IndexOnlyScan);
    }

    #[test]
    fn mysql_access_type_ref_is_index_scan() {
        let node = parse_mysql_table_node(&serde_json::json!({
            "table_name": "t",
            "access_type": "ref",
            "key": "idx_col"
        }));
        assert_eq!(node.node_type, NodeType::IndexScan);
        assert_eq!(node.index_name.as_deref(), Some("idx_col"));
    }

    #[test]
    fn mysql_access_type_index_merge() {
        let node = parse_mysql_table_node(&serde_json::json!({
            "table_name": "t",
            "access_type": "index_merge"
        }));
        assert_eq!(node.node_type, NodeType::BitmapIndexScan);
    }

    // ---- SQLite parser ----

    fn sqlite_simple() -> &'static str {
        "2|0|0|SCAN TABLE users"
    }

    fn sqlite_index_search() -> &'static str {
        "3|0|0|SEARCH TABLE orders USING INDEX idx_orders_user_id (user_id=?)"
    }

    fn sqlite_multi_line() -> &'static str {
        "2|0|0|SCAN TABLE orders\n\
         3|0|0|SEARCH TABLE users USING INDEX users_pkey (id=?)"
    }

    fn sqlite_sort() -> &'static str {
        "2|0|0|SCAN TABLE users\n\
         4|0|0|USE TEMP B-TREE FOR ORDER BY"
    }

    #[test]
    fn sqlite_parse_simple_scan() {
        let plan = parse_sqlite_explain(sqlite_simple()).expect("parse");
        assert_eq!(plan.root.node_type, NodeType::SeqScan);
        assert_eq!(plan.root.relation.as_deref(), Some("users"));
    }

    #[test]
    fn sqlite_parse_index_search() {
        let plan = parse_sqlite_explain(sqlite_index_search()).expect("parse");
        assert_eq!(plan.root.node_type, NodeType::IndexScan);
        assert_eq!(plan.root.relation.as_deref(), Some("orders"));
        assert_eq!(plan.root.index_name.as_deref(), Some("idx_orders_user_id"));
    }

    #[test]
    fn sqlite_parse_multi_line() {
        let plan = parse_sqlite_explain(sqlite_multi_line()).expect("parse");
        assert_eq!(plan.root.node_type, NodeType::SeqScan);
        assert_eq!(plan.root.children.len(), 1);
        assert_eq!(plan.root.children[0].node_type, NodeType::IndexScan);
    }

    #[test]
    fn sqlite_parse_sort() {
        let plan = parse_sqlite_explain(sqlite_sort()).expect("parse");
        assert!(
            plan.root
                .children
                .iter()
                .any(|c| c.node_type == NodeType::Sort)
                || plan.root.node_type == NodeType::Sort
        );
    }

    #[test]
    fn sqlite_parse_empty() {
        let result = parse_sqlite_explain("");
        assert!(result.is_err());
    }

    #[test]
    fn sqlite_parse_raw_detail_preserved() {
        let plan = parse_sqlite_explain(sqlite_simple()).expect("parse");
        assert!(plan.root.raw_detail.is_some());
        assert!(plan
            .root
            .raw_detail
            .as_deref()
            .unwrap_or("")
            .contains("SCAN TABLE users"));
    }

    #[test]
    fn sqlite_extract_table_scan() {
        assert_eq!(
            extract_sqlite_table("SCAN TABLE users"),
            Some("users".to_string())
        );
    }

    #[test]
    fn sqlite_extract_table_search() {
        assert_eq!(
            extract_sqlite_table("SEARCH TABLE orders USING INDEX idx (id=?)"),
            Some("orders".to_string())
        );
    }

    #[test]
    fn sqlite_extract_table_none() {
        assert_eq!(extract_sqlite_table("USE TEMP B-TREE FOR ORDER BY"), None);
    }

    #[test]
    fn sqlite_extract_index_using() {
        assert_eq!(
            extract_sqlite_index("SEARCH TABLE t USING INDEX idx_col (col=?)"),
            Some("idx_col".to_string())
        );
    }

    #[test]
    fn sqlite_extract_covering_index() {
        assert_eq!(
            extract_sqlite_index("SCAN TABLE t USING COVERING INDEX idx_all"),
            Some("idx_all".to_string())
        );
    }

    #[test]
    fn sqlite_extract_index_none() {
        assert_eq!(extract_sqlite_index("SCAN TABLE users"), None);
    }

    // ---- Serialization ----

    #[test]
    fn explain_plan_serialize_roundtrip() {
        let plan = parse_postgres_explain(pg_simple_json()).expect("parse");
        let json = serde_json::to_string(&plan).expect("serialize");
        let deserialized: ExplainPlan = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(plan, deserialized);
    }

    #[test]
    fn node_type_serialize_roundtrip() {
        let nt = NodeType::HashJoin;
        let json = serde_json::to_string(&nt).expect("serialize");
        let d: NodeType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(nt, d);
    }

    #[test]
    fn join_type_serialize_roundtrip() {
        let jt = JoinType::Left;
        let json = serde_json::to_string(&jt).expect("serialize");
        let d: JoinType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(jt, d);
    }

    // ---- MySQL grouping_operation ----

    #[test]
    fn mysql_parse_grouping() {
        let json = r#"{
          "query_block": {
            "select_id": 1,
            "grouping_operation": {
              "table": {
                "table_name": "events",
                "access_type": "ALL",
                "rows_examined_per_scan": 5000
              }
            }
          }
        }"#;
        let plan = parse_mysql_explain(json).expect("parse");
        assert_eq!(plan.root.node_type, NodeType::HashAggregate);
        assert_eq!(plan.root.relation.as_deref(), Some("events"));
    }

    // ---- Edge cases ----

    #[test]
    fn mysql_empty_nested_loop() {
        let json = r#"{
          "query_block": {
            "select_id": 1,
            "nested_loop": []
          }
        }"#;
        let plan = parse_mysql_explain(json).expect("parse");
        assert_eq!(plan.root.node_type, NodeType::NestedLoop);
        assert!(plan.root.children.is_empty());
    }

    #[test]
    fn mysql_single_table_nested_loop() {
        let json = r#"{
          "query_block": {
            "select_id": 1,
            "nested_loop": [
              {
                "table": {
                  "table_name": "t",
                  "access_type": "ALL",
                  "rows_examined_per_scan": 100
                }
              }
            ]
          }
        }"#;
        let plan = parse_mysql_explain(json).expect("parse");
        assert_eq!(plan.root.node_type, NodeType::SeqScan);
        assert_eq!(plan.root.relation.as_deref(), Some("t"));
    }

    #[test]
    fn pg_unknown_node_type() {
        let json = r#"[
          {
            "Plan": {
              "Node Type": "Custom Scan (DecompressChunk)",
              "Startup Cost": 0.0,
              "Total Cost": 10.0,
              "Plan Rows": 100,
              "Plan Width": 32
            }
          }
        ]"#;
        let plan = parse_postgres_explain(json).expect("parse");
        assert_eq!(plan.root.node_type, NodeType::Other);
    }

    #[test]
    fn sqlite_compound_subquery() {
        let text = "2|0|0|COMPOUND SUBQUERY 1";
        let plan = parse_sqlite_explain(text).expect("parse");
        assert_eq!(plan.root.node_type, NodeType::SubqueryScan);
    }
}

// ---- Formatters (RelExpr to EXPLAIN text) ----

/// Format an `ExplainNode` as `PostgreSQL` EXPLAIN text output.
///
/// Generates output matching `EXPLAIN` (text format, not JSON) from `PostgreSQL`,
/// including cost estimates, row counts, and indented tree structure.
///
/// # Example
///
/// ```text
/// Limit  (cost=4.46..4.46 rows=1 width=4)
///   ->  Sort  (cost=4.46..4.46 rows=1 width=4)
///         Sort Key: order_id
///         ->  Index Only Scan using orders_test_shipping_date_order_id_idx on orders_test  (cost=0.43..4.45 rows=1 width=4)
///               Index Cond: ((shipping_date >= '2022-05-01'::date) AND (shipping_date <= '2022-05-01'::date))
/// ```
pub fn format_postgres_explain(node: &ExplainNode) -> String {
    let mut buf = String::new();
    format_postgres_node(&mut buf, node, "", true);
    buf
}

fn format_postgres_node(buf: &mut String, node: &ExplainNode, indent: &str, is_first: bool) {
    // PostgreSQL EXPLAIN format uses:
    // - No prefix for root
    // - "   ->  " for direct children (with leading spaces matching parent)

    if !is_first {
        buf.push_str(indent);
        buf.push_str("   ->  ");
    }

    // Node type
    buf.push_str(&node.node_type.to_string());

    if let Some(ref join_type) = node.join_type {
        buf.push(' ');
        buf.push_str(&join_type.to_string());
    }

    if let Some(ref rel) = node.relation {
        buf.push_str(" on ");
        buf.push_str(rel);
    }

    if let Some(ref idx) = node.index_name {
        buf.push_str(" using ");
        buf.push_str(idx);
    }

    // Cost info
    buf.push_str("  (cost=");
    if let Some(startup) = node.startup_cost {
        let _ = write!(buf, "{startup:.2}");
    } else {
        buf.push_str("0.00");
    }
    buf.push_str("..");
    if let Some(total) = node.total_cost {
        let _ = write!(buf, "{total:.2}");
    } else {
        buf.push_str("0.00");
    }

    buf.push_str(" rows=");
    if let Some(rows) = node.estimated_rows {
        let _ = write!(buf, "{}", rows.round() as i64);
    } else {
        buf.push('1');
    }

    if let Some(width) = node.estimated_width {
        let _ = write!(buf, " width={width}");
    }
    buf.push_str(")\n");

    // Additional details (filter, index cond, etc.)
    // These are indented further
    let detail_prefix = if is_first {
        "         "
    } else {
        &format!("{indent}            ")
    };

    if let Some(ref filter) = node.filter {
        buf.push_str(detail_prefix);
        buf.push_str("Filter: ");
        buf.push_str(filter);
        buf.push('\n');
    }

    if let Some(ref scan_dir) = node.scan_direction {
        if !scan_dir.is_empty() && scan_dir != "Forward" {
            buf.push_str(detail_prefix);
            buf.push_str("Scan Direction: ");
            buf.push_str(scan_dir);
            buf.push('\n');
        }
    }

    // Children - each indented
    if !node.children.is_empty() {
        let child_indent = if is_first {
            String::new()
        } else {
            format!("{indent}      ")
        };

        for child in &node.children {
            format_postgres_node(buf, child, &child_indent, false);
        }
    }
}

/// Format an `ExplainNode` as `MySQL` EXPLAIN text output.
///
/// Generates simple text output similar to `EXPLAIN` (non-JSON) from `MySQL`.
/// `MySQL` text format is less detailed than `PostgreSQL`.
pub fn format_mysql_explain(node: &ExplainNode) -> String {
    let mut buf = String::new();
    format_mysql_node(&mut buf, node, 0);
    buf
}

fn format_mysql_node(buf: &mut String, node: &ExplainNode, depth: usize) {
    let indent = "  ".repeat(depth);

    buf.push_str(&indent);
    buf.push_str("-> ");
    buf.push_str(&node.node_type.to_string());

    if let Some(ref rel) = node.relation {
        buf.push_str(" on ");
        buf.push_str(rel);
    }

    if let Some(ref idx) = node.index_name {
        buf.push_str(" (using ");
        buf.push_str(idx);
        buf.push(')');
    }

    if let Some(rows) = node.estimated_rows {
        let _ = write!(buf, "  (rows={rows:.0})");
    }

    if let Some(cost) = node.total_cost {
        let _ = write!(buf, " (cost={cost:.2})");
    }

    buf.push('\n');

    if let Some(ref filter) = node.filter {
        buf.push_str(&indent);
        buf.push_str("    Filter: ");
        buf.push_str(filter);
        buf.push('\n');
    }

    for child in &node.children {
        format_mysql_node(buf, child, depth + 1);
    }
}

/// Format an `ExplainNode` as `SQLite` EXPLAIN QUERY PLAN text output.
///
/// Generates output matching `EXPLAIN QUERY PLAN` from `SQLite`, which uses
/// a simple indented text format with operation descriptions.
pub fn format_sqlite_explain(node: &ExplainNode) -> String {
    let mut buf = String::new();
    format_sqlite_node(&mut buf, node, 0);
    buf
}

fn format_sqlite_node(buf: &mut String, node: &ExplainNode, depth: usize) {
    let indent = "  ".repeat(depth);

    buf.push_str(&indent);

    // SQLite uses descriptive text rather than structured operators
    match node.node_type {
        NodeType::SeqScan => {
            buf.push_str("SCAN");
            if let Some(ref rel) = node.relation {
                buf.push_str(" TABLE ");
                buf.push_str(rel);
            }
        }
        NodeType::IndexScan | NodeType::IndexOnlyScan => {
            buf.push_str("SEARCH");
            if let Some(ref rel) = node.relation {
                buf.push_str(" TABLE ");
                buf.push_str(rel);
            }
            if let Some(ref idx) = node.index_name {
                buf.push_str(" USING");
                if node.node_type == NodeType::IndexOnlyScan {
                    buf.push_str(" COVERING");
                }
                buf.push_str(" INDEX ");
                buf.push_str(idx);
            }
        }
        NodeType::Sort => {
            buf.push_str("USE TEMP B-TREE FOR ORDER BY");
        }
        NodeType::NestedLoop => {
            buf.push_str("NESTED LOOP");
        }
        NodeType::HashAggregate | NodeType::GroupAggregate => {
            buf.push_str("AGGREGATE");
        }
        NodeType::Limit => {
            buf.push_str("LIMIT");
        }
        _ => {
            // For other node types, use the display name
            buf.push_str(&node.node_type.to_string().to_uppercase());
        }
    }

    if let Some(ref filter) = node.filter {
        buf.push_str(" (");
        buf.push_str(filter);
        buf.push(')');
    }

    buf.push('\n');

    for child in &node.children {
        format_sqlite_node(buf, child, depth + 1);
    }
}

// ---- RelExpr to ExplainNode conversion ----

/// Convert a `RelExpr` (RA's internal representation) to an `ExplainNode`
/// (database EXPLAIN format).
///
/// This enables using the EXPLAIN formatters with RA's optimized plans.
/// Cost and row estimates are set to `None` and should be filled by the
/// optimizer later.
///
/// # Mapping Rules
///
/// - **Scan** → `SeqScan`
/// - **`IndexScan`** → `IndexScan`
/// - **`IndexOnlyScan`** → `IndexOnlyScan`
/// - **Join** → `HashJoin` (Inner/Left/Right/Full) or `NestedLoop` (Cross/Semi/Anti)
/// - **Aggregate** → `HashAggregate` (no GROUP BY) or `GroupAggregate` (with GROUP BY)
/// - **Sort/IncrementalSort** → `Sort`
/// - **Limit** → `Limit`
/// - **Union** → `Append` (UNION ALL) or `SetOp` (UNION)
/// - **Intersect/Except** → `SetOp`
/// - **Distinct** → `Unique`
/// - **Window** → `WindowAgg`
/// - **Values** → `ValuesScan`
/// - **CTE/RecursiveCTE** → `CteScan`
/// - **Unnest/TableFunction** → `FunctionScan`
/// - **`BitmapIndexScan`** → `BitmapIndexScan`
/// - **`BitmapHeapScan`** → `BitmapHeapScan`
/// - **`ParallelScan`** → `SeqScan` (with parallel workers note)
/// - **Gather** → `Gather`
/// - **`MvScan`** → `SeqScan` (with materialized view note)
/// - **`RowPattern`** → `Other` (with row pattern note)
///
/// # Example
///
/// ```rust
/// use ra_core::algebra::{RelExpr, JoinType};
/// use ra_core::expr::{Expr, Const};
/// use ra_metadata::explain::{relexpr_to_explain_node, format_postgres_explain};
///
/// let plan = RelExpr::Join {
///     join_type: JoinType::Inner,
///     condition: Expr::Const(Const::Bool(true)),
///     left: Box::new(RelExpr::scan("users")),
///     right: Box::new(RelExpr::scan("orders")),
/// };
///
/// let node = relexpr_to_explain_node(&plan);
/// let text = format_postgres_explain(&node);
/// println!("{}", text);
/// ```
#[expect(
    clippy::too_many_lines,
    reason = "RelExpr to explain node conversion requires handling many RelExpr variants with detailed formatting"
)]
pub fn relexpr_to_explain_node(expr: &ra_core::algebra::RelExpr) -> ExplainNode {
    use ra_core::algebra::RelExpr;

    match expr {
        RelExpr::Scan { table, .. } => ExplainNode {
            node_type: NodeType::SeqScan,
            join_type: None,
            relation: Some(table.clone()),
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: None,
            children: Vec::new(),
        },

        RelExpr::IndexScan { table, column } => ExplainNode {
            node_type: NodeType::IndexScan,
            join_type: None,
            relation: Some(table.clone()),
            index_name: Some(format!("idx_{column}")),
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: Some("Forward".to_string()),
            raw_detail: None,
            children: Vec::new(),
        },

        RelExpr::IndexOnlyScan {
            table,
            index,
            predicate,
            ..
        } => ExplainNode {
            node_type: NodeType::IndexOnlyScan,
            join_type: None,
            relation: Some(table.clone()),
            index_name: Some(index.clone()),
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: Some(format!("{predicate:?}")),
            scan_direction: Some("Forward".to_string()),
            raw_detail: None,
            children: Vec::new(),
        },

        RelExpr::BitmapIndexScan {
            table,
            index,
            predicate,
        } => ExplainNode {
            node_type: NodeType::BitmapIndexScan,
            join_type: None,
            relation: Some(table.clone()),
            index_name: Some(index.clone()),
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: Some(format!("{predicate:?}")),
            scan_direction: None,
            raw_detail: None,
            children: Vec::new(),
        },

        RelExpr::BitmapHeapScan {
            table,
            bitmap,
            recheck_cond,
        } => ExplainNode {
            node_type: NodeType::BitmapHeapScan,
            join_type: None,
            relation: Some(table.clone()),
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: recheck_cond.as_ref().map(|c| format!("{c:?}")),
            scan_direction: None,
            raw_detail: None,
            children: vec![relexpr_to_explain_node(bitmap)],
        },

        RelExpr::BitmapAnd { inputs } | RelExpr::BitmapOr { inputs } => {
            let node_type = NodeType::BitmapIndexScan;
            ExplainNode {
                node_type,
                join_type: None,
                relation: None,
                index_name: None,
                startup_cost: None,
                total_cost: None,
                estimated_rows: None,
                estimated_width: None,
                filter: None,
                scan_direction: None,
                raw_detail: Some(if matches!(expr, RelExpr::BitmapAnd { .. }) {
                    "BitmapAnd".to_string()
                } else {
                    "BitmapOr".to_string()
                }),
                children: inputs.iter().map(|b| relexpr_to_explain_node(b)).collect(),
            }
        }

        RelExpr::Filter { predicate, input } => {
            let mut child = relexpr_to_explain_node(input);
            child.filter = Some(format!("{predicate:?}"));
            child
        }

        RelExpr::Project { input, .. } => relexpr_to_explain_node(input),

        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => {
            let (node_type, explain_join_type) = match join_type {
                ra_core::algebra::JoinType::Inner => (NodeType::HashJoin, JoinType::Inner),
                ra_core::algebra::JoinType::LeftOuter => (NodeType::HashJoin, JoinType::Left),
                ra_core::algebra::JoinType::RightOuter => (NodeType::HashJoin, JoinType::Right),
                ra_core::algebra::JoinType::FullOuter => (NodeType::HashJoin, JoinType::Full),
                ra_core::algebra::JoinType::Cross => (NodeType::NestedLoop, JoinType::Cross),
                ra_core::algebra::JoinType::Semi => (NodeType::NestedLoop, JoinType::Semi),
                ra_core::algebra::JoinType::Anti => (NodeType::NestedLoop, JoinType::Anti),
            };

            ExplainNode {
                node_type,
                join_type: Some(explain_join_type),
                relation: None,
                index_name: None,
                startup_cost: None,
                total_cost: None,
                estimated_rows: None,
                estimated_width: None,
                filter: Some(format!("{condition:?}")),
                scan_direction: None,
                raw_detail: None,
                children: vec![
                    relexpr_to_explain_node(left),
                    relexpr_to_explain_node(right),
                ],
            }
        }

        RelExpr::ParallelHashJoin {
            join_type,
            condition,
            left,
            right,
            workers,
        } => {
            let (_, explain_join_type) = match join_type {
                ra_core::algebra::JoinType::Inner => (NodeType::HashJoin, JoinType::Inner),
                ra_core::algebra::JoinType::LeftOuter => (NodeType::HashJoin, JoinType::Left),
                ra_core::algebra::JoinType::RightOuter => (NodeType::HashJoin, JoinType::Right),
                ra_core::algebra::JoinType::FullOuter => (NodeType::HashJoin, JoinType::Full),
                ra_core::algebra::JoinType::Cross => (NodeType::HashJoin, JoinType::Cross),
                ra_core::algebra::JoinType::Semi => (NodeType::HashJoin, JoinType::Semi),
                ra_core::algebra::JoinType::Anti => (NodeType::HashJoin, JoinType::Anti),
            };

            ExplainNode {
                node_type: NodeType::HashJoin,
                join_type: Some(explain_join_type),
                relation: None,
                index_name: None,
                startup_cost: None,
                total_cost: None,
                estimated_rows: None,
                estimated_width: None,
                filter: Some(format!("{condition:?}")),
                scan_direction: None,
                raw_detail: Some(format!("Parallel workers: {workers}")),
                children: vec![
                    relexpr_to_explain_node(left),
                    relexpr_to_explain_node(right),
                ],
            }
        }

        RelExpr::Aggregate {
            group_by, input, ..
        } => {
            let node_type = if group_by.is_empty() {
                NodeType::HashAggregate
            } else {
                NodeType::GroupAggregate
            };

            ExplainNode {
                node_type,
                join_type: None,
                relation: None,
                index_name: None,
                startup_cost: None,
                total_cost: None,
                estimated_rows: None,
                estimated_width: None,
                filter: None,
                scan_direction: None,
                raw_detail: None,
                children: vec![relexpr_to_explain_node(input)],
            }
        }

        RelExpr::ParallelAggregate {
            group_by,
            input,
            workers,
            ..
        } => {
            let node_type = if group_by.is_empty() {
                NodeType::HashAggregate
            } else {
                NodeType::GroupAggregate
            };

            ExplainNode {
                node_type,
                join_type: None,
                relation: None,
                index_name: None,
                startup_cost: None,
                total_cost: None,
                estimated_rows: None,
                estimated_width: None,
                filter: None,
                scan_direction: None,
                raw_detail: Some(format!("Parallel workers: {workers}")),
                children: vec![relexpr_to_explain_node(input)],
            }
        }

        RelExpr::Sort { input, .. } | RelExpr::IncrementalSort { input, .. } => {
            let node_type = NodeType::Sort;

            let raw_detail = if matches!(expr, RelExpr::IncrementalSort { .. }) {
                Some("Incremental Sort".to_string())
            } else {
                None
            };

            ExplainNode {
                node_type,
                join_type: None,
                relation: None,
                index_name: None,
                startup_cost: None,
                total_cost: None,
                estimated_rows: None,
                estimated_width: None,
                filter: None,
                scan_direction: None,
                raw_detail,
                children: vec![relexpr_to_explain_node(input)],
            }
        }

        RelExpr::Limit { input, .. } => ExplainNode {
            node_type: NodeType::Limit,
            join_type: None,
            relation: None,
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: None,
            children: vec![relexpr_to_explain_node(input)],
        },

        RelExpr::Union { left, right, all } => ExplainNode {
            node_type: if *all {
                NodeType::Append
            } else {
                NodeType::SetOp
            },
            join_type: None,
            relation: None,
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: Some(if *all {
                "UNION ALL".to_string()
            } else {
                "UNION".to_string()
            }),
            children: vec![
                relexpr_to_explain_node(left),
                relexpr_to_explain_node(right),
            ],
        },

        RelExpr::Intersect { left, right, .. } => ExplainNode {
            node_type: NodeType::SetOp,
            join_type: None,
            relation: None,
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: Some("INTERSECT".to_string()),
            children: vec![
                relexpr_to_explain_node(left),
                relexpr_to_explain_node(right),
            ],
        },

        RelExpr::Except { left, right, .. } => ExplainNode {
            node_type: NodeType::SetOp,
            join_type: None,
            relation: None,
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: Some("EXCEPT".to_string()),
            children: vec![
                relexpr_to_explain_node(left),
                relexpr_to_explain_node(right),
            ],
        },

        RelExpr::Distinct { input } => ExplainNode {
            node_type: NodeType::Unique,
            join_type: None,
            relation: None,
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: None,
            children: vec![relexpr_to_explain_node(input)],
        },

        RelExpr::Window { input, .. } => ExplainNode {
            node_type: NodeType::WindowAgg,
            join_type: None,
            relation: None,
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: None,
            children: vec![relexpr_to_explain_node(input)],
        },

        RelExpr::Values { .. } => ExplainNode {
            node_type: NodeType::ValuesScan,
            join_type: None,
            relation: None,
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: None,
            children: Vec::new(),
        },

        RelExpr::CTE {
            name,
            definition,
            body,
        } => ExplainNode {
            node_type: NodeType::CteScan,
            join_type: None,
            relation: Some(name.clone()),
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: None,
            children: vec![
                relexpr_to_explain_node(definition),
                relexpr_to_explain_node(body),
            ],
        },

        RelExpr::RecursiveCTE {
            name,
            base_case,
            recursive_case,
            body,
            ..
        } => ExplainNode {
            node_type: NodeType::CteScan,
            join_type: None,
            relation: Some(name.clone()),
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: Some("Recursive CTE".to_string()),
            children: vec![
                relexpr_to_explain_node(base_case),
                relexpr_to_explain_node(recursive_case),
                relexpr_to_explain_node(body),
            ],
        },

        RelExpr::Unnest { input, alias, .. } => {
            let children = input
                .as_ref()
                .map(|i| vec![relexpr_to_explain_node(i)])
                .unwrap_or_default();

            ExplainNode {
                node_type: NodeType::FunctionScan,
                join_type: None,
                relation: alias.clone(),
                index_name: None,
                startup_cost: None,
                total_cost: None,
                estimated_rows: None,
                estimated_width: None,
                filter: None,
                scan_direction: None,
                raw_detail: Some("UNNEST".to_string()),
                children,
            }
        }

        RelExpr::MultiUnnest { .. } => ExplainNode {
            node_type: NodeType::FunctionScan,
            join_type: None,
            relation: None,
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: Some("Multi-UNNEST".to_string()),
            children: Vec::new(),
        },

        RelExpr::TableFunction { name, input, .. } => {
            let children = input
                .as_ref()
                .map(|i| vec![relexpr_to_explain_node(i)])
                .unwrap_or_default();

            ExplainNode {
                node_type: NodeType::FunctionScan,
                join_type: None,
                relation: Some(name.clone()),
                index_name: None,
                startup_cost: None,
                total_cost: None,
                estimated_rows: None,
                estimated_width: None,
                filter: None,
                scan_direction: None,
                raw_detail: None,
                children,
            }
        }

        RelExpr::RowPattern { input, .. } => ExplainNode {
            node_type: NodeType::Other,
            join_type: None,
            relation: None,
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: Some("Row Pattern Recognition".to_string()),
            children: vec![relexpr_to_explain_node(input)],
        },

        RelExpr::ParallelScan { table, workers } => ExplainNode {
            node_type: NodeType::SeqScan,
            join_type: None,
            relation: Some(table.clone()),
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: Some(format!("Parallel Seq Scan (workers: {workers})")),
            children: Vec::new(),
        },

        RelExpr::Gather { input, workers } => ExplainNode {
            node_type: NodeType::Gather,
            join_type: None,
            relation: None,
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: Some(format!("Workers: {workers}")),
            children: vec![relexpr_to_explain_node(input)],
        },

        RelExpr::MvScan { view_name, .. } => ExplainNode {
            node_type: NodeType::SeqScan,
            join_type: None,
            relation: Some(view_name.clone()),
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: Some("Materialized View Scan".to_string()),
            children: Vec::new(),
        },
        RelExpr::TopK { input, .. } => ExplainNode {
            node_type: NodeType::Limit,
            join_type: None,
            relation: None,
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: Some("Vector TopK Scan".to_string()),
            children: vec![relexpr_to_explain_node(input)],
        },
        RelExpr::VectorFilter { input, .. } => ExplainNode {
            node_type: NodeType::SeqScan,
            join_type: None,
            relation: None,
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: Some("Vector Distance Filter".to_string()),
            children: vec![relexpr_to_explain_node(input)],
        },

        RelExpr::Insert { table, source, .. } => ExplainNode {
            node_type: NodeType::Other,
            join_type: None,
            relation: Some(table.clone()),
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: Some("Insert".to_string()),
            children: vec![relexpr_to_explain_node(source)],
        },

        RelExpr::Update {
            table,
            filter,
            from,
            ..
        } => {
            let mut children = Vec::new();
            if let Some(f) = from {
                children.push(relexpr_to_explain_node(f));
            }
            ExplainNode {
                node_type: NodeType::Other,
                join_type: None,
                relation: Some(table.clone()),
                index_name: None,
                startup_cost: None,
                total_cost: None,
                estimated_rows: None,
                estimated_width: None,
                filter: filter.as_ref().map(|f| format!("{f:?}")),
                scan_direction: None,
                raw_detail: Some("Update".to_string()),
                children,
            }
        }

        RelExpr::Delete {
            table,
            filter,
            using,
            ..
        } => {
            let mut children = Vec::new();
            if let Some(u) = using {
                children.push(relexpr_to_explain_node(u));
            }
            ExplainNode {
                node_type: NodeType::Other,
                join_type: None,
                relation: Some(table.clone()),
                index_name: None,
                startup_cost: None,
                total_cost: None,
                estimated_rows: None,
                estimated_width: None,
                filter: filter.as_ref().map(|f| format!("{f:?}")),
                scan_direction: None,
                raw_detail: Some("Delete".to_string()),
                children,
            }
        }

        RelExpr::Merge {
            target,
            source,
            on,
            ..
        } => ExplainNode {
            node_type: NodeType::Other,
            join_type: None,
            relation: Some(target.clone()),
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: Some(format!("{on:?}")),
            scan_direction: None,
            raw_detail: Some("Merge".to_string()),
            children: vec![relexpr_to_explain_node(source)],
        },

        RelExpr::GraphTable { graph, .. } => ExplainNode {
            node_type: NodeType::Other,
            join_type: None,
            relation: Some(graph.clone()),
            index_name: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            estimated_width: None,
            filter: None,
            scan_direction: None,
            raw_detail: Some("GraphTable".to_string()),
            children: vec![],
        },
    }
}

#[cfg(test)]
mod formatter_tests {
    use super::*;

    fn create_test_node() -> ExplainNode {
        ExplainNode {
            node_type: NodeType::IndexOnlyScan,
            join_type: None,
            relation: Some("orders_test".to_string()),
            index_name: Some("orders_test_shipping_date_order_id_idx".to_string()),
            startup_cost: Some(0.43),
            total_cost: Some(4.45),
            estimated_rows: Some(1.0),
            estimated_width: Some(4),
            filter: Some(
                "((shipping_date >= '2022-05-01') AND (shipping_date <= '2022-05-01'))".to_string(),
            ),
            scan_direction: Some("Forward".to_string()),
            raw_detail: None,
            children: Vec::new(),
        }
    }

    fn create_test_tree() -> ExplainNode {
        let index_scan = create_test_node();

        let sort = ExplainNode {
            node_type: NodeType::Sort,
            join_type: None,
            relation: None,
            index_name: None,
            startup_cost: Some(4.46),
            total_cost: Some(4.46),
            estimated_rows: Some(1.0),
            estimated_width: Some(4),
            filter: None,
            scan_direction: None,
            raw_detail: None,
            children: vec![index_scan],
        };

        ExplainNode {
            node_type: NodeType::Limit,
            join_type: None,
            relation: None,
            index_name: None,
            startup_cost: Some(4.46),
            total_cost: Some(4.46),
            estimated_rows: Some(1.0),
            estimated_width: Some(4),
            filter: None,
            scan_direction: None,
            raw_detail: None,
            children: vec![sort],
        }
    }

    #[test]
    fn format_postgres_simple_node() {
        let node = create_test_node();
        let output = format_postgres_explain(&node);

        assert!(output.contains("Index Only Scan"));
        assert!(output.contains("orders_test"));
        assert!(output.contains("orders_test_shipping_date_order_id_idx"));
        assert!(output.contains("cost=0.43..4.45"));
        assert!(output.contains("rows=1"));
        assert!(output.contains("width=4"));
        assert!(output.contains("Filter:"));
        assert!(output.contains("shipping_date"));
    }

    #[test]
    fn format_postgres_tree() {
        let tree = create_test_tree();
        let output = format_postgres_explain(&tree);

        assert!(output.contains("Limit"));
        assert!(output.contains("Sort"));
        assert!(output.contains("Index Only Scan"));
        assert!(output.contains("   ->  "));
    }

    #[test]
    fn format_mysql_simple_node() {
        let node = create_test_node();
        let output = format_mysql_explain(&node);

        assert!(output.contains("Index Only Scan"));
        assert!(output.contains("orders_test"));
        assert!(output.contains("orders_test_shipping_date_order_id_idx"));
        assert!(output.contains("rows=1"));
        assert!(output.contains("cost=4.45"));
    }

    #[test]
    fn format_sqlite_simple_node() {
        let node = create_test_node();
        let output = format_sqlite_explain(&node);

        assert!(output.contains("SEARCH"));
        assert!(output.contains("TABLE"));
        assert!(output.contains("orders_test"));
        assert!(output.contains("COVERING INDEX"));
        assert!(output.contains("orders_test_shipping_date_order_id_idx"));
    }

    #[test]
    fn format_sqlite_tree() {
        let tree = create_test_tree();
        let output = format_sqlite_explain(&tree);

        assert!(output.contains("LIMIT"));
        assert!(output.contains("USE TEMP B-TREE FOR ORDER BY"));
        assert!(output.contains("SEARCH"));
    }

    #[test]
    fn postgres_format_preserves_hierarchy() {
        let tree = create_test_tree();
        let output = format_postgres_explain(&tree);

        // Check that output has proper indentation
        let lines: Vec<&str> = output.lines().collect();
        assert!(lines.len() >= 3);

        // Root should have no leading spaces before node name
        assert!(lines[0].starts_with("Limit"));

        // Children should be indented with arrow
        assert!(lines.iter().any(|l| l.contains("   ->  Sort")));
        assert!(lines.iter().any(|l| l.contains("Index Only Scan")));
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod relexpr_conversion_tests {
    use super::*;
    use ra_core::algebra::{
        AggregateExpr, AggregateFunction, JoinType as RAJoinType, ProjectionColumn, RelExpr,
        SortDirection, SortKey,
    };
    use ra_core::expr::{ColumnRef, Const, Expr};

    #[test]
    fn convert_scan() {
        let expr = RelExpr::scan("users");
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::SeqScan);
        assert_eq!(node.relation.as_deref(), Some("users"));
        assert!(node.children.is_empty());
    }

    #[test]
    fn convert_index_scan() {
        let expr = RelExpr::IndexScan {
            table: "users".to_string(),
            column: "id".to_string(),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::IndexScan);
        assert_eq!(node.relation.as_deref(), Some("users"));
        assert_eq!(node.index_name.as_deref(), Some("idx_id"));
        assert_eq!(node.scan_direction.as_deref(), Some("Forward"));
    }

    #[test]
    fn convert_index_only_scan() {
        let expr = RelExpr::IndexOnlyScan {
            table: "users".to_string(),
            index: "idx_email".to_string(),
            columns: vec![],
            predicate: Expr::Const(Const::Bool(true)),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::IndexOnlyScan);
        assert_eq!(node.relation.as_deref(), Some("users"));
        assert_eq!(node.index_name.as_deref(), Some("idx_email"));
        assert!(node.filter.is_some());
    }

    #[test]
    fn convert_filter() {
        let expr = RelExpr::scan("users").filter(Expr::Const(Const::Bool(true)));
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::SeqScan);
        assert!(node.filter.is_some());
    }

    #[test]
    fn convert_project() {
        let expr = RelExpr::scan("users").project(vec![ProjectionColumn {
            expr: Expr::Column(ColumnRef::new("id")),
            alias: None,
        }]);
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::SeqScan);
    }

    #[test]
    fn convert_inner_join() {
        let expr = RelExpr::Join {
            join_type: RAJoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::HashJoin);
        assert_eq!(node.join_type, Some(JoinType::Inner));
        assert_eq!(node.children.len(), 2);
        assert!(node.filter.is_some());
    }

    #[test]
    fn convert_left_outer_join() {
        let expr = RelExpr::Join {
            join_type: RAJoinType::LeftOuter,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::HashJoin);
        assert_eq!(node.join_type, Some(JoinType::Left));
    }

    #[test]
    fn convert_cross_join() {
        let expr = RelExpr::Join {
            join_type: RAJoinType::Cross,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::NestedLoop);
        assert_eq!(node.join_type, Some(JoinType::Cross));
    }

    #[test]
    fn convert_semi_join() {
        let expr = RelExpr::Join {
            join_type: RAJoinType::Semi,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::NestedLoop);
        assert_eq!(node.join_type, Some(JoinType::Semi));
    }

    #[test]
    fn convert_anti_join() {
        let expr = RelExpr::Join {
            join_type: RAJoinType::Anti,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::NestedLoop);
        assert_eq!(node.join_type, Some(JoinType::Anti));
    }

    #[test]
    fn convert_aggregate_with_group_by() {
        let expr = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("dept"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("cnt".to_string()),
            }],
            input: Box::new(RelExpr::scan("employees")),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::GroupAggregate);
        assert_eq!(node.children.len(), 1);
    }

    #[test]
    fn convert_aggregate_without_group_by() {
        let expr = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(Expr::Column(ColumnRef::new("amount"))),
                distinct: false,
                alias: None,
            }],
            input: Box::new(RelExpr::scan("orders")),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::HashAggregate);
    }

    #[test]
    fn convert_sort() {
        let expr = RelExpr::Sort {
            keys: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("name")),
                direction: SortDirection::Asc,
                nulls: ra_core::algebra::NullOrdering::Last,
            }],
            input: Box::new(RelExpr::scan("users")),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::Sort);
        assert_eq!(node.children.len(), 1);
    }

    #[test]
    fn convert_incremental_sort() {
        let expr = RelExpr::IncrementalSort {
            prefix_keys: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("dept")),
                direction: SortDirection::Asc,
                nulls: ra_core::algebra::NullOrdering::Last,
            }],
            suffix_keys: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("name")),
                direction: SortDirection::Asc,
                nulls: ra_core::algebra::NullOrdering::Last,
            }],
            input: Box::new(RelExpr::scan("employees")),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::Sort);
        assert_eq!(node.raw_detail.as_deref(), Some("Incremental Sort"));
    }

    #[test]
    fn convert_limit() {
        let expr = RelExpr::scan("users").limit(10, 5);
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::Limit);
        assert_eq!(node.children.len(), 1);
    }

    #[test]
    fn convert_union_all() {
        let expr = RelExpr::Union {
            all: true,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::Append);
        assert_eq!(node.raw_detail.as_deref(), Some("UNION ALL"));
        assert_eq!(node.children.len(), 2);
    }

    #[test]
    fn convert_union() {
        let expr = RelExpr::Union {
            all: false,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::SetOp);
        assert_eq!(node.raw_detail.as_deref(), Some("UNION"));
    }

    #[test]
    fn convert_intersect() {
        let expr = RelExpr::Intersect {
            all: false,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::SetOp);
        assert_eq!(node.raw_detail.as_deref(), Some("INTERSECT"));
    }

    #[test]
    fn convert_except() {
        let expr = RelExpr::Except {
            all: false,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::SetOp);
        assert_eq!(node.raw_detail.as_deref(), Some("EXCEPT"));
    }

    #[test]
    fn convert_distinct() {
        let expr = RelExpr::scan("users").distinct();
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::Unique);
        assert_eq!(node.children.len(), 1);
    }

    #[test]
    fn convert_window() {
        let expr = RelExpr::Window {
            functions: vec![],
            input: Box::new(RelExpr::scan("sales")),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::WindowAgg);
        assert_eq!(node.children.len(), 1);
    }

    #[test]
    fn convert_values() {
        let expr = RelExpr::Values {
            rows: vec![vec![Expr::Const(Const::Int(1))]],
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::ValuesScan);
        assert!(node.children.is_empty());
    }

    #[test]
    fn convert_cte() {
        let expr = RelExpr::CTE {
            name: "temp".to_string(),
            definition: Box::new(RelExpr::scan("base")),
            body: Box::new(RelExpr::scan("temp")),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::CteScan);
        assert_eq!(node.relation.as_deref(), Some("temp"));
        assert_eq!(node.children.len(), 2);
    }

    #[test]
    fn convert_recursive_cte() {
        let expr = RelExpr::RecursiveCTE {
            name: "reachable".to_string(),
            base_case: Box::new(RelExpr::scan("edges")),
            recursive_case: Box::new(RelExpr::scan("edges")),
            body: Box::new(RelExpr::scan("reachable")),
            cycle_detection: None,
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::CteScan);
        assert_eq!(node.relation.as_deref(), Some("reachable"));
        assert_eq!(node.raw_detail.as_deref(), Some("Recursive CTE"));
        assert_eq!(node.children.len(), 3);
    }

    #[test]
    fn convert_bitmap_index_scan() {
        let expr = RelExpr::BitmapIndexScan {
            table: "users".to_string(),
            index: "idx_age".to_string(),
            predicate: Expr::Const(Const::Bool(true)),
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::BitmapIndexScan);
        assert_eq!(node.relation.as_deref(), Some("users"));
        assert_eq!(node.index_name.as_deref(), Some("idx_age"));
    }

    #[test]
    fn convert_bitmap_heap_scan() {
        let expr = RelExpr::BitmapHeapScan {
            table: "users".to_string(),
            bitmap: Box::new(RelExpr::BitmapIndexScan {
                table: "users".to_string(),
                index: "idx_age".to_string(),
                predicate: Expr::Const(Const::Bool(true)),
            }),
            recheck_cond: None,
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::BitmapHeapScan);
        assert_eq!(node.relation.as_deref(), Some("users"));
        assert_eq!(node.children.len(), 1);
    }

    #[test]
    fn convert_bitmap_and() {
        let expr = RelExpr::BitmapAnd {
            inputs: vec![
                Box::new(RelExpr::BitmapIndexScan {
                    table: "users".to_string(),
                    index: "idx1".to_string(),
                    predicate: Expr::Const(Const::Bool(true)),
                }),
                Box::new(RelExpr::BitmapIndexScan {
                    table: "users".to_string(),
                    index: "idx2".to_string(),
                    predicate: Expr::Const(Const::Bool(true)),
                }),
            ],
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::BitmapIndexScan);
        assert_eq!(node.raw_detail.as_deref(), Some("BitmapAnd"));
        assert_eq!(node.children.len(), 2);
    }

    #[test]
    fn convert_bitmap_or() {
        let expr = RelExpr::BitmapOr {
            inputs: vec![Box::new(RelExpr::BitmapIndexScan {
                table: "users".to_string(),
                index: "idx1".to_string(),
                predicate: Expr::Const(Const::Bool(true)),
            })],
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::BitmapIndexScan);
        assert_eq!(node.raw_detail.as_deref(), Some("BitmapOr"));
    }

    #[test]
    fn convert_parallel_scan() {
        let expr = RelExpr::ParallelScan {
            table: "big_table".to_string(),
            workers: 4,
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::SeqScan);
        assert_eq!(node.relation.as_deref(), Some("big_table"));
        assert!(node.raw_detail.as_ref().unwrap().contains("workers: 4"));
    }

    #[test]
    fn convert_parallel_hash_join() {
        let expr = RelExpr::ParallelHashJoin {
            join_type: RAJoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
            workers: 8,
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::HashJoin);
        assert_eq!(node.join_type, Some(JoinType::Inner));
        assert!(node.raw_detail.as_ref().unwrap().contains("workers: 8"));
    }

    #[test]
    fn convert_parallel_aggregate() {
        let expr = RelExpr::ParallelAggregate {
            group_by: vec![Expr::Column(ColumnRef::new("region"))],
            aggregates: vec![],
            input: Box::new(RelExpr::scan("sales")),
            workers: 4,
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::GroupAggregate);
        assert!(node.raw_detail.as_ref().unwrap().contains("workers: 4"));
    }

    #[test]
    fn convert_gather() {
        let expr = RelExpr::Gather {
            input: Box::new(RelExpr::ParallelScan {
                table: "t".to_string(),
                workers: 4,
            }),
            workers: 4,
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::Gather);
        assert!(node.raw_detail.as_ref().unwrap().contains("Workers: 4"));
        assert_eq!(node.children.len(), 1);
    }

    #[test]
    fn convert_mv_scan() {
        let expr = RelExpr::MvScan {
            view_name: "mv_sales".to_string(),
            alias: None,
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::SeqScan);
        assert_eq!(node.relation.as_deref(), Some("mv_sales"));
        assert_eq!(node.raw_detail.as_deref(), Some("Materialized View Scan"));
    }

    #[test]
    fn convert_unnest_standalone() {
        let expr = RelExpr::unnest(
            Expr::Column(ColumnRef::new("arr")),
            Some("elems".to_string()),
        );
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::FunctionScan);
        assert_eq!(node.relation.as_deref(), Some("elems"));
        assert_eq!(node.raw_detail.as_deref(), Some("UNNEST"));
        assert!(node.children.is_empty());
    }

    #[test]
    fn convert_unnest_lateral() {
        let expr = RelExpr::Unnest {
            expr: Expr::Column(ColumnRef::new("arr")),
            alias: None,
            input: Some(Box::new(RelExpr::scan("t"))),
            with_ordinality: false,
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::FunctionScan);
        assert_eq!(node.children.len(), 1);
    }

    #[test]
    fn convert_multi_unnest() {
        let expr = RelExpr::MultiUnnest {
            exprs: vec![
                Expr::Column(ColumnRef::new("arr1")),
                Expr::Column(ColumnRef::new("arr2")),
            ],
            aliases: vec![None, None],
            with_ordinality: false,
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::FunctionScan);
        assert_eq!(node.raw_detail.as_deref(), Some("Multi-UNNEST"));
    }

    #[test]
    fn convert_table_function() {
        let expr = RelExpr::table_function(
            "generate_series",
            vec![Expr::Const(Const::Int(1)), Expr::Const(Const::Int(10))],
            vec![("n".to_string(), "integer".to_string())],
        );
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::FunctionScan);
        assert_eq!(node.relation.as_deref(), Some("generate_series"));
    }

    #[test]
    fn convert_row_pattern() {
        use ra_core::row_pattern::{MatchMode, PatternExpr, SkipMode};

        let expr = RelExpr::RowPattern {
            input: Box::new(RelExpr::scan("stock_prices")),
            partition_by: vec![],
            order_by: vec![],
            pattern: PatternExpr::Var("A".to_string()),
            defines: vec![],
            measures: vec![],
            mode: MatchMode::OneRowPerMatch,
            skip_mode: SkipMode::PastLastRow,
        };
        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::Other);
        assert_eq!(node.raw_detail.as_deref(), Some("Row Pattern Recognition"));
        assert_eq!(node.children.len(), 1);
    }

    #[test]
    fn convert_complex_plan() {
        let expr = RelExpr::scan("orders")
            .filter(Expr::Const(Const::Bool(true)))
            .limit(100, 0);

        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::Limit);
        assert_eq!(node.children.len(), 1);
        assert_eq!(node.children[0].node_type, NodeType::SeqScan);
        assert!(node.children[0].filter.is_some());
    }

    #[test]
    fn convert_nested_join() {
        let expr = RelExpr::Join {
            join_type: RAJoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::Join {
                join_type: RAJoinType::Inner,
                condition: Expr::Const(Const::Bool(true)),
                left: Box::new(RelExpr::scan("a")),
                right: Box::new(RelExpr::scan("b")),
            }),
            right: Box::new(RelExpr::scan("c")),
        };

        let node = relexpr_to_explain_node(&expr);

        assert_eq!(node.node_type, NodeType::HashJoin);
        assert_eq!(node.children.len(), 2);
        assert_eq!(node.children[0].node_type, NodeType::HashJoin);
        assert_eq!(node.children[1].node_type, NodeType::SeqScan);
    }

    #[test]
    fn convert_all_join_types() {
        let join_types = vec![
            (RAJoinType::Inner, JoinType::Inner, NodeType::HashJoin),
            (RAJoinType::LeftOuter, JoinType::Left, NodeType::HashJoin),
            (RAJoinType::RightOuter, JoinType::Right, NodeType::HashJoin),
            (RAJoinType::FullOuter, JoinType::Full, NodeType::HashJoin),
            (RAJoinType::Cross, JoinType::Cross, NodeType::NestedLoop),
            (RAJoinType::Semi, JoinType::Semi, NodeType::NestedLoop),
            (RAJoinType::Anti, JoinType::Anti, NodeType::NestedLoop),
        ];

        for (ra_type, explain_type, node_type) in join_types {
            let expr = RelExpr::Join {
                join_type: ra_type,
                condition: Expr::Const(Const::Bool(true)),
                left: Box::new(RelExpr::scan("a")),
                right: Box::new(RelExpr::scan("b")),
            };
            let node = relexpr_to_explain_node(&expr);

            assert_eq!(node.node_type, node_type);
            assert_eq!(node.join_type, Some(explain_type));
        }
    }

    #[test]
    fn cost_estimates_are_none() {
        let expr = RelExpr::scan("users");
        let node = relexpr_to_explain_node(&expr);

        assert!(node.startup_cost.is_none());
        assert!(node.total_cost.is_none());
        assert!(node.estimated_rows.is_none());
        assert!(node.estimated_width.is_none());
    }

    #[test]
    fn convert_format_postgres() {
        let expr = RelExpr::scan("users").limit(10, 0);
        let node = relexpr_to_explain_node(&expr);
        let output = format_postgres_explain(&node);

        assert!(output.contains("Limit"));
        assert!(output.contains("Seq Scan"));
    }

    #[test]
    fn convert_format_mysql() {
        let expr = RelExpr::Join {
            join_type: RAJoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let node = relexpr_to_explain_node(&expr);
        let output = format_mysql_explain(&node);

        assert!(output.contains("Hash Join"));
    }

    #[test]
    fn convert_format_sqlite() {
        let expr = RelExpr::Sort {
            keys: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("name")),
                direction: SortDirection::Asc,
                nulls: ra_core::algebra::NullOrdering::Last,
            }],
            input: Box::new(RelExpr::scan("users")),
        };
        let node = relexpr_to_explain_node(&expr);
        let output = format_sqlite_explain(&node);

        assert!(output.contains("USE TEMP B-TREE FOR ORDER BY"));
    }
}
