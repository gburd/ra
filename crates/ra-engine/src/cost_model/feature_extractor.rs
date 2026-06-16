//! Feature extraction from `RelExpr` for cost model training.
//!
//! Walks a relational algebra tree and extracts numerical features
//! used by the simple cost model for prediction.

#![expect(
    clippy::match_same_arms,
    reason = "match arms are kept separate for documentation of node-type semantics"
)]

use std::collections::HashMap;

use ra_core::algebra::{JoinType, RelExpr, WindowExpr};
use ra_core::expr::{BinOp, Expr};
use ra_core::statistics::Statistics;

use super::QueryFeatures;

/// Raw structural counts from expression tree traversal.
///
/// Produced by [`FeatureExtractor::structural_counts()`] for use in
/// optimization routing without duplicating the tree walk.
#[derive(Debug, Clone, Default)]
pub struct StructuralCounts {
    pub table_count: u32,
    pub join_count: u32,
    pub filter_count: u32,
    pub aggregate_count: u32,
    pub subquery_count: u32,
    pub window_count: u32,
    pub cross_join_count: u32,
    pub equi_join_count: u32,
    pub non_equi_join_count: u32,
    pub has_limit: bool,
    pub has_distinct: bool,
    pub has_group_by: bool,
}

/// Extract query features from a parsed `RelExpr`.
///
/// This walks the entire expression tree and counts structural elements
/// like joins, aggregates, filters, etc.
#[must_use] 
pub fn extract_features(expr: &RelExpr) -> QueryFeatures {
    let mut extractor = FeatureExtractor::new();
    extractor.visit(expr);
    extractor.into_features()
}

/// Extract query features using table statistics for improved cardinality estimates.
///
/// Produces the same 12-dimensional `QueryFeatures` as [`extract_features`], but
/// replaces the structural `max_join_cardinality` heuristic with an estimate based
/// on actual row counts from `table_stats`.
///
/// # Cardinality estimation
///
/// - When statistics are available for ≥1 table, `max_join_cardinality` is set to
///   `log10(geometric_mean_rows × join_count_factor)` — a scale-aware estimate that
///   distinguishes a 3-table join over 1 M-row tables from one over 1 K-row tables.
/// - Falls back to the structural heuristic when no matching stats are found.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "callers always use the standard hasher; generic hasher would force public API churn"
)]
pub fn extract_features_with_stats(
    expr: &RelExpr,
    table_stats: &HashMap<String, Statistics>,
) -> QueryFeatures {
    let mut base = extract_features(expr);
    if table_stats.is_empty() {
        return base;
    }

    // Collect row counts for tables referenced in the plan
    let table_names = collect_table_names(expr);
    let row_counts: Vec<f64> = table_names
        .iter()
        .filter_map(|name| {
            // Try exact match first, then case-insensitive
            table_stats
                .get(name.as_str())
                .or_else(|| {
                    let lower = name.to_lowercase();
                    table_stats.iter().find(|(k, _)| k.to_lowercase() == lower).map(|(_, v)| v)
                })
                .map(|s| s.row_count)
        })
        .filter(|&r| r > 0.0)
        .collect();

    if row_counts.is_empty() {
        return base;
    }

    // Geometric mean of observed row counts (log-scale average)
    let log_sum: f64 = row_counts.iter().map(|r| r.log10().max(0.0)).sum();
    let log_mean = log_sum / row_counts.len() as f64;

    // Scale by join count: each join multiplies cardinality, but real predicates
    // reduce it.  Use join_count as a mild amplifier (cap at ×3).
    let join_factor = (1.0 + f64::from(base.join_count) * 0.5).min(3.0);
    let cardinality_estimate = (log_mean + join_factor.log10()).clamp(0.0, 12.0);

    base.max_join_cardinality = cardinality_estimate as f32;
    base
}

/// Collect all base-table names referenced in an expression tree (lowercase).
fn collect_table_names(expr: &RelExpr) -> Vec<String> {
    let mut names = Vec::new();
    collect_table_names_inner(expr, &mut names);
    names.sort();
    names.dedup();
    names
}

