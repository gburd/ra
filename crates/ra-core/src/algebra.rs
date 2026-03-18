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

    /// Common Table Expression (WITH clause).
    Cte {
        /// The CTE name.
        name: String,
        /// The CTE definition.
        definition: Box<RelExpr>,
        /// The body that references the CTE.
        body: Box<RelExpr>,
    },

    /// Window function application.
    Window {
        /// Window function expressions to compute.
        functions: Vec<WindowExpr>,
        /// The input relation.
        input: Box<RelExpr>,
    },

    /// Remove duplicate rows.
    Distinct {
        /// The input relation.
        input: Box<RelExpr>,
    },

    /// Inline row values (e.g., VALUES (1, 'a'), (2, 'b')).
    Values {
        /// Each inner Vec is one row of expressions.
        rows: Vec<Vec<Expr>>,
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
    /// Population standard deviation.
    StddevPop,
    /// Sample standard deviation.
    StddevSamp,
    /// Population variance.
    VariancePop,
    /// Sample variance.
    VarianceSamp,
    /// Concatenate strings with a separator.
    StringAgg,
    /// Collect values into an array.
    ArrayAgg,
    /// The statistical mode (most frequent value).
    Mode,
    /// Boolean AND of all values.
    BoolAnd,
    /// Boolean OR of all values.
    BoolOr,
}

/// A window function expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowExpr {
    /// The window function to apply.
    pub function: WindowFunction,
    /// PARTITION BY expressions.
    pub partition_by: Vec<Expr>,
    /// ORDER BY within the window.
    pub order_by: Vec<SortKey>,
    /// The window frame specification.
    pub frame: Option<WindowFrame>,
    /// Optional output alias.
    pub alias: Option<String>,
}

/// Window function types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WindowFunction {
    /// An aggregate used as a window function.
    Aggregate(AggregateExpr),
    /// ROW_NUMBER().
    RowNumber,
    /// RANK().
    Rank,
    /// DENSE_RANK().
    DenseRank,
    /// PERCENT_RANK().
    PercentRank,
    /// CUME_DIST().
    CumeDist,
    /// NTILE(n).
    Ntile {
        /// Number of buckets.
        n: Expr,
    },
    /// LAG(expr, offset, default).
    Lag {
        /// The expression to retrieve.
        expr: Expr,
        /// Offset rows back (default 1).
        offset: Option<Expr>,
        /// Default value if offset goes past partition boundary.
        default: Option<Expr>,
    },
    /// LEAD(expr, offset, default).
    Lead {
        /// The expression to retrieve.
        expr: Expr,
        /// Offset rows forward (default 1).
        offset: Option<Expr>,
        /// Default value if offset goes past partition boundary.
        default: Option<Expr>,
    },
    /// FIRST_VALUE(expr).
    FirstValue {
        /// The expression to retrieve.
        expr: Expr,
    },
    /// LAST_VALUE(expr).
    LastValue {
        /// The expression to retrieve.
        expr: Expr,
    },
    /// NTH_VALUE(expr, n).
    NthValue {
        /// The expression to retrieve.
        expr: Expr,
        /// The 1-based position.
        n: Expr,
    },
}

/// Window frame specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowFrame {
    /// Whether the frame uses ROWS or RANGE.
    pub mode: WindowFrameMode,
    /// The start bound.
    pub start: WindowFrameBound,
    /// The end bound.
    pub end: WindowFrameBound,
}

/// Frame mode: ROWS counts physical rows, RANGE uses value ranges.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum WindowFrameMode {
    /// Frame defined by physical row offsets.
    Rows,
    /// Frame defined by value ranges.
    Range,
    /// Frame defined by groups of peer rows.
    Groups,
}

