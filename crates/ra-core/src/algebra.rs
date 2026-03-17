//! Relational algebra AST types.
//!
//! This module defines the core relational expression tree that
//! represents query plans. Each variant of [`RelExpr`] corresponds
//! to a standard relational algebra operator.

use serde::{Deserialize, Serialize};

use crate::expr::{ColumnRef, Expr};

/// A relational expression (query plan node).
///
/// Each variant wraps its children in `Box<RelExpr>` to form a tree.
/// Leaf nodes are `Scan` operations over named tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RelExpr {
    /// Scan a base table.
    Scan {
        /// Table name.
        table: String,
        /// Optional alias for the scan.
        alias: Option<String>,
    },

    /// Filter rows by a predicate.
    Filter {
        /// The predicate expression.
        predicate: Expr,
        /// The input relation.
        input: Box<RelExpr>,
    },

    /// Project (select) a set of columns or expressions.
    Project {
        /// The expressions to project, each with an optional alias.
        columns: Vec<ProjectionColumn>,
        /// The input relation.
        input: Box<RelExpr>,
    },

    /// Join two relations.
    Join {
        /// The type of join.
        join_type: JoinType,
        /// The join condition.
        condition: Expr,
        /// The left input relation.
        left: Box<RelExpr>,
        /// The right input relation.
        right: Box<RelExpr>,
    },

    /// Aggregate (GROUP BY) with optional grouping keys.
    Aggregate {
        /// Grouping key expressions.
        group_by: Vec<Expr>,
        /// Aggregate function calls.
        aggregates: Vec<AggregateExpr>,
        /// The input relation.
        input: Box<RelExpr>,
    },

    /// Sort by one or more keys.
    Sort {
        /// The sort keys with direction.
        keys: Vec<SortKey>,
        /// The input relation.
        input: Box<RelExpr>,
    },

    /// Limit the number of rows, with optional offset.
    Limit {
        /// Maximum number of rows to return.
        count: u64,
        /// Number of rows to skip before returning.
        offset: u64,
        /// The input relation.
        input: Box<RelExpr>,
    },

    /// Set union of two relations.
    Union {
        /// Whether to remove duplicates.
        all: bool,
        /// The left input relation.
        left: Box<RelExpr>,
        /// The right input relation.
        right: Box<RelExpr>,
    },

    /// Set intersection of two relations.
    Intersect {
        /// Whether to keep duplicates.
        all: bool,
        /// The left input relation.
        left: Box<RelExpr>,
        /// The right input relation.
        right: Box<RelExpr>,
    },

    /// Set difference of two relations.
    Except {
        /// Whether to keep duplicates.
        all: bool,
        /// The left input relation.
        left: Box<RelExpr>,
        /// The right input relation.
        right: Box<RelExpr>,
    },
}

/// A column in a projection list, consisting of an expression
/// and an optional output alias.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectionColumn {
    /// The expression to project.
    pub expr: Expr,
    /// Optional alias for the projected column.
    pub alias: Option<String>,
}

/// The type of join operation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum JoinType {
    /// Inner join: rows matching the condition from both sides.
    Inner,
    /// Left outer join: all left rows, matching right rows or NULL.
    LeftOuter,
    /// Right outer join: all right rows, matching left rows or NULL.
    RightOuter,
    /// Full outer join: all rows from both sides, NULLs where unmatched.
    FullOuter,
    /// Cross join: cartesian product of both sides.
    Cross,
    /// Semi join: left rows that have at least one match on the right.
    Semi,
    /// Anti join: left rows that have no match on the right.
    Anti,
}

/// An aggregate function call in a GROUP BY.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AggregateExpr {
    /// The aggregate function (e.g., "COUNT", "SUM", "AVG").
    pub function: AggregateFunction,
    /// The argument expression (e.g., the column to sum).
    pub arg: Option<Expr>,
    /// Whether DISTINCT was specified.
    pub distinct: bool,
    /// Optional output alias.
    pub alias: Option<String>,
}

