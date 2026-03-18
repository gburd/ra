//! Differential validator comparing RA optimizer plans with
//! database EXPLAIN plans.
//!
//! Extracts structural features from both plan representations
//! and produces a comparison report with agreements, disagreements,
//! and confidence scores.

use serde::{Deserialize, Serialize};

use crate::explain::{ExplainPlan, JoinAlgorithm, PlanNode};
use ra_core::algebra::{JoinType, RelExpr};

/// Result of comparing an RA plan with a database EXPLAIN plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffReport {
    /// SQL query being compared.
    pub query: String,
    /// Database engine that produced the EXPLAIN plan.
    pub engine: String,
    /// Points where the two plans agree.
    pub agreements: Vec<DiffPoint>,
    /// Points where the two plans disagree.
    pub disagreements: Vec<DiffPoint>,
    /// Overall confidence score (0.0 to 1.0).
    pub confidence: f64,
    /// Summary assessment.
    pub summary: String,
}

/// A single point of comparison between plans.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffPoint {
    /// What aspect is being compared.
    pub aspect: DiffAspect,
    /// Value from the RA optimizer plan.
    pub ra_value: String,
    /// Value from the database EXPLAIN plan.
    pub db_value: String,
    /// Whether these values agree.
    pub agrees: bool,
    /// Confidence in this comparison (0.0 to 1.0).
    pub confidence: f64,
    /// Human-readable explanation.
    pub explanation: String,
}

/// Aspects of plans that can be compared.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum DiffAspect {
    /// Tables accessed by the query.
    TableAccess,
    /// Order of joins.
    JoinOrder,
    /// Algorithm chosen for joins.
    JoinAlgorithm,
    /// Whether indexes are used.
    IndexUsage,
    /// Placement of filter predicates.
    FilterPlacement,
    /// Strategy for aggregation.
    AggregationStrategy,
    /// Presence and type of sort operations.
    SortOperation,
}

impl std::fmt::Display for DiffAspect {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::TableAccess => write!(f, "Table Access"),
            Self::JoinOrder => write!(f, "Join Order"),
            Self::JoinAlgorithm => {
                write!(f, "Join Algorithm")
            }
            Self::IndexUsage => write!(f, "Index Usage"),
            Self::FilterPlacement => {
                write!(f, "Filter Placement")
            }
            Self::AggregationStrategy => {
                write!(f, "Aggregation Strategy")
            }
            Self::SortOperation => {
                write!(f, "Sort Operation")
            }
        }
    }
}

/// Compare an RA optimizer plan with a database EXPLAIN plan.
pub fn compare_plans(
    ra_plan: &RelExpr,
    explain: &ExplainPlan,
) -> DiffReport {
    let mut agreements = Vec::new();
    let mut disagreements = Vec::new();

    compare_table_access(
        ra_plan,
        &explain.root,
        &mut agreements,
        &mut disagreements,
    );

    compare_join_algorithm(
        ra_plan,
        &explain.root,
        &mut agreements,
        &mut disagreements,
    );

    compare_index_usage(
        &explain.root,
        &mut agreements,
        &mut disagreements,
    );

    compare_filter_placement(
        ra_plan,
        &explain.root,
        &mut agreements,
        &mut disagreements,
    );

    compare_aggregation(
        ra_plan,
        &explain.root,
        &mut agreements,
        &mut disagreements,
    );

    compare_sort(
        ra_plan,
        &explain.root,
        &mut agreements,
        &mut disagreements,
    );

    let total = agreements.len() + disagreements.len();
    let confidence = if total == 0 {
        0.5
    } else {
        let weighted_agree: f64 =
            agreements.iter().map(|a| a.confidence).sum();
        let weighted_total: f64 = agreements
            .iter()
            .chain(disagreements.iter())
            .map(|d| d.confidence)
            .sum();
        if weighted_total > 0.0 {
            weighted_agree / weighted_total
        } else {
            0.5
        }
    };

    let summary = build_summary(
        &agreements,
        &disagreements,
        confidence,
    );

    DiffReport {
        query: explain.query.clone(),
        engine: explain.engine.clone(),
        agreements,
        disagreements,
        confidence,
        summary,
    }
}

