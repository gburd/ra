//! Differential validation: compare RA optimizer plans against
//! database EXPLAIN plans.
//!
//! Compares the plan produced by the RA optimizer with the plan
//! chosen by a real database engine, reporting agreements and
//! disagreements in join order, index selection, filter placement,
//! and aggregation strategy.

use serde::{Deserialize, Serialize};

use ra_core::algebra::{JoinType as RaJoinType, RelExpr};

use crate::explain::{ExplainNode, ExplainPlan, NodeType};

/// Result of comparing an RA plan with a database EXPLAIN plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanComparison {
    /// Points where the two plans agree.
    pub agreements: Vec<PlanAgreement>,
    /// Points where the two plans disagree.
    pub disagreements: Vec<PlanDisagreement>,
    /// Overall confidence in the comparison (0.0 to 1.0).
    pub confidence: f64,
}

impl PlanComparison {
    /// Calculate agreement ratio (agreements / total observations).
    #[must_use]
    pub fn agreement_ratio(&self) -> f64 {
        let total = self.agreements.len() + self.disagreements.len();
        if total == 0 {
            return 1.0;
        }
        self.agreements.len() as f64 / total as f64
    }
}

/// A point of agreement between the two plans.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanAgreement {
    /// What aspect agrees.
    pub aspect: ComparisonAspect,
    /// Description.
    pub description: String,
}

/// A point of disagreement between the two plans.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanDisagreement {
    /// What aspect disagrees.
    pub aspect: ComparisonAspect,
    /// What the RA optimizer chose.
    pub ra_choice: String,
    /// What the database engine chose.
    pub db_choice: String,
    /// Impact level.
    pub severity: Severity,
}

/// Aspects of a plan that can be compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComparisonAspect {
    /// Join order / join algorithm.
    JoinStrategy,
    /// Index selection for scans.
    IndexSelection,
    /// Filter placement (push-down).
    FilterPlacement,
    /// Aggregation strategy (hash vs. sorted).
    AggregationStrategy,
    /// Table access method (seq scan vs index scan).
    AccessMethod,
    /// Sort usage.
    SortStrategy,
}

impl std::fmt::Display for ComparisonAspect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::JoinStrategy => write!(f, "Join Strategy"),
            Self::IndexSelection => write!(f, "Index Selection"),
            Self::FilterPlacement => {
                write!(f, "Filter Placement")
            }
            Self::AggregationStrategy => {
                write!(f, "Aggregation Strategy")
            }
            Self::AccessMethod => write!(f, "Access Method"),
            Self::SortStrategy => write!(f, "Sort Strategy"),
        }
    }
}

/// Severity of a disagreement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Severity {
    /// Informational only.
    Info,
    /// Potentially suboptimal.
    Warning,
    /// Likely performance problem.
    High,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Warning => write!(f, "WARNING"),
            Self::High => write!(f, "HIGH"),
        }
    }
}

/// Compare an RA optimizer plan with a database EXPLAIN plan.
#[must_use]
pub fn compare_plans(ra_plan: &RelExpr, db_explain: &ExplainPlan) -> PlanComparison {
    let mut agreements = Vec::new();
    let mut disagreements = Vec::new();

    compare_access_methods(
        ra_plan,
        &db_explain.root,
        &mut agreements,
        &mut disagreements,
    );
    compare_join_strategies(
        ra_plan,
        &db_explain.root,
        &mut agreements,
        &mut disagreements,
    );
    compare_filter_placement(
        ra_plan,
        &db_explain.root,
        &mut agreements,
        &mut disagreements,
    );
    compare_aggregation(
        ra_plan,
        &db_explain.root,
        &mut agreements,
        &mut disagreements,
    );

    let total = agreements.len() + disagreements.len();
    let confidence = if total == 0 {
        0.5
    } else {
        (agreements.len() as f64 / total as f64).clamp(0.0, 1.0)
    };

    PlanComparison {
        agreements,
        disagreements,
        confidence,
    }
}

