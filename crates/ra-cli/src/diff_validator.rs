//! Differential validator: compares RA optimizer plans against
//! database EXPLAIN plans to identify agreements and disagreements.
//!
//! The comparison types and functions are exercised by unit tests
//! in this module; `format_explain_tree` is used by CLI commands.

// Comparison types/functions are only used in tests within this module.
#![allow(dead_code)]

use std::fmt::Write;

use ra_core::algebra::{JoinType as RaJoinType, RelExpr};
use ra_metadata::explain::{
    ExplainNode, ExplainPlan, JoinType as DbJoinType, NodeType,
};

/// Result of comparing an RA plan against a database EXPLAIN plan.
#[derive(Debug, Clone)]
pub struct PlanComparison {
    /// Points where the RA plan and DB plan agree.
    pub agreements: Vec<Agreement>,
    /// Points where the RA plan and DB plan disagree.
    pub disagreements: Vec<Disagreement>,
    /// Confidence score in [0.0, 1.0] representing overall agreement.
    pub confidence: f64,
}

/// A point of agreement between the RA plan and the DB plan.
#[derive(Debug, Clone)]
pub struct Agreement {
    /// What was compared.
    pub aspect: ComparisonAspect,
    /// Human-readable description.
    pub description: String,
}

/// A point of disagreement between the RA plan and the DB plan.
#[derive(Debug, Clone)]
pub struct Disagreement {
    /// What was compared.
    pub aspect: ComparisonAspect,
    /// What the RA optimizer chose.
    pub ra_choice: String,
    /// What the database chose.
    pub db_choice: String,
    /// Possible explanation for the difference.
    pub explanation: String,
}

/// Aspects of a plan that can be compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonAspect {
    /// Table access method (scan vs index).
    AccessMethod,
    /// Join algorithm (hash, merge, nested loop).
    JoinAlgorithm,
    /// Join order (which table is on which side).
    JoinOrder,
    /// Sort presence or absence.
    SortStrategy,
    /// Aggregation strategy (hash vs group).
    AggregationStrategy,
    /// Filter placement.
    FilterPlacement,
}

impl std::fmt::Display for ComparisonAspect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AccessMethod => write!(f, "Access Method"),
            Self::JoinAlgorithm => write!(f, "Join Algorithm"),
            Self::JoinOrder => write!(f, "Join Order"),
            Self::SortStrategy => write!(f, "Sort Strategy"),
            Self::AggregationStrategy => write!(f, "Aggregation Strategy"),
            Self::FilterPlacement => write!(f, "Filter Placement"),
        }
    }
}

/// Compare an RA optimizer plan against a database EXPLAIN plan.
pub fn compare_plans(ra_plan: &RelExpr, db_explain: &ExplainPlan) -> PlanComparison {
    let mut agreements = Vec::new();
    let mut disagreements = Vec::new();

    compare_node(ra_plan, &db_explain.root, &mut agreements, &mut disagreements);

    let total = agreements.len() + disagreements.len();
    #[allow(clippy::cast_precision_loss)]
    let confidence = if total == 0 {
        1.0
    } else {
        agreements.len() as f64 / total as f64
    };

    PlanComparison {
        agreements,
        disagreements,
        confidence,
    }
}

fn compare_node(
    ra: &RelExpr,
    db: &ExplainNode,
    agreements: &mut Vec<Agreement>,
    disagreements: &mut Vec<Disagreement>,
) {
    match ra {
        RelExpr::Scan { table, .. } => {
            compare_scan(table, db, agreements, disagreements);
        }
        RelExpr::Join {
            join_type,
            left,
            right,
            ..
        } => {
            compare_join(
                *join_type, left, right, db, agreements, disagreements,
            );
        }
        RelExpr::Sort { input, .. } => {
            compare_sort(db, agreements, disagreements);
            compare_node(input, db, agreements, disagreements);
        }
        RelExpr::Aggregate { input, .. } => {
            compare_aggregation(db, agreements, disagreements);
            compare_node(input, db, agreements, disagreements);
        }
        RelExpr::Filter { input, .. } => {
            compare_filter(db, agreements, disagreements);
            compare_node(input, db, agreements, disagreements);
        }
        RelExpr::Project { input, .. }
        | RelExpr::Distinct { input }
        | RelExpr::Limit { input, .. }
        | RelExpr::Window { input, .. } => {
            compare_node(input, db, agreements, disagreements);
        }
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            if let Some(child) = db.children.first() {
                compare_node(left, child, agreements, disagreements);
            }
            if let Some(child) = db.children.get(1) {
                compare_node(right, child, agreements, disagreements);
            }
        }
        RelExpr::CTE { body, .. }
        | RelExpr::RecursiveCTE { body, .. } => {
            compare_node(body, db, agreements, disagreements);
        }
        RelExpr::Values { .. } => {}
    }
}