fn collect_ra_tables(plan: &RelExpr) -> Vec<String> {
    let mut tables = Vec::new();
    collect_ra_tables_inner(plan, &mut tables);
    tables.sort();
    tables.dedup();
    tables
}

fn collect_ra_tables_inner(
    plan: &RelExpr,
    tables: &mut Vec<String>,
) {
    match plan {
        RelExpr::Scan { table, .. } => {
            tables.push(table.clone());
        }
        other => {
            for child in other.children() {
                collect_ra_tables_inner(child, tables);
            }
        }
    }
}

fn collect_explain_tables(node: &PlanNode) -> Vec<String> {
    let mut tables = Vec::new();
    collect_explain_tables_inner(node, &mut tables);
    tables.sort();
    tables.dedup();
    tables
}

fn collect_explain_tables_inner(
    node: &PlanNode,
    tables: &mut Vec<String>,
) {
    if let Some(ref rel) = node.relation {
        tables.push(rel.clone());
    }
    for child in &node.children {
        collect_explain_tables_inner(child, tables);
    }
}

fn compare_table_access(
    ra_plan: &RelExpr,
    explain_root: &PlanNode,
    agreements: &mut Vec<DiffPoint>,
    disagreements: &mut Vec<DiffPoint>,
) {
    let ra_tables = collect_ra_tables(ra_plan);
    let db_tables = collect_explain_tables(explain_root);

    let ra_str = ra_tables.join(", ");
    let db_str = db_tables.join(", ");

    let agrees = ra_tables == db_tables;

    let point = DiffPoint {
        aspect: DiffAspect::TableAccess,
        ra_value: ra_str,
        db_value: db_str,
        agrees,
        confidence: 0.9,
        explanation: if agrees {
            "Both plans access the same tables".to_string()
        } else {
            "Plans access different tables".to_string()
        },
    };

    if agrees {
        agreements.push(point);
    } else {
        disagreements.push(point);
    }
}

fn collect_ra_joins(plan: &RelExpr) -> Vec<JoinType> {
    let mut joins = Vec::new();
    collect_ra_joins_inner(plan, &mut joins);
    joins
}

fn collect_ra_joins_inner(
    plan: &RelExpr,
    joins: &mut Vec<JoinType>,
) {
    if let RelExpr::Join {
        join_type,
        left,
        right,
        ..
    } = plan
    {
        joins.push(*join_type);
        collect_ra_joins_inner(left, joins);
        collect_ra_joins_inner(right, joins);
    } else {
        for child in plan.children() {
            collect_ra_joins_inner(child, joins);
        }
    }
}

fn collect_explain_join_algorithms(
    node: &PlanNode,
) -> Vec<JoinAlgorithm> {
    let mut algs = Vec::new();
    collect_explain_join_algs_inner(node, &mut algs);
    algs
}

fn collect_explain_join_algs_inner(
    node: &PlanNode,
    algs: &mut Vec<JoinAlgorithm>,
) {
    if let Some(alg) = node.join_algorithm() {
        algs.push(alg);
    }
    for child in &node.children {
        collect_explain_join_algs_inner(child, algs);
    }
}