fn collect_table_names_inner(expr: &RelExpr, out: &mut Vec<String>) {
    match expr {
        RelExpr::Scan { table, .. }
        | RelExpr::IndexScan { table, .. }
        | RelExpr::BitmapHeapScan { table, .. }
        | RelExpr::BitmapIndexScan { table, .. }
        | RelExpr::IndexOnlyScan { table, .. }
        | RelExpr::ParallelScan { table, .. }
        | RelExpr::MvScan { view_name: table, .. } => {
            out.push(table.to_lowercase());
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input }
        | RelExpr::DistinctOn { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Gather { input, .. }
        | RelExpr::ParallelAggregate { input, .. }
        | RelExpr::TopK { input, .. }
        | RelExpr::VectorFilter { input, .. }
        | RelExpr::IncrementalSort { input, .. } => collect_table_names_inner(input, out),
        RelExpr::Join { left, right, .. }
        | RelExpr::ParallelHashJoin { left, right, .. }
        | RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            collect_table_names_inner(left, out);
            collect_table_names_inner(right, out);
        }
        RelExpr::CTE { definition, body, .. } => {
            collect_table_names_inner(definition, out);
            collect_table_names_inner(body, out);
        }
        RelExpr::RecursiveCTE { base_case, recursive_case, body, .. } => {
            collect_table_names_inner(base_case, out);
            collect_table_names_inner(recursive_case, out);
            collect_table_names_inner(body, out);
        }
        RelExpr::BitmapAnd { inputs } | RelExpr::BitmapOr { inputs } => {
            for i in inputs {
                collect_table_names_inner(i, out);
            }
        }
        RelExpr::Unnest { input, .. } | RelExpr::TableFunction { input, .. } => {
            if let Some(inp) = input {
                collect_table_names_inner(inp, out);
            }
        }
        RelExpr::Values { .. }
        | RelExpr::MultiUnnest { .. }
        | RelExpr::RowPattern { .. } => {}
        RelExpr::Insert { table, source, .. } => {
            out.push(table.to_lowercase());
            collect_table_names_inner(source, out);
        }
        RelExpr::Update { table, from, .. } => {
            out.push(table.to_lowercase());
            if let Some(f) = from {
                collect_table_names_inner(f, out);
            }
        }
        RelExpr::Delete { table, using, .. } => {
            out.push(table.to_lowercase());
            if let Some(u) = using {
                collect_table_names_inner(u, out);
            }
        }
        RelExpr::Merge { target, source, .. } => {
            out.push(target.to_lowercase());
            collect_table_names_inner(source, out);
        }
        // GRAPH_TABLE references a graph, not base tables.
        RelExpr::GraphTable { .. } => {}
    }
}

/// Visitor that accumulates feature counts from a `RelExpr` tree.
///
/// Produces both the 12-dimensional [`QueryFeatures`] (via
/// [`into_features()`](Self::into_features)) and the raw
/// [`StructuralCounts`] (via [`structural_counts()`](Self::structural_counts))
/// needed by the speculative router.
pub struct FeatureExtractor {
    table_count: u32,
    join_count: u32,
    filter_count: u32,
    aggregate_count: u32,
    subquery_count: u32,
    cte_count: u32,
    window_function_count: u32,
    order_by_count: u32,
    group_by_count: u32,
    distinct_flag: bool,
    limit_present: bool,
    max_join_cardinality_estimate: f64,
    cross_join_count: u32,
    equi_join_count: u32,
    non_equi_join_count: u32,
}

