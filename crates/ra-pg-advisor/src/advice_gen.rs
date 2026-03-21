//! Convert RA optimizer output to `pg_plan_advice` format.
//!
//! Walks an optimized `RelExpr` tree and extracts join order,
//! join methods, and scan strategies, emitting them as advice
//! strings compatible with the `pg_plan_advice` mini-language.

use std::fmt;

use ra_core::{JoinType, RelExpr};
use serde::{Deserialize, Serialize};

/// Category of optimization advice.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum AdviceType {
    /// Suggest creating or using a specific index.
    IndexSuggestion,
    /// Reorder joins for better performance.
    JoinReordering,
    /// Prefer a specific join algorithm.
    JoinMethod,
    /// Prefer a specific scan strategy.
    ScanMethod,
    /// Control parallel execution workers.
    ParallelExecution,
    /// Suppress Gather/Gather Merge nodes.
    NoGather,
}

impl fmt::Display for AdviceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IndexSuggestion => write!(f, "INDEX_SCAN"),
            Self::JoinReordering => write!(f, "JOIN_ORDER"),
            Self::JoinMethod => write!(f, "JOIN_METHOD"),
            Self::ScanMethod => write!(f, "SCAN_METHOD"),
            Self::ParallelExecution => write!(f, "PARALLEL"),
            Self::NoGather => write!(f, "NO_GATHER"),
        }
    }
}

/// A single piece of plan advice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanAdvice {
    /// Which query this advice applies to.
    pub query_id: String,
    /// Category of advice.
    pub advice_type: AdviceType,
    /// Formatted advice string in `pg_plan_advice` syntax.
    pub advice_string: String,
    /// Estimated cost improvement ratio (1.0 = no change, >1.0 = faster).
    pub estimated_improvement: f64,
    /// Confidence in the advice (0.0 to 1.0).
    pub confidence: f64,
}

impl fmt::Display for PlanAdvice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.advice_string)
    }
}

/// Join method extracted from an optimized plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinHint {
    /// The relation on the inner side of the join.
    pub relation: String,
    /// The join algorithm to use.
    pub method: JoinMethodHint,
}

/// Supported join algorithm hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JoinMethodHint {
    /// Hash join.
    Hash,
    /// Merge join (sort-merge).
    Merge,
    /// Nested loop join.
    NestedLoop,
}

impl fmt::Display for JoinMethodHint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hash => write!(f, "HASH_JOIN"),
            Self::Merge => write!(f, "MERGE_JOIN"),
            Self::NestedLoop => write!(f, "NESTED_LOOP"),
        }
    }
}

/// Scan method extracted from an optimized plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanHint {
    /// The relation to scan.
    pub relation: String,
    /// The scan strategy.
    pub method: ScanMethodHint,
}

/// Supported scan strategy hints.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ScanMethodHint {
    /// Sequential (full table) scan.
    Sequential,
    /// Index scan with a named index.
    Index(String),
}

impl fmt::Display for ScanMethodHint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sequential => write!(f, "SEQ_SCAN"),
            Self::Index(idx) => write!(f, "INDEX_SCAN({idx})"),
        }
    }
}

/// Generates advice from optimized `RelExpr` trees.
pub struct AdviceGenerator {
    /// Default confidence for generated advice.
    default_confidence: f64,
}

impl AdviceGenerator {
    /// Create a generator with the given default confidence.
    #[must_use]
    pub fn new(default_confidence: f64) -> Self {
        Self {
            default_confidence: default_confidence.clamp(0.0, 1.0),
        }
    }

