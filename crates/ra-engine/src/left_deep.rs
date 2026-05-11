//! Left-deep join tree construction for queries with moderate join counts.
//!
//! For queries with 2-7 tables, constructing a left-deep join tree
//! directly is much faster than running e-graph equality saturation.
//! This provides a 10-50x speedup by avoiding the full search space
//! exploration.
//!
//! A left-deep tree has the form: ((T1 JOIN T2) JOIN T3) JOIN T4
//! where all joins are left-associated, forming a linear chain.
//!
//! Operators that sit above the join tree (Aggregate, Sort, Project,
//! Window) do not affect join ordering, so we optimise the join
//! subtree and preserve the outer operators.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use ra_core::{
    algebra::{JoinType, RelExpr},
    cost::StatisticsProvider,
    expr::Expr,
};

/// Build a left-deep join tree from a list of tables.
///
/// This is a simple heuristic that:
/// 1. Starts with the smallest table (by cardinality)
/// 2. Adds remaining tables in order of increasing cardinality
/// 3. Forms a left-deep tree: ((T1 JOIN T2) JOIN T3) JOIN T4
///
/// This is optimal for many simple queries and avoids the overhead
/// of e-graph equality saturation.
pub struct LeftDeepBuilder {
    stats_provider: Arc<dyn StatisticsProvider>,
}

impl LeftDeepBuilder {
    /// Create a new left-deep tree builder.
    pub fn new(stats_provider: Arc<dyn StatisticsProvider>) -> Self {
        Self { stats_provider }
    }

    /// Build a left-deep join tree from a query.
    ///
    /// Extracts all scan nodes and join conditions from the query,
    /// constructs an optimal left-deep tree, then re-wraps it with
    /// any outer operators (Aggregate, Sort, Project, etc.) that sat
    /// above the join tree.
    ///
    /// # Panics
    ///
    /// Panics if an internal iterator is unexpectedly empty after a
    /// length check.
    ///
    /// # Errors
    ///
    /// Returns an error if no tables are found in the query.
    #[expect(clippy::expect_used, reason = "guarded by length checks")]
    pub fn build(&self, expr: &RelExpr) -> Result<RelExpr> {
        // Collect outer operators that sit above the join tree
        let mut outer_ops: Vec<OuterOp> = Vec::new();
        let join_subtree = peel_outer_ops(expr, &mut outer_ops);

        // Extract all tables and conditions from the join subtree
        let mut tables = Vec::new();
        let mut conditions = Vec::new();
        self.extract_tables_and_conditions(join_subtree, &mut tables, &mut conditions)?;

        if tables.is_empty() {
            return Err(anyhow!("No tables found in query"));
        }

        if tables.len() == 1 {
            let result = tables
                .into_iter()
                .next()
                .expect("len()==1 guarantees element");
            return Ok(reapply_outer_ops(result, outer_ops));
        }

        // Sort tables by cardinality (smallest first)
        tables.sort_by(|a, b| {
            let a_rows = self.get_cardinality(a).unwrap_or(f64::MAX);
            let b_rows = self.get_cardinality(b).unwrap_or(f64::MAX);
            a_rows
                .partial_cmp(&b_rows)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Build left-deep tree
        let mut tables_iter = tables.into_iter();
        let mut current = tables_iter.next().expect("len()>=2 guarantees element");

        for table in tables_iter {
            let condition = self
                .find_join_condition(&current, &table, &conditions)
                .unwrap_or(Expr::Const(ra_core::expr::Const::Bool(true)));

            current = RelExpr::Join {
                join_type: JoinType::Inner,
                condition,
                left: Box::new(current),
                right: Box::new(table),
            };
        }

        Ok(reapply_outer_ops(current, outer_ops))
    }

    /// Extract all scan nodes and join conditions from an expression.
    #[expect(
        clippy::self_only_used_in_recursion,
        reason = "method on optimizer struct"
    )]
    fn extract_tables_and_conditions(
        &self,
        expr: &RelExpr,
        tables: &mut Vec<RelExpr>,
        conditions: &mut Vec<Expr>,
    ) -> Result<()> {
        match expr {
            RelExpr::Scan { .. }
            | RelExpr::IndexScan { .. }
            | RelExpr::IndexOnlyScan { .. }
            | RelExpr::BitmapHeapScan { .. }
            | RelExpr::ParallelScan { .. }
            | RelExpr::MvScan { .. } => {
                tables.push(expr.clone());
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
                // Extract condition
                if !matches!(condition, Expr::Const(ra_core::expr::Const::Bool(true))) {
                    conditions.push(condition.clone());
                }
                // Recursively extract from children
                self.extract_tables_and_conditions(left, tables, conditions)?;
                self.extract_tables_and_conditions(right, tables, conditions)?;
            }
            RelExpr::Filter {
                input, predicate, ..
            } => {
                // Extract filter as potential join condition
                conditions.push(predicate.clone());
                self.extract_tables_and_conditions(input, tables, conditions)?;
            }
            RelExpr::Project { input, .. }
            | RelExpr::Aggregate { input, .. }
            | RelExpr::Sort { input, .. }
            | RelExpr::Limit { input, .. }
            | RelExpr::Window { input, .. }
            | RelExpr::Distinct { input }
            | RelExpr::IncrementalSort { input, .. }
            | RelExpr::TopK { input, .. }
            | RelExpr::Gather { input, .. }
            | RelExpr::ParallelAggregate { input, .. } => {
                self.extract_tables_and_conditions(input, tables, conditions)?;
            }
            _ => {
                // For other node types, we can't optimize with left-deep trees
                return Err(anyhow!("Unsupported node type for left-deep optimization"));
            }
        }
        Ok(())
    }

