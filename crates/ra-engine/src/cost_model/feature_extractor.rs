//! Feature extraction from RelExpr for cost model training.
//!
//! Walks a relational algebra tree and extracts numerical features
//! used by the simple cost model for prediction.

use ra_core::algebra::{RelExpr, WindowExpr};
use ra_core::expr::Expr;

use super::simple_model::QueryFeatures;

/// Extract query features from a parsed RelExpr.
///
/// This walks the entire expression tree and counts structural elements
/// like joins, aggregates, filters, etc.
pub fn extract_features(expr: &RelExpr) -> QueryFeatures {
    let mut extractor = FeatureExtractor::new();
    extractor.visit(expr);
    extractor.into_features()
}

/// Internal visitor that accumulates feature counts.
struct FeatureExtractor {
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
}

impl FeatureExtractor {
    fn new() -> Self {
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
        }
    }

    fn visit(&mut self, expr: &RelExpr) {
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

            RelExpr::Project { input, .. } => {
                self.visit(input);
            }

            RelExpr::Join {
                left,
                right,
                condition,
                ..
            }
            | RelExpr::ParallelHashJoin {
                left,
                right,
                condition,
                ..
            } => {
                self.join_count += 1;
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

            RelExpr::Distinct { input } => {
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

    /// Convert accumulated counts into QueryFeatures.
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