impl Default for FeatureExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl FeatureExtractor {
    #[must_use] 
    pub fn new() -> Self {
        Self {
            table_count: 0,
            join_count: 0,
            filter_count: 0,
            aggregate_count: 0,
            subquery_count: 0,
            cte_count: 0,
            window_function_count: 0,
            order_by_count: 0,
            group_by_count: 0,
            distinct_flag: false,
            limit_present: false,
            max_join_cardinality_estimate: 1.0,
            cross_join_count: 0,
            equi_join_count: 0,
            non_equi_join_count: 0,
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "tree walk over RelExpr; per-variant logic is clearer inline than dispatched"
    )]
    pub fn visit(&mut self, expr: &RelExpr) {
        match expr {
            RelExpr::Scan { .. }
            | RelExpr::ParallelScan { .. }
            | RelExpr::IndexScan { .. }
            | RelExpr::MvScan { .. } => {
                self.table_count += 1;
            }

            RelExpr::Filter { input, predicate, .. } => {
                // Count the number of filter predicates
                self.filter_count += Self::count_predicates(predicate);
                self.visit(input);
            }

            RelExpr::Project { input, columns } => {
                // Check columns for aggregate expressions (like COUNT, SUM, etc.)
                for column in columns {
                    self.aggregate_count += Self::count_aggregates_in_expr(&column.expr);
                }
                self.visit(input);
            }

            RelExpr::Join {
                join_type,
                left,
                right,
                condition,
            }
            | RelExpr::ParallelHashJoin {
                join_type,
                left,
                right,
                condition,
                ..
            } => {
                self.join_count += 1;
                self.classify_join(join_type, condition);

                // Estimate join cardinality based on table count
                // This is a rough heuristic: assume each join multiplies cardinality
                let cardinality_estimate = f64::from(self.table_count + 2).powi(2);
                self.max_join_cardinality_estimate =
                    self.max_join_cardinality_estimate.max(cardinality_estimate);

                // Count predicates in join condition
                self.filter_count += Self::count_predicates(condition);

                self.visit(left);
                self.visit(right);
            }

            RelExpr::Aggregate {
                group_by,
                aggregates,
                input,
                ..
            }
            | RelExpr::ParallelAggregate {
                group_by,
                aggregates,
                input,
                ..
            } => {
                self.aggregate_count += aggregates.len() as u32;
                self.group_by_count += group_by.len() as u32;
                self.visit(input);
            }

            RelExpr::Sort { keys, input, .. } => {
                self.order_by_count += keys.len() as u32;
                self.visit(input);
            }

            RelExpr::IncrementalSort {
                prefix_keys,
                suffix_keys,
                input,
            } => {
                self.order_by_count += (prefix_keys.len() + suffix_keys.len()) as u32;
                self.visit(input);
            }

            RelExpr::Limit { input, .. } => {
                self.limit_present = true;
                self.visit(input);
            }

            RelExpr::Union { left, right, .. }
            | RelExpr::Intersect { left, right, .. }
            | RelExpr::Except { left, right, .. } => {
                self.visit(left);
                self.visit(right);
            }

            RelExpr::CTE {
                definition, body, ..
            } => {
                self.cte_count += 1;
                self.visit(definition);
                self.visit(body);
            }

            RelExpr::RecursiveCTE {
                base_case,
                recursive_case,
                body,
                ..
            } => {
                self.cte_count += 1;
                self.visit(base_case);
                self.visit(recursive_case);
                self.visit(body);
            }

            RelExpr::Window { functions, input } => {
                self.window_function_count += Self::count_window_functions(functions);
                self.visit(input);
            }

            RelExpr::Distinct { input } | RelExpr::DistinctOn { input, .. } => {
                self.distinct_flag = true;
                self.visit(input);
            }

            RelExpr::Values { .. } => {
                // Values nodes don't add to table count
            }

            RelExpr::Unnest { input, expr, .. } => {
                // Count subqueries in the unnest expression
                self.subquery_count += Self::count_subqueries(expr);
                if let Some(inp) = input {
                    self.visit(inp);
                }
            }

            RelExpr::MultiUnnest { exprs, .. } => {
                for expr in exprs {
                    self.subquery_count += Self::count_subqueries(expr);
                }
            }

            RelExpr::TableFunction { input, args, .. } => {
                for arg in args {
                    self.subquery_count += Self::count_subqueries(arg);
                }
                if let Some(inp) = input {
                    self.visit(inp);
                }
            }

            RelExpr::RowPattern { input, .. } => {
                self.visit(input);
            }

            RelExpr::IndexOnlyScan { .. }
            | RelExpr::BitmapIndexScan { .. } => {
                self.table_count += 1;
            }

            RelExpr::BitmapAnd { inputs } | RelExpr::BitmapOr { inputs } => {
                for bitmap in inputs {
                    self.visit(bitmap);
                }
            }

            RelExpr::BitmapHeapScan { bitmap, .. } => {
                self.table_count += 1;
                self.visit(bitmap);
            }

            RelExpr::Gather { input, .. } => {
                self.visit(input);
            }

            RelExpr::TopK { input, .. } | RelExpr::VectorFilter { input, .. } => {
                self.visit(input);
            }

            RelExpr::Insert { source, .. } => {
                self.table_count += 1;
                self.visit(source);
            }
            RelExpr::Update { from, .. } => {
                self.table_count += 1;
                if let Some(f) = from {
                    self.visit(f);
                }
            }
            RelExpr::Delete { using, .. } => {
                self.table_count += 1;
                if let Some(u) = using {
                    self.visit(u);
                }
            }
            RelExpr::Merge { source, .. } => {
                self.table_count += 1;
                self.visit(source);
            }
            RelExpr::GraphTable { .. } => {
                self.table_count += 1;
            }
        }
    }

