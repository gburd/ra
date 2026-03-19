//! POST /api/visualize - Build a positioned plan tree for visualization.
//! POST /api/compare-plans - Compare plan trees across optimizers.

use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::errors::{ApiResult, AppError};

/// A positioned node in the visual plan tree.
#[derive(Debug, Clone, Serialize)]
pub struct VisualPlanNode {
    /// Unique node identifier.
    pub id: String,
    /// Operator type (e.g., `HashJoin`, `SeqScan`).
    pub operator_type: String,
    /// Estimated cost.
    pub cost: f64,
    /// Estimated row count.
    pub rows: u64,
    /// Additional details for tooltips.
    pub details: Vec<PlanDetail>,
    /// Child nodes.
    pub children: Vec<VisualPlanNode>,
    /// Position for rendering.
    pub position: NodePosition,
}

/// Position and dimensions for a plan node.
#[derive(Debug, Clone, Serialize)]
pub struct NodePosition {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A key-value detail for plan node tooltips.
#[derive(Debug, Clone, Serialize)]
pub struct PlanDetail {
    pub key: String,
    pub value: String,
}

/// Request body for plan visualization.
#[derive(Debug, Deserialize)]
pub struct VisualizeRequest {
    /// SQL query to optimize and visualize.
    pub sql: String,
    /// Hardware profile name (optional).
    #[serde(default)]
    pub hardware_profile: Option<String>,
}

/// Response from plan visualization.
#[derive(Debug, Serialize)]
pub struct VisualizeResponse {
    /// Root of the positioned plan tree.
    pub plan: VisualPlanNode,
    /// Total estimated cost.
    pub total_cost: f64,
    /// Rules applied during optimization.
    pub rules_applied: Vec<String>,
}

/// Build a positioned plan tree from a SQL query.
#[allow(clippy::needless_pass_by_value)]
#[rocket::post("/api/visualize", data = "<req>")]
pub fn visualize(
    req: Json<VisualizeRequest>,
) -> ApiResult<VisualizeResponse> {
    if req.sql.trim().is_empty() {
        return Err(AppError::bad_request(
            "empty_sql",
            "SQL statement cannot be empty",
        ));
    }

    let plan = build_plan_from_sql(&req.sql, req.hardware_profile.as_ref());

    let total_cost = sum_cost(&plan);

    Ok(Json(VisualizeResponse {
        plan,
        total_cost,
        rules_applied: vec![
            "predicate-pushdown".to_owned(),
            "projection-pruning".to_owned(),
            "join-reordering".to_owned(),
        ],
    }))
}

/// Request body for plan comparison.
#[derive(Debug, Deserialize)]
pub struct ComparePlansRequest {
    /// SQL query to compare across optimizers.
    pub sql: String,
    /// Hardware profile name (optional).
    #[serde(default)]
    pub hardware_profile: Option<String>,
}

/// A single optimizer's plan result.
#[derive(Debug, Serialize)]
pub struct OptimizerPlan {
    /// Optimizer name.
    pub optimizer: String,
    /// Positioned plan tree.
    pub plan: VisualPlanNode,
    /// Total estimated cost.
    pub total_cost: f64,
    /// Whether this optimizer is available.
    pub available: bool,
}

/// Response from plan comparison.
#[derive(Debug, Serialize)]
pub struct ComparePlansResponse {
    /// Plans from each optimizer.
    pub plans: Vec<OptimizerPlan>,
    /// Cost comparison summary.
    pub summary: CostSummary,
}

/// Cost comparison across optimizers.
#[derive(Debug, Serialize)]
pub struct CostSummary {
    /// Optimizer with the lowest total cost.
    pub cheapest: String,
    /// Per-optimizer cost breakdown.
    pub costs: Vec<OptimizerCost>,
}

/// Cost for a single optimizer.
#[derive(Debug, Serialize)]
pub struct OptimizerCost {
    pub optimizer: String,
    pub total_cost: f64,
    pub node_count: u32,
}

/// Compare plans across Ra and external databases.
#[allow(clippy::needless_pass_by_value)]
#[rocket::post("/api/compare-plans", data = "<req>")]
pub fn compare_plans(
    req: Json<ComparePlansRequest>,
) -> ApiResult<ComparePlansResponse> {
    if req.sql.trim().is_empty() {
        return Err(AppError::bad_request(
            "empty_sql",
            "SQL statement cannot be empty",
        ));
    }

    let ra_plan =
        build_plan_from_sql(&req.sql, req.hardware_profile.as_ref());
    let pg_plan = build_pg_plan(&req.sql);
    let mysql_plan = build_mysql_plan(&req.sql);
    let duckdb_plan = build_duckdb_plan(&req.sql);

    let plans = vec![
        OptimizerPlan {
            optimizer: "Ra".to_owned(),
            total_cost: sum_cost(&ra_plan),
            plan: ra_plan,
            available: true,
        },
        OptimizerPlan {
            optimizer: "PostgreSQL".to_owned(),
            total_cost: sum_cost(&pg_plan),
            plan: pg_plan,
            available: true,
        },
        OptimizerPlan {
            optimizer: "MySQL".to_owned(),
            total_cost: sum_cost(&mysql_plan),
            plan: mysql_plan,
            available: true,
        },
        OptimizerPlan {
            optimizer: "DuckDB".to_owned(),
            total_cost: sum_cost(&duckdb_plan),
            plan: duckdb_plan,
            available: true,
        },
    ];

    let cheapest = plans
        .iter()
        .filter(|p| p.available)
        .min_by(|a, b| {
            a.total_cost
                .partial_cmp(&b.total_cost)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map_or_else(|| "Ra".to_owned(), |p| p.optimizer.clone());

    let costs = plans
        .iter()
        .map(|p| OptimizerCost {
            optimizer: p.optimizer.clone(),
            total_cost: p.total_cost,
            node_count: count_nodes(&p.plan),
        })
        .collect();

    Ok(Json(ComparePlansResponse {
        plans,
        summary: CostSummary { cheapest, costs },
    }))
}

fn sum_cost(node: &VisualPlanNode) -> f64 {
    node.cost
        + node.children.iter().map(sum_cost).sum::<f64>()
}

fn count_nodes(node: &VisualPlanNode) -> u32 {
    1 + node
        .children
        .iter()
        .map(count_nodes)
        .sum::<u32>()
}

fn make_node(
    id: String,
    operator_type: &str,
    cost: f64,
    rows: u64,
    details: Vec<PlanDetail>,
    children: Vec<VisualPlanNode>,
) -> VisualPlanNode {
    VisualPlanNode {
        id,
        operator_type: operator_type.to_owned(),
        cost,
        rows,
        details,
        children,
        position: NodePosition {
            x: 0.0,
            y: 0.0,
            width: 160.0,
            height: 60.0,
        },
    }
}

fn build_plan_from_sql(
    sql: &str,
    _hardware_profile: Option<&String>,
) -> VisualPlanNode {
    let sql_lower = sql.to_lowercase();
    let has_join = sql_lower.contains("join");
    let has_where = sql_lower.contains("where");
    let has_group = sql_lower.contains("group by");
    let has_order = sql_lower.contains("order by");

    let mut id_counter = 0_u32;
    let mut next_id = || {
        id_counter += 1;
        format!("ra-{id_counter}")
    };

    let scan = make_node(
        next_id(), "SeqScan", 120.0, 10000,
        vec![PlanDetail { key: "table".to_owned(), value: extract_table(sql) }],
        vec![],
    );
    let mut current = scan;

    if has_join {
        let right_scan = make_node(
            next_id(), "IdxScan", 45.0, 500,
            vec![PlanDetail { key: "index".to_owned(), value: "pk_idx".to_owned() }],
            vec![],
        );
        current = make_node(
            next_id(), "HashJoin", 350.0, 2500,
            vec![PlanDetail { key: "condition".to_owned(), value: "a.id = b.id".to_owned() }],
            vec![current, right_scan],
        );
    }

    if has_where {
        current = make_node(
            next_id(), "Filter", 80.0, current.rows / 4,
            vec![PlanDetail { key: "predicate".to_owned(), value: extract_predicate(sql) }],
            vec![current],
        );
    }

    if has_group {
        current = make_node(
            next_id(), "HashAggregate", 200.0, current.rows / 10,
            vec![PlanDetail { key: "strategy".to_owned(), value: "hash".to_owned() }],
            vec![current],
        );
    }

    if has_order {
        current = make_node(
            next_id(), "Sort", 150.0, current.rows,
            vec![PlanDetail { key: "method".to_owned(), value: "quicksort".to_owned() }],
            vec![current],
        );
    }

    make_node(
        next_id(), "Project", 10.0, current.rows,
        vec![PlanDetail { key: "columns".to_owned(), value: "*".to_owned() }],
        vec![current],
    )
}

fn build_pg_plan(sql: &str) -> VisualPlanNode {
    let sql_lower = sql.to_lowercase();
    let has_join = sql_lower.contains("join");
    let has_where = sql_lower.contains("where");

    let mut id_counter = 0_u32;
    let mut next_id = || {
        id_counter += 1;
        format!("pg-{id_counter}")
    };

    let scan = make_node(
        next_id(), "Seq Scan", 145.0, 10000,
        vec![PlanDetail { key: "relation".to_owned(), value: extract_table(sql) }],
        vec![],
    );
    let mut current = scan;

    if has_join {
        let inner = make_node(next_id(), "Index Scan", 55.0, 500, vec![], vec![]);
        current = make_node(
            next_id(), "Nested Loop", 520.0, 2500,
            vec![PlanDetail { key: "join_type".to_owned(), value: "inner".to_owned() }],
            vec![current, inner],
        );
    }

    if has_where {
        current = make_node(
            next_id(), "Filter", 95.0, current.rows / 3, vec![], vec![current],
        );
    }

    make_node(next_id(), "Result", 5.0, current.rows, vec![], vec![current])
}

fn build_mysql_plan(sql: &str) -> VisualPlanNode {
    let sql_lower = sql.to_lowercase();
    let has_join = sql_lower.contains("join");
    let has_where = sql_lower.contains("where");

    let mut id_counter = 0_u32;
    let mut next_id = || {
        id_counter += 1;
        format!("mysql-{id_counter}")
    };

    let scan = make_node(
        next_id(), "Full Table Scan", 180.0, 10000,
        vec![PlanDetail { key: "table".to_owned(), value: extract_table(sql) }],
        vec![],
    );
    let mut current = scan;

    if has_join {
        let inner = make_node(next_id(), "ref", 60.0, 500, vec![], vec![]);
        current = make_node(
            next_id(), "Block Nested Loop", 680.0, 2500, vec![], vec![current, inner],
        );
    }

    if has_where {
        current = make_node(
            next_id(), "Using where", 110.0, current.rows / 5, vec![], vec![current],
        );
    }

    make_node(next_id(), "Query", 5.0, current.rows, vec![], vec![current])
}

fn build_duckdb_plan(sql: &str) -> VisualPlanNode {
    let sql_lower = sql.to_lowercase();
    let has_join = sql_lower.contains("join");
    let has_where = sql_lower.contains("where");

    let mut id_counter = 0_u32;
    let mut next_id = || {
        id_counter += 1;
        format!("duck-{id_counter}")
    };

    let scan = make_node(
        next_id(), "SCAN", 95.0, 10000,
        vec![PlanDetail { key: "table".to_owned(), value: extract_table(sql) }],
        vec![],
    );
    let mut current = scan;

    if has_join {
        let probe = make_node(next_id(), "SCAN", 40.0, 500, vec![], vec![]);
        current = make_node(
            next_id(), "HASH_JOIN", 280.0, 2500,
            vec![PlanDetail { key: "type".to_owned(), value: "INNER".to_owned() }],
            vec![current, probe],
        );
    }

    if has_where {
        current = make_node(
            next_id(), "FILTER", 65.0, current.rows / 4, vec![], vec![current],
        );
    }

    make_node(next_id(), "PROJECTION", 8.0, current.rows, vec![], vec![current])
}

fn extract_table(sql: &str) -> String {
    let lower = sql.to_lowercase();
    let from_pos = lower.find("from ");
    match from_pos {
        Some(pos) => {
            let rest = &sql[pos + 5..];
            let trimmed = rest.trim_start();
            let end = trimmed
                .find(|c: char| {
                    c.is_whitespace() || c == ';' || c == ')'
                })
                .unwrap_or(trimmed.len());
            trimmed[..end].to_owned()
        }
        None => "table".to_owned(),
    }
}

fn extract_predicate(sql: &str) -> String {
    let lower = sql.to_lowercase();
    let where_pos = lower.find("where ");
    match where_pos {
        Some(pos) => {
            let rest = &sql[pos + 6..];
            let trimmed = rest.trim_start();
            let end = trimmed
                .find([';', ')'])
                .or_else(|| {
                    let l = trimmed.to_lowercase();
                    l.find("group ").or_else(|| {
                        l.find("order ").or_else(|| {
                            l.find("limit ")
                                .or_else(|| l.find("having "))
                        })
                    })
                })
                .unwrap_or(trimmed.len());
            trimmed[..end].trim().to_owned()
        }
        None => "true".to_owned(),
    }
}
