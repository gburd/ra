//! EXPLAIN plan generators for database-specific output formats.
//!
//! Converts [`ExplainPlan`] trees into the EXPLAIN output format used
//! by `PostgreSQL`, `MySQL`, Oracle, and SQL Server. Also provides
//! [`from_relexpr`] to build an `ExplainPlan` from a relational
//! algebra expression tree.

use std::fmt::Write;

use ra_core::algebra::{JoinType as CoreJoinType, RelExpr};

use crate::explain::{ExplainNode, ExplainPlan, JoinType, NodeType};

/// Target database for EXPLAIN output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExplainFormat {
    /// `PostgreSQL` `EXPLAIN (FORMAT JSON)`.
    PostgresJson,
    /// `PostgreSQL` `EXPLAIN` text output.
    PostgresText,
    /// `MySQL` `EXPLAIN FORMAT=JSON`.
    MysqlJson,
    /// Oracle `EXPLAIN PLAN` text output.
    OracleText,
    /// SQL Server `SET SHOWPLAN_XML ON`.
    SqlServerXml,
}

/// Cost parameters calibrated for a specific database engine.
///
/// These are used when generating cost estimates in EXPLAIN output.
/// Each database uses different internal cost units.
#[derive(Debug, Clone)]
pub struct DatabaseCostParams {
    /// Baseline cost for a sequential page read.
    pub seq_page_cost: f64,
    /// Baseline cost for a random page read.
    pub random_page_cost: f64,
    /// Per-tuple CPU processing cost.
    pub cpu_tuple_cost: f64,
    /// Per-operator CPU cost for evaluating expressions.
    pub cpu_operator_cost: f64,
    /// Assumed page size in bytes.
    pub page_size: u64,
    /// Default number of rows if unknown.
    pub default_rows: f64,
    /// Default row width in bytes.
    pub default_width: u32,
}

impl DatabaseCostParams {
    /// `PostgreSQL` default cost parameters (from `postgresql.conf`).
    #[must_use]
    pub fn postgres_default() -> Self {
        Self {
            seq_page_cost: 1.0,
            random_page_cost: 4.0,
            cpu_tuple_cost: 0.01,
            cpu_operator_cost: 0.0025,
            page_size: 8192,
            default_rows: 1000.0,
            default_width: 64,
        }
    }

    /// `MySQL` default cost parameters (`InnoDB` engine).
    #[must_use]
    pub fn mysql_default() -> Self {
        Self {
            seq_page_cost: 1.0,
            random_page_cost: 1.0,
            cpu_tuple_cost: 0.1,
            cpu_operator_cost: 0.2,
            page_size: 16384,
            default_rows: 1000.0,
            default_width: 100,
        }
    }

    /// Oracle default cost parameters.
    #[must_use]
    pub fn oracle_default() -> Self {
        Self {
            seq_page_cost: 1.0,
            random_page_cost: 1.5,
            cpu_tuple_cost: 0.005,
            cpu_operator_cost: 0.001,
            page_size: 8192,
            default_rows: 1000.0,
            default_width: 80,
        }
    }

    /// SQL Server default cost parameters.
    #[must_use]
    pub fn sqlserver_default() -> Self {
        Self {
            seq_page_cost: 0.000_741,
            random_page_cost: 0.003_125,
            cpu_tuple_cost: 0.000_1,
            cpu_operator_cost: 0.000_015_7,
            page_size: 8192,
            default_rows: 1000.0,
            default_width: 64,
        }
    }

    /// Estimate total cost for scanning `row_count` rows.
    fn scan_total_cost(&self, row_count: f64, width: u32) -> f64 {
        let rows_per_page = (self.page_size as f64 / f64::from(width.max(1))).max(1.0);
        let pages = (row_count / rows_per_page).ceil().max(1.0);
        pages * self.seq_page_cost + row_count * self.cpu_tuple_cost
    }

    /// Estimate cost for an index scan on `row_count` rows.
    pub fn index_scan_cost(&self, row_count: f64, selectivity: f64) -> f64 {
        let selected = (row_count * selectivity).max(1.0);
        selected * self.random_page_cost
            + selected * self.cpu_tuple_cost
            + selected * self.cpu_operator_cost
    }

    /// Estimate cost for a hash join.
    fn hash_join_cost(&self, left_rows: f64, right_rows: f64) -> (f64, f64) {
        let build_cost = left_rows * self.cpu_tuple_cost;
        let probe_cost = right_rows * self.cpu_tuple_cost;
        let startup = build_cost;
        let total = startup + probe_cost + (left_rows + right_rows) * self.cpu_operator_cost;
        (startup, total)
    }

    /// Estimate cost for a sort of `row_count` rows.
    fn sort_cost(&self, row_count: f64) -> (f64, f64) {
        let n_log_n = if row_count > 1.0 {
            row_count * row_count.log2()
        } else {
            row_count
        };
        let cost = n_log_n * self.cpu_operator_cost * 2.0;
        (cost, cost)
    }

    /// Estimate cost for a hash aggregate.
    fn aggregate_cost(&self, row_count: f64, group_count: f64) -> (f64, f64) {
        let startup = row_count * self.cpu_tuple_cost + group_count * self.cpu_operator_cost;
        (startup, startup)
    }
}

// ---- RelExpr -> ExplainPlan conversion ----

/// Build an [`ExplainPlan`] from a relational algebra expression.
///
/// Uses `cost_params` to generate plausible cost estimates in the
/// target database's cost units.
#[must_use]
pub fn from_relexpr(plan: &RelExpr, cost_params: &DatabaseCostParams) -> ExplainPlan {
    let root = relexpr_to_node(plan, cost_params);
    let total_cost = root.total_cost;
    let total_rows = root.estimated_rows;

    ExplainPlan {
        root,
        query: None,
        total_cost,
        total_rows,
    }
}

