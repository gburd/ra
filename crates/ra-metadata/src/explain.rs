//! EXPLAIN plan parser for PostgreSQL, MySQL, and SQLite.
//!
//! Parses the output of EXPLAIN commands from each database engine
//! into a common [`ExplainPlan`] representation that can be compared
//! with RA optimizer plans.

use serde::{Deserialize, Serialize};

use crate::error::MetadataError;

/// A parsed EXPLAIN plan in a database-agnostic format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExplainPlan {
    /// The database engine that produced this plan.
    pub engine: String,
    /// The SQL query that was explained.
    pub query: String,
    /// Root node of the plan tree.
    pub root: PlanNode,
    /// Total estimated cost (engine-specific units).
    pub total_cost: Option<f64>,
    /// Total estimated rows returned.
    pub total_rows: Option<f64>,
}

/// A node in an EXPLAIN plan tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanNode {
    /// Node type (e.g., "Seq Scan", "Hash Join", "Index Scan").
    pub node_type: String,
    /// Relation (table) name, if applicable.
    pub relation: Option<String>,
    /// Alias for the relation, if any.
    pub alias: Option<String>,
    /// Join type for join nodes.
    pub join_type: Option<String>,
    /// Index name used, if any.
    pub index_name: Option<String>,
    /// Filter condition applied at this node.
    pub filter: Option<String>,
    /// Join condition (for join nodes).
    pub join_condition: Option<String>,
    /// Sort keys (for sort nodes).
    pub sort_keys: Option<Vec<String>>,
    /// Startup cost.
    pub startup_cost: Option<f64>,
    /// Total cost.
    pub total_cost: Option<f64>,
    /// Estimated rows.
    pub estimated_rows: Option<f64>,
    /// Estimated row width in bytes.
    pub row_width: Option<u32>,
    /// Child plan nodes.
    pub children: Vec<PlanNode>,
    /// Extra engine-specific properties.
    pub extra: serde_json::Value,
}

impl PlanNode {
    /// Create a new plan node with the given type.
    #[must_use]
    pub fn new(node_type: impl Into<String>) -> Self {
        Self {
            node_type: node_type.into(),
            relation: None,
            alias: None,
            join_type: None,
            index_name: None,
            filter: None,
            join_condition: None,
            sort_keys: None,
            startup_cost: None,
            total_cost: None,
            estimated_rows: None,
            row_width: None,
            children: Vec::new(),
            extra: serde_json::Value::Null,
        }
    }

    /// Recursively collect all node types in pre-order.
    #[must_use]
    pub fn all_node_types(&self) -> Vec<&str> {
        let mut types = vec![self.node_type.as_str()];
        for child in &self.children {
            types.extend(child.all_node_types());
        }
        types
    }

    /// Find the join algorithm used (if this is a join node).
    #[must_use]
    pub fn join_algorithm(&self) -> Option<JoinAlgorithm> {
        let lower = self.node_type.to_lowercase();
        if lower.contains("nested loop") {
            Some(JoinAlgorithm::NestedLoop)
        } else if lower.contains("hash join")
            || lower.contains("hash")
                && lower.contains("join")
        {
            Some(JoinAlgorithm::Hash)
        } else if lower.contains("merge join")
            || lower.contains("merge")
                && lower.contains("join")
        {
            Some(JoinAlgorithm::SortMerge)
        } else {
            None
        }
    }

    /// Check whether this node uses an index scan.
    #[must_use]
    pub fn uses_index(&self) -> bool {
        let lower = self.node_type.to_lowercase();
        lower.contains("index scan")
            || lower.contains("index only scan")
            || lower.contains("bitmap index scan")
            || lower.contains("idx")
    }
}

/// Join algorithm identified from an EXPLAIN plan.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum JoinAlgorithm {
    /// Nested-loop join.
    NestedLoop,
    /// Hash join.
    Hash,
    /// Sort-merge join.
    SortMerge,
}

impl std::fmt::Display for JoinAlgorithm {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::NestedLoop => write!(f, "Nested Loop"),
            Self::Hash => write!(f, "Hash Join"),
            Self::SortMerge => write!(f, "Sort-Merge Join"),
        }
    }
}