    /// Generate all advice from an optimized plan.
    ///
    /// Returns a list of `PlanAdvice` items covering join order,
    /// join methods, and scan strategies extracted from the tree.
    #[must_use]
    pub fn generate(
        &self,
        query_id: &str,
        optimized: &RelExpr,
    ) -> Vec<PlanAdvice> {
        let mut advice = Vec::new();

        let join_order = extract_join_order(optimized);
        if join_order.len() >= 2 {
            let order_str = format!(
                "JOIN_ORDER({})",
                join_order.join(" ")
            );
            advice.push(PlanAdvice {
                query_id: query_id.to_string(),
                advice_type: AdviceType::JoinReordering,
                advice_string: order_str,
                estimated_improvement: 1.0,
                confidence: self.default_confidence,
            });
        }

        let joins = extract_join_hints(optimized);
        for hint in &joins {
            let advice_str =
                format!("{}({})", hint.method, hint.relation);
            advice.push(PlanAdvice {
                query_id: query_id.to_string(),
                advice_type: AdviceType::JoinMethod,
                advice_string: advice_str,
                estimated_improvement: 1.0,
                confidence: self.default_confidence,
            });
        }

        let scans = extract_scan_hints(optimized);
        for hint in &scans {
            let advice_str = match &hint.method {
                ScanMethodHint::Sequential => {
                    format!("SEQ_SCAN({})", hint.relation)
                }
                ScanMethodHint::Index(idx) => {
                    format!(
                        "INDEX_SCAN({} {})",
                        hint.relation, idx
                    )
                }
            };
            advice.push(PlanAdvice {
                query_id: query_id.to_string(),
                advice_type: AdviceType::ScanMethod,
                advice_string: advice_str,
                estimated_improvement: 1.0,
                confidence: self.default_confidence,
            });
        }

        advice
    }