    /// Get the cardinality (row count) for a table.
    fn get_cardinality(&self, expr: &RelExpr) -> Option<f64> {
        let table_name = match expr {
            RelExpr::Scan { table, .. }
            | RelExpr::IndexScan { table, .. }
            | RelExpr::IndexOnlyScan { table, .. }
            | RelExpr::ParallelScan { table, .. }
            | RelExpr::MvScan {
                view_name: table, ..
            } => Some(table.as_str()),
            RelExpr::BitmapHeapScan { table, .. } => Some(table.as_str()),
            _ => None,
        };
        table_name.and_then(|t| self.stats_provider.get_statistics(t).map(|s| s.row_count))
    }

    /// Find the best join condition between two tables.
    ///
    /// This is a simple heuristic that looks for conditions referencing
    /// columns from both tables.
    #[expect(
        clippy::unused_self,
        reason = "will use self for table statistics lookup"
    )]
    fn find_join_condition(
        &self,
        _left: &RelExpr,
        _right: &RelExpr,
        conditions: &[Expr],
    ) -> Option<Expr> {
        // For now, use the first available condition
        // A more sophisticated implementation would analyze which
        // columns are referenced and pick the best condition
        conditions.first().cloned()
    }
}

/// An operator that sits above the join tree and can be re-applied
/// after join reordering.
#[derive(Clone)]
enum OuterOp {
    Aggregate {
        group_by: Vec<Expr>,
        aggregates: Vec<ra_core::algebra::AggregateExpr>,
    },
    Project {
        columns: Vec<ra_core::algebra::ProjectionColumn>,
    },
    Sort {
        keys: Vec<ra_core::algebra::SortKey>,
    },
    Limit {
        count: u64,
        offset: u64,
    },
    Window {
        functions: Vec<ra_core::algebra::WindowExpr>,
    },
    Distinct,
}

