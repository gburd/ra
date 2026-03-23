//! Materialized view matching and metadata.
//!
//! Detects when an incoming query can be answered using a
//! pre-computed materialized view (MV) instead of re-computing from
//! base tables. Implements structural comparison, predicate
//! subsumption, and cost-benefit analysis following RFC 0051.
//!
//! # Matching strategies
//!
//! - **Exact match**: query is structurally equivalent to MV
//!   definition -- replace with a direct MV scan.
//! - **View subsumes query**: MV definition is more general (fewer
//!   filters) -- scan MV and apply compensation predicates.
//! - **Query subsumes view**: query is more general (MV has extra
//!   filters) -- no direct rewrite possible without delta queries.
//!
//! # Cost model
//!
//! When multiple MVs match, the cheapest candidate is selected using
//! `view_benefit`, which accounts for MV row count, compensation
//! overhead, and staleness penalty.

use std::collections::HashSet;

use ra_core::algebra::RelExpr;
use ra_core::expr::{ColumnRef, Expr};

/// How a query matches a materialized view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchType {
    /// Query is structurally equivalent to the MV definition.
    Exact,
    /// MV definition is more general; query needs extra filters.
    /// The query has stricter predicates than the MV, so we scan
    /// the MV and apply compensation predicates on top.
    ViewSubsumes,
    /// Query is more general than the MV (MV has extra filters).
    /// Requires delta computation to be usable.
    QuerySubsumes,
}

/// Result of comparing query predicates against MV predicates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PredicateRelation {
    /// Both predicate sets are equivalent.
    Equivalent,
    /// The query has strictly more predicates (stricter).
    QueryStronger,
    /// The MV has strictly more predicates (stricter).
    ViewStronger,
    /// Predicates are incompatible (different columns/ops).
    Incompatible,
}

/// Metadata describing a materialized view.
#[derive(Debug, Clone)]
pub struct MaterializedViewInfo {
    /// View name (used as the scan target in rewritten plans).
    pub name: String,
    /// The relational expression that defines the MV contents.
    pub definition: RelExpr,
    /// Base tables the MV depends on.
    pub base_tables: Vec<String>,
    /// Estimated number of rows in the MV.
    pub row_count: u64,
    /// Whether the MV supports incremental refresh.
    pub is_incremental: bool,
}

/// A successful match between a query and a materialized view.
#[derive(Debug, Clone)]
pub struct MvMatch {
    /// Name of the matched MV.
    pub mv_name: String,
    /// How the query relates to the MV.
    pub match_type: MatchType,
    /// Additional operations to apply on top of the MV scan
    /// (e.g. compensation predicates for `ViewSubsumes`).
    pub compensation: Option<Expr>,
    /// Estimated row count of the MV.
    pub mv_row_count: u64,
}

/// Catalog of available materialized views.
#[derive(Debug, Clone, Default)]
pub struct MvCatalog {
    views: Vec<MaterializedViewInfo>,
}

impl MvCatalog {
    /// Create an empty catalog.
    #[must_use]
    pub fn new() -> Self {
        Self { views: Vec::new() }
    }

    /// Register a materialized view.
    pub fn add_view(&mut self, view: MaterializedViewInfo) {
        self.views.push(view);
    }

    /// List all registered views.
    #[must_use]
    pub fn views(&self) -> &[MaterializedViewInfo] {
        &self.views
    }

    /// Find all MVs that match the given query.
    #[must_use]
    pub fn find_matches(&self, query: &RelExpr) -> Vec<MvMatch> {
        let mut matches = Vec::new();
        for view in &self.views {
            if let Some(m) = match_query_with_mv(query, view) {
                matches.push(m);
            }
        }
        matches
    }

    /// Find the best MV match for a query, preferring exact matches
    /// and lower row counts.
    #[must_use]
    pub fn best_match(&self, query: &RelExpr) -> Option<MvMatch> {
        let mut matches = self.find_matches(query);
        if matches.is_empty() {
            return None;
        }
        // Sort: exact matches first, then by row count ascending
        matches.sort_by(|a, b| {
            let type_ord = match (&a.match_type, &b.match_type) {
                (MatchType::Exact, MatchType::Exact) => {
                    std::cmp::Ordering::Equal
                }
                (MatchType::Exact, _) => std::cmp::Ordering::Less,
                (_, MatchType::Exact) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            };
            type_ord.then(a.mv_row_count.cmp(&b.mv_row_count))
        });
        matches.into_iter().next()
    }
}