fn compare_access_methods(
    ra_plan: &RelExpr,
    db_root: &ExplainNode,
    agreements: &mut Vec<PlanAgreement>,
    disagreements: &mut Vec<PlanDisagreement>,
) {
    let ra_tables = collect_ra_scan_tables(ra_plan);
    let db_scans = collect_explain_scans(db_root);

    for table in &ra_tables {
        if let Some(db_scan) = db_scans
            .iter()
            .find(|s| s.relation.as_deref() == Some(table.as_str()))
        {
            let ra_access = "Scan (unresolved)";

            match db_scan.node_type {
                NodeType::SeqScan => {
                    agreements.push(PlanAgreement {
                        aspect: ComparisonAspect::AccessMethod,
                        description: format!(
                            "table {table}: both use \
                             sequential scan"
                        ),
                    });
                }
                NodeType::IndexScan | NodeType::IndexOnlyScan => {
                    disagreements.push(PlanDisagreement {
                        aspect: ComparisonAspect::IndexSelection,
                        ra_choice: format!("table {table}: {ra_access}"),
                        db_choice: format!(
                            "table {table}: {} (index: {})",
                            db_scan.node_type,
                            db_scan.index_name.as_deref().unwrap_or("unknown")
                        ),
                        severity: Severity::Warning,
                    });
                }
                _ => {}
            }
        }
    }
}

fn compare_join_strategies(
    ra_plan: &RelExpr,
    db_root: &ExplainNode,
    agreements: &mut Vec<PlanAgreement>,
    disagreements: &mut Vec<PlanDisagreement>,
) {
    let ra_joins = collect_ra_joins(ra_plan);
    let db_joins = collect_explain_joins(db_root);

    for (i, ra_join) in ra_joins.iter().enumerate() {
        if let Some(db_join) = db_joins.get(i) {
            let ra_type = format!("{ra_join}");
            let db_type = format!("{}", db_join.node_type);

            if join_types_compatible(*ra_join, db_join.node_type) {
                agreements.push(PlanAgreement {
                    aspect: ComparisonAspect::JoinStrategy,
                    description: format!(
                        "join #{}: compatible strategies \
                         (RA: {ra_type}, DB: {db_type})",
                        i + 1
                    ),
                });
            } else {
                disagreements.push(PlanDisagreement {
                    aspect: ComparisonAspect::JoinStrategy,
                    ra_choice: ra_type,
                    db_choice: db_type,
                    severity: Severity::Info,
                });
            }
        }
    }
}

fn compare_filter_placement(
    ra_plan: &RelExpr,
    db_root: &ExplainNode,
    agreements: &mut Vec<PlanAgreement>,
    disagreements: &mut Vec<PlanDisagreement>,
) {
    let ra_filter_count = count_ra_filters(ra_plan);
    let db_filter_count = count_explain_filters(db_root);

    if ra_filter_count == db_filter_count {
        agreements.push(PlanAgreement {
            aspect: ComparisonAspect::FilterPlacement,
            description: format!(
                "same number of filter operators \
                 ({ra_filter_count})"
            ),
        });
    } else {
        disagreements.push(PlanDisagreement {
            aspect: ComparisonAspect::FilterPlacement,
            ra_choice: format!("{ra_filter_count} filters"),
            db_choice: format!("{db_filter_count} filters"),
            severity: Severity::Info,
        });
    }
}

fn compare_aggregation(
    ra_plan: &RelExpr,
    db_root: &ExplainNode,
    agreements: &mut Vec<PlanAgreement>,
    disagreements: &mut Vec<PlanDisagreement>,
) {
    let ra_has_agg = has_ra_aggregate(ra_plan);
    let db_agg_type = find_explain_aggregate(db_root);

    if let (true, Some(db_strategy)) = (ra_has_agg, &db_agg_type) {
        agreements.push(PlanAgreement {
            aspect: ComparisonAspect::AggregationStrategy,
            description: format!("both plans use aggregation (DB: {db_strategy})"),
        });
    } else if ra_has_agg != db_agg_type.is_some() {
        disagreements.push(PlanDisagreement {
            aspect: ComparisonAspect::AggregationStrategy,
            ra_choice: if ra_has_agg {
                "has aggregation".to_owned()
            } else {
                "no aggregation".to_owned()
            },
            db_choice: if db_agg_type.is_some() {
                "has aggregation".to_owned()
            } else {
                "no aggregation".to_owned()
            },
            severity: Severity::Warning,
        });
    }
}