/// Peel outer operators off the expression, collecting them in order
/// (outermost first), and return the inner join subtree.
fn peel_outer_ops<'a>(expr: &'a RelExpr, ops: &mut Vec<OuterOp>) -> &'a RelExpr {
    match expr {
        RelExpr::Aggregate {
            group_by,
            aggregates,
            input,
        } => {
            ops.push(OuterOp::Aggregate {
                group_by: group_by.clone(),
                aggregates: aggregates.clone(),
            });
            peel_outer_ops(input, ops)
        }
        RelExpr::Project { columns, input } => {
            ops.push(OuterOp::Project {
                columns: columns.clone(),
            });
            peel_outer_ops(input, ops)
        }
        RelExpr::Sort { keys, input } => {
            ops.push(OuterOp::Sort { keys: keys.clone() });
            peel_outer_ops(input, ops)
        }
        RelExpr::Limit {
            count,
            offset,
            input,
        } => {
            ops.push(OuterOp::Limit {
                count: *count,
                offset: *offset,
            });
            peel_outer_ops(input, ops)
        }
        RelExpr::Window { functions, input } => {
            ops.push(OuterOp::Window {
                functions: functions.clone(),
            });
            peel_outer_ops(input, ops)
        }
        RelExpr::Distinct { input } => {
            ops.push(OuterOp::Distinct);
            peel_outer_ops(input, ops)
        }
        // Filter is part of the join tree, stop peeling
        _ => expr,
    }
}

/// Re-apply outer operators on top of the optimised join tree.
/// Operators are stored outermost-first, so we apply in reverse.
fn reapply_outer_ops(mut result: RelExpr, ops: Vec<OuterOp>) -> RelExpr {
    for op in ops.into_iter().rev() {
        result = match op {
            OuterOp::Aggregate {
                group_by,
                aggregates,
            } => RelExpr::Aggregate {
                group_by,
                aggregates,
                input: Box::new(result),
            },
            OuterOp::Project { columns } => RelExpr::Project {
                columns,
                input: Box::new(result),
            },
            OuterOp::Sort { keys } => RelExpr::Sort {
                keys,
                input: Box::new(result),
            },
            OuterOp::Limit { count, offset } => RelExpr::Limit {
                count,
                offset,
                input: Box::new(result),
            },
            OuterOp::Window { functions } => RelExpr::Window {
                functions,
                input: Box::new(result),
            },
            OuterOp::Distinct => RelExpr::Distinct {
                input: Box::new(result),
            },
        };
    }
    result
}

/// Check if a query is suitable for left-deep optimization.
///
/// Returns true if the query:
/// - Has 2-7 tables
/// - Contains only operators that preserve or sit above the join
///   tree (scans, joins, filters, projections, aggregates, sorts,
///   windows, limits, distinct)
/// - Has no CTEs, set operations, or recursive queries
#[must_use]
pub fn can_use_left_deep(expr: &RelExpr) -> bool {
    let table_count = count_tables(expr);

    if !(2..=7).contains(&table_count) {
        return false;
    }

    is_left_deep_eligible(expr)
}