fn relexpr_to_node(expr: &RelExpr, params: &DatabaseCostParams) -> ExplainNode {
    match expr {
        RelExpr::Scan { table, .. } => convert_scan(table, params),
        RelExpr::Filter {
            predicate, input, ..
        } => convert_filter(predicate, input, params),
        RelExpr::Project { input, columns, .. } => convert_project(input, columns, params),
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
            ..
        } => convert_join(*join_type, condition, left, right, params),
        RelExpr::Sort { input, .. } => convert_sort(input, params),
        RelExpr::Limit { count, input, .. } => convert_limit(*count, input, params),
        RelExpr::Aggregate {
            input, group_by, ..
        } => convert_aggregate(input, group_by, params),
        RelExpr::Union {
            left, right, all, ..
        } => convert_union(left, right, *all, params),
        RelExpr::Distinct { input, .. } => convert_distinct(input, params),
        RelExpr::Window { input, .. } => convert_window(input, params),
        _ => convert_fallback(expr, params),
    }
}

fn convert_scan(table: &str, params: &DatabaseCostParams) -> ExplainNode {
    let rows = params.default_rows;
    let width = params.default_width;
    let total = params.scan_total_cost(rows, width);
    ExplainNode {
        node_type: NodeType::SeqScan,
        join_type: None,
        relation: Some(table.to_owned()),
        index_name: None,
        startup_cost: Some(0.0),
        total_cost: Some(total),
        estimated_rows: Some(rows),
        estimated_width: Some(width),
        filter: None,
        scan_direction: None,
        raw_detail: None,
        children: Vec::new(),
    }
}

fn convert_filter(
    predicate: &ra_core::expr::Expr,
    input: &RelExpr,
    params: &DatabaseCostParams,
) -> ExplainNode {
    let child = relexpr_to_node(input, params);
    let child_rows = child.estimated_rows.unwrap_or(params.default_rows);
    let selectivity = 0.33;
    let filtered_rows = (child_rows * selectivity).max(1.0);
    let startup = child.startup_cost.unwrap_or(0.0);
    let total = child.total_cost.unwrap_or(0.0) + child_rows * params.cpu_operator_cost;

    ExplainNode {
        node_type: child.node_type,
        join_type: None,
        relation: child.relation.clone(),
        index_name: child.index_name.clone(),
        startup_cost: Some(startup),
        total_cost: Some(total),
        estimated_rows: Some(filtered_rows),
        estimated_width: child.estimated_width,
        filter: Some(format!("{predicate:?}")),
        scan_direction: child.scan_direction.clone(),
        raw_detail: None,
        children: child.children,
    }
}

fn convert_project(
    input: &RelExpr,
    columns: &[ra_core::algebra::ProjectionColumn],
    params: &DatabaseCostParams,
) -> ExplainNode {
    let child = relexpr_to_node(input, params);
    let child_rows = child.estimated_rows.unwrap_or(params.default_rows);
    let width = (columns.len() as u32 * 8).max(params.default_width);
    let total = child.total_cost.unwrap_or(0.0) + child_rows * params.cpu_tuple_cost;

    ExplainNode {
        node_type: NodeType::Result,
        join_type: None,
        relation: None,
        index_name: None,
        startup_cost: child.startup_cost,
        total_cost: Some(total),
        estimated_rows: Some(child_rows),
        estimated_width: Some(width),
        filter: None,
        scan_direction: None,
        raw_detail: None,
        children: vec![child],
    }
}

fn convert_join(
    join_type: CoreJoinType,
    condition: &ra_core::expr::Expr,
    left: &RelExpr,
    right: &RelExpr,
    params: &DatabaseCostParams,
) -> ExplainNode {
    let left_child = relexpr_to_node(left, params);
    let right_child = relexpr_to_node(right, params);
    let left_rows = left_child.estimated_rows.unwrap_or(params.default_rows);
    let right_rows = right_child.estimated_rows.unwrap_or(params.default_rows);

    let (startup, total) = params.hash_join_cost(left_rows, right_rows);
    let output_rows = (left_rows * right_rows * 0.01).max(1.0);

    let explain_join = convert_join_type(join_type);

    let hash_node = ExplainNode {
        node_type: NodeType::Hash,
        join_type: None,
        relation: None,
        index_name: None,
        startup_cost: Some(left_rows * params.cpu_tuple_cost),
        total_cost: Some(left_rows * params.cpu_tuple_cost),
        estimated_rows: Some(left_rows),
        estimated_width: left_child.estimated_width,
        filter: None,
        scan_direction: None,
        raw_detail: None,
        children: vec![left_child],
    };

    ExplainNode {
        node_type: NodeType::HashJoin,
        join_type: Some(explain_join),
        relation: None,
        index_name: None,
        startup_cost: Some(startup),
        total_cost: Some(total),
        estimated_rows: Some(output_rows),
        estimated_width: Some(params.default_width * 2),
        filter: Some(format!("{condition:?}")),
        scan_direction: None,
        raw_detail: None,
        children: vec![right_child, hash_node],
    }
}

fn convert_sort(input: &RelExpr, params: &DatabaseCostParams) -> ExplainNode {
    let child = relexpr_to_node(input, params);
    let child_rows = child.estimated_rows.unwrap_or(params.default_rows);
    let (startup, total) = params.sort_cost(child_rows);
    let child_total = child.total_cost.unwrap_or(0.0);

    ExplainNode {
        node_type: NodeType::Sort,
        join_type: None,
        relation: None,
        index_name: None,
        startup_cost: Some(startup + child_total),
        total_cost: Some(total + child_total),
        estimated_rows: Some(child_rows),
        estimated_width: child.estimated_width,
        filter: None,
        scan_direction: None,
        raw_detail: None,
        children: vec![child],
    }
}