    /// Count aggregate functions in an expression.
    fn count_aggregates_in_expr(expr: &Expr) -> u32 {
        match expr {
            Expr::Function { name, args } => {
                // Check if this is an aggregate function
                let aggregate_functions = [
                    "count", "sum", "avg", "min", "max", "array_agg", "string_agg",
                    "count_distinct", "stddev", "variance", "covar_pop", "covar_samp",
                    "corr", "regr_slope", "regr_intercept", "percentile_cont", "percentile_disc"
                ];

                let mut count = 0;
                if aggregate_functions.iter().any(|&func| name.eq_ignore_ascii_case(func)) {
                    count += 1;
                }

                // Recursively check arguments
                for arg in args {
                    count += Self::count_aggregates_in_expr(arg);
                }
                count
            }
            Expr::BinOp { left, right, .. } => {
                Self::count_aggregates_in_expr(left) + Self::count_aggregates_in_expr(right)
            }
            Expr::UnaryOp { operand, .. } => {
                Self::count_aggregates_in_expr(operand)
            }
            Expr::Case { .. } => {
                // For simplicity, don't traverse Case expressions
                // Most CASE expressions won't contain aggregates in typical queries
                0
            }
            _ => 0, // Constants, columns, etc.
        }
    }

    /// Count the number of predicates in an expression.
    /// Treats AND/OR as combining multiple predicates.
    fn count_predicates(expr: &Expr) -> u32 {
        match expr {
            Expr::BinOp { op, left, right } => {
                if matches!(
                    op,
                    ra_core::expr::BinOp::And | ra_core::expr::BinOp::Or
                ) {
                    // AND/OR combine multiple predicates
                    Self::count_predicates(left) + Self::count_predicates(right)
                } else {
                    // Other binary ops count as one predicate
                    1
                }
            }
            Expr::UnaryOp { .. } => 1,
            Expr::Function { .. } => 1,
            Expr::Case { when_clauses, .. } => when_clauses.len() as u32,
            _ => 0, // Constants, columns don't count as predicates
        }
    }

    /// Count subqueries in an expression tree.
    fn count_subqueries(expr: &Expr) -> u32 {
        let mut count = 0;
        Self::count_subqueries_recursive(expr, &mut count);
        count
    }