// ── PostgreSQL EXPLAIN JSON parser ──────────────────────────

/// Parse PostgreSQL `EXPLAIN (FORMAT JSON)` output.
pub fn parse_postgres_explain(
    json_text: &str,
    query: &str,
) -> Result<ExplainPlan, MetadataError> {
    let parsed: serde_json::Value =
        serde_json::from_str(json_text).map_err(|e| {
            MetadataError::ExplainParseFailed(format!(
                "invalid JSON: {e}"
            ))
        })?;

    let plan_array = parsed.as_array().ok_or_else(|| {
        MetadataError::ExplainParseFailed(
            "expected top-level JSON array".to_string(),
        )
    })?;

    let first = plan_array.first().ok_or_else(|| {
        MetadataError::ExplainParseFailed(
            "empty EXPLAIN JSON array".to_string(),
        )
    })?;

    let plan_obj = first.get("Plan").ok_or_else(|| {
        MetadataError::ExplainParseFailed(
            "missing 'Plan' key in EXPLAIN output".to_string(),
        )
    })?;

    let root = parse_pg_node(plan_obj)?;

    Ok(ExplainPlan {
        engine: "PostgreSQL".to_string(),
        query: query.to_string(),
        root,
        total_cost: first
            .get("Plan")
            .and_then(|p| p.get("Total Cost"))
            .and_then(serde_json::Value::as_f64),
        total_rows: first
            .get("Plan")
            .and_then(|p| p.get("Plan Rows"))
            .and_then(serde_json::Value::as_f64),
    })
}

fn parse_pg_node(
    obj: &serde_json::Value,
) -> Result<PlanNode, MetadataError> {
    let node_type = obj
        .get("Node Type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Unknown")
        .to_string();

    let mut node = PlanNode::new(node_type);

    node.relation = obj
        .get("Relation Name")
        .and_then(serde_json::Value::as_str)
        .map(String::from);
    node.alias = obj
        .get("Alias")
        .and_then(serde_json::Value::as_str)
        .map(String::from);
    node.join_type = obj
        .get("Join Type")
        .and_then(serde_json::Value::as_str)
        .map(String::from);
    node.index_name = obj
        .get("Index Name")
        .and_then(serde_json::Value::as_str)
        .map(String::from);
    node.filter = obj
        .get("Filter")
        .and_then(serde_json::Value::as_str)
        .map(String::from);
    node.join_condition = obj
        .get("Join Filter")
        .or_else(|| obj.get("Hash Cond"))
        .or_else(|| obj.get("Merge Cond"))
        .and_then(serde_json::Value::as_str)
        .map(String::from);
    node.sort_keys = obj.get("Sort Key").and_then(|v| {
        v.as_array().map(|arr| {
            arr.iter()
                .filter_map(serde_json::Value::as_str)
                .map(String::from)
                .collect()
        })
    });
    node.startup_cost = obj
        .get("Startup Cost")
        .and_then(serde_json::Value::as_f64);
    node.total_cost = obj
        .get("Total Cost")
        .and_then(serde_json::Value::as_f64);
    node.estimated_rows = obj
        .get("Plan Rows")
        .and_then(serde_json::Value::as_f64);
    node.row_width = obj
        .get("Plan Width")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok());

    if let Some(plans) = obj.get("Plans") {
        if let Some(arr) = plans.as_array() {
            for child in arr {
                node.children.push(parse_pg_node(child)?);
            }
        }
    }

    Ok(node)
}

// ── MySQL EXPLAIN JSON parser ───────────────────────────────

/// Parse MySQL `EXPLAIN FORMAT=JSON` output.
pub fn parse_mysql_explain(
    json_text: &str,
    query: &str,
) -> Result<ExplainPlan, MetadataError> {
    let parsed: serde_json::Value =
        serde_json::from_str(json_text).map_err(|e| {
            MetadataError::ExplainParseFailed(format!(
                "invalid JSON: {e}"
            ))
        })?;

    let query_block =
        parsed.get("query_block").ok_or_else(|| {
            MetadataError::ExplainParseFailed(
                "missing 'query_block' in MySQL EXPLAIN"
                    .to_string(),
            )
        })?;

    let root = parse_mysql_node(query_block)?;

    Ok(ExplainPlan {
        engine: "MySQL".to_string(),
        query: query.to_string(),
        root,
        total_cost: query_block
            .get("cost_info")
            .and_then(|c| c.get("query_cost"))
            .and_then(serde_json::Value::as_str)
            .and_then(|s| s.parse::<f64>().ok()),
        total_rows: None,
    })
}