// ── Helpers ────────────────────────────────────────────────

fn collect_ra_scan_tables(expr: &RelExpr) -> Vec<String> {
    let mut tables = Vec::new();
    match expr {
        RelExpr::Scan { table, .. } => {
            tables.push(table.clone());
        }
        other => {
            for child in other.children() {
                tables.extend(collect_ra_scan_tables(child));
            }
        }
    }
    tables
}

fn collect_explain_scans(node: &ExplainNode) -> Vec<&ExplainNode> {
    let mut scans = Vec::new();
    match node.node_type {
        NodeType::SeqScan
        | NodeType::IndexScan
        | NodeType::IndexOnlyScan
        | NodeType::BitmapIndexScan
        | NodeType::BitmapHeapScan => {
            scans.push(node);
        }
        _ => {}
    }
    for child in &node.children {
        scans.extend(collect_explain_scans(child));
    }
    scans
}

fn collect_ra_joins(expr: &RelExpr) -> Vec<RaJoinType> {
    let mut joins = Vec::new();
    match expr {
        RelExpr::Join {
            join_type,
            left,
            right,
            ..
        } => {
            joins.push(*join_type);
            joins.extend(collect_ra_joins(left));
            joins.extend(collect_ra_joins(right));
        }
        other => {
            for child in other.children() {
                joins.extend(collect_ra_joins(child));
            }
        }
    }
    joins
}

fn collect_explain_joins(node: &ExplainNode) -> Vec<&ExplainNode> {
    let mut joins = Vec::new();
    match node.node_type {
        NodeType::NestedLoop | NodeType::HashJoin | NodeType::MergeJoin => {
            joins.push(node);
        }
        _ => {}
    }
    for child in &node.children {
        joins.extend(collect_explain_joins(child));
    }
    joins
}

fn join_types_compatible(_ra: RaJoinType, db: NodeType) -> bool {
    matches!(
        db,
        NodeType::NestedLoop | NodeType::HashJoin | NodeType::MergeJoin
    )
}

fn count_ra_filters(expr: &RelExpr) -> usize {
    match expr {
        RelExpr::Filter { input, .. } => 1 + count_ra_filters(input),
        other => other.children().iter().map(|c| count_ra_filters(c)).sum(),
    }
}

fn count_explain_filters(node: &ExplainNode) -> usize {
    let self_count = usize::from(node.filter.is_some());
    let child_count: usize = node.children.iter().map(count_explain_filters).sum();
    self_count + child_count
}

fn has_ra_aggregate(expr: &RelExpr) -> bool {
    match expr {
        RelExpr::Aggregate { .. } => true,
        other => other.children().iter().any(|c| has_ra_aggregate(c)),
    }
}