fn compare_scan(
    table: &str,
    db: &ExplainNode,
    agreements: &mut Vec<Agreement>,
    disagreements: &mut Vec<Disagreement>,
) {
    let db_scan = find_scan_for_table(db, table);
    let Some(scan_node) = db_scan else {
        return;
    };

    let ra_method = "Seq Scan";
    let db_method = match scan_node.node_type {
        NodeType::SeqScan => "Seq Scan",
        NodeType::IndexScan => "Index Scan",
        NodeType::IndexOnlyScan => "Index Only Scan",
        NodeType::BitmapIndexScan | NodeType::BitmapHeapScan => "Bitmap Scan",
        _ => "Other",
    };

    if ra_method == db_method {
        agreements.push(Agreement {
            aspect: ComparisonAspect::AccessMethod,
            description: format!(
                "Table '{table}': both use {db_method}"
            ),
        });
    } else {
        let explanation = match scan_node.node_type {
            NodeType::IndexScan | NodeType::IndexOnlyScan => {
                let idx = scan_node
                    .index_name
                    .as_deref()
                    .unwrap_or("unknown");
                format!(
                    "DB uses index '{idx}' on '{table}'. \
                     RA optimizer lacks index metadata -- \
                     gather schema to enable index-aware optimization."
                )
            }
            NodeType::BitmapIndexScan | NodeType::BitmapHeapScan => {
                "DB uses bitmap scan for multi-column predicate. \
                 RA optimizer defaults to sequential scan without \
                 index statistics."
                    .to_owned()
            }
            _ => "Different access method chosen.".to_owned(),
        };

        disagreements.push(Disagreement {
            aspect: ComparisonAspect::AccessMethod,
            ra_choice: format!("{ra_method} on '{table}'"),
            db_choice: format!("{db_method} on '{table}'"),
            explanation,
        });
    }
}

fn compare_join(
    ra_join_type: RaJoinType,
    ra_left: &RelExpr,
    ra_right: &RelExpr,
    db: &ExplainNode,
    agreements: &mut Vec<Agreement>,
    disagreements: &mut Vec<Disagreement>,
) {
    let db_join = find_join_node(db);
    let Some(join_node) = db_join else {
        return;
    };

    let ra_algo = "Hash Join";
    let db_algo = match join_node.node_type {
        NodeType::HashJoin => "Hash Join",
        NodeType::MergeJoin => "Merge Join",
        NodeType::NestedLoop => "Nested Loop",
        _ => "Other",
    };

    if ra_algo == db_algo {
        agreements.push(Agreement {
            aspect: ComparisonAspect::JoinAlgorithm,
            description: format!("Both use {db_algo}"),
        });
    } else {
        let explanation = match join_node.node_type {
            NodeType::NestedLoop => {
                "DB chose nested loop -- often optimal for small \
                 outer table or indexed inner table."
                    .to_owned()
            }
            NodeType::MergeJoin => {
                "DB chose merge join -- data may already be sorted \
                 or an index provides order."
                    .to_owned()
            }
            _ => "Different join algorithm selected.".to_owned(),
        };

        disagreements.push(Disagreement {
            aspect: ComparisonAspect::JoinAlgorithm,
            ra_choice: ra_algo.to_owned(),
            db_choice: db_algo.to_owned(),
            explanation,
        });
    }

    if let Some(db_jt) = join_node.join_type {
        let ra_jt_name = format_ra_join_type(ra_join_type);
        let db_jt_name = format_db_join_type(db_jt);
        if ra_jt_name != db_jt_name {
            disagreements.push(Disagreement {
                aspect: ComparisonAspect::JoinOrder,
                ra_choice: ra_jt_name,
                db_choice: db_jt_name,
                explanation: "Join types differ between RA plan and DB."
                    .to_owned(),
            });
        }
    }

    if let Some(child) = join_node.children.first() {
        compare_node(ra_left, child, agreements, disagreements);
    }
    if let Some(child) = join_node.children.get(1) {
        compare_node(ra_right, child, agreements, disagreements);
    }
}