    /// Combine multiple advice items into a single advice string.
    #[must_use]
    pub fn combine_advice(items: &[PlanAdvice]) -> String {
        items
            .iter()
            .map(|a| a.advice_string.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Convert advice to `pg_hint_plan` format for PG 15-18 fallback.
    #[must_use]
    pub fn to_pg_hint_plan(items: &[PlanAdvice]) -> String {
        let mut hints = Vec::new();
        for item in items {
            let hint = convert_to_pg_hint_plan(item);
            if !hint.is_empty() {
                hints.push(hint);
            }
        }
        if hints.is_empty() {
            return String::new();
        }
        format!("/*+ {} */", hints.join(" "))
    }
}

/// Extract leaf table names in join-order (left-to-right DFS).
#[must_use]
pub fn extract_join_order(expr: &RelExpr) -> Vec<String> {
    let mut tables = Vec::new();
    collect_join_order(expr, &mut tables);
    tables
}

fn collect_join_order(expr: &RelExpr, out: &mut Vec<String>) {
    match expr {
        RelExpr::Scan { table, alias, .. } => {
            out.push(
                alias.as_deref().unwrap_or(table).to_string()
            );
        }
        RelExpr::IndexScan { table, .. } | RelExpr::IndexOnlyScan { table, .. } => {
            out.push(table.clone());
        }
        RelExpr::Join { left, right, .. }
        | RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            collect_join_order(left, out);
            collect_join_order(right, out);
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::IncrementalSort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::RowPattern { input, .. } => {
            collect_join_order(input, out);
        }
        RelExpr::CTE { body, .. }
        | RelExpr::RecursiveCTE { body, .. } => {
            collect_join_order(body, out);
        }
        RelExpr::Unnest { input, .. }
        | RelExpr::TableFunction { input, .. } => {
            if let Some(inp) = input {
                collect_join_order(inp, out);
            }
        }
        RelExpr::IndexScan { .. }
        | RelExpr::IndexOnlyScan { .. }
        | RelExpr::BitmapIndexScan { .. }
        | RelExpr::BitmapHeapScan { .. }
        | RelExpr::ParallelScan { .. } => {}
        RelExpr::BitmapAnd { inputs }
        | RelExpr::BitmapOr { inputs } => {
            for inp in inputs {
                collect_join_order(inp, out);
            }
        }
        RelExpr::ParallelHashJoin {
            left, right, ..
        } => {
            collect_join_order(left, out);
            collect_join_order(right, out);
        }
        RelExpr::ParallelAggregate { input, .. }
        | RelExpr::Gather { input, .. } => {
            collect_join_order(input, out);
        }
        RelExpr::Values { .. } | RelExpr::MultiUnnest { .. } => {}
    }
}

/// Extract join method hints from the optimized tree.
#[must_use]
pub fn extract_join_hints(expr: &RelExpr) -> Vec<JoinHint> {
    let mut hints = Vec::new();
    collect_join_hints(expr, &mut hints);
    hints
}

fn collect_join_hints(expr: &RelExpr, out: &mut Vec<JoinHint>) {
    match expr {
        RelExpr::Join {
            join_type,
            left,
            right,
            ..
        } => {
            let right_name = first_table_name(right);
            if let Some(rel) = right_name {
                if let Some(m) = join_type_to_method(*join_type) {
                    out.push(JoinHint {
                        relation: rel,
                        method: m,
                    });
                }
            }
            collect_join_hints(left, out);
            collect_join_hints(right, out);
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::IncrementalSort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::RowPattern { input, .. } => {
            collect_join_hints(input, out);
        }
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            collect_join_hints(left, out);
            collect_join_hints(right, out);
        }
        RelExpr::CTE { body, .. }
        | RelExpr::RecursiveCTE { body, .. } => {
            collect_join_hints(body, out);
        }
        RelExpr::Unnest { input, .. }
        | RelExpr::TableFunction { input, .. } => {
            if let Some(inp) = input {
                collect_join_hints(inp, out);
            }
        }
        RelExpr::IndexScan { .. }
        | RelExpr::IndexOnlyScan { .. }
        | RelExpr::BitmapIndexScan { .. }
        | RelExpr::BitmapHeapScan { .. }
        | RelExpr::ParallelScan { .. } => {}
        RelExpr::BitmapAnd { inputs }
        | RelExpr::BitmapOr { inputs } => {
            for inp in inputs {
                collect_join_hints(inp, out);
            }
        }
        RelExpr::ParallelHashJoin {
            join_type,
            left,
            right,
            ..
        } => {
            let right_name = first_table_name(right);
            if let Some(rel) = right_name {
                if let Some(m) = join_type_to_method(*join_type) {
                    out.push(JoinHint {
                        relation: rel,
                        method: m,
                    });
                }
            }
            collect_join_hints(left, out);
            collect_join_hints(right, out);
        }
        RelExpr::ParallelAggregate { input, .. }
        | RelExpr::Gather { input, .. } => {
            collect_join_hints(input, out);
        }
        RelExpr::Scan { .. }
        | RelExpr::Values { .. }
        | RelExpr::MultiUnnest { .. } => {}
    }
}

/// Extract scan method hints from the tree.
///
/// Currently emits `SEQ_SCAN` for all leaf scans. Index hints
/// require cost model integration to determine when an index
/// scan is preferred.
#[must_use]
pub fn extract_scan_hints(expr: &RelExpr) -> Vec<ScanHint> {
    let mut hints = Vec::new();
    collect_scan_hints(expr, &mut hints);
    hints
}

fn collect_scan_hints(expr: &RelExpr, out: &mut Vec<ScanHint>) {
    match expr {
        RelExpr::Scan { table, alias, .. } => {
            let name =
                alias.as_deref().unwrap_or(table).to_string();
            out.push(ScanHint {
                relation: name,
                method: ScanMethodHint::Sequential,
            });
        }
        RelExpr::Join { left, right, .. }
        | RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            collect_scan_hints(left, out);
            collect_scan_hints(right, out);
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::IncrementalSort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::RowPattern { input, .. } => {
            collect_scan_hints(input, out);
        }
        RelExpr::CTE { body, .. }
        | RelExpr::RecursiveCTE { body, .. } => {
            collect_scan_hints(body, out);
        }
        RelExpr::Unnest { input, .. }
        | RelExpr::TableFunction { input, .. } => {
            if let Some(inp) = input {
                collect_scan_hints(inp, out);
            }
        }
        RelExpr::BitmapIndexScan { table, .. } => {
            out.push(ScanHint {
                relation: table.clone(),
                method: ScanMethodHint::Index(String::new()),
            });
        }
        RelExpr::BitmapHeapScan { table, bitmap, .. } => {
            out.push(ScanHint {
                relation: table.clone(),
                method: ScanMethodHint::Index(String::new()),
            });
            collect_scan_hints(bitmap, out);
        }
        RelExpr::BitmapAnd { inputs }
        | RelExpr::BitmapOr { inputs } => {
            for inp in inputs {
                collect_scan_hints(inp, out);
            }
        }
        RelExpr::ParallelScan { table, .. } => {
            out.push(ScanHint {
                relation: table.clone(),
                method: ScanMethodHint::Sequential,
            });
        }
        RelExpr::ParallelHashJoin {
            left, right, ..
        } => {
            collect_scan_hints(left, out);
            collect_scan_hints(right, out);
        }
        RelExpr::ParallelAggregate { input, .. }
        | RelExpr::Gather { input, .. } => {
            collect_scan_hints(input, out);
        }
        RelExpr::Values { .. }
        | RelExpr::MultiUnnest { .. } => {}
    }
}

/// Get the first table name from a subtree (for labeling joins).
fn first_table_name(expr: &RelExpr) -> Option<String> {
    match expr {
        RelExpr::Scan { table, alias, .. } => {
            Some(
                alias.as_deref().unwrap_or(table).to_string()
            )
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Distinct { input, .. } => {
            first_table_name(input)
        }
        RelExpr::Join { left, .. } => first_table_name(left),
        _ => None,
    }
}

/// Map RA `JoinType` to a hint method. Returns `None` for join
/// types where no specific algorithm preference applies (cross).
fn join_type_to_method(jt: JoinType) -> Option<JoinMethodHint> {
    match jt {
        JoinType::Inner
        | JoinType::LeftOuter
        | JoinType::RightOuter
        | JoinType::FullOuter => Some(JoinMethodHint::Hash),
        JoinType::Cross
        | JoinType::Semi
        | JoinType::Anti => None,
    }
}

/// Convert a single advice item to `pg_hint_plan` comment syntax.
fn convert_to_pg_hint_plan(advice: &PlanAdvice) -> String {
    let s = &advice.advice_string;

    if let Some(inner) = s.strip_prefix("JOIN_ORDER(")
        .and_then(|r| r.strip_suffix(')'))
    {
        return format!("Leading({inner})");
    }
    if let Some(inner) = s.strip_prefix("HASH_JOIN(")
        .and_then(|r| r.strip_suffix(')'))
    {
        return format!("HashJoin({inner})");
    }
    if let Some(inner) = s.strip_prefix("MERGE_JOIN(")
        .and_then(|r| r.strip_suffix(')'))
    {
        return format!("MergeJoin({inner})");
    }
    if let Some(inner) = s.strip_prefix("NESTED_LOOP(")
        .and_then(|r| r.strip_suffix(')'))
    {
        return format!("NestLoop({inner})");
    }
    if let Some(inner) = s.strip_prefix("SEQ_SCAN(")
        .and_then(|r| r.strip_suffix(')'))
    {
        return format!("SeqScan({inner})");
    }
    if let Some(inner) = s.strip_prefix("INDEX_SCAN(")
        .and_then(|r| r.strip_suffix(')'))
    {
        return format!("IndexScan({inner})");
    }
    if s.starts_with("NO_GATHER(") {
        return String::new();
    }
    if let Some(inner) = s.strip_prefix("PARALLEL(")
        .and_then(|r| r.strip_suffix(')'))
    {
        return format!("Parallel({inner})");
    }

    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::{Const, Expr};

    fn simple_scan(name: &str) -> RelExpr {
        RelExpr::Scan {
            table: name.to_string(),
            alias: None,
        }
    }

    fn true_expr() -> Expr {
        Expr::Const(Const::Bool(true))
    }

    fn two_way_join() -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: true_expr(),
            left: Box::new(simple_scan("orders")),
            right: Box::new(simple_scan("customers")),
        }
    }

    fn three_way_join() -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: true_expr(),
            left: Box::new(two_way_join()),
            right: Box::new(simple_scan("products")),
        }
    }

    #[test]
    fn join_order_two_tables() {
        let order = extract_join_order(&two_way_join());
        assert_eq!(order, vec!["orders", "customers"]);
    }

    #[test]
    fn join_order_three_tables() {
        let order = extract_join_order(&three_way_join());
        assert_eq!(
            order,
            vec!["orders", "customers", "products"]
        );
    }

    #[test]
    fn join_order_uses_alias() {
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: true_expr(),
            left: Box::new(RelExpr::Scan {
                table: "orders".to_string(),
                alias: Some("o".to_string()),
            }),
            right: Box::new(RelExpr::Scan {
                table: "customers".to_string(),
                alias: Some("c".to_string()),
            }),
        };
        let order = extract_join_order(&expr);
        assert_eq!(order, vec!["o", "c"]);
    }

    #[test]
    fn join_hints_from_inner() {
        let hints = extract_join_hints(&two_way_join());
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].relation, "customers");
        assert_eq!(hints[0].method, JoinMethodHint::Hash);
    }

    #[test]
    fn join_hints_three_way() {
        let hints = extract_join_hints(&three_way_join());
        assert_eq!(hints.len(), 2);
        assert_eq!(hints[0].relation, "products");
        assert_eq!(hints[1].relation, "customers");
    }

    #[test]
    fn scan_hints_from_join() {
        let scans = extract_scan_hints(&two_way_join());
        assert_eq!(scans.len(), 2);
        assert_eq!(scans[0].relation, "orders");
        assert_eq!(scans[1].relation, "customers");
    }

    #[test]
    fn scan_hint_single_table() {
        let scans = extract_scan_hints(&simple_scan("t"));
        assert_eq!(scans.len(), 1);
        assert_eq!(scans[0].relation, "t");
        assert_eq!(
            scans[0].method,
            ScanMethodHint::Sequential
        );
    }

    #[test]
    fn generate_full_advice() {
        let gen = AdviceGenerator::new(0.8);
        let advice = gen.generate("q1", &three_way_join());

        assert!(advice.len() >= 3);

        let combined = AdviceGenerator::combine_advice(&advice);
        assert!(combined.contains("JOIN_ORDER("));
        assert!(combined.contains("HASH_JOIN("));
        assert!(combined.contains("SEQ_SCAN("));
    }

    #[test]
    fn pg_hint_plan_conversion() {
        let gen = AdviceGenerator::new(0.9);
        let advice = gen.generate("q2", &two_way_join());
        let hint = AdviceGenerator::to_pg_hint_plan(&advice);

        assert!(hint.starts_with("/*+"));
        assert!(hint.ends_with("*/"));
        assert!(hint.contains("Leading("));
        assert!(hint.contains("HashJoin("));
    }

    #[test]
    fn combine_empty_advice() {
        let combined = AdviceGenerator::combine_advice(&[]);
        assert!(combined.is_empty());
    }

    #[test]
    fn pg_hint_plan_empty() {
        let hint = AdviceGenerator::to_pg_hint_plan(&[]);
        assert!(hint.is_empty());
    }

    #[test]
    fn confidence_clamped() {
        let gen = AdviceGenerator::new(1.5);
        let advice = gen.generate("q", &simple_scan("t"));
        for a in &advice {
            assert!(a.confidence <= 1.0);
        }
    }

    #[test]
    fn cross_join_no_method_hint() {
        let expr = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: true_expr(),
            left: Box::new(simple_scan("a")),
            right: Box::new(simple_scan("b")),
        };
        let hints = extract_join_hints(&expr);
        assert!(hints.is_empty());
    }

    #[test]
    fn convert_individual_hints() {
        let cases = vec![
            ("JOIN_ORDER(a b c)", "Leading(a b c)"),
            ("HASH_JOIN(t)", "HashJoin(t)"),
            ("MERGE_JOIN(t)", "MergeJoin(t)"),
            ("NESTED_LOOP(t)", "NestLoop(t)"),
            ("SEQ_SCAN(t)", "SeqScan(t)"),
            ("INDEX_SCAN(t idx_t)", "IndexScan(t idx_t)"),
            ("PARALLEL(t 4)", "Parallel(t 4)"),
        ];
        for (input, expected) in cases {
            let advice = PlanAdvice {
                query_id: "test".into(),
                advice_type: AdviceType::JoinMethod,
                advice_string: input.into(),
                estimated_improvement: 1.0,
                confidence: 0.8,
            };
            assert_eq!(
                convert_to_pg_hint_plan(&advice),
                expected,
                "failed for input: {input}"
            );
        }
    }

    #[test]
    fn filter_wrapping_join() {
        let expr = RelExpr::Filter {
            predicate: Expr::Const(Const::Bool(true)),
            input: Box::new(two_way_join()),
        };
        let order = extract_join_order(&expr);
        assert_eq!(order, vec!["orders", "customers"]);
    }

    #[test]
    fn plan_advice_display() {
        let advice = PlanAdvice {
            query_id: "q1".into(),
            advice_type: AdviceType::JoinReordering,
            advice_string: "JOIN_ORDER(a b)".into(),
            estimated_improvement: 2.0,
            confidence: 0.9,
        };
        assert_eq!(format!("{advice}"), "JOIN_ORDER(a b)");
    }

    #[test]
    fn advice_type_display() {
        assert_eq!(
            format!("{}", AdviceType::JoinReordering),
            "JOIN_ORDER"
        );
        assert_eq!(
            format!("{}", AdviceType::ScanMethod),
            "SCAN_METHOD"
        );
    }
}