fn parse_mysql_node(
    obj: &serde_json::Value,
) -> Result<PlanNode, MetadataError> {
    if let Some(table) = obj.get("table") {
        return parse_mysql_table_node(table);
    }

    if let Some(nested) = obj.get("nested_loop") {
        return parse_mysql_nested_loop(nested);
    }

    if let Some(ordering) = obj.get("ordering_operation") {
        return parse_mysql_ordering(ordering);
    }

    if let Some(grouping) = obj.get("grouping_operation") {
        return parse_mysql_grouping(grouping);
    }

    let mut node = PlanNode::new("Query Block");
    node.estimated_rows = obj
        .get("select_id")
        .and_then(serde_json::Value::as_f64);

    if let Some(table) = obj.get("table") {
        node.children.push(parse_mysql_table_node(table)?);
    }

    Ok(node)
}

#[allow(clippy::unnecessary_wraps)]
fn parse_mysql_table_node(
    table: &serde_json::Value,
) -> Result<PlanNode, MetadataError> {
    let access_type = table
        .get("access_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("ALL");

    let node_type = match access_type {
        "ref" | "eq_ref" | "ref_or_null" | "range" => {
            "Index Scan"
        }
        "index" => "Index Only Scan",
        "const" | "system" => "Const Lookup",
        "ALL" => "Full Table Scan",
        other => other,
    };

    let mut node = PlanNode::new(node_type);
    node.relation = table
        .get("table_name")
        .and_then(serde_json::Value::as_str)
        .map(String::from);
    node.index_name = table
        .get("key")
        .and_then(serde_json::Value::as_str)
        .map(String::from);
    node.estimated_rows = table
        .get("rows_examined_per_scan")
        .or_else(|| table.get("rows_produced_per_join"))
        .and_then(serde_json::Value::as_f64);
    node.filter = table
        .get("attached_condition")
        .and_then(serde_json::Value::as_str)
        .map(String::from);
    node.total_cost = table
        .get("cost_info")
        .and_then(|c| c.get("read_cost"))
        .and_then(serde_json::Value::as_str)
        .and_then(|s| s.parse::<f64>().ok());

    Ok(node)
}

fn parse_mysql_nested_loop(
    arr: &serde_json::Value,
) -> Result<PlanNode, MetadataError> {
    let mut node = PlanNode::new("Nested Loop");
    if let Some(items) = arr.as_array() {
        for item in items {
            if let Some(table) = item.get("table") {
                node.children
                    .push(parse_mysql_table_node(table)?);
            }
        }
    }
    Ok(node)
}

fn parse_mysql_ordering(
    obj: &serde_json::Value,
) -> Result<PlanNode, MetadataError> {
    let mut node = PlanNode::new("Sort");
    node.extra = obj
        .get("using_filesort")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    if let Some(nested) = obj.get("nested_loop") {
        node.children.push(parse_mysql_nested_loop(nested)?);
    } else if let Some(table) = obj.get("table") {
        node.children
            .push(parse_mysql_table_node(table)?);
    }

    Ok(node)
}

fn parse_mysql_grouping(
    obj: &serde_json::Value,
) -> Result<PlanNode, MetadataError> {
    let mut node = PlanNode::new("Aggregate");

    if let Some(ordering) = obj.get("ordering_operation") {
        node.children.push(parse_mysql_ordering(ordering)?);
    } else if let Some(nested) = obj.get("nested_loop") {
        node.children.push(parse_mysql_nested_loop(nested)?);
    } else if let Some(table) = obj.get("table") {
        node.children
            .push(parse_mysql_table_node(table)?);
    }

    Ok(node)
}

// ── SQLite EXPLAIN QUERY PLAN parser ────────────────────────

/// Parse SQLite `EXPLAIN QUERY PLAN` text output.
///
/// SQLite EXPLAIN QUERY PLAN produces lines like:
/// ```text
/// QUERY PLAN
/// |--SCAN orders
/// |--SEARCH customers USING INDEX idx_name (name=?)
/// `--USE TEMP B-TREE FOR ORDER BY
/// ```
///
/// Or in newer SQLite (tree format):
/// ```text
/// id  parent  notused  detail
/// 2   0       0        SCAN orders
/// 3   0       0        SEARCH customers USING INDEX idx_name (name=?)
/// ```
pub fn parse_sqlite_explain(
    text: &str,
    query: &str,
) -> Result<ExplainPlan, MetadataError> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        return Err(MetadataError::ExplainParseFailed(
            "empty EXPLAIN QUERY PLAN output".to_string(),
        ));
    }

    let mut children = Vec::new();

    for line in &lines {
        if let Some(node) = parse_sqlite_detail_line(line) {
            children.push(node);
        }
    }

    let root = if children.len() == 1 {
        children.remove(0)
    } else {
        let mut root = PlanNode::new("Query Plan");
        root.children = children;
        root
    };

    Ok(ExplainPlan {
        engine: "SQLite".to_string(),
        query: query.to_string(),
        root,
        total_cost: None,
        total_rows: None,
    })
}

