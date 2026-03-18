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
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| {
        MetadataError::ExplainParse {
            message: format!("invalid JSON: {e}"),
        }
    })?;

    let plans = value.as_array().ok_or_else(|| MetadataError::ExplainParse {
        message: "expected JSON array".to_string(),
    })?;

    let first = plans.first().ok_or_else(|| MetadataError::ExplainParse {
        message: "empty plan array".to_string(),
    })?;

    let plan_obj = first.get("Plan").ok_or_else(|| MetadataError::ExplainParse {
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
        total_cost: value
            .get("Total Cost")
            .and_then(serde_json::Value::as_f64),
        estimated_rows: value
            .get("Plan Rows")
            .and_then(serde_json::Value::as_f64),
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
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| {
        MetadataError::ExplainParse {
            message: format!("invalid JSON: {e}"),
        }
    })?;

    let query_block =
        value
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

fn parse_mysql_query_block(
    block: &serde_json::Value,
) -> Result<ExplainNode, MetadataError> {
    if let Some(nested_loop) = block.get("nested_loop").and_then(serde_json::Value::as_array) {
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
        "range" | "ref" | "eq_ref" | "const" | "ref_or_null" | "fulltext" => {
            NodeType::IndexScan
        }
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

        let id: i64 = parts[0].trim().parse().map_err(|_| MetadataError::ExplainParse {
            message: format!("invalid id: {}", parts[0]),
        })?;
        let parent: i64 =
            parts[1].trim().parse().map_err(|_| MetadataError::ExplainParse {
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
        let grandchildren: Vec<&(i64, i64, String)> =
            nodes.iter().filter(|(_, p, _)| *p == child_data.0).collect();
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
    } else if upper.contains("USE TEMP B-TREE FOR ORDER BY")
        || upper.contains("SORT")
    {
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
    for keyword in &[
        "SCAN TABLE ", "SEARCH TABLE ", "SCAN ", "SEARCH ",
    ] {
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
        1 + self
            .children
            .iter()
            .map(Self::depth)
            .max()
            .unwrap_or(0)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
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
        assert_eq!(parse_pg_node_type("Index Only Scan"), NodeType::IndexOnlyScan);
        assert_eq!(parse_pg_node_type("Bitmap Index Scan"), NodeType::BitmapIndexScan);
        assert_eq!(parse_pg_node_type("Bitmap Heap Scan"), NodeType::BitmapHeapScan);
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
        assert_eq!(
            right.filter.as_deref(),
            Some("users.id = orders.user_id")
        );
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
        assert_eq!(
            plan.root.index_name.as_deref(),
            Some("idx_orders_user_id")
        );
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
        assert!(plan.root.children.iter().any(|c| c.node_type == NodeType::Sort)
            || plan.root.node_type == NodeType::Sort);
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
        let deserialized: ExplainPlan =
            serde_json::from_str(&json).expect("deserialize");
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