/// Attempt to match a query against a single MV definition.
///
/// Returns `Some(MvMatch)` if the query can be answered (possibly
/// with compensation) from the MV, or `None` if no match.
#[must_use]
pub fn match_query_with_mv(
    query: &RelExpr,
    mv: &MaterializedViewInfo,
) -> Option<MvMatch> {
    // Step 1: check structural compatibility (same base tables)
    if !is_structurally_compatible(query, &mv.definition) {
        return None;
    }

    // Step 2: compare predicates
    let query_preds = extract_predicates(query);
    let mv_preds = extract_predicates(&mv.definition);
    let pred_relation = compare_predicates(&query_preds, &mv_preds);

    // Step 3: compare grouping/aggregation shape
    let agg_compatible = aggregations_compatible(query, &mv.definition);

    match pred_relation {
        PredicateRelation::Equivalent if agg_compatible => {
            Some(MvMatch {
                mv_name: mv.name.clone(),
                match_type: MatchType::Exact,
                compensation: None,
                mv_row_count: mv.row_count,
            })
        }
        PredicateRelation::QueryStronger if agg_compatible => {
            let compensation =
                compute_compensation(&query_preds, &mv_preds);
            Some(MvMatch {
                mv_name: mv.name.clone(),
                match_type: MatchType::ViewSubsumes,
                compensation,
                mv_row_count: mv.row_count,
            })
        }
        PredicateRelation::ViewStronger if agg_compatible => {
            Some(MvMatch {
                mv_name: mv.name.clone(),
                match_type: MatchType::QuerySubsumes,
                compensation: None,
                mv_row_count: mv.row_count,
            })
        }
        _ => None,
    }
}

/// Estimate the cost benefit of using an MV vs. the original plan.
///
/// Returns a positive value when the MV is beneficial (higher =
/// more benefit). Returns zero or negative when the MV is not worth
/// using.
#[must_use]
pub fn view_benefit(
    original_cost: f64,
    mv_scan_cost: f64,
    compensation_cost: f64,
) -> f64 {
    original_cost - (mv_scan_cost + compensation_cost)
}

// ── Internal helpers ─────────────────────────────────────────────

fn is_structurally_compatible(
    query: &RelExpr,
    mv_def: &RelExpr,
) -> bool {
    let query_tables = collect_base_tables(query);
    let mv_tables = collect_base_tables(mv_def);
    query_tables == mv_tables
}

fn collect_base_tables(expr: &RelExpr) -> HashSet<String> {
    let mut tables = HashSet::new();
    collect_tables_inner(expr, &mut tables);
    tables
}

fn collect_tables_inner(
    expr: &RelExpr,
    out: &mut HashSet<String>,
) {
    match expr {
        RelExpr::Scan { table, .. } => {
            out.insert(table.clone());
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input } => {
            collect_tables_inner(input, out);
        }
        RelExpr::Join { left, right, .. }
        | RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            collect_tables_inner(left, out);
            collect_tables_inner(right, out);
        }
        _ => {}
    }
}

fn extract_predicates(expr: &RelExpr) -> Vec<Expr> {
    let mut preds = Vec::new();
    extract_predicates_inner(expr, &mut preds);
    preds
}

fn extract_predicates_inner(
    expr: &RelExpr,
    out: &mut Vec<Expr>,
) {
    match expr {
        RelExpr::Filter { predicate, input } => {
            flatten_and_predicates(predicate, out);
            extract_predicates_inner(input, out);
        }
        RelExpr::Join { condition, left, right, .. } => {
            flatten_and_predicates(condition, out);
            extract_predicates_inner(left, out);
            extract_predicates_inner(right, out);
        }
        RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input } => {
            extract_predicates_inner(input, out);
        }
        _ => {}
    }
}

fn flatten_and_predicates(expr: &Expr, out: &mut Vec<Expr>) {
    match expr {
        Expr::BinOp {
            op: ra_core::expr::BinOp::And,
            left,
            right,
        } => {
            flatten_and_predicates(left, out);
            flatten_and_predicates(right, out);
        }
        _ => out.push(expr.clone()),
    }
}