fn convert_limit(count: u64, input: &RelExpr, params: &DatabaseCostParams) -> ExplainNode {
    let child = relexpr_to_node(input, params);
    let child_rows = child.estimated_rows.unwrap_or(params.default_rows);
    let limited = (count as f64).min(child_rows);

    ExplainNode {
        node_type: NodeType::Limit,
        join_type: None,
        relation: None,
        index_name: None,
        startup_cost: child.startup_cost,
        total_cost: child.total_cost,
        estimated_rows: Some(limited),
        estimated_width: child.estimated_width,
        filter: None,
        scan_direction: None,
        raw_detail: None,
        children: vec![child],
    }
}

fn convert_aggregate(
    input: &RelExpr,
    group_by: &[ra_core::expr::Expr],
    params: &DatabaseCostParams,
) -> ExplainNode {
    let child = relexpr_to_node(input, params);
    let child_rows = child.estimated_rows.unwrap_or(params.default_rows);
    let group_count = if group_by.is_empty() {
        1.0
    } else {
        (child_rows * 0.1).max(1.0)
    };
    let (startup, total) = params.aggregate_cost(child_rows, group_count);
    let child_total = child.total_cost.unwrap_or(0.0);

    ExplainNode {
        node_type: NodeType::HashAggregate,
        join_type: None,
        relation: None,
        index_name: None,
        startup_cost: Some(startup + child_total),
        total_cost: Some(total + child_total),
        estimated_rows: Some(group_count),
        estimated_width: child.estimated_width,
        filter: None,
        scan_direction: None,
        raw_detail: None,
        children: vec![child],
    }
}

fn convert_union(
    left: &RelExpr,
    right: &RelExpr,
    all: bool,
    params: &DatabaseCostParams,
) -> ExplainNode {
    let left_child = relexpr_to_node(left, params);
    let right_child = relexpr_to_node(right, params);
    let left_rows = left_child.estimated_rows.unwrap_or(params.default_rows);
    let right_rows = right_child.estimated_rows.unwrap_or(params.default_rows);
    let rows = if all {
        left_rows + right_rows
    } else {
        (left_rows + right_rows) * 0.8
    };

    ExplainNode {
        node_type: NodeType::Append,
        join_type: None,
        relation: None,
        index_name: None,
        startup_cost: left_child.startup_cost,
        total_cost: Some(
            left_child.total_cost.unwrap_or(0.0) + right_child.total_cost.unwrap_or(0.0),
        ),
        estimated_rows: Some(rows),
        estimated_width: left_child.estimated_width,
        filter: None,
        scan_direction: None,
        raw_detail: None,
        children: vec![left_child, right_child],
    }
}

fn convert_distinct(input: &RelExpr, params: &DatabaseCostParams) -> ExplainNode {
    let child = relexpr_to_node(input, params);
    let child_rows = child.estimated_rows.unwrap_or(params.default_rows);
    let unique_rows = (child_rows * 0.8).max(1.0);

    ExplainNode {
        node_type: NodeType::Unique,
        join_type: None,
        relation: None,
        index_name: None,
        startup_cost: child.startup_cost,
        total_cost: child.total_cost,
        estimated_rows: Some(unique_rows),
        estimated_width: child.estimated_width,
        filter: None,
        scan_direction: None,
        raw_detail: None,
        children: vec![child],
    }
}

fn convert_window(input: &RelExpr, params: &DatabaseCostParams) -> ExplainNode {
    let child = relexpr_to_node(input, params);
    let child_rows = child.estimated_rows.unwrap_or(params.default_rows);
    let total = child.total_cost.unwrap_or(0.0) + child_rows * params.cpu_operator_cost;

    ExplainNode {
        node_type: NodeType::WindowAgg,
        join_type: None,
        relation: None,
        index_name: None,
        startup_cost: child.startup_cost,
        total_cost: Some(total),
        estimated_rows: Some(child_rows),
        estimated_width: child.estimated_width,
        filter: None,
        scan_direction: None,
        raw_detail: None,
        children: vec![child],
    }
}

fn convert_fallback(expr: &RelExpr, params: &DatabaseCostParams) -> ExplainNode {
    let children: Vec<ExplainNode> = expr
        .children()
        .iter()
        .map(|c| relexpr_to_node(c, params))
        .collect();
    let total_cost = children.iter().filter_map(|c| c.total_cost).sum::<f64>();
    let rows = children
        .first()
        .and_then(|c| c.estimated_rows)
        .unwrap_or(params.default_rows);

    ExplainNode {
        node_type: NodeType::Result,
        join_type: None,
        relation: None,
        index_name: None,
        startup_cost: Some(0.0),
        total_cost: Some(total_cost),
        estimated_rows: Some(rows),
        estimated_width: Some(params.default_width),
        filter: None,
        scan_direction: None,
        raw_detail: None,
        children,
    }
}

fn convert_join_type(jt: CoreJoinType) -> JoinType {
    match jt {
        CoreJoinType::Inner => JoinType::Inner,
        CoreJoinType::LeftOuter => JoinType::Left,
        CoreJoinType::RightOuter => JoinType::Right,
        CoreJoinType::FullOuter => JoinType::Full,
        CoreJoinType::Cross => JoinType::Cross,
        CoreJoinType::Semi => JoinType::Semi,
        CoreJoinType::Anti => JoinType::Anti,
    }
}

// ---- Format output methods ----