fn parse_sqlite_detail_line(line: &str) -> Option<PlanNode> {
    let detail = line
        .trim_start_matches(|c: char| {
            c == '|' || c == '-' || c == '`' || c == ' '
        })
        .trim();

    if detail.is_empty()
        || detail == "QUERY PLAN"
        || detail.starts_with("id")
    {
        return None;
    }

    // Try tabular format: "id parent notused detail"
    let parts: Vec<&str> = detail.splitn(4, char::is_whitespace).collect();
    let detail_text = if parts.len() == 4 {
        if parts[0].parse::<u32>().is_ok()
            && parts[1].parse::<u32>().is_ok()
        {
            parts[3]
        } else {
            detail
        }
    } else {
        detail
    };

    let lower = detail_text.to_lowercase();

    if lower.starts_with("scan") {
        let table = extract_table_name(detail_text, "SCAN");
        let mut node = PlanNode::new("Seq Scan");
        node.relation = table;
        return Some(node);
    }

    if lower.starts_with("search") {
        let table = extract_table_name(detail_text, "SEARCH");
        let idx = extract_using_index(detail_text);
        let mut node = PlanNode::new("Index Scan");
        node.relation = table;
        node.index_name = idx;
        return Some(node);
    }

    if lower.contains("use temp b-tree for order by") {
        return Some(PlanNode::new("Sort"));
    }

    if lower.contains("use temp b-tree for group by") {
        return Some(PlanNode::new("Aggregate"));
    }

    if lower.contains("compound subqueries") {
        return Some(PlanNode::new("Union"));
    }

    let mut node = PlanNode::new(detail_text);
    node.extra =
        serde_json::Value::String(detail_text.to_string());
    Some(node)
}

fn extract_table_name(
    detail: &str,
    prefix: &str,
) -> Option<String> {
    let after = detail
        .strip_prefix(prefix)?
        .trim_start();
    let table = after
        .split_whitespace()
        .next()?;
    Some(table.to_string())
}