/// Standard aggregate functions.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum AggregateFunction {
    /// Count rows.
    Count,
    /// Sum values.
    Sum,
    /// Average values.
    Avg,
    /// Minimum value.
    Min,
    /// Maximum value.
    Max,
}

/// A sort key with direction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SortKey {
    /// The expression to sort by.
    pub expr: Expr,
    /// Sort direction.
    pub direction: SortDirection,
    /// How to handle NULLs.
    pub nulls: NullOrdering,
}

/// Sort direction.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum SortDirection {
    /// Ascending order.
    Asc,
    /// Descending order.
    Desc,
}

/// NULL ordering in sorts.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum NullOrdering {
    /// NULLs sort before all other values.
    First,
    /// NULLs sort after all other values.
    Last,
}

impl RelExpr {
    /// Create a table scan.
    #[must_use]
    pub fn scan(table: impl Into<String>) -> Self {
        Self::Scan {
            table: table.into(),
            alias: None,
        }
    }

    /// Create a filter over this expression.
    #[must_use]
    pub fn filter(self, predicate: Expr) -> Self {
        Self::Filter {
            predicate,
            input: Box::new(self),
        }
    }

    /// Create a projection over this expression.
    #[must_use]
    pub fn project(self, columns: Vec<ProjectionColumn>) -> Self {
        Self::Project {
            columns,
            input: Box::new(self),
        }
    }

    /// Create a limit over this expression.
    #[must_use]
    pub fn limit(self, count: u64, offset: u64) -> Self {
        Self::Limit {
            count,
            offset,
            input: Box::new(self),
        }
    }

    /// Return the direct child inputs of this node.
    #[must_use]
    pub fn children(&self) -> Vec<&RelExpr> {
        match self {
            Self::Scan { .. } => vec![],
            Self::Filter { input, .. }
            | Self::Project { input, .. }
            | Self::Aggregate { input, .. }
            | Self::Sort { input, .. }
            | Self::Limit { input, .. } => vec![input],
            Self::Join { left, right, .. }
            | Self::Union { left, right, .. }
            | Self::Intersect { left, right, .. }
            | Self::Except { left, right, .. } => vec![left, right],
        }
    }

    /// Collect all column references used in this expression tree.
    #[must_use]
    pub fn referenced_columns(&self) -> Vec<ColumnRef> {
        let mut cols = Vec::new();
        self.collect_columns(&mut cols);
        cols
    }

    fn collect_columns(&self, out: &mut Vec<ColumnRef>) {
        match self {
            Self::Scan { .. } => {}
            Self::Filter {
                predicate, input, ..
            } => {
                collect_expr_columns(predicate, out);
                input.collect_columns(out);
            }
            Self::Project {
                columns, input, ..
            } => {
                for pc in columns {
                    collect_expr_columns(&pc.expr, out);
                }
                input.collect_columns(out);
            }
            Self::Join {
                condition,
                left,
                right,
                ..
            } => {
                collect_expr_columns(condition, out);
                left.collect_columns(out);
                right.collect_columns(out);
            }
            Self::Aggregate {
                group_by,
                aggregates,
                input,
                ..
            } => {
                for expr in group_by {
                    collect_expr_columns(expr, out);
                }
                for agg in aggregates {
                    if let Some(arg) = &agg.arg {
                        collect_expr_columns(arg, out);
                    }
                }
                input.collect_columns(out);
            }
            Self::Sort { keys, input, .. } => {
                for key in keys {
                    collect_expr_columns(&key.expr, out);
                }
                input.collect_columns(out);
            }
            Self::Limit { input, .. } => {
                input.collect_columns(out);
            }
            Self::Union { left, right, .. }
            | Self::Intersect { left, right, .. }
            | Self::Except { left, right, .. } => {
                left.collect_columns(out);
                right.collect_columns(out);
            }
        }
    }
}