fn compare_sort(
    db: &ExplainNode,
    agreements: &mut Vec<Agreement>,
    disagreements: &mut Vec<Disagreement>,
) {
    let has_db_sort = has_node_type(db, NodeType::Sort);

    if has_db_sort {
        agreements.push(Agreement {
            aspect: ComparisonAspect::SortStrategy,
            description: "Both plans include a Sort operator".to_owned(),
        });
    } else {
        disagreements.push(Disagreement {
            aspect: ComparisonAspect::SortStrategy,
            ra_choice: "Explicit Sort".to_owned(),
            db_choice: "No Sort (index provides order)".to_owned(),
            explanation: "DB avoids sort by using an index that \
                         provides the required ordering."
                .to_owned(),
        });
    }
}

fn compare_aggregation(
    db: &ExplainNode,
    agreements: &mut Vec<Agreement>,
    disagreements: &mut Vec<Disagreement>,
) {
    let db_hash_agg = has_node_type(db, NodeType::HashAggregate);
    let db_group_agg = has_node_type(db, NodeType::GroupAggregate);

    if db_hash_agg {
        agreements.push(Agreement {
            aspect: ComparisonAspect::AggregationStrategy,
            description: "Both plans use hash-based aggregation"
                .to_owned(),
        });
    } else if db_group_agg {
        disagreements.push(Disagreement {
            aspect: ComparisonAspect::AggregationStrategy,
            ra_choice: "Hash Aggregate".to_owned(),
            db_choice: "Group Aggregate (sorted)".to_owned(),
            explanation: "DB chose streaming group aggregate -- \
                         input may already be sorted by an index."
                .to_owned(),
        });
    }
}

fn compare_filter(
    db: &ExplainNode,
    agreements: &mut Vec<Agreement>,
    disagreements: &mut Vec<Disagreement>,
) {
    let has_filter = db.filter.is_some()
        || db
            .children
            .iter()
            .any(|c| c.filter.is_some());

    if has_filter {
        agreements.push(Agreement {
            aspect: ComparisonAspect::FilterPlacement,
            description: "Both plans apply filter predicates".to_owned(),
        });
    } else {
        disagreements.push(Disagreement {
            aspect: ComparisonAspect::FilterPlacement,
            ra_choice: "Explicit Filter operator".to_owned(),
            db_choice: "Filter pushed into scan/index condition"
                .to_owned(),
            explanation: "DB pushed the filter into the scan operator \
                         as an index condition, eliminating the need \
                         for a separate Filter node."
                .to_owned(),
        });
    }
}

// ---- Tree search helpers ----

fn find_scan_for_table<'a>(
    node: &'a ExplainNode,
    table: &str,
) -> Option<&'a ExplainNode> {
    if node.relation.as_deref() == Some(table) {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_scan_for_table(child, table) {
            return Some(found);
        }
    }
    None
}

fn find_join_node(node: &ExplainNode) -> Option<&ExplainNode> {
    match node.node_type {
        NodeType::HashJoin | NodeType::MergeJoin | NodeType::NestedLoop => {
            Some(node)
        }
        _ => {
            for child in &node.children {
                if let Some(found) = find_join_node(child) {
                    return Some(found);
                }
            }
            None
        }
    }
}

fn has_node_type(node: &ExplainNode, target: NodeType) -> bool {
    if node.node_type == target {
        return true;
    }
    node.children.iter().any(|c| has_node_type(c, target))
}

fn format_ra_join_type(jt: RaJoinType) -> String {
    match jt {
        RaJoinType::Inner => "Inner".to_owned(),
        RaJoinType::LeftOuter => "Left".to_owned(),
        RaJoinType::RightOuter => "Right".to_owned(),
        RaJoinType::FullOuter => "Full".to_owned(),
        RaJoinType::Cross => "Cross".to_owned(),
        RaJoinType::Semi => "Semi".to_owned(),
        RaJoinType::Anti => "Anti".to_owned(),
    }
}

fn format_db_join_type(jt: DbJoinType) -> String {
    match jt {
        DbJoinType::Inner => "Inner".to_owned(),
        DbJoinType::Left => "Left".to_owned(),
        DbJoinType::Right => "Right".to_owned(),
        DbJoinType::Full => "Full".to_owned(),
        DbJoinType::Cross => "Cross".to_owned(),
        DbJoinType::Semi => "Semi".to_owned(),
        DbJoinType::Anti => "Anti".to_owned(),
    }
}