/// A bound in a window frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WindowFrameBound {
    /// CURRENT ROW.
    CurrentRow,
    /// N PRECEDING.
    Preceding(Option<u64>),
    /// N FOLLOWING.
    Following(Option<u64>),
    /// UNBOUNDED PRECEDING.
    UnboundedPreceding,
    /// UNBOUNDED FOLLOWING.
    UnboundedFollowing,
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

    /// Create a distinct over this expression.
    #[must_use]
    pub fn distinct(self) -> Self {
        Self::Distinct {
            input: Box::new(self),
        }
    }

    /// Create a sort over this expression.
    #[must_use]
    pub fn sort(self, keys: Vec<SortKey>) -> Self {
        Self::Sort {
            keys,
            input: Box::new(self),
        }
    }

    /// Return the direct child inputs of this node.
    #[must_use]
    pub fn children(&self) -> Vec<&RelExpr> {
        match self {
            Self::Scan { .. } | Self::Values { .. } => vec![],
            Self::Filter { input, .. }
            | Self::Project { input, .. }
            | Self::Aggregate { input, .. }
            | Self::Sort { input, .. }
            | Self::Limit { input, .. }
            | Self::Window { input, .. }
            | Self::Distinct { input, .. } => vec![input],
            Self::Join { left, right, .. }
            | Self::Union { left, right, .. }
            | Self::Intersect { left, right, .. }
            | Self::Except { left, right, .. } => vec![left, right],
            Self::Cte {
                definition, body, ..
            } => vec![definition, body],
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
            Self::Values { rows } => {
                for row in rows {
                    for expr in row {
                        collect_expr_columns(expr, out);
                    }
                }
            }
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
            Self::Limit { input, .. }
            | Self::Distinct { input, .. } => {
                input.collect_columns(out);
            }
            Self::Union { left, right, .. }
            | Self::Intersect { left, right, .. }
            | Self::Except { left, right, .. } => {
                left.collect_columns(out);
                right.collect_columns(out);
            }
            Self::Cte {
                definition, body, ..
            } => {
                definition.collect_columns(out);
                body.collect_columns(out);
            }
            Self::Window {
                functions, input, ..
            } => {
                for wf in functions {
                    collect_window_expr_columns(wf, out);
                }
                input.collect_columns(out);
            }
        }
    }
}