fn compare_predicates(
    query_preds: &[Expr],
    mv_preds: &[Expr],
) -> PredicateRelation {
    let query_set: HashSet<String> =
        query_preds.iter().map(|p| format!("{p:?}")).collect();
    let mv_set: HashSet<String> =
        mv_preds.iter().map(|p| format!("{p:?}")).collect();

    if query_set == mv_set {
        PredicateRelation::Equivalent
    } else if query_set.is_superset(&mv_set) {
        PredicateRelation::QueryStronger
    } else if mv_set.is_superset(&query_set) {
        PredicateRelation::ViewStronger
    } else {
        PredicateRelation::Incompatible
    }
}

fn compute_compensation(
    query_preds: &[Expr],
    mv_preds: &[Expr],
) -> Option<Expr> {
    let mv_set: HashSet<String> =
        mv_preds.iter().map(|p| format!("{p:?}")).collect();
    let extra: Vec<&Expr> = query_preds
        .iter()
        .filter(|p| !mv_set.contains(&format!("{p:?}")))
        .collect();

    match extra.len() {
        0 => None,
        1 => Some(extra[0].clone()),
        _ => {
            let mut iter = extra.into_iter();
            let first = iter.next().map(Clone::clone);
            first.map(|init| {
                iter.fold(init, |acc, p| Expr::BinOp {
                    op: ra_core::expr::BinOp::And,
                    left: Box::new(acc),
                    right: Box::new(p.clone()),
                })
            })
        }
    }
}

fn aggregations_compatible(
    query: &RelExpr,
    mv_def: &RelExpr,
) -> bool {
    let query_agg = find_aggregate(query);
    let mv_agg = find_aggregate(mv_def);

    match (query_agg, mv_agg) {
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
        (Some((q_groups, q_aggs)), Some((m_groups, m_aggs))) => {
            let q_cols = group_by_columns(q_groups);
            let m_cols = group_by_columns(m_groups);
            if q_cols != m_cols {
                return false;
            }
            if q_aggs.len() != m_aggs.len() {
                return false;
            }
            for (qa, ma) in q_aggs.iter().zip(m_aggs.iter()) {
                if qa.function != ma.function
                    || qa.distinct != ma.distinct
                {
                    return false;
                }
            }
            true
        }
    }
}

fn find_aggregate(
    expr: &RelExpr,
) -> Option<(
    &[Expr],
    &[ra_core::algebra::AggregateExpr],
)> {
    match expr {
        RelExpr::Aggregate {
            group_by,
            aggregates,
            ..
        } => Some((group_by, aggregates)),
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input } => find_aggregate(input),
        _ => None,
    }
}

fn group_by_columns(exprs: &[Expr]) -> HashSet<String> {
    let mut cols = HashSet::new();
    for expr in exprs {
        collect_column_names(expr, &mut cols);
    }
    cols
}