impl ExplainPlan {
    /// Render as `PostgreSQL` `EXPLAIN (FORMAT JSON)` output.
    #[must_use]
    pub fn to_postgres_json(&self) -> String {
        let mut buf = String::with_capacity(512);
        buf.push_str("[\n  {\n    \"Plan\": ");
        node_to_pg_json(&self.root, &mut buf, 4);
        buf.push_str("\n  }\n]");
        buf
    }

    /// Render as `PostgreSQL` `EXPLAIN` text output.
    #[must_use]
    pub fn to_postgres_text(&self) -> String {
        let mut buf = String::with_capacity(256);
        node_to_pg_text(&self.root, &mut buf, 0);
        buf
    }

    /// Render as `MySQL` `EXPLAIN FORMAT=JSON` output.
    #[must_use]
    pub fn to_mysql_json(&self) -> String {
        let mut buf = String::with_capacity(512);
        buf.push_str("{\n  \"query_block\": {\n");
        buf.push_str("    \"select_id\": 1");
        if let Some(cost) = self.total_cost {
            let _ = write!(
                buf,
                ",\n    \"cost_info\": {{ \"query_cost\": \"{cost:.2}\" }}"
            );
        }
        node_to_mysql_json(&self.root, &mut buf, 4);
        buf.push_str("\n  }\n}");
        buf
    }

    /// Render as Oracle `EXPLAIN PLAN` text output.
    #[must_use]
    pub fn to_oracle_text(&self) -> String {
        let mut rows = Vec::new();
        collect_oracle_rows(&self.root, &mut rows, 0, 0);

        let id_w = 4;
        let op_w = rows
            .iter()
            .map(|(_, _, op, _, _)| op.len())
            .max()
            .unwrap_or(9)
            .max(9);
        let cost_w = 7;
        let card_w = 11;

        let mut buf = String::with_capacity(256);
        let _ = writeln!(
            buf,
            " {:<id_w$} | {:<op_w$} | {:<cost_w$} | {:<card_w$} |",
            "Id", "Operation", "Cost", "Cardinality",
        );
        let _ = writeln!(
            buf,
            "-{}-+-{}-+-{}-+-{}-+",
            "-".repeat(id_w),
            "-".repeat(op_w),
            "-".repeat(cost_w),
            "-".repeat(card_w),
        );

        for (id, depth, op, cost, card) in &rows {
            let indent = " ".repeat(*depth);
            let op_str = format!("{indent}{op}");
            let _ = writeln!(
                buf,
                " {id:>id_w$} | {op_str:<op_w$} | {cost:>cost_w$} | {card:>card_w$} |",
            );
        }

        buf
    }

    /// Render as SQL Server `SET SHOWPLAN_XML ON` output.
    #[must_use]
    pub fn to_sqlserver_xml(&self) -> String {
        let mut buf = String::with_capacity(512);
        buf.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
        buf.push_str(
            "<ShowPlanXML xmlns=\"http://schemas.microsoft.com/sqlserver/2004/07/showplan\">\n",
        );
        buf.push_str("  <BatchSequence>\n");
        buf.push_str("    <Batch>\n");
        buf.push_str("      <Statements>\n");
        buf.push_str("        <StmtSimple>\n");
        buf.push_str("          <QueryPlan>\n");
        node_to_sqlserver_xml(&self.root, &mut buf, 12, 1);
        buf.push_str("          </QueryPlan>\n");
        buf.push_str("        </StmtSimple>\n");
        buf.push_str("      </Statements>\n");
        buf.push_str("    </Batch>\n");
        buf.push_str("  </BatchSequence>\n");
        buf.push_str("</ShowPlanXML>");
        buf
    }

    /// Render to the specified format.
    #[must_use]
    pub fn to_format(&self, format: ExplainFormat) -> String {
        match format {
            ExplainFormat::PostgresJson => self.to_postgres_json(),
            ExplainFormat::PostgresText => self.to_postgres_text(),
            ExplainFormat::MysqlJson => self.to_mysql_json(),
            ExplainFormat::OracleText => self.to_oracle_text(),
            ExplainFormat::SqlServerXml => self.to_sqlserver_xml(),
        }
    }
}

// ---- PostgreSQL JSON helpers ----

fn node_to_pg_json(node: &ExplainNode, buf: &mut String, indent: usize) {
    let pad = " ".repeat(indent);
    buf.push_str("{\n");

    let _ = write!(buf, "{pad}  \"Node Type\": \"{}\"", node.node_type);

    if let Some(jt) = &node.join_type {
        let _ = write!(buf, ",\n{pad}  \"Join Type\": \"{jt}\"");
    }
    if let Some(rel) = &node.relation {
        let _ = write!(buf, ",\n{pad}  \"Relation Name\": \"{rel}\"");
    }
    if let Some(idx) = &node.index_name {
        let _ = write!(buf, ",\n{pad}  \"Index Name\": \"{idx}\"");
    }
    if let Some(dir) = &node.scan_direction {
        let _ = write!(buf, ",\n{pad}  \"Scan Direction\": \"{dir}\"");
    }
    if let Some(startup) = node.startup_cost {
        let _ = write!(buf, ",\n{pad}  \"Startup Cost\": {startup:.2}");
    }
    if let Some(total) = node.total_cost {
        let _ = write!(buf, ",\n{pad}  \"Total Cost\": {total:.2}");
    }
    if let Some(rows) = node.estimated_rows {
        let _ = write!(buf, ",\n{pad}  \"Plan Rows\": {rows:.0}");
    }
    if let Some(width) = node.estimated_width {
        let _ = write!(buf, ",\n{pad}  \"Plan Width\": {width}");
    }
    if let Some(filter) = &node.filter {
        let escaped = escape_json_str(filter);
        let _ = write!(buf, ",\n{pad}  \"Filter\": \"{escaped}\"");
    }

    if !node.children.is_empty() {
        let _ = write!(buf, ",\n{pad}  \"Plans\": [\n");
        for (i, child) in node.children.iter().enumerate() {
            if i > 0 {
                buf.push_str(",\n");
            }
            let _ = write!(buf, "{pad}    ");
            node_to_pg_json(child, buf, indent + 4);
        }
        let _ = write!(buf, "\n{pad}  ]");
    }

    let _ = write!(buf, "\n{pad}}}");
}