/// Collect column references from a window expression.
fn collect_window_expr_columns(
    wf: &WindowExpr,
    out: &mut Vec<ColumnRef>,
) {
    for expr in &wf.partition_by {
        collect_expr_columns(expr, out);
    }
    for key in &wf.order_by {
        collect_expr_columns(&key.expr, out);
    }
    match &wf.function {
        WindowFunction::Aggregate(agg) => {
            if let Some(arg) = &agg.arg {
                collect_expr_columns(arg, out);
            }
        }
        WindowFunction::Lag { expr, offset, default }
        | WindowFunction::Lead { expr, offset, default } => {
            collect_expr_columns(expr, out);
            if let Some(o) = offset {
                collect_expr_columns(o, out);
            }
            if let Some(d) = default {
                collect_expr_columns(d, out);
            }
        }
        WindowFunction::FirstValue { expr }
        | WindowFunction::LastValue { expr } => {
            collect_expr_columns(expr, out);
        }
        WindowFunction::NthValue { expr, n } => {
            collect_expr_columns(expr, out);
            collect_expr_columns(n, out);
        }
        WindowFunction::Ntile { n } => {
            collect_expr_columns(n, out);
        }
        WindowFunction::RowNumber
        | WindowFunction::Rank
        | WindowFunction::DenseRank
        | WindowFunction::PercentRank
        | WindowFunction::CumeDist => {}
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
            Self::StddevPop => "STDDEV_POP",
            Self::StddevSamp => "STDDEV_SAMP",
            Self::VariancePop => "VAR_POP",
            Self::VarianceSamp => "VAR_SAMP",
            Self::StringAgg => "STRING_AGG",
            Self::ArrayAgg => "ARRAY_AGG",
            Self::Mode => "MODE",
            Self::BoolAnd => "BOOL_AND",
            Self::BoolOr => "BOOL_OR",
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

impl std::fmt::Display for WindowFrameMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rows => write!(f, "ROWS"),
            Self::Range => write!(f, "RANGE"),
            Self::Groups => write!(f, "GROUPS"),
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
        let plan =
            RelExpr::scan("t").filter(Expr::Const(Const::Bool(true)));
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
    fn extended_aggregate_function_display() {
        assert_eq!(
            AggregateFunction::StddevPop.to_string(),
            "STDDEV_POP"
        );
        assert_eq!(
            AggregateFunction::StringAgg.to_string(),
            "STRING_AGG"
        );
        assert_eq!(
            AggregateFunction::ArrayAgg.to_string(),
            "ARRAY_AGG"
        );
        assert_eq!(AggregateFunction::BoolAnd.to_string(), "BOOL_AND");
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

    #[test]
    fn distinct_builder() {
        let plan = RelExpr::scan("t").distinct();
        if let RelExpr::Distinct { input } = &plan {
            assert!(matches!(input.as_ref(), RelExpr::Scan { .. }));
        } else {
            panic!("expected Distinct variant");
        }
    }

    #[test]
    fn children_distinct_one() {
        let plan = RelExpr::scan("t").distinct();
        assert_eq!(plan.children().len(), 1);
    }

    #[test]
    fn children_cte_two() {
        let cte = RelExpr::Cte {
            name: "sub".into(),
            definition: Box::new(RelExpr::scan("orders")),
            body: Box::new(RelExpr::scan("sub")),
        };
        assert_eq!(cte.children().len(), 2);
    }

    #[test]
    fn children_values_empty() {
        let vals = RelExpr::Values {
            rows: vec![vec![Expr::Const(Const::Int(1))]],
        };
        assert!(vals.children().is_empty());
    }

    #[test]
    fn children_window_one() {
        let win = RelExpr::Window {
            functions: vec![WindowExpr {
                function: WindowFunction::RowNumber,
                partition_by: vec![],
                order_by: vec![],
                frame: None,
                alias: Some("rn".into()),
            }],
            input: Box::new(RelExpr::scan("t")),
        };
        assert_eq!(win.children().len(), 1);
    }

    #[test]
    fn referenced_columns_window() {
        let win = RelExpr::Window {
            functions: vec![WindowExpr {
                function: WindowFunction::RowNumber,
                partition_by: vec![Expr::Column(ColumnRef::new("dept"))],
                order_by: vec![SortKey {
                    expr: Expr::Column(ColumnRef::new("salary")),
                    direction: SortDirection::Desc,
                    nulls: NullOrdering::Last,
                }],
                frame: None,
                alias: None,
            }],
            input: Box::new(RelExpr::scan("employees")),
        };
        let cols = win.referenced_columns();
        assert_eq!(cols.len(), 2);
    }

    #[test]
    fn cte_referenced_columns() {
        let cte = RelExpr::Cte {
            name: "recent".into(),
            definition: Box::new(
                RelExpr::scan("orders").filter(Expr::BinOp {
                    op: ExprBinOp::Gt,
                    left: Box::new(Expr::Column(ColumnRef::new("dt"))),
                    right: Box::new(Expr::Const(Const::Int(2024))),
                }),
            ),
            body: Box::new(RelExpr::scan("recent")),
        };
        let cols = cte.referenced_columns();
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].column, "dt");
    }

    #[test]
    fn window_frame_mode_display() {
        assert_eq!(WindowFrameMode::Rows.to_string(), "ROWS");
        assert_eq!(WindowFrameMode::Range.to_string(), "RANGE");
        assert_eq!(WindowFrameMode::Groups.to_string(), "GROUPS");
    }

    #[test]
    fn serialize_roundtrip_cte() {
        let plan = RelExpr::Cte {
            name: "x".into(),
            definition: Box::new(RelExpr::scan("t")),
            body: Box::new(RelExpr::scan("x")),
        };
        let json = serde_json::to_string(&plan)
            .expect("serialization should succeed");
        let deserialized: RelExpr = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(plan, deserialized);
    }

    #[test]
    fn serialize_roundtrip_distinct() {
        let plan = RelExpr::scan("t").distinct();
        let json = serde_json::to_string(&plan)
            .expect("serialization should succeed");
        let deserialized: RelExpr = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(plan, deserialized);
    }

    #[test]
    fn serialize_roundtrip_window() {
        let plan = RelExpr::Window {
            functions: vec![WindowExpr {
                function: WindowFunction::Rank,
                partition_by: vec![],
                order_by: vec![],
                frame: Some(WindowFrame {
                    mode: WindowFrameMode::Rows,
                    start: WindowFrameBound::UnboundedPreceding,
                    end: WindowFrameBound::CurrentRow,
                }),
                alias: None,
            }],
            input: Box::new(RelExpr::scan("t")),
        };
        let json = serde_json::to_string(&plan)
            .expect("serialization should succeed");
        let deserialized: RelExpr = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(plan, deserialized);
    }

    #[test]
    fn sort_builder() {
        let plan = RelExpr::scan("t").sort(vec![SortKey {
            expr: Expr::Column(ColumnRef::new("id")),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }]);
        if let RelExpr::Sort { keys, .. } = &plan {
            assert_eq!(keys.len(), 1);
        } else {
            panic!("expected Sort variant");
        }
    }

    #[test]
    fn values_columns() {
        let vals = RelExpr::Values {
            rows: vec![
                vec![
                    Expr::Column(ColumnRef::new("a")),
                    Expr::Const(Const::Int(1)),
                ],
                vec![
                    Expr::Column(ColumnRef::new("b")),
                    Expr::Const(Const::Int(2)),
                ],
            ],
        };
        let cols = vals.referenced_columns();
        assert_eq!(cols.len(), 2);
    }
}