/// Format an EXPLAIN plan tree as a human-readable string.
pub fn format_explain_tree(plan: &ExplainPlan) -> String {
    let mut buf = String::new();
    format_explain_node(&plan.root, &mut buf, "", true);
    buf
}

fn format_explain_node(
    node: &ExplainNode,
    buf: &mut String,
    prefix: &str,
    is_last: bool,
) {
    let connector = if is_last { "└─ " } else { "├─ " };
    let child_ext = if is_last { "   " } else { "│  " };

    buf.push_str(prefix);
    buf.push_str(connector);
    buf.push_str(&node.node_type.to_string());

    if let Some(ref rel) = node.relation {
        buf.push_str(" on ");
        buf.push_str(rel);
    }
    if let Some(ref idx) = node.index_name {
        buf.push_str(" using ");
        buf.push_str(idx);
    }
    if let Some(rows) = node.estimated_rows {
        let _ = write!(buf, " (rows={rows:.0})");
    }
    if let Some(cost) = node.total_cost {
        let _ = write!(buf, " [cost={cost:.2}]");
    }
    buf.push('\n');

    if let Some(ref filter) = node.filter {
        buf.push_str(prefix);
        buf.push_str(child_ext);
        buf.push_str("  filter: ");
        buf.push_str(filter);
        buf.push('\n');
    }

    let child_prefix = format!("{prefix}{child_ext}");
    let last_idx = node.children.len().saturating_sub(1);
    for (i, child) in node.children.iter().enumerate() {
        format_explain_node(child, buf, &child_prefix, i == last_idx);
    }
}