    fn count_subqueries_recursive(expr: &Expr, count: &mut u32) {
        match expr {
            Expr::SubQuery { .. } => {
                *count += 1;
            }
            Expr::BinOp { left, right, .. } => {
                Self::count_subqueries_recursive(left, count);
                Self::count_subqueries_recursive(right, count);
            }
            Expr::UnaryOp { operand, .. } => {
                Self::count_subqueries_recursive(operand, count);
            }
            Expr::Function { args, .. } => {
                for arg in args {
                    Self::count_subqueries_recursive(arg, count);
                }
            }
            Expr::Case {
                operand,
                when_clauses,
                else_result,
            } => {
                if let Some(op) = operand {
                    Self::count_subqueries_recursive(op, count);
                }
                for (cond, result) in when_clauses {
                    Self::count_subqueries_recursive(cond, count);
                    Self::count_subqueries_recursive(result, count);
                }
                if let Some(el) = else_result {
                    Self::count_subqueries_recursive(el, count);
                }
            }
            Expr::Cast { expr, .. } | Expr::FieldAccess { expr, .. } => {
                Self::count_subqueries_recursive(expr, count);
            }
            Expr::Array(elements) => {
                for elem in elements {
                    Self::count_subqueries_recursive(elem, count);
                }
            }
            Expr::ArrayIndex(array, index) => {
                Self::count_subqueries_recursive(array, count);
                Self::count_subqueries_recursive(index, count);
            }
            Expr::ArraySlice { array, start, end } => {
                Self::count_subqueries_recursive(array, count);
                if let Some(s) = start {
                    Self::count_subqueries_recursive(s, count);
                }
                if let Some(e) = end {
                    Self::count_subqueries_recursive(e, count);
                }
            }
            Expr::VectorDistance { column, target, .. } => {
                Self::count_subqueries_recursive(column, count);
                Self::count_subqueries_recursive(target, count);
            }
            _ => {} // Other expression types don't contain subqueries
        }
    }

    /// Count window functions in a list of window expressions.
    fn count_window_functions(functions: &[WindowExpr]) -> u32 {
        functions.len() as u32
    }

    /// Classify a join as cross, equi, or non-equi.
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "consistency with sibling methods that take refs to Expr"
    )]
    fn classify_join(&mut self, join_type: &JoinType, condition: &Expr) {
        // Semi/Anti joins with trivial conditions are existence checks
        // (e.g., decorrelated EXISTS), not cross products. Treat them as
        // non-equi joins rather than inflating cross_join_count.
        if matches!(join_type, JoinType::Cross) {
            self.cross_join_count += 1;
            return;
        }
        if Self::is_trivial_condition(condition) {
            if matches!(join_type, JoinType::Semi | JoinType::Anti) {
                self.non_equi_join_count += 1;
            } else {
                self.cross_join_count += 1;
            }
            return;
        }

        if Self::is_equi_join_condition(condition) {
            self.equi_join_count += 1;
        } else {
            self.non_equi_join_count += 1;
        }
    }

    /// Check if condition is trivially true (cross join).
    fn is_trivial_condition(expr: &Expr) -> bool {
        matches!(
            expr,
            Expr::Const(ra_core::expr::Const::Bool(true))
        )
    }

    /// Check if condition contains at least one equi-join predicate
    /// (column = column).
    fn is_equi_join_condition(expr: &Expr) -> bool {
        match expr {
            Expr::BinOp {
                op: BinOp::Eq,
                left,
                right,
            } => {
                matches!(left.as_ref(), Expr::Column(_))
                    && matches!(right.as_ref(), Expr::Column(_))
            }
            Expr::BinOp {
                op: BinOp::And,
                left,
                right,
            } => {
                Self::is_equi_join_condition(left)
                    || Self::is_equi_join_condition(right)
            }
            _ => false,
        }
    }

    /// Return the raw structural counts for use by the speculative router.
    #[must_use] 
    pub fn structural_counts(&self) -> StructuralCounts {
        StructuralCounts {
            table_count: self.table_count,
            join_count: self.join_count,
            filter_count: self.filter_count,
            aggregate_count: self.aggregate_count,
            subquery_count: self.subquery_count,
            window_count: self.window_function_count,
            cross_join_count: self.cross_join_count,
            equi_join_count: self.equi_join_count,
            non_equi_join_count: self.non_equi_join_count,
            has_limit: self.limit_present,
            has_distinct: self.distinct_flag,
            has_group_by: self.group_by_count > 0,
        }
    }

    /// Convert accumulated counts into `QueryFeatures`.
    fn into_features(self) -> QueryFeatures {
        QueryFeatures {
            table_count: self.table_count as f32,
            join_count: self.join_count as f32,
            filter_count: self.filter_count as f32,
            aggregate_count: self.aggregate_count as f32,
            subquery_count: self.subquery_count as f32,
            cte_count: self.cte_count as f32,
            window_function_count: self.window_function_count as f32,
            order_by_count: self.order_by_count as f32,
            group_by_count: self.group_by_count as f32,
            distinct_flag: if self.distinct_flag { 1.0 } else { 0.0 },
            limit_present: if self.limit_present { 1.0 } else { 0.0 },
            max_join_cardinality: self.max_join_cardinality_estimate as f32,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "feature counts are integer-valued floats; exact equality is the right check"
)]
mod tests {
    use super::*;
    use ra_core::algebra::{AggregateExpr, AggregateFunction, JoinType};
    use ra_core::expr::{BinOp, ColumnRef, Const};