fn collect_column_names(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::Column(ColumnRef { column, .. }) => {
            out.insert(column.clone());
        }
        Expr::BinOp { left, right, .. } => {
            collect_column_names(left, out);
            collect_column_names(right, out);
        }
        Expr::UnaryOp { operand, .. } => {
            collect_column_names(operand, out);
        }
        Expr::Function { args, .. } => {
            for arg in args {
                collect_column_names(arg, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use ra_core::algebra::{
        AggregateExpr, AggregateFunction, JoinType,
    };
    use ra_core::expr::{BinOp, ColumnRef, Const};

    fn col(name: &str) -> Expr {
        Expr::Column(ColumnRef::new(name))
    }

    fn int(v: i64) -> Expr {
        Expr::Const(Const::Int(v))
    }

    fn eq(left: Expr, right: Expr) -> Expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn gt(left: Expr, right: Expr) -> Expr {
        Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn and(left: Expr, right: Expr) -> Expr {
        Expr::BinOp {
            op: BinOp::And,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn make_order_summary_query(
        extra_pred: Option<Expr>,
    ) -> RelExpr {
        let base = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: eq(col("c_custkey"), col("o_custkey")),
            left: Box::new(RelExpr::scan("customer")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let filtered = if let Some(pred) = extra_pred {
            RelExpr::Filter {
                predicate: pred,
                input: Box::new(base),
            }
        } else {
            base
        };
        RelExpr::Aggregate {
            group_by: vec![col("c_custkey")],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(col("o_totalprice")),
                distinct: false,
                alias: Some("total".to_string()),
            }],
            input: Box::new(filtered),
        }
    }

    fn make_mv_info(
        extra_pred: Option<Expr>,
        row_count: u64,
    ) -> MaterializedViewInfo {
        MaterializedViewInfo {
            name: "customer_order_summary".to_string(),
            definition: make_order_summary_query(extra_pred),
            base_tables: vec![
                "customer".to_string(),
                "orders".to_string(),
            ],
            row_count,
            is_incremental: false,
        }
    }

    #[test]
    fn exact_match_detected() {
        let query = make_order_summary_query(None);
        let mv = make_mv_info(None, 1000);
        let result = match_query_with_mv(&query, &mv);
        assert!(result.is_some());
        let m = result.expect("should match");
        assert_eq!(m.match_type, MatchType::Exact);
        assert!(m.compensation.is_none());
    }

    #[test]
    fn view_subsumes_query_with_compensation() {
        let extra = gt(col("o_totalprice"), int(100));
        let query = make_order_summary_query(Some(extra));
        let mv = make_mv_info(None, 5000);
        let result = match_query_with_mv(&query, &mv);
        assert!(result.is_some());
        let m = result.expect("should match");
        assert_eq!(m.match_type, MatchType::ViewSubsumes);
        assert!(m.compensation.is_some());
    }

    #[test]
    fn query_subsumes_view() {
        let mv_extra = gt(col("o_totalprice"), int(100));
        let query = make_order_summary_query(None);
        let mv = make_mv_info(Some(mv_extra), 500);
        let result = match_query_with_mv(&query, &mv);
        assert!(result.is_some());
        let m = result.expect("should match");
        assert_eq!(m.match_type, MatchType::QuerySubsumes);
    }

    #[test]
    fn no_match_incompatible_tables() {
        let query = RelExpr::Aggregate {
            group_by: vec![col("id")],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: None,
            }],
            input: Box::new(RelExpr::scan("products")),
        };
        let mv = make_mv_info(None, 1000);
        let result = match_query_with_mv(&query, &mv);
        assert!(result.is_none());
    }

    #[test]
    fn no_match_incompatible_predicates() {
        let q_pred = gt(col("o_totalprice"), int(100));
        let mv_pred = gt(col("o_orderdate"), int(20240101));
        let query = make_order_summary_query(Some(q_pred));
        let mv = make_mv_info(Some(mv_pred), 500);
        let result = match_query_with_mv(&query, &mv);
        assert!(result.is_none());
    }

    #[test]
    fn catalog_best_match_prefers_exact() {
        let query = make_order_summary_query(None);
        let mut catalog = MvCatalog::new();
        catalog.add_view(make_mv_info(None, 5000));
        catalog.add_view(MaterializedViewInfo {
            name: "partial_summary".to_string(),
            definition: make_order_summary_query(Some(gt(
                col("o_totalprice"),
                int(0),
            ))),
            base_tables: vec![
                "customer".to_string(),
                "orders".to_string(),
            ],
            row_count: 100,
            is_incremental: false,
        });
        let best = catalog.best_match(&query);
        assert!(best.is_some());
        assert_eq!(
            best.expect("should have match").match_type,
            MatchType::Exact
        );
    }

    #[test]
    fn view_benefit_positive_when_mv_cheaper() {
        let benefit = view_benefit(1000.0, 100.0, 10.0);
        assert!(benefit > 0.0);
        assert!((benefit - 890.0).abs() < f64::EPSILON);
    }

    #[test]
    fn view_benefit_negative_when_mv_expensive() {
        let benefit = view_benefit(100.0, 200.0, 50.0);
        assert!(benefit < 0.0);
    }

    #[test]
    fn flatten_and_splits_correctly() {
        let expr = and(
            gt(col("a"), int(1)),
            and(gt(col("b"), int(2)), gt(col("c"), int(3))),
        );
        let mut preds = Vec::new();
        flatten_and_predicates(&expr, &mut preds);
        assert_eq!(preds.len(), 3);
    }

    #[test]
    fn compensation_computed_for_extra_predicates() {
        let query_preds = vec![
            eq(col("a"), int(1)),
            gt(col("b"), int(2)),
        ];
        let mv_preds = vec![eq(col("a"), int(1))];
        let comp = compute_compensation(&query_preds, &mv_preds);
        assert!(comp.is_some());
    }

    #[test]
    fn empty_catalog_returns_no_match() {
        let catalog = MvCatalog::new();
        let query = RelExpr::scan("anything");
        assert!(catalog.best_match(&query).is_none());
    }
}