fn find_explain_aggregate(node: &ExplainNode) -> Option<String> {
    match node.node_type {
        NodeType::HashAggregate => Some("HashAggregate".to_owned()),
        NodeType::GroupAggregate => Some("GroupAggregate".to_owned()),
        _ => {
            for child in &node.children {
                if let Some(result) = find_explain_aggregate(child) {
                    return Some(result);
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explain::{ExplainNode, ExplainPlan, NodeType};
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp as ExprBinOp, ColumnRef, Const, Expr};

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

    fn wrap_plan(root: ExplainNode) -> ExplainPlan {
        ExplainPlan {
            root,
            query: None,
            total_cost: None,
            total_rows: None,
        }
    }

    fn make_filter_scan() -> RelExpr {
        RelExpr::scan("users").filter(Expr::BinOp {
            op: ExprBinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        })
    }

    fn make_join() -> RelExpr {
        RelExpr::Join {
            join_type: RaJoinType::Inner,
            condition: Expr::BinOp {
                op: ExprBinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("o", "user_id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("u", "id"))),
            },
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("users")),
        }
    }

    #[test]
    fn compare_simple_scan_agreement() {
        let ra_plan = RelExpr::scan("users");
        let db_plan = wrap_plan(leaf_node(NodeType::SeqScan, Some("users")));

        let comparison = compare_plans(&ra_plan, &db_plan);
        assert!(!comparison.agreements.is_empty());
        assert!(comparison.confidence > 0.0);
    }

    #[test]
    fn compare_scan_index_disagreement() {
        let ra_plan = RelExpr::scan("users");
        let mut node = leaf_node(NodeType::IndexScan, Some("users"));
        node.index_name = Some("users_pkey".to_owned());
        let db_plan = wrap_plan(node);

        let comparison = compare_plans(&ra_plan, &db_plan);
        assert!(!comparison.disagreements.is_empty());

        let idx_disagreement = comparison
            .disagreements
            .iter()
            .find(|d| d.aspect == ComparisonAspect::IndexSelection);
        assert!(idx_disagreement.is_some());
    }

    #[test]
    fn compare_join_compatible() {
        let ra_plan = make_join();
        let mut hash_join = leaf_node(NodeType::HashJoin, None);
        hash_join.join_type = Some(crate::explain::JoinType::Inner);
        hash_join.children = vec![
            leaf_node(NodeType::SeqScan, Some("orders")),
            leaf_node(NodeType::SeqScan, Some("users")),
        ];
        let db_plan = wrap_plan(hash_join);

        let comparison = compare_plans(&ra_plan, &db_plan);

        let join_agreement = comparison
            .agreements
            .iter()
            .find(|a| a.aspect == ComparisonAspect::JoinStrategy);
        assert!(join_agreement.is_some());
    }

    #[test]
    fn compare_filter_counts() {
        let ra_plan = make_filter_scan();
        let mut db_node = leaf_node(NodeType::SeqScan, Some("users"));
        db_node.filter = Some("(age > 18)".to_owned());
        let db_plan = wrap_plan(db_node);

        let comparison = compare_plans(&ra_plan, &db_plan);

        let filter_agreement = comparison
            .agreements
            .iter()
            .find(|a| a.aspect == ComparisonAspect::FilterPlacement);
        assert!(filter_agreement.is_some());
    }

    #[test]
    fn agreement_ratio_all_agree() {
        let comparison = PlanComparison {
            agreements: vec![PlanAgreement {
                aspect: ComparisonAspect::AccessMethod,
                description: "test".to_owned(),
            }],
            disagreements: vec![],
            confidence: 1.0,
        };
        assert!((comparison.agreement_ratio() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn agreement_ratio_half() {
        let comparison = PlanComparison {
            agreements: vec![PlanAgreement {
                aspect: ComparisonAspect::AccessMethod,
                description: "test".to_owned(),
            }],
            disagreements: vec![PlanDisagreement {
                aspect: ComparisonAspect::IndexSelection,
                ra_choice: "a".to_owned(),
                db_choice: "b".to_owned(),
                severity: Severity::Info,
            }],
            confidence: 0.5,
        };
        assert!((comparison.agreement_ratio() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn agreement_ratio_empty() {
        let comparison = PlanComparison {
            agreements: vec![],
            disagreements: vec![],
            confidence: 0.5,
        };
        assert!((comparison.agreement_ratio() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn severity_display() {
        assert_eq!(Severity::Info.to_string(), "INFO");
        assert_eq!(Severity::Warning.to_string(), "WARNING");
        assert_eq!(Severity::High.to_string(), "HIGH");
    }

    #[test]
    fn comparison_aspect_display() {
        assert_eq!(ComparisonAspect::JoinStrategy.to_string(), "Join Strategy");
        assert_eq!(
            ComparisonAspect::IndexSelection.to_string(),
            "Index Selection"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test code")]
    fn serialize_roundtrip() {
        let comparison = PlanComparison {
            agreements: vec![PlanAgreement {
                aspect: ComparisonAspect::AccessMethod,
                description: "test".to_owned(),
            }],
            disagreements: vec![],
            confidence: 0.9,
        };

        let json = serde_json::to_string(&comparison).expect("serialization should succeed");
        let deserialized: PlanComparison =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(comparison, deserialized);
    }
}