fn extract_using_index(detail: &str) -> Option<String> {
    let idx_start = detail.find("USING INDEX ")?;
    let rest = &detail[idx_start + "USING INDEX ".len()..];
    let end = rest.find([' ', '('])
        .unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pg_simple_seq_scan() {
        let json = r#"[{
            "Plan": {
                "Node Type": "Seq Scan",
                "Relation Name": "users",
                "Alias": "users",
                "Startup Cost": 0.0,
                "Total Cost": 35.5,
                "Plan Rows": 2550,
                "Plan Width": 64
            }
        }]"#;

        let plan =
            parse_postgres_explain(json, "SELECT * FROM users");
        let plan = plan.expect("should parse");
        assert_eq!(plan.engine, "PostgreSQL");
        assert_eq!(plan.root.node_type, "Seq Scan");
        assert_eq!(
            plan.root.relation.as_deref(),
            Some("users")
        );
        assert_eq!(plan.root.estimated_rows, Some(2550.0));
    }

    #[test]
    fn parse_pg_hash_join() {
        let json = r#"[{
            "Plan": {
                "Node Type": "Hash Join",
                "Join Type": "Inner",
                "Hash Cond": "(o.customer_id = c.id)",
                "Startup Cost": 10.0,
                "Total Cost": 100.5,
                "Plan Rows": 500,
                "Plan Width": 128,
                "Plans": [
                    {
                        "Node Type": "Seq Scan",
                        "Relation Name": "orders",
                        "Alias": "o",
                        "Startup Cost": 0.0,
                        "Total Cost": 50.0,
                        "Plan Rows": 1000,
                        "Plan Width": 64
                    },
                    {
                        "Node Type": "Hash",
                        "Startup Cost": 10.0,
                        "Total Cost": 10.0,
                        "Plan Rows": 100,
                        "Plan Width": 64,
                        "Plans": [
                            {
                                "Node Type": "Seq Scan",
                                "Relation Name": "customers",
                                "Alias": "c",
                                "Startup Cost": 0.0,
                                "Total Cost": 10.0,
                                "Plan Rows": 100,
                                "Plan Width": 64
                            }
                        ]
                    }
                ]
            }
        }]"#;

        let plan = parse_postgres_explain(
            json,
            "SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id",
        )
        .expect("should parse");

        assert_eq!(plan.root.node_type, "Hash Join");
        assert_eq!(
            plan.root.join_type.as_deref(),
            Some("Inner")
        );
        assert_eq!(plan.root.children.len(), 2);
        assert!(plan.root.join_algorithm().is_some());
        assert_eq!(
            plan.root.join_algorithm(),
            Some(JoinAlgorithm::Hash)
        );
    }

    #[test]
    fn parse_pg_index_scan() {
        let json = r#"[{
            "Plan": {
                "Node Type": "Index Scan",
                "Relation Name": "users",
                "Index Name": "users_pkey",
                "Startup Cost": 0.29,
                "Total Cost": 8.30,
                "Plan Rows": 1,
                "Plan Width": 64,
                "Filter": "(id = 42)"
            }
        }]"#;

        let plan = parse_postgres_explain(
            json,
            "SELECT * FROM users WHERE id = 42",
        )
        .expect("should parse");
        assert!(plan.root.uses_index());
        assert_eq!(
            plan.root.index_name.as_deref(),
            Some("users_pkey")
        );
    }

    #[test]
    fn parse_mysql_simple_scan() {
        let json = r#"{
            "query_block": {
                "select_id": 1,
                "cost_info": { "query_cost": "35.50" },
                "table": {
                    "table_name": "users",
                    "access_type": "ALL",
                    "rows_examined_per_scan": 2550,
                    "rows_produced_per_join": 2550,
                    "cost_info": { "read_cost": "35.50" }
                }
            }
        }"#;

        let plan =
            parse_mysql_explain(json, "SELECT * FROM users")
                .expect("should parse");
        assert_eq!(plan.engine, "MySQL");
        assert_eq!(plan.total_cost, Some(35.50));
    }

    #[test]
    fn parse_mysql_index_scan() {
        let json = r#"{
            "query_block": {
                "select_id": 1,
                "table": {
                    "table_name": "users",
                    "access_type": "ref",
                    "key": "idx_email",
                    "rows_examined_per_scan": 1,
                    "attached_condition": "users.email = 'test@example.com'"
                }
            }
        }"#;

        let plan = parse_mysql_explain(
            json,
            "SELECT * FROM users WHERE email = 'test@example.com'",
        )
        .expect("should parse");
        assert_eq!(plan.root.node_type, "Index Scan");
        assert_eq!(
            plan.root.index_name.as_deref(),
            Some("idx_email")
        );
    }

    #[test]
    fn parse_mysql_nested_loop() {
        let json = r#"{
            "query_block": {
                "select_id": 1,
                "nested_loop": [
                    {
                        "table": {
                            "table_name": "orders",
                            "access_type": "ALL",
                            "rows_examined_per_scan": 1000
                        }
                    },
                    {
                        "table": {
                            "table_name": "customers",
                            "access_type": "eq_ref",
                            "key": "PRIMARY",
                            "rows_examined_per_scan": 1
                        }
                    }
                ]
            }
        }"#;

        let plan = parse_mysql_explain(
            json,
            "SELECT * FROM orders JOIN customers ON orders.cid = customers.id",
        )
        .expect("should parse");
        assert_eq!(plan.root.node_type, "Nested Loop");
        assert_eq!(plan.root.children.len(), 2);
    }

    #[test]
    fn parse_sqlite_scan() {
        let text = "QUERY PLAN\n\
                     |--SCAN orders\n";

        let plan =
            parse_sqlite_explain(text, "SELECT * FROM orders")
                .expect("should parse");
        assert_eq!(plan.engine, "SQLite");
        assert_eq!(plan.root.node_type, "Seq Scan");
        assert_eq!(
            plan.root.relation.as_deref(),
            Some("orders")
        );
    }

    #[test]
    fn parse_sqlite_index_search() {
        let text =
            "QUERY PLAN\n\
             |--SEARCH users USING INDEX idx_email (email=?)\n";

        let plan = parse_sqlite_explain(
            text,
            "SELECT * FROM users WHERE email = ?",
        )
        .expect("should parse");
        assert_eq!(plan.root.node_type, "Index Scan");
        assert_eq!(
            plan.root.index_name.as_deref(),
            Some("idx_email")
        );
    }

    #[test]
    fn parse_sqlite_multiple_tables() {
        let text = "QUERY PLAN\n\
                     |--SCAN orders\n\
                     |--SEARCH customers USING INDEX idx_cust_id (id=?)\n\
                     `--USE TEMP B-TREE FOR ORDER BY\n";

        let plan = parse_sqlite_explain(
            text,
            "SELECT * FROM orders JOIN customers ORDER BY name",
        )
        .expect("should parse");
        assert_eq!(plan.root.node_type, "Query Plan");
        assert_eq!(plan.root.children.len(), 3);
    }

    #[test]
    fn join_algorithm_detection() {
        let nl = PlanNode::new("Nested Loop");
        assert_eq!(
            nl.join_algorithm(),
            Some(JoinAlgorithm::NestedLoop)
        );

        let hj = PlanNode::new("Hash Join");
        assert_eq!(
            hj.join_algorithm(),
            Some(JoinAlgorithm::Hash)
        );

        let mj = PlanNode::new("Merge Join");
        assert_eq!(
            mj.join_algorithm(),
            Some(JoinAlgorithm::SortMerge)
        );

        let ss = PlanNode::new("Seq Scan");
        assert_eq!(ss.join_algorithm(), None);
    }

    #[test]
    fn uses_index_detection() {
        assert!(PlanNode::new("Index Scan").uses_index());
        assert!(
            PlanNode::new("Index Only Scan").uses_index()
        );
        assert!(
            PlanNode::new("Bitmap Index Scan").uses_index()
        );
        assert!(!PlanNode::new("Seq Scan").uses_index());
    }

    #[test]
    fn all_node_types_recursive() {
        let mut root = PlanNode::new("Hash Join");
        root.children.push(PlanNode::new("Seq Scan"));
        let mut hash = PlanNode::new("Hash");
        hash.children.push(PlanNode::new("Seq Scan"));
        root.children.push(hash);

        let types = root.all_node_types();
        assert_eq!(
            types,
            vec!["Hash Join", "Seq Scan", "Hash", "Seq Scan"]
        );
    }

    #[test]
    fn plan_node_serialization() {
        let node = PlanNode::new("Seq Scan");
        let json = serde_json::to_string(&node)
            .expect("should serialize");
        let roundtrip: PlanNode = serde_json::from_str(&json)
            .expect("should deserialize");
        assert_eq!(node, roundtrip);
    }

    #[test]
    fn join_algorithm_display() {
        assert_eq!(
            JoinAlgorithm::NestedLoop.to_string(),
            "Nested Loop"
        );
        assert_eq!(
            JoinAlgorithm::Hash.to_string(),
            "Hash Join"
        );
        assert_eq!(
            JoinAlgorithm::SortMerge.to_string(),
            "Sort-Merge Join"
        );
    }
}