fn escape_json_str(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

// ---- PostgreSQL text helpers ----

fn node_to_pg_text(node: &ExplainNode, buf: &mut String, depth: usize) {
    let indent = if depth == 0 {
        String::new()
    } else {
        " ".repeat((depth - 1) * 6) + "->  "
    };

    buf.push_str(&indent);
    buf.push_str(&node.node_type.to_string());

    if let Some(jt) = &node.join_type {
        let _ = write!(buf, " ({jt})");
    }
    if let Some(rel) = &node.relation {
        let _ = write!(buf, " on {rel}");
    }
    if let Some(idx) = &node.index_name {
        let _ = write!(buf, " using {idx}");
    }

    let startup = node.startup_cost.unwrap_or(0.0);
    let total = node.total_cost.unwrap_or(0.0);
    let rows = node.estimated_rows.unwrap_or(0.0);
    let width = node.estimated_width.unwrap_or(0);

    let _ = write!(
        buf,
        "  (cost={startup:.2}..{total:.2} rows={rows:.0} width={width})"
    );
    buf.push('\n');

    if let Some(filter) = &node.filter {
        let filter_indent = " ".repeat(depth * 6 + if depth > 0 { 4 } else { 2 });
        let _ = writeln!(buf, "{filter_indent}Filter: {filter}");
    }

    for child in &node.children {
        node_to_pg_text(child, buf, depth + 1);
    }
}

// ---- MySQL JSON helpers ----

fn node_to_mysql_json(node: &ExplainNode, buf: &mut String, indent: usize) {
    let pad = " ".repeat(indent);

    match node.node_type {
        NodeType::NestedLoop | NodeType::HashJoin | NodeType::MergeJoin => {
            let _ = write!(buf, ",\n{pad}\"nested_loop\": [");
            for (i, child) in node.children.iter().enumerate() {
                if i > 0 {
                    buf.push(',');
                }
                let _ = write!(buf, "\n{pad}  {{");
                write_mysql_table_node(child, buf, indent + 4);
                let _ = write!(buf, "\n{pad}  }}");
            }
            let _ = write!(buf, "\n{pad}]");
        }
        NodeType::Sort => {
            let _ = write!(buf, ",\n{pad}\"ordering_operation\": {{");
            for child in &node.children {
                node_to_mysql_json(child, buf, indent + 2);
            }
            let _ = write!(buf, "\n{pad}}}");
        }
        NodeType::HashAggregate | NodeType::GroupAggregate => {
            let _ = write!(buf, ",\n{pad}\"grouping_operation\": {{");
            for child in &node.children {
                node_to_mysql_json(child, buf, indent + 2);
            }
            let _ = write!(buf, "\n{pad}}}");
        }
        _ => {
            write_mysql_table_node(node, buf, indent);
        }
    }
}

fn write_mysql_table_node(node: &ExplainNode, buf: &mut String, indent: usize) {
    let pad = " ".repeat(indent);

    let access_type = match node.node_type {
        NodeType::IndexScan => "ref",
        NodeType::IndexOnlyScan => "index",
        NodeType::BitmapIndexScan => "index_merge",
        _ => "ALL",
    };

    let _ = write!(buf, ",\n{pad}\"table\": {{");
    if let Some(rel) = &node.relation {
        let _ = write!(buf, "\n{pad}  \"table_name\": \"{rel}\",");
    }
    let _ = write!(buf, "\n{pad}  \"access_type\": \"{access_type}\"");
    if let Some(key) = &node.index_name {
        let _ = write!(buf, ",\n{pad}  \"key\": \"{key}\"");
    }
    if let Some(rows) = node.estimated_rows {
        let _ = write!(buf, ",\n{pad}  \"rows_examined_per_scan\": {rows:.0}");
    }
    if let Some(cost) = node.total_cost {
        let _ = write!(
            buf,
            ",\n{pad}  \"cost_info\": {{ \"read_cost\": \"{cost:.2}\" }}"
        );
    }
    if let Some(filter) = &node.filter {
        let escaped = escape_json_str(filter);
        let _ = write!(buf, ",\n{pad}  \"attached_condition\": \"{escaped}\"");
    }
    let _ = write!(buf, "\n{pad}}}");

    for child in &node.children {
        if child.relation.is_some() {
            write_mysql_table_node(child, buf, indent);
        }
    }
}

// ---- Oracle text helpers ----

fn collect_oracle_rows(
    node: &ExplainNode,
    rows: &mut Vec<(usize, usize, String, String, String)>,
    id: usize,
    depth: usize,
) -> usize {
    let op_name = oracle_op_name(node);
    let cost = node
        .total_cost
        .map_or_else(|| "    ".to_owned(), |c| format!("{c:.0}"));
    let card = node
        .estimated_rows
        .map_or_else(|| "    ".to_owned(), |r| format!("{r:.0}"));

    rows.push((id, depth, op_name, cost, card));

    let mut next_id = id + 1;
    for child in &node.children {
        next_id = collect_oracle_rows(child, rows, next_id, depth + 1);
    }
    next_id
}

fn oracle_op_name(node: &ExplainNode) -> String {
    let base = match node.node_type {
        NodeType::SeqScan => "TABLE ACCESS FULL",
        NodeType::IndexScan => "INDEX RANGE SCAN",
        NodeType::IndexOnlyScan => "INDEX FAST FULL SCAN",
        NodeType::HashJoin => "HASH JOIN",
        NodeType::MergeJoin => "SORT MERGE JOIN",
        NodeType::NestedLoop => "NESTED LOOPS",
        NodeType::Sort => "SORT ORDER BY",
        NodeType::HashAggregate => "HASH GROUP BY",
        NodeType::GroupAggregate => "SORT GROUP BY",
        NodeType::Limit => "COUNT STOPKEY",
        NodeType::Append => "UNION-ALL",
        NodeType::Unique => "SORT UNIQUE",
        NodeType::WindowAgg => "WINDOW SORT",
        NodeType::Hash => "HASH",
        NodeType::Result => "SELECT STATEMENT",
        _ => "OTHER",
    };

    if let Some(rel) = &node.relation {
        format!("{base} {rel}")
    } else {
        base.to_owned()
    }
}

// ---- SQL Server XML helpers ----

fn node_to_sqlserver_xml(node: &ExplainNode, buf: &mut String, indent: usize, node_id: usize) {
    let pad = " ".repeat(indent);
    let physical_op = sqlserver_physical_op(node);
    let logical_op = sqlserver_logical_op(node);
    let rows = node.estimated_rows.unwrap_or(0.0);
    let cost = node.total_cost.unwrap_or(0.0);

    let _ = writeln!(
        buf,
        "{pad}<RelOp NodeId=\"{node_id}\" \
         PhysicalOp=\"{physical_op}\" \
         LogicalOp=\"{logical_op}\" \
         EstimateRows=\"{rows:.0}\" \
         EstimatedTotalSubtreeCost=\"{cost:.6}\">"
    );

    if let Some(filter) = &node.filter {
        let escaped = escape_xml(filter);
        let _ = write!(
            buf,
            "{pad}  <Predicate>\n\
             {pad}    <ScalarOperator>{escaped}</ScalarOperator>\n\
             {pad}  </Predicate>\n"
        );
    }

    if let Some(rel) = &node.relation {
        let _ = write!(buf, "{pad}  <Object Table=\"{rel}\"");
        if let Some(idx) = &node.index_name {
            let _ = write!(buf, " Index=\"{idx}\"");
        }
        buf.push_str(" />\n");
    }

    let mut child_id = node_id + 1;
    for child in &node.children {
        node_to_sqlserver_xml(child, buf, indent + 2, child_id);
        child_id += child_node_count(child);
    }

    let _ = writeln!(buf, "{pad}</RelOp>");
}

fn child_node_count(node: &ExplainNode) -> usize {
    1 + node.children.iter().map(child_node_count).sum::<usize>()
}

fn sqlserver_physical_op(node: &ExplainNode) -> &'static str {
    match node.node_type {
        NodeType::SeqScan => "Clustered Index Scan",
        NodeType::IndexScan => "Index Seek",
        NodeType::IndexOnlyScan => "Index Scan",
        NodeType::HashJoin | NodeType::HashAggregate | NodeType::Hash => "Hash Match",
        NodeType::MergeJoin => "Merge Join",
        NodeType::NestedLoop => "Nested Loops",
        NodeType::Sort | NodeType::Unique => "Sort",
        NodeType::GroupAggregate => "Stream Aggregate",
        NodeType::Limit => "Top",
        NodeType::Append => "Concatenation",
        NodeType::WindowAgg => "Sequence Project",
        _ => "Compute Scalar",
    }
}