/// Format a plan comparison as a human-readable report.
pub fn format_comparison(comparison: &PlanComparison) -> String {
    let mut buf = String::new();

    let _ = write!(
        buf,
        "Confidence: {:.0}% ({} agreements, {} disagreements)\n\n",
        comparison.confidence * 100.0,
        comparison.agreements.len(),
        comparison.disagreements.len(),
    );

    if !comparison.agreements.is_empty() {
        buf.push_str("Agreements:\n");
        for a in &comparison.agreements {
            let _ = writeln!(buf, "  [OK] {}: {}", a.aspect, a.description);
        }
        buf.push('\n');
    }

    if !comparison.disagreements.is_empty() {
        buf.push_str("Disagreements:\n");
        for d in &comparison.disagreements {
            let _ = writeln!(buf, "  [DIFF] {}:", d.aspect);
            let _ = writeln!(buf, "    RA optimizer: {}", d.ra_choice);
            let _ = writeln!(buf, "    Database:     {}", d.db_choice);
            let _ = writeln!(buf, "    Reason:       {}", d.explanation);
        }
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
    use ra_metadata::explain::{ExplainNode, ExplainPlan, NodeType};

    fn leaf_node(nt: NodeType, rel: Option<&str>) -> ExplainNode {
        ExplainNode {
            node_type: nt,
            join_type: None,
            relation: rel.map(String::from),
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

    fn wrap_plan(root: ExplainNode) -> ExplainPlan {
        ExplainPlan {
            root,
            query: None,
            total_cost: None,
            total_rows: None,
        }
    }

    fn eq_expr(col: &str, val: i64) -> Expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new(col))),
            right: Box::new(Expr::Const(Const::Int(val))),
        }
    }

    #[test]
    fn scan_agreement_seq_scan() {
        let ra = RelExpr::scan("users");
        let db = wrap_plan(leaf_node(NodeType::SeqScan, Some("users")));

        let result = compare_plans(&ra, &db);
        assert_eq!(result.agreements.len(), 1);
        assert!(result.disagreements.is_empty());
        assert!((result.confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn scan_disagreement_index_vs_seq() {
        let ra = RelExpr::scan("users");
        let mut node = leaf_node(NodeType::IndexScan, Some("users"));
        node.index_name = Some("users_pkey".to_owned());
        let db = wrap_plan(node);

        let result = compare_plans(&ra, &db);
        assert!(result.agreements.is_empty());
        assert_eq!(result.disagreements.len(), 1);
        assert!(result.disagreements[0].explanation.contains("index"));
    }

    #[test]
    fn join_algorithm_agreement() {
        let ra = RelExpr::Join {
            join_type: RaJoinType::Inner,
            condition: eq_expr("id", 1),
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };

        let db = wrap_plan(ExplainNode {
            node_type: NodeType::HashJoin,
            join_type: Some(DbJoinType::Inner),
            children: vec![
                leaf_node(NodeType::SeqScan, Some("a")),
                leaf_node(NodeType::SeqScan, Some("b")),
            ],
            ..leaf_node(NodeType::HashJoin, None)
        });

        let result = compare_plans(&ra, &db);
        let algo_agreements: Vec<_> = result
            .agreements
            .iter()
            .filter(|a| a.aspect == ComparisonAspect::JoinAlgorithm)
            .collect();
        assert_eq!(algo_agreements.len(), 1);
    }

    #[test]
    fn join_algorithm_disagreement_nested_loop() {
        let ra = RelExpr::Join {
            join_type: RaJoinType::Inner,
            condition: eq_expr("id", 1),
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };

        let db = wrap_plan(ExplainNode {
            node_type: NodeType::NestedLoop,
            join_type: Some(DbJoinType::Inner),
            children: vec![
                leaf_node(NodeType::SeqScan, Some("a")),
                leaf_node(NodeType::IndexScan, Some("b")),
            ],
            ..leaf_node(NodeType::NestedLoop, None)
        });

        let result = compare_plans(&ra, &db);
        let algo_disagreements: Vec<_> = result
            .disagreements
            .iter()
            .filter(|d| d.aspect == ComparisonAspect::JoinAlgorithm)
            .collect();
        assert_eq!(algo_disagreements.len(), 1);
        assert!(algo_disagreements[0].db_choice.contains("Nested Loop"));
    }

    #[test]
    fn sort_agreement() {
        let ra = RelExpr::Sort {
            keys: vec![],
            input: Box::new(RelExpr::scan("t")),
        };

        let db = wrap_plan(ExplainNode {
            node_type: NodeType::Sort,
            children: vec![leaf_node(NodeType::SeqScan, Some("t"))],
            ..leaf_node(NodeType::Sort, None)
        });

        let result = compare_plans(&ra, &db);
        let sort_agreements: Vec<_> = result
            .agreements
            .iter()
            .filter(|a| a.aspect == ComparisonAspect::SortStrategy)
            .collect();
        assert_eq!(sort_agreements.len(), 1);
    }

    #[test]
    fn sort_disagreement_index_order() {
        let ra = RelExpr::Sort {
            keys: vec![],
            input: Box::new(RelExpr::scan("t")),
        };

        let db = wrap_plan(leaf_node(NodeType::IndexScan, Some("t")));

        let result = compare_plans(&ra, &db);
        let sort_disagreements: Vec<_> = result
            .disagreements
            .iter()
            .filter(|d| d.aspect == ComparisonAspect::SortStrategy)
            .collect();
        assert_eq!(sort_disagreements.len(), 1);
        assert!(sort_disagreements[0]
            .explanation
            .contains("index"));
    }

    #[test]
    fn aggregation_hash_agreement() {
        let ra = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![],
            input: Box::new(RelExpr::scan("t")),
        };

        let db = wrap_plan(ExplainNode {
            node_type: NodeType::HashAggregate,
            children: vec![leaf_node(NodeType::SeqScan, Some("t"))],
            ..leaf_node(NodeType::HashAggregate, None)
        });

        let result = compare_plans(&ra, &db);
        let agg_agreements: Vec<_> = result
            .agreements
            .iter()
            .filter(|a| a.aspect == ComparisonAspect::AggregationStrategy)
            .collect();
        assert_eq!(agg_agreements.len(), 1);
    }

    #[test]
    fn aggregation_group_disagreement() {
        let ra = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![],
            input: Box::new(RelExpr::scan("t")),
        };

        let db = wrap_plan(ExplainNode {
            node_type: NodeType::GroupAggregate,
            children: vec![leaf_node(NodeType::SeqScan, Some("t"))],
            ..leaf_node(NodeType::GroupAggregate, None)
        });

        let result = compare_plans(&ra, &db);
        let agg_dis: Vec<_> = result
            .disagreements
            .iter()
            .filter(|d| d.aspect == ComparisonAspect::AggregationStrategy)
            .collect();
        assert_eq!(agg_dis.len(), 1);
    }

    #[test]
    fn filter_agreement() {
        let ra = RelExpr::scan("t").filter(eq_expr("id", 1));

        let mut scan = leaf_node(NodeType::SeqScan, Some("t"));
        scan.filter = Some("(id = 1)".to_owned());
        let db = wrap_plan(scan);

        let result = compare_plans(&ra, &db);
        let filter_agreements: Vec<_> = result
            .agreements
            .iter()
            .filter(|a| a.aspect == ComparisonAspect::FilterPlacement)
            .collect();
        assert_eq!(filter_agreements.len(), 1);
    }

    #[test]
    fn filter_pushed_into_index() {
        let ra = RelExpr::scan("t").filter(eq_expr("id", 1));

        let db = wrap_plan(leaf_node(NodeType::IndexScan, Some("t")));

        let result = compare_plans(&ra, &db);
        let filter_dis: Vec<_> = result
            .disagreements
            .iter()
            .filter(|d| d.aspect == ComparisonAspect::FilterPlacement)
            .collect();
        assert_eq!(filter_dis.len(), 1);
        assert!(filter_dis[0].explanation.contains("pushed"));
    }

    #[test]
    fn empty_comparison_full_confidence() {
        let ra = RelExpr::Values {
            rows: vec![],
        };
        let db = wrap_plan(leaf_node(NodeType::Result, None));

        let result = compare_plans(&ra, &db);
        assert!((result.confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn format_explain_tree_simple() {
        let db = wrap_plan(ExplainNode {
            node_type: NodeType::SeqScan,
            relation: Some("users".to_owned()),
            estimated_rows: Some(1000.0),
            total_cost: Some(35.5),
            ..leaf_node(NodeType::SeqScan, None)
        });

        let output = format_explain_tree(&db);
        assert!(output.contains("Seq Scan"));
        assert!(output.contains("users"));
        assert!(output.contains("rows=1000"));
        assert!(output.contains("cost=35.50"));
    }

    #[test]
    fn format_explain_tree_with_filter() {
        let mut node = leaf_node(NodeType::SeqScan, Some("t"));
        node.filter = Some("(age > 18)".to_owned());
        let db = wrap_plan(node);

        let output = format_explain_tree(&db);
        assert!(output.contains("filter: (age > 18)"));
    }

    #[test]
    fn format_explain_tree_nested() {
        let db = wrap_plan(ExplainNode {
            node_type: NodeType::HashJoin,
            children: vec![
                leaf_node(NodeType::SeqScan, Some("a")),
                leaf_node(NodeType::SeqScan, Some("b")),
            ],
            ..leaf_node(NodeType::HashJoin, None)
        });

        let output = format_explain_tree(&db);
        assert!(output.contains("Hash Join"));
        assert!(output.contains("Seq Scan on a"));
        assert!(output.contains("Seq Scan on b"));
    }

    #[test]
    fn format_comparison_report() {
        let comp = PlanComparison {
            agreements: vec![Agreement {
                aspect: ComparisonAspect::AccessMethod,
                description: "Both use Seq Scan on users".to_owned(),
            }],
            disagreements: vec![Disagreement {
                aspect: ComparisonAspect::JoinAlgorithm,
                ra_choice: "Hash Join".to_owned(),
                db_choice: "Nested Loop".to_owned(),
                explanation: "Small table".to_owned(),
            }],
            confidence: 0.5,
        };

        let report = format_comparison(&comp);
        assert!(report.contains("50%"));
        assert!(report.contains("[OK]"));
        assert!(report.contains("[DIFF]"));
        assert!(report.contains("Hash Join"));
        assert!(report.contains("Nested Loop"));
    }

    #[test]
    fn comparison_aspect_display() {
        assert_eq!(
            ComparisonAspect::AccessMethod.to_string(),
            "Access Method"
        );
        assert_eq!(
            ComparisonAspect::JoinAlgorithm.to_string(),
            "Join Algorithm"
        );
    }

    #[test]
    fn bitmap_scan_disagreement() {
        let ra = RelExpr::scan("orders");
        let db = wrap_plan(leaf_node(
            NodeType::BitmapHeapScan,
            Some("orders"),
        ));

        let result = compare_plans(&ra, &db);
        assert_eq!(result.disagreements.len(), 1);
        assert!(result.disagreements[0]
            .db_choice
            .contains("Bitmap"));
    }
}