fn compare_join_algorithm(
    ra_plan: &RelExpr,
    explain_root: &PlanNode,
    agreements: &mut Vec<DiffPoint>,
    disagreements: &mut Vec<DiffPoint>,
) {
    let ra_joins = collect_ra_joins(ra_plan);
    let db_algs = collect_explain_join_algorithms(explain_root);

    if ra_joins.is_empty() && db_algs.is_empty() {
        return;
    }

    let ra_str = if ra_joins.is_empty() {
        "no joins".to_string()
    } else {
        ra_joins
            .iter()
            .map(|j| format!("{j}"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let db_str = if db_algs.is_empty() {
        "no join algorithms detected".to_string()
    } else {
        db_algs
            .iter()
            .map(|a| format!("{a}"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let agrees =
        ra_joins.len() == db_algs.len();

    let point = DiffPoint {
        aspect: DiffAspect::JoinAlgorithm,
        ra_value: ra_str,
        db_value: db_str,
        agrees,
        confidence: 0.7,
        explanation: if agrees {
            "Join count matches between plans".to_string()
        } else {
            "Different number of join operations"
                .to_string()
        },
    };

    if agrees {
        agreements.push(point);
    } else {
        disagreements.push(point);
    }
}

fn compare_index_usage(
    explain_root: &PlanNode,
    agreements: &mut Vec<DiffPoint>,
    disagreements: &mut Vec<DiffPoint>,
) {
    let all_types = explain_root.all_node_types();
    let uses_idx =
        all_types.iter().any(|t| {
            let lower = t.to_lowercase();
            lower.contains("index")
        });
    let uses_seq =
        all_types.iter().any(|t| {
            let lower = t.to_lowercase();
            lower.contains("seq scan")
                || lower.contains("full table scan")
        });

    let db_value = if uses_idx && uses_seq {
        "Mixed (index + sequential scans)".to_string()
    } else if uses_idx {
        "Index scans only".to_string()
    } else {
        "Sequential scans only".to_string()
    };

    let point = DiffPoint {
        aspect: DiffAspect::IndexUsage,
        ra_value: "N/A (RA plans are logical)".to_string(),
        db_value,
        agrees: true,
        confidence: 0.5,
        explanation: "Index usage is a physical decision; \
                      RA plans are logical"
            .to_string(),
    };

    agreements.push(point);

    let _ = disagreements;
}

fn has_ra_filter(plan: &RelExpr) -> bool {
    match plan {
        RelExpr::Filter { .. } => true,
        other => other.children().iter().any(|c| has_ra_filter(c)),
    }
}

fn has_explain_filter(node: &PlanNode) -> bool {
    if node.filter.is_some() {
        return true;
    }
    node.children.iter().any(has_explain_filter)
}

fn compare_filter_placement(
    ra_plan: &RelExpr,
    explain_root: &PlanNode,
    agreements: &mut Vec<DiffPoint>,
    disagreements: &mut Vec<DiffPoint>,
) {
    let ra_has_filter = has_ra_filter(ra_plan);
    let db_has_filter = has_explain_filter(explain_root);

    let agrees = ra_has_filter == db_has_filter;

    let point = DiffPoint {
        aspect: DiffAspect::FilterPlacement,
        ra_value: if ra_has_filter {
            "has filters".to_string()
        } else {
            "no filters".to_string()
        },
        db_value: if db_has_filter {
            "has filters".to_string()
        } else {
            "no filters".to_string()
        },
        agrees,
        confidence: 0.8,
        explanation: if agrees {
            "Both plans agree on filter presence"
                .to_string()
        } else {
            "Filter presence differs between plans"
                .to_string()
        },
    };

    if agrees {
        agreements.push(point);
    } else {
        disagreements.push(point);
    }
}

fn has_ra_aggregate(plan: &RelExpr) -> bool {
    match plan {
        RelExpr::Aggregate { .. } => true,
        other => {
            other.children().iter().any(|c| has_ra_aggregate(c))
        }
    }
}

fn has_explain_aggregate(node: &PlanNode) -> bool {
    let lower = node.node_type.to_lowercase();
    if lower.contains("aggregate")
        || lower.contains("group")
        || lower.contains("hash aggregate")
    {
        return true;
    }
    node.children.iter().any(has_explain_aggregate)
}

fn compare_aggregation(
    ra_plan: &RelExpr,
    explain_root: &PlanNode,
    agreements: &mut Vec<DiffPoint>,
    disagreements: &mut Vec<DiffPoint>,
) {
    let ra_has_agg = has_ra_aggregate(ra_plan);
    let db_has_agg = has_explain_aggregate(explain_root);

    if !ra_has_agg && !db_has_agg {
        return;
    }

    let agrees = ra_has_agg == db_has_agg;

    let point = DiffPoint {
        aspect: DiffAspect::AggregationStrategy,
        ra_value: if ra_has_agg {
            "has aggregation".to_string()
        } else {
            "no aggregation".to_string()
        },
        db_value: if db_has_agg {
            "has aggregation".to_string()
        } else {
            "no aggregation".to_string()
        },
        agrees,
        confidence: 0.8,
        explanation: if agrees {
            "Both plans agree on aggregation presence"
                .to_string()
        } else {
            "Aggregation presence differs between plans"
                .to_string()
        },
    };

    if agrees {
        agreements.push(point);
    } else {
        disagreements.push(point);
    }
}

fn has_ra_sort(plan: &RelExpr) -> bool {
    match plan {
        RelExpr::Sort { .. } => true,
        other => other.children().iter().any(|c| has_ra_sort(c)),
    }
}

fn has_explain_sort(node: &PlanNode) -> bool {
    let lower = node.node_type.to_lowercase();
    if lower.contains("sort") {
        return true;
    }
    node.children.iter().any(has_explain_sort)
}

fn compare_sort(
    ra_plan: &RelExpr,
    explain_root: &PlanNode,
    agreements: &mut Vec<DiffPoint>,
    disagreements: &mut Vec<DiffPoint>,
) {
    let ra_has_sort = has_ra_sort(ra_plan);
    let db_has_sort = has_explain_sort(explain_root);

    if !ra_has_sort && !db_has_sort {
        return;
    }

    let agrees = ra_has_sort == db_has_sort;

    let point = DiffPoint {
        aspect: DiffAspect::SortOperation,
        ra_value: if ra_has_sort {
            "has sort".to_string()
        } else {
            "no sort".to_string()
        },
        db_value: if db_has_sort {
            "has sort".to_string()
        } else {
            "no sort".to_string()
        },
        agrees,
        confidence: 0.8,
        explanation: if agrees {
            "Both plans agree on sort presence"
                .to_string()
        } else {
            "Sort presence differs between plans"
                .to_string()
        },
    };

    if agrees {
        agreements.push(point);
    } else {
        disagreements.push(point);
    }
}

fn build_summary(
    agreements: &[DiffPoint],
    disagreements: &[DiffPoint],
    confidence: f64,
) -> String {
    let total = agreements.len() + disagreements.len();
    if total == 0 {
        return "No comparison points available".to_string();
    }

    let pct = confidence * 100.0;
    format!(
        "{} agreement(s), {} disagreement(s) \
         ({pct:.0}% confidence). \
         Compared: {}.",
        agreements.len(),
        disagreements.len(),
        agreements
            .iter()
            .chain(disagreements.iter())
            .map(|d| d.aspect.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explain::PlanNode;

    fn simple_scan_plan() -> RelExpr {
        RelExpr::scan("users")
    }

    fn simple_scan_explain() -> ExplainPlan {
        ExplainPlan {
            engine: "PostgreSQL".to_string(),
            query: "SELECT * FROM users".to_string(),
            root: PlanNode {
                node_type: "Seq Scan".to_string(),
                relation: Some("users".to_string()),
                alias: None,
                join_type: None,
                index_name: None,
                filter: None,
                join_condition: None,
                sort_keys: None,
                startup_cost: Some(0.0),
                total_cost: Some(35.5),
                estimated_rows: Some(2550.0),
                row_width: Some(64),
                children: vec![],
                extra: serde_json::Value::Null,
            },
            total_cost: Some(35.5),
            total_rows: Some(2550.0),
        }
    }

    #[test]
    fn compare_simple_scan_agreement() {
        let report = compare_plans(
            &simple_scan_plan(),
            &simple_scan_explain(),
        );

        assert_eq!(report.engine, "PostgreSQL");
        assert!(!report.agreements.is_empty());
        assert!(report.confidence > 0.0);
    }

    #[test]
    fn compare_table_access_match() {
        let ra = simple_scan_plan();
        let explain = simple_scan_explain();

        let mut agreements = Vec::new();
        let mut disagreements = Vec::new();

        compare_table_access(
            &ra,
            &explain.root,
            &mut agreements,
            &mut disagreements,
        );

        assert_eq!(agreements.len(), 1);
        assert!(agreements[0].agrees);
        assert_eq!(
            agreements[0].aspect,
            DiffAspect::TableAccess
        );
    }

    #[test]
    fn compare_table_access_mismatch() {
        let ra = RelExpr::scan("orders");
        let explain = simple_scan_explain();

        let mut agreements = Vec::new();
        let mut disagreements = Vec::new();

        compare_table_access(
            &ra,
            &explain.root,
            &mut agreements,
            &mut disagreements,
        );

        assert_eq!(disagreements.len(), 1);
        assert!(!disagreements[0].agrees);
    }

    #[test]
    fn compare_filter_both_present() {
        let ra = RelExpr::scan("users").filter(
            ra_core::expr::Expr::Const(
                ra_core::expr::Const::Bool(true),
            ),
        );

        let mut explain_node =
            PlanNode::new("Seq Scan");
        explain_node.relation =
            Some("users".to_string());
        explain_node.filter =
            Some("(active = true)".to_string());

        let mut agreements = Vec::new();
        let mut disagreements = Vec::new();

        compare_filter_placement(
            &ra,
            &explain_node,
            &mut agreements,
            &mut disagreements,
        );

        assert_eq!(agreements.len(), 1);
        assert!(agreements[0].agrees);
    }

    #[test]
    fn compare_no_joins() {
        let ra = simple_scan_plan();
        let explain = simple_scan_explain();

        let mut agreements = Vec::new();
        let mut disagreements = Vec::new();

        compare_join_algorithm(
            &ra,
            &explain.root,
            &mut agreements,
            &mut disagreements,
        );

        // No joins to compare, no points generated
        assert!(agreements.is_empty());
        assert!(disagreements.is_empty());
    }

    #[test]
    fn diff_report_serialization() {
        let report = DiffReport {
            query: "SELECT 1".to_string(),
            engine: "SQLite".to_string(),
            agreements: vec![],
            disagreements: vec![],
            confidence: 0.5,
            summary: "No comparison points".to_string(),
        };

        let json = serde_json::to_string(&report)
            .expect("should serialize");
        let roundtrip: DiffReport =
            serde_json::from_str(&json)
                .expect("should deserialize");
        assert_eq!(report, roundtrip);
    }

    #[test]
    fn diff_aspect_display() {
        assert_eq!(
            DiffAspect::TableAccess.to_string(),
            "Table Access"
        );
        assert_eq!(
            DiffAspect::JoinOrder.to_string(),
            "Join Order"
        );
        assert_eq!(
            DiffAspect::IndexUsage.to_string(),
            "Index Usage"
        );
    }

    #[test]
    fn confidence_all_agree() {
        let ra = simple_scan_plan();
        let explain = simple_scan_explain();
        let report = compare_plans(&ra, &explain);
        assert!(report.confidence > 0.5);
    }

    #[test]
    fn summary_format() {
        let summary = build_summary(
            &[DiffPoint {
                aspect: DiffAspect::TableAccess,
                ra_value: "users".to_string(),
                db_value: "users".to_string(),
                agrees: true,
                confidence: 0.9,
                explanation: "match".to_string(),
            }],
            &[],
            0.9,
        );
        assert!(summary.contains("1 agreement(s)"));
        assert!(summary.contains("0 disagreement(s)"));
    }
}