fn sqlserver_logical_op(node: &ExplainNode) -> &'static str {
    match node.node_type {
        NodeType::SeqScan => "Clustered Index Scan",
        NodeType::IndexScan => "Index Seek",
        NodeType::IndexOnlyScan => "Index Scan",
        NodeType::HashJoin | NodeType::MergeJoin | NodeType::NestedLoop => "Inner Join",
        NodeType::Sort => "Sort",
        NodeType::HashAggregate | NodeType::GroupAggregate | NodeType::Hash => "Aggregate",
        NodeType::Limit => "Top",
        NodeType::Append => "Concatenation",
        NodeType::Unique => "Distinct Sort",
        _ => "Compute Scalar",
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::explain::{parse_postgres_explain, ExplainNode, ExplainPlan, JoinType, NodeType};

    fn sample_plan() -> ExplainPlan {
        ExplainPlan {
            root: ExplainNode {
                node_type: NodeType::HashJoin,
                join_type: Some(JoinType::Inner),
                relation: None,
                index_name: None,
                startup_cost: Some(125.50),
                total_cost: Some(2845.75),
                estimated_rows: Some(50_000.0),
                estimated_width: Some(128),
                filter: Some("(a.id = b.a_id)".to_owned()),
                scan_direction: None,
                raw_detail: None,
                children: vec![
                    ExplainNode {
                        node_type: NodeType::SeqScan,
                        join_type: None,
                        relation: Some("orders".to_owned()),
                        index_name: None,
                        startup_cost: Some(0.0),
                        total_cost: Some(1225.0),
                        estimated_rows: Some(100_000.0),
                        estimated_width: Some(64),
                        filter: None,
                        scan_direction: None,
                        raw_detail: None,
                        children: Vec::new(),
                    },
                    ExplainNode {
                        node_type: NodeType::Hash,
                        join_type: None,
                        relation: None,
                        index_name: None,
                        startup_cost: Some(500.25),
                        total_cost: Some(500.25),
                        estimated_rows: Some(10_000.0),
                        estimated_width: Some(64),
                        filter: None,
                        scan_direction: None,
                        raw_detail: None,
                        children: vec![ExplainNode {
                            node_type: NodeType::SeqScan,
                            join_type: None,
                            relation: Some("users".to_owned()),
                            index_name: None,
                            startup_cost: Some(0.0),
                            total_cost: Some(500.25),
                            estimated_rows: Some(10_000.0),
                            estimated_width: Some(64),
                            filter: None,
                            scan_direction: None,
                            raw_detail: None,
                            children: Vec::new(),
                        }],
                    },
                ],
            },
            query: None,
            total_cost: Some(2845.75),
            total_rows: Some(50_000.0),
        }
    }

    #[test]
    fn pg_json_contains_node_type() {
        let json = sample_plan().to_postgres_json();
        assert!(json.contains("\"Node Type\": \"Hash Join\""));
    }

    #[test]
    fn pg_json_contains_costs() {
        let json = sample_plan().to_postgres_json();
        assert!(json.contains("\"Startup Cost\": 125.50"));
        assert!(json.contains("\"Total Cost\": 2845.75"));
    }

    #[test]
    fn pg_json_contains_rows() {
        let json = sample_plan().to_postgres_json();
        assert!(json.contains("\"Plan Rows\": 50000"));
    }

    #[test]
    fn pg_json_contains_children() {
        let json = sample_plan().to_postgres_json();
        assert!(json.contains("\"Plans\":"));
        assert!(json.contains("\"Relation Name\": \"orders\""));
        assert!(json.contains("\"Relation Name\": \"users\""));
    }

    #[test]
    fn pg_json_roundtrip() {
        let json = sample_plan().to_postgres_json();
        let parsed = parse_postgres_explain(&json).expect("parse");
        assert_eq!(parsed.root.node_type, NodeType::HashJoin);
        assert_eq!(parsed.root.children.len(), 2);
    }

    #[test]
    fn pg_json_filter_escaped() {
        let json = sample_plan().to_postgres_json();
        assert!(json.contains("\"Filter\":"));
    }

    #[test]
    fn pg_text_contains_node_types() {
        let text = sample_plan().to_postgres_text();
        assert!(text.contains("Hash Join"));
        assert!(text.contains("Seq Scan on orders"));
        assert!(text.contains("Hash"));
    }

    #[test]
    fn pg_text_contains_costs() {
        let text = sample_plan().to_postgres_text();
        assert!(text.contains("cost=125.50..2845.75"));
    }

    #[test]
    fn pg_text_contains_rows() {
        let text = sample_plan().to_postgres_text();
        assert!(text.contains("rows=50000"));
    }

    #[test]
    fn pg_text_contains_filter() {
        let text = sample_plan().to_postgres_text();
        assert!(text.contains("Filter:"));
    }

    #[test]
    fn mysql_json_contains_query_block() {
        let json = sample_plan().to_mysql_json();
        assert!(json.contains("\"query_block\""));
    }

    #[test]
    fn mysql_json_contains_cost_info() {
        let json = sample_plan().to_mysql_json();
        assert!(json.contains("\"query_cost\""));
    }

    #[test]
    fn mysql_json_contains_nested_loop() {
        let json = sample_plan().to_mysql_json();
        assert!(json.contains("\"nested_loop\""));
    }

    #[test]
    fn mysql_json_contains_table_names() {
        let json = sample_plan().to_mysql_json();
        assert!(json.contains("\"table_name\": \"orders\""));
    }

    #[test]
    fn oracle_text_contains_header() {
        let text = sample_plan().to_oracle_text();
        assert!(text.contains("Id"));
        assert!(text.contains("Operation"));
        assert!(text.contains("Cost"));
        assert!(text.contains("Cardinality"));
    }

    #[test]
    fn oracle_text_contains_hash_join() {
        let text = sample_plan().to_oracle_text();
        assert!(text.contains("HASH JOIN"));
    }

    #[test]
    fn oracle_text_contains_table_access() {
        let text = sample_plan().to_oracle_text();
        assert!(text.contains("TABLE ACCESS FULL orders"));
    }

    #[test]
    fn sqlserver_xml_has_showplan_root() {
        let xml = sample_plan().to_sqlserver_xml();
        assert!(xml.contains("<ShowPlanXML"));
        assert!(xml.contains("</ShowPlanXML>"));
    }

    #[test]
    fn sqlserver_xml_has_relop() {
        let xml = sample_plan().to_sqlserver_xml();
        assert!(xml.contains("<RelOp"));
        assert!(xml.contains("PhysicalOp=\"Hash Match\""));
    }

    #[test]
    fn sqlserver_xml_has_estimate_rows() {
        let xml = sample_plan().to_sqlserver_xml();
        assert!(xml.contains("EstimateRows=\"50000\""));
    }

    #[test]
    fn sqlserver_xml_has_subtree_cost() {
        let xml = sample_plan().to_sqlserver_xml();
        assert!(xml.contains("EstimatedTotalSubtreeCost="));
    }

    #[test]
    fn sqlserver_xml_has_predicate() {
        let xml = sample_plan().to_sqlserver_xml();
        assert!(xml.contains("<Predicate>"));
    }

    #[test]
    fn from_relexpr_scan() {
        let expr = RelExpr::scan("users");
        let plan = from_relexpr(&expr, &DatabaseCostParams::postgres_default());
        assert_eq!(plan.root.node_type, NodeType::SeqScan);
        assert_eq!(plan.root.relation.as_deref(), Some("users"));
        assert!(plan.root.total_cost.is_some());
        assert!(plan.root.estimated_rows.is_some());
    }

    #[test]
    fn from_relexpr_filter() {
        use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
        let expr = RelExpr::scan("orders").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("amount"))),
            right: Box::new(Expr::Const(Const::Int(100))),
        });
        let plan = from_relexpr(&expr, &DatabaseCostParams::postgres_default());
        assert!(plan.root.filter.is_some());
        assert!(plan.root.estimated_rows.unwrap_or(0.0) < 1000.0);
    }

    #[test]
    fn from_relexpr_join() {
        use ra_core::expr::{BinOp, ColumnRef, Expr};
        let join = RelExpr::Join {
            join_type: CoreJoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("a.id"))),
                right: Box::new(Expr::Column(ColumnRef::new("b.a_id"))),
            },
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let plan = from_relexpr(&join, &DatabaseCostParams::postgres_default());
        assert_eq!(plan.root.node_type, NodeType::HashJoin);
        assert_eq!(plan.root.join_type, Some(JoinType::Inner));
        assert_eq!(plan.root.children.len(), 2);
    }

    #[test]
    fn from_relexpr_to_postgres_json_roundtrip() {
        let expr = RelExpr::scan("users");
        let plan = from_relexpr(&expr, &DatabaseCostParams::postgres_default());
        let json = plan.to_postgres_json();
        let parsed = parse_postgres_explain(&json).expect("parse");
        assert_eq!(parsed.root.node_type, NodeType::SeqScan);
        assert_eq!(parsed.root.relation.as_deref(), Some("users"));
    }

    #[test]
    fn to_format_dispatches() {
        let plan = sample_plan();
        assert!(plan
            .to_format(ExplainFormat::PostgresJson)
            .contains("\"Node Type\""));
        assert!(plan
            .to_format(ExplainFormat::PostgresText)
            .contains("Hash Join"));
        assert!(plan
            .to_format(ExplainFormat::MysqlJson)
            .contains("\"query_block\""));
        assert!(plan
            .to_format(ExplainFormat::OracleText)
            .contains("HASH JOIN"));
        assert!(plan
            .to_format(ExplainFormat::SqlServerXml)
            .contains("<ShowPlanXML"));
    }

    #[test]
    fn postgres_params_positive() {
        let p = DatabaseCostParams::postgres_default();
        assert!(p.seq_page_cost > 0.0);
        assert!(p.random_page_cost > 0.0);
        assert!(p.cpu_tuple_cost > 0.0);
    }

    #[test]
    fn mysql_params_positive() {
        let p = DatabaseCostParams::mysql_default();
        assert!(p.seq_page_cost > 0.0);
        assert!(p.cpu_tuple_cost > 0.0);
    }

    #[test]
    fn oracle_params_positive() {
        let p = DatabaseCostParams::oracle_default();
        assert!(p.seq_page_cost > 0.0);
        assert!(p.cpu_tuple_cost > 0.0);
    }

    #[test]
    fn sqlserver_params_positive() {
        let p = DatabaseCostParams::sqlserver_default();
        assert!(p.seq_page_cost > 0.0);
        assert!(p.cpu_tuple_cost > 0.0);
    }

    #[test]
    fn scan_cost_increases_with_rows() {
        let p = DatabaseCostParams::postgres_default();
        let small = p.scan_total_cost(100.0, 64);
        let large = p.scan_total_cost(100_000.0, 64);
        assert!(large > small);
    }

    #[test]
    fn hash_join_cost_positive() {
        let p = DatabaseCostParams::postgres_default();
        let (startup, total) = p.hash_join_cost(1000.0, 5000.0);
        assert!(startup > 0.0);
        assert!(total > startup);
    }

    #[test]
    fn sort_cost_increases_with_rows() {
        let p = DatabaseCostParams::postgres_default();
        let (_, small) = p.sort_cost(100.0);
        let (_, large) = p.sort_cost(100_000.0);
        assert!(large > small);
    }

    #[test]
    fn escape_json_special_chars() {
        assert_eq!(escape_json_str("a\"b"), "a\\\"b");
        assert_eq!(escape_json_str("a\\b"), "a\\\\b");
        assert_eq!(escape_json_str("a\nb"), "a\\nb");
    }

    #[test]
    fn escape_xml_special_chars() {
        assert_eq!(escape_xml("a<b"), "a&lt;b");
        assert_eq!(escape_xml("a&b"), "a&amp;b");
        assert_eq!(escape_xml("a\"b"), "a&quot;b");
    }

    #[test]
    fn convert_all_join_types() {
        assert_eq!(convert_join_type(CoreJoinType::Inner), JoinType::Inner);
        assert_eq!(convert_join_type(CoreJoinType::LeftOuter), JoinType::Left);
        assert_eq!(convert_join_type(CoreJoinType::RightOuter), JoinType::Right);
        assert_eq!(convert_join_type(CoreJoinType::FullOuter), JoinType::Full);
        assert_eq!(convert_join_type(CoreJoinType::Cross), JoinType::Cross);
        assert_eq!(convert_join_type(CoreJoinType::Semi), JoinType::Semi);
        assert_eq!(convert_join_type(CoreJoinType::Anti), JoinType::Anti);
    }

    #[test]
    fn oracle_rows_sequential_ids() {
        let mut rows = Vec::new();
        collect_oracle_rows(&sample_plan().root, &mut rows, 0, 0);
        for (i, (id, _, _, _, _)) in rows.iter().enumerate() {
            assert_eq!(*id, i);
        }
    }

    #[test]
    fn child_node_count_leaf() {
        let leaf = ExplainNode {
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
            raw_detail: None,
            children: Vec::new(),
        };
        assert_eq!(child_node_count(&leaf), 1);
    }

    #[test]
    fn child_node_count_with_children() {
        assert_eq!(child_node_count(&sample_plan().root), 4);
    }
}