/// Recursively collect column references from a scalar expression.
fn collect_expr_columns(expr: &Expr, out: &mut Vec<ColumnRef>) {
    match expr {
        Expr::Column(col) => out.push(col.clone()),
        Expr::Const(_) => {}
        Expr::BinOp { left, right, .. } => {
            collect_expr_columns(left, out);
            collect_expr_columns(right, out);
        }
        Expr::UnaryOp { operand, .. } => {
            collect_expr_columns(operand, out);
        }
        Expr::Function { args, .. } => {
            for arg in args {
                collect_expr_columns(arg, out);
            }
        }
        Expr::Case {
            operand,
            when_clauses,
            else_result,
        } => {
            if let Some(op) = operand {
                collect_expr_columns(op, out);
            }
            for (cond, result) in when_clauses {
                collect_expr_columns(cond, out);
                collect_expr_columns(result, out);
            }
            if let Some(el) = else_result {
                collect_expr_columns(el, out);
            }
        }
        Expr::Cast { expr, .. } => {
            collect_expr_columns(expr, out);
        }
    }
}

impl std::fmt::Display for JoinType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Inner => "INNER",
            Self::LeftOuter => "LEFT OUTER",
            Self::RightOuter => "RIGHT OUTER",
            Self::FullOuter => "FULL OUTER",
            Self::Cross => "CROSS",
            Self::Semi => "SEMI",
            Self::Anti => "ANTI",
        };
        write!(f, "{name}")
    }
}

impl std::fmt::Display for AggregateFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Count => "COUNT",
            Self::Sum => "SUM",
            Self::Avg => "AVG",
            Self::Min => "MIN",
            Self::Max => "MAX",
        };
        write!(f, "{name}")
    }
}

impl std::fmt::Display for SortDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Asc => write!(f, "ASC"),
            Self::Desc => write!(f, "DESC"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{BinOp as ExprBinOp, Const};

    #[test]
    fn scan_builder() {
        let scan = RelExpr::scan("users");
        if let RelExpr::Scan { table, alias } = &scan {
            assert_eq!(table, "users");
            assert_eq!(*alias, None);
        } else {
            panic!("expected Scan variant");
        }
    }

    #[test]
    fn filter_chain() {
        let plan = RelExpr::scan("orders").filter(Expr::BinOp {
            op: ExprBinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("amount"))),
            right: Box::new(Expr::Const(Const::Int(100))),
        });

        if let RelExpr::Filter { input, .. } = &plan {
            assert!(matches!(input.as_ref(), RelExpr::Scan { .. }));
        } else {
            panic!("expected Filter variant");
        }
    }

    #[test]
    fn children_scan_empty() {
        let scan = RelExpr::scan("t");
        assert!(scan.children().is_empty());
    }

    #[test]
    fn children_filter_one() {
        let plan = RelExpr::scan("t").filter(Expr::Const(Const::Bool(true)));
        assert_eq!(plan.children().len(), 1);
    }

    #[test]
    fn children_join_two() {
        let join = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        assert_eq!(join.children().len(), 2);
    }

    #[test]
    fn referenced_columns_filter() {
        let plan = RelExpr::scan("t").filter(Expr::BinOp {
            op: ExprBinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("x"))),
            right: Box::new(Expr::Const(Const::Int(1))),
        });

        let cols = plan.referenced_columns();
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].column, "x");
    }

    #[test]
    fn join_type_display() {
        assert_eq!(JoinType::Inner.to_string(), "INNER");
        assert_eq!(JoinType::LeftOuter.to_string(), "LEFT OUTER");
        assert_eq!(JoinType::Anti.to_string(), "ANTI");
    }

    #[test]
    fn aggregate_function_display() {
        assert_eq!(AggregateFunction::Count.to_string(), "COUNT");
        assert_eq!(AggregateFunction::Avg.to_string(), "AVG");
    }

    #[test]
    fn serialize_roundtrip() {
        let plan = RelExpr::scan("users").limit(10, 0);
        let json = serde_json::to_string(&plan)
            .expect("serialization should succeed");
        let deserialized: RelExpr = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(plan, deserialized);
    }
}