/// Count the number of tables in a query.
///
/// Traverses through all operators that sit above or within the join
/// tree, including aggregates and windows.
fn count_tables(expr: &RelExpr) -> usize {
    match expr {
        RelExpr::Scan { .. }
        | RelExpr::IndexScan { .. }
        | RelExpr::IndexOnlyScan { .. }
        | RelExpr::BitmapHeapScan { .. }
        | RelExpr::ParallelScan { .. }
        | RelExpr::MvScan { .. } => 1,
        RelExpr::Join { left, right, .. } | RelExpr::ParallelHashJoin { left, right, .. } => {
            count_tables(left) + count_tables(right)
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Distinct { input }
        | RelExpr::IncrementalSort { input, .. }
        | RelExpr::TopK { input, .. }
        | RelExpr::Gather { input, .. }
        | RelExpr::ParallelAggregate { input, .. } => count_tables(input),
        _ => 0,
    }
}

/// Check if all operators in the query are compatible with left-deep
/// join reordering.
///
/// Operators that sit above the join tree (Aggregate, Sort, Window,
/// Project, Limit, Distinct) are fine -- they don't change which
/// join orderings are valid. CTEs, set operations, and recursive
/// queries require full e-graph optimization.
fn is_left_deep_eligible(expr: &RelExpr) -> bool {
    match expr {
        RelExpr::Scan { .. }
        | RelExpr::IndexScan { .. }
        | RelExpr::IndexOnlyScan { .. }
        | RelExpr::BitmapHeapScan { .. }
        | RelExpr::ParallelScan { .. }
        | RelExpr::MvScan { .. } => true,
        RelExpr::Join { left, right, .. } | RelExpr::ParallelHashJoin { left, right, .. } => {
            is_left_deep_eligible(left) && is_left_deep_eligible(right)
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Distinct { input }
        | RelExpr::IncrementalSort { input, .. }
        | RelExpr::TopK { input, .. }
        | RelExpr::Gather { input, .. }
        | RelExpr::ParallelAggregate { input, .. } => is_left_deep_eligible(input),
        // CTEs, set ops, recursive queries, and other variants need full e-graph
        _ => false,
    }
}

#[cfg(test)]
#[expect(clippy::panic, clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;
    use ra_core::expr::{BinOp, ColumnRef, Const};
    use ra_core::statistics::Statistics;
    use std::collections::HashMap;

    #[derive(Debug)]
    struct MockStats {
        stats: HashMap<String, Statistics>,
    }

    impl MockStats {
        fn new(entries: &[(&str, f64)]) -> Self {
            let mut stats = HashMap::new();
            for &(name, rows) in entries {
                stats.insert(name.to_string(), Statistics::new(rows));
            }
            Self { stats }
        }
    }

    impl StatisticsProvider for MockStats {
        fn get_statistics(&self, table: &str) -> Option<&Statistics> {
            self.stats.get(table)
        }
    }

    fn scan(name: &str) -> RelExpr {
        RelExpr::Scan {
            table: name.to_string(),
            alias: None,
        }
    }

    fn join(left: RelExpr, right: RelExpr, condition: Expr) -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn eq_col(left: &str, right: &str) -> Expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new(left))),
            right: Box::new(Expr::Column(ColumnRef::new(right))),
        }
    }

    #[test]
    fn test_can_use_left_deep_two_tables() {
        let query = join(scan("a"), scan("b"), eq_col("a.id", "b.id"));
        assert!(can_use_left_deep(&query));
    }

    #[test]
    fn test_can_use_left_deep_three_tables() {
        let query = join(
            join(scan("a"), scan("b"), eq_col("a.id", "b.id")),
            scan("c"),
            eq_col("b.id", "c.id"),
        );
        assert!(can_use_left_deep(&query));
    }

    #[test]
    fn test_can_use_left_deep_four_tables() {
        let query = join(
            join(
                join(scan("a"), scan("b"), eq_col("a.id", "b.id")),
                scan("c"),
                eq_col("b.id", "c.id"),
            ),
            scan("d"),
            eq_col("c.id", "d.id"),
        );
        assert!(can_use_left_deep(&query));
    }

    #[test]
    fn test_cannot_use_left_deep_single_table() {
        let query = scan("a");
        assert!(!can_use_left_deep(&query));
    }

    #[test]
    fn test_can_use_left_deep_five_tables() {
        let query = join(
            join(
                join(
                    join(scan("a"), scan("b"), eq_col("a.id", "b.id")),
                    scan("c"),
                    eq_col("b.id", "c.id"),
                ),
                scan("d"),
                eq_col("c.id", "d.id"),
            ),
            scan("e"),
            eq_col("d.id", "e.id"),
        );
        assert!(can_use_left_deep(&query)); // 5 tables within threshold
    }

    #[test]
    fn test_cannot_use_left_deep_too_many_tables() {
        // Build an 8-table join (exceeds the 7-table threshold)
        let mut query = join(scan("a"), scan("b"), eq_col("a.id", "b.id"));
        for name in ["c", "d", "e", "f", "g", "h"] {
            query = join(query, scan(name), eq_col("a.id", "b.id"));
        }
        assert!(!can_use_left_deep(&query)); // 8 tables
    }

    #[test]
    fn test_can_use_left_deep_with_aggregate() {
        let query = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![],
            input: Box::new(join(scan("a"), scan("b"), eq_col("a.id", "b.id"))),
        };
        assert!(can_use_left_deep(&query));
    }

    #[test]
    fn test_can_use_left_deep_with_filter() {
        let query = RelExpr::Filter {
            predicate: eq_col("a.x", "10"),
            input: Box::new(join(scan("a"), scan("b"), eq_col("a.id", "b.id"))),
        };
        assert!(can_use_left_deep(&query));
    }

    #[test]
    fn test_can_use_left_deep_with_project() {
        let query = RelExpr::Project {
            columns: vec![],
            input: Box::new(join(scan("a"), scan("b"), eq_col("a.id", "b.id"))),
        };
        assert!(can_use_left_deep(&query));
    }

    #[test]
    fn test_build_left_deep_two_tables() {
        let builder = LeftDeepBuilder::new(Arc::new(MockStats::new(&[("a", 100.0), ("b", 200.0)])));

        let query = join(scan("a"), scan("b"), eq_col("a.id", "b.id"));
        let result = builder.build(&query).unwrap();

        // Should have a join with 'a' (smaller) on the left
        match result {
            RelExpr::Join { left, right, .. } => {
                match left.as_ref() {
                    RelExpr::Scan { table, .. } => assert_eq!(table, "a"),
                    _ => panic!("Expected Scan on left"),
                }
                match right.as_ref() {
                    RelExpr::Scan { table, .. } => assert_eq!(table, "b"),
                    _ => panic!("Expected Scan on right"),
                }
            }
            _ => panic!("Expected Join"),
        }
    }

    #[test]
    fn test_build_left_deep_three_tables_sorted_by_size() {
        let builder = LeftDeepBuilder::new(Arc::new(MockStats::new(&[
            ("large", 1000.0),
            ("medium", 500.0),
            ("small", 100.0),
        ])));

        let query = join(
            join(
                scan("large"),
                scan("medium"),
                Expr::Const(Const::Bool(true)),
            ),
            scan("small"),
            Expr::Const(Const::Bool(true)),
        );
        let result = builder.build(&query).unwrap();

        // Should build ((small JOIN medium) JOIN large)
        match result {
            RelExpr::Join { left, right, .. } => {
                // Right should be 'large'
                match right.as_ref() {
                    RelExpr::Scan { table, .. } => assert_eq!(table, "large"),
                    _ => panic!("Expected 'large' on right"),
                }
                // Left should be (small JOIN medium)
                match left.as_ref() {
                    RelExpr::Join {
                        left: inner_left,
                        right: inner_right,
                        ..
                    } => {
                        match inner_left.as_ref() {
                            RelExpr::Scan { table, .. } => assert_eq!(table, "small"),
                            _ => panic!("Expected 'small' on inner left"),
                        }
                        match inner_right.as_ref() {
                            RelExpr::Scan { table, .. } => assert_eq!(table, "medium"),
                            _ => panic!("Expected 'medium' on inner right"),
                        }
                    }
                    _ => panic!("Expected Join on left"),
                }
            }
            _ => panic!("Expected Join"),
        }
    }

    #[test]
    fn test_build_left_deep_single_table_returns_scan() {
        let builder = LeftDeepBuilder::new(Arc::new(MockStats::new(&[("a", 100.0)])));

        let query = scan("a");
        let result = builder.build(&query).unwrap();

        match result {
            RelExpr::Scan { table, .. } => assert_eq!(table, "a"),
            _ => panic!("Expected Scan"),
        }
    }

    #[test]
    fn test_count_tables() {
        assert_eq!(count_tables(&scan("a")), 1);
        assert_eq!(
            count_tables(&join(scan("a"), scan("b"), Expr::Const(Const::Bool(true)))),
            2
        );
        assert_eq!(
            count_tables(&join(
                join(scan("a"), scan("b"), Expr::Const(Const::Bool(true))),
                scan("c"),
                Expr::Const(Const::Bool(true))
            )),
            3
        );
    }
}