    #[test]
    fn test_extract_simple_scan() {
        let expr = RelExpr::Scan {
            table: "users".to_owned(),
            alias: None,
        };
        let features = extract_features(&expr);
        assert_eq!(features.table_count, 1.0);
        assert_eq!(features.join_count, 0.0);
        assert_eq!(features.filter_count, 0.0);
    }

    #[test]
    fn test_extract_filter() {
        let expr = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(21))),
            },
            input: Box::new(RelExpr::Scan {
                table: "users".to_owned(),
                alias: None,
            }),
        };
        let features = extract_features(&expr);
        assert_eq!(features.table_count, 1.0);
        assert_eq!(features.filter_count, 1.0);
    }

    #[test]
    fn test_extract_join() {
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("a.id"))),
                right: Box::new(Expr::Column(ColumnRef::new("b.id"))),
            },
            left: Box::new(RelExpr::Scan {
                table: "orders".to_owned(),
                alias: Some("a".to_owned()),
            }),
            right: Box::new(RelExpr::Scan {
                table: "customers".to_owned(),
                alias: Some("b".to_owned()),
            }),
        };
        let features = extract_features(&expr);
        assert_eq!(features.table_count, 2.0);
        assert_eq!(features.join_count, 1.0);
        assert_eq!(features.filter_count, 1.0); // Join condition counts as filter
        assert!(features.max_join_cardinality > 1.0);
    }

    #[test]
    fn test_extract_aggregate() {
        let expr = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("category"))],
            aggregates: vec![
                AggregateExpr {
                    function: AggregateFunction::Count,
                    arg: None,
                    distinct: false,
                    alias: Some("cnt".to_owned()),
                },
                AggregateExpr {
                    function: AggregateFunction::Sum,
                    arg: Some(Expr::Column(ColumnRef::new("amount"))),
                    distinct: false,
                    alias: Some("total".to_owned()),
                },
            ],
            input: Box::new(RelExpr::Scan {
                table: "orders".to_owned(),
                alias: None,
            }),
        };
        let features = extract_features(&expr);
        assert_eq!(features.table_count, 1.0);
        assert_eq!(features.aggregate_count, 2.0);
        assert_eq!(features.group_by_count, 1.0);
    }

    #[test]
    fn test_extract_distinct() {
        let expr = RelExpr::Distinct {
            input: Box::new(RelExpr::Scan {
                table: "users".to_owned(),
                alias: None,
            }),
        };
        let features = extract_features(&expr);
        assert_eq!(features.distinct_flag, 1.0);
    }

    #[test]
    fn test_extract_limit() {
        let expr = RelExpr::Limit {
            count: 10,
            offset: 0,
            input: Box::new(RelExpr::Scan {
                table: "users".to_owned(),
                alias: None,
            }),
        };
        let features = extract_features(&expr);
        assert_eq!(features.limit_present, 1.0);
    }

    #[test]
    fn test_extract_cte() {
        let expr = RelExpr::CTE {
            name: "temp".to_owned(),
            definition: Box::new(RelExpr::Scan {
                table: "orders".to_owned(),
                alias: None,
            }),
            body: Box::new(RelExpr::Scan {
                table: "temp".to_owned(),
                alias: None,
            }),
        };
        let features = extract_features(&expr);
        assert_eq!(features.cte_count, 1.0);
        assert_eq!(features.table_count, 2.0); // Both scans count
    }

    #[test]
    fn test_count_and_predicates() {
        let expr = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::And,
                left: Box::new(Expr::BinOp {
                    op: BinOp::Gt,
                    left: Box::new(Expr::Column(ColumnRef::new("age"))),
                    right: Box::new(Expr::Const(Const::Int(21))),
                }),
                right: Box::new(Expr::BinOp {
                    op: BinOp::Eq,
                    left: Box::new(Expr::Column(ColumnRef::new("status"))),
                    right: Box::new(Expr::Const(Const::String("active".to_owned()))),
                }),
            },
            input: Box::new(RelExpr::Scan {
                table: "users".to_owned(),
                alias: None,
            }),
        };
        let features = extract_features(&expr);
        // AND combines two predicates
        assert_eq!(features.filter_count, 2.0);
    }

    #[test]
    fn test_complex_query() {
        // SELECT COUNT(*), SUM(amount)
        // FROM orders o
        // JOIN customers c ON o.customer_id = c.id
        // WHERE o.status = 'completed' AND c.age > 18
        // GROUP BY c.region
        // ORDER BY COUNT(*) DESC
        // LIMIT 10

        let expr = RelExpr::Limit {
            count: 10,
            offset: 0,
            input: Box::new(RelExpr::Sort {
                keys: vec![ra_core::algebra::SortKey {
                    expr: Expr::Column(ColumnRef::new("cnt")),
                    direction: ra_core::algebra::SortDirection::Desc,
                    nulls: ra_core::algebra::NullOrdering::Last,
                }],
                input: Box::new(RelExpr::Aggregate {
                    group_by: vec![Expr::Column(ColumnRef::new("region"))],
                    aggregates: vec![
                        AggregateExpr {
                            function: AggregateFunction::Count,
                            arg: None,
                            distinct: false,
                            alias: Some("cnt".to_owned()),
                        },
                        AggregateExpr {
                            function: AggregateFunction::Sum,
                            arg: Some(Expr::Column(ColumnRef::new("amount"))),
                            distinct: false,
                            alias: Some("total".to_owned()),
                        },
                    ],
                    input: Box::new(RelExpr::Filter {
                        predicate: Expr::BinOp {
                            op: BinOp::And,
                            left: Box::new(Expr::BinOp {
                                op: BinOp::Eq,
                                left: Box::new(Expr::Column(ColumnRef::new("status"))),
                                right: Box::new(Expr::Const(Const::String("completed".to_owned()))),
                            }),
                            right: Box::new(Expr::BinOp {
                                op: BinOp::Gt,
                                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                                right: Box::new(Expr::Const(Const::Int(18))),
                            }),
                        },
                        input: Box::new(RelExpr::Join {
                            join_type: JoinType::Inner,
                            condition: Expr::BinOp {
                                op: BinOp::Eq,
                                left: Box::new(Expr::Column(ColumnRef::new("customer_id"))),
                                right: Box::new(Expr::Column(ColumnRef::new("id"))),
                            },
                            left: Box::new(RelExpr::Scan {
                                table: "orders".to_owned(),
                                alias: Some("o".to_owned()),
                            }),
                            right: Box::new(RelExpr::Scan {
                                table: "customers".to_owned(),
                                alias: Some("c".to_owned()),
                            }),
                        }),
                    }),
                }),
            }),
        };

        let features = extract_features(&expr);
        assert_eq!(features.table_count, 2.0);
        assert_eq!(features.join_count, 1.0);
        assert_eq!(features.filter_count, 3.0); // 2 in WHERE + 1 in JOIN condition
        assert_eq!(features.aggregate_count, 2.0);
        assert_eq!(features.group_by_count, 1.0);
        assert_eq!(features.order_by_count, 1.0);
        assert_eq!(features.limit_present, 1.0);
    }
}
