#![allow(clippy::doc_markdown)]
//! Relational algebra AST types.
//!
//! This module defines the core relational expression tree that
//! represents query plans. Each variant of [`RelExpr`] corresponds
//! to a standard relational algebra operator.

use serde::{Deserialize, Serialize};

use crate::expr::{ColumnRef, Expr};
use crate::row_pattern::{
    MatchMode, PatternDefine, PatternExpr, PatternMeasure, SkipMode,
};
use crate::search_types::DistanceMetric;

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
    CTE {
        /// The CTE name.
        name: String,
        /// The CTE definition query.
        definition: Box<RelExpr>,
        /// The body query that references the CTE.
        body: Box<RelExpr>,
    },

    /// Window function operator.
    Window {
        /// Window function expressions to compute.
        functions: Vec<WindowExpr>,
        /// The input relation.
        input: Box<RelExpr>,
    },

    /// Deduplicate rows (SELECT DISTINCT).
    Distinct {
        /// The input relation.
        input: Box<RelExpr>,
    },

    /// Inline values (VALUES clause).
    Values {
        /// Rows of constant expressions.
        rows: Vec<Vec<Expr>>,
    },

    /// Recursive CTE with explicit base/recursive separation.
    RecursiveCTE {
        /// CTE name.
        name: String,
        /// Base case (anchor member) - executed once.
        base_case: Box<RelExpr>,
        /// Recursive case (recursive member) - executed iteratively.
        recursive_case: Box<RelExpr>,
        /// Body query using the CTE.
        body: Box<RelExpr>,
        /// Cycle detection configuration.
        cycle_detection: Option<CycleDetection>,
    },

    /// Unnest an array or set expression into rows.
    Unnest {
        /// Expression producing array or set.
        expr: Expr,
        /// Column alias for unnested values.
        alias: Option<String>,
        /// Correlated input relation (for LATERAL unnest).
        input: Option<Box<RelExpr>>,
        /// Whether WITH ORDINALITY was specified.
        with_ordinality: bool,
    },

    /// Multi-argument unnest (parallel unnest of multiple arrays).
    MultiUnnest {
        /// Expressions producing arrays to unnest in parallel.
        exprs: Vec<Expr>,
        /// Column aliases for each unnested array.
        aliases: Vec<Option<String>>,
        /// Whether WITH ORDINALITY was specified.
        with_ordinality: bool,
    },

    /// General table-valued function (generate_series, etc.).
    TableFunction {
        /// Function name.
        name: String,
        /// Function arguments.
        args: Vec<Expr>,
        /// Output column definitions (name, type as string).
        columns: Vec<(String, String)>,
        /// Correlated input relation (for LATERAL).
        input: Option<Box<RelExpr>>,
    },

    /// Row Pattern Recognition (SQL:2016 MATCH_RECOGNIZE).
    RowPattern {
        /// The input relation to match patterns against.
        input: Box<RelExpr>,
        /// PARTITION BY expressions.
        partition_by: Vec<Expr>,
        /// ORDER BY keys (required for pattern matching).
        order_by: Vec<SortKey>,
        /// The regex-like pattern expression.
        pattern: PatternExpr,
        /// Variable definitions (DEFINE clause).
        defines: Vec<PatternDefine>,
        /// Measure computations (MEASURES clause).
        measures: Vec<PatternMeasure>,
        /// Match output mode (ONE ROW / ALL ROWS).
        mode: MatchMode,
        /// Skip strategy after a match.
        skip_mode: SkipMode,
    },

    /// Incremental sort: input is already sorted by `prefix_keys`,
    /// so only sort within each prefix group by `suffix_keys`.
    ///
    /// Cost is O(n * log(m)) where m is average group size, vs
    /// O(n * log(n)) for a full sort. Effective when the prefix
    /// covers high-cardinality columns.
    IncrementalSort {
        /// Keys the input is already sorted by (prefix).
        prefix_keys: Vec<SortKey>,
        /// Additional keys to sort within each prefix group.
        suffix_keys: Vec<SortKey>,
        /// The input relation (already sorted by `prefix_keys`).
        input: Box<RelExpr>,
    },

    /// Index scan: reads from a B-tree index to retrieve rows in
    /// index order. Used for MIN/MAX index optimization where only
    /// the first or last key is needed.
    IndexScan {
        /// Table name.
        table: String,
        /// Column covered by the index (first column in index).
        column: String,
    },

    /// Bitmap index scan: scans an index and produces a bitmap of
    /// matching heap pages. Used as input to BitmapAnd/BitmapOr.
    BitmapIndexScan {
        /// Table name.
        table: String,
        /// Index name to scan.
        index: String,
        /// Predicate to evaluate using the index.
        predicate: Expr,
    },

    /// Bitmap AND: combines multiple bitmap index scans with bitwise
    /// AND to produce a bitmap of pages matching all predicates.
    BitmapAnd {
        /// Input bitmap scans to combine with AND.
        inputs: Vec<Box<RelExpr>>,
    },

    /// Bitmap OR: combines multiple bitmap index scans with bitwise
    /// OR to produce a bitmap of pages matching any predicate.
    BitmapOr {
        /// Input bitmap scans to combine with OR.
        inputs: Vec<Box<RelExpr>>,
    },

    /// Bitmap heap scan: uses a bitmap to fetch matching rows from
    /// the heap in physical page order, minimizing random I/O.
    BitmapHeapScan {
        /// Table name.
        table: String,
        /// Bitmap input (BitmapIndexScan, BitmapAnd, or BitmapOr).
        bitmap: Box<RelExpr>,
        /// Optional condition to recheck on heap tuples.
        recheck_cond: Option<Expr>,
    },

    // ===== Covering Index (Index-Only Scan) =====

    /// Index-only scan: reads all needed columns from a covering
    /// index, avoiding heap fetches entirely.
    IndexOnlyScan {
        /// Table name.
        table: String,
        /// Index name (or "auto" for deferred resolution).
        index: String,
        /// Projected columns.
        columns: Vec<ProjectionColumn>,
        /// Filter predicate evaluated within the index.
        predicate: Expr,
    },

    // ===== Parallel Query Execution Operators =====

    /// Parallel scan: distributes table scan across multiple workers.
    /// Each worker scans a subset of the table pages/blocks.
    ParallelScan {
        /// Table name.
        table: String,
        /// Number of parallel workers.
        workers: usize,
    },

    /// Parallel hash join: distributes join processing across workers.
    /// Build phase creates shared hash table, probe phase is parallelized.
    ParallelHashJoin {
        /// The type of join.
        join_type: JoinType,
        /// The join condition.
        condition: Expr,
        /// The left input relation.
        left: Box<RelExpr>,
        /// The right input relation.
        right: Box<RelExpr>,
        /// Number of parallel workers.
        workers: usize,
    },

    /// Parallel aggregation: distributes GROUP BY across workers.
    /// Uses two-phase aggregation: partial per-worker, then final combine.
    ParallelAggregate {
        /// Grouping key expressions.
        group_by: Vec<Expr>,
        /// Aggregate function calls.
        aggregates: Vec<AggregateExpr>,
        /// The input relation.
        input: Box<RelExpr>,
        /// Number of parallel workers.
        workers: usize,
    },

    /// Gather: collects results from parallel workers.
    /// Merges parallel execution streams back into a single stream.
    Gather {
        /// The parallel input relation.
        input: Box<RelExpr>,
        /// Number of parallel workers to gather from.
        workers: usize,
    },

    // ===== Materialized View Scan =====

    /// Scan a materialized view instead of re-computing from base
    /// tables. The optimizer rewrites eligible sub-trees into this
    /// node when an MV is cheaper than the original plan.
    MvScan {
        /// Materialized view name (the scan target).
        view_name: String,
        /// Optional alias for the MV scan.
        alias: Option<String>,
    },

    // ===== Vector Search Operators =====

    /// Top-K vector similarity search: returns K nearest neighbors.
    /// Used for ORDER BY vector_distance(...) LIMIT k queries.
    TopK {
        /// The vector column or expression to compare.
        vector_expr: Expr,
        /// The query vector to compare against.
        query_vector: Expr,
        /// Distance metric (L2, Cosine, InnerProduct).
        metric: DistanceMetric,
        /// Number of nearest neighbors to return.
        k: u64,
        /// The input relation.
        input: Box<RelExpr>,
    },

    /// Vector filter: filters rows by distance threshold.
    /// Used for WHERE vector_distance(...) < threshold queries.
    VectorFilter {
        /// The vector column or expression to compare.
        vector_expr: Expr,
        /// The query vector to compare against.
        query_vector: Expr,
        /// Distance metric (L2, Cosine, InnerProduct).
        metric: DistanceMetric,
        /// Distance threshold for filtering.
        threshold: f64,
        /// The input relation.
        input: Box<RelExpr>,
    },
}

/// Configuration for cycle detection in recursive CTEs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CycleDetection {
    /// Columns to track for cycles.
    pub track_columns: Vec<String>,
    /// Maximum recursion depth (prevents infinite loops).
    pub max_depth: Option<u32>,
    /// Cycle mark column name (SQL standard: CYCLE clause).
    pub cycle_mark_column: Option<String>,
    /// Path tracking column (optional).
    pub path_column: Option<String>,
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
    /// Standard deviation.
    StdDev,
    /// Variance.
    Variance,
    /// String aggregation.
    StringAgg,
    /// Array aggregation.
    ArrayAgg,
}

/// A window function expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowExpr {
    /// The window function to apply.
    pub function: WindowFunction,
    /// Argument expression (e.g., the column to aggregate).
    pub arg: Option<Expr>,
    /// PARTITION BY expressions.
    pub partition_by: Vec<Expr>,
    /// ORDER BY within the window.
    pub order_by: Vec<SortKey>,
    /// Window frame specification.
    pub frame: Option<WindowFrame>,
    /// Output alias.
    pub alias: Option<String>,
}

/// Window function types.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum WindowFunction {
    /// Aggregate: AVG.
    Avg,
    /// Aggregate: SUM.
    Sum,
    /// Aggregate: COUNT.
    Count,
    /// Aggregate: MIN.
    Min,
    /// Aggregate: MAX.
    Max,
    /// Ranking: ROW_NUMBER.
    RowNumber,
    /// Ranking: RANK.
    Rank,
    /// Ranking: DENSE_RANK.
    DenseRank,
    /// Ranking: PERCENT_RANK.
    PercentRank,
    /// Ranking: NTILE.
    Ntile,
    /// Value: LAG.
    Lag,
    /// Value: LEAD.
    Lead,
    /// Value: FIRST_VALUE.
    FirstValue,
    /// Value: LAST_VALUE.
    LastValue,
    /// Value: NTH_VALUE.
    NthValue,
}

/// Window frame specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowFrame {
    /// Frame mode (ROWS, RANGE, GROUPS).
    pub mode: WindowFrameMode,
    /// Start bound.
    pub start: WindowFrameBound,
    /// End bound.
    pub end: WindowFrameBound,
}

/// Window frame mode.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum WindowFrameMode {
    /// Frame defined by row positions.
    Rows,
    /// Frame defined by value ranges.
    Range,
    /// Frame defined by peer groups.
    Groups,
}

/// Window frame bound.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WindowFrameBound {
    /// UNBOUNDED PRECEDING.
    UnboundedPreceding,
    /// N PRECEDING.
    Preceding(u64),
    /// CURRENT ROW.
    CurrentRow,
    /// N FOLLOWING.
    Following(u64),
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

    /// Wrap this expression in a Distinct node.
    #[must_use]
    pub fn distinct(self) -> Self {
        Self::Distinct {
            input: Box::new(self),
        }
    }

    /// Create a standalone unnest operator.
    #[must_use]
    pub fn unnest(expr: Expr, alias: Option<String>) -> Self {
        Self::Unnest {
            expr,
            alias,
            input: None,
            with_ordinality: false,
        }
    }

    /// Create a correlated (lateral) unnest operator.
    #[must_use]
    pub fn unnest_lateral(
        expr: Expr,
        input: RelExpr,
        alias: Option<String>,
    ) -> Self {
        Self::Unnest {
            expr,
            alias,
            input: Some(Box::new(input)),
            with_ordinality: false,
        }
    }

    /// Create a table-valued function operator.
    #[must_use]
    pub fn table_function(
        name: impl Into<String>,
        args: Vec<Expr>,
        columns: Vec<(String, String)>,
    ) -> Self {
        Self::TableFunction {
            name: name.into(),
            args,
            columns,
            input: None,
        }
    }

    /// Return the direct child inputs of this node.
    #[must_use]
    pub fn children(&self) -> Vec<&RelExpr> {
        match self {
            Self::Scan { .. }
            | Self::Values { .. }
            | Self::MultiUnnest { .. }
            | Self::IndexScan { .. }
            | Self::BitmapIndexScan { .. }
            | Self::ParallelScan { .. }
            | Self::IndexOnlyScan { .. }
            | Self::MvScan { .. } => vec![],
            Self::Filter { input, .. }
            | Self::Project { input, .. }
            | Self::Aggregate { input, .. }
            | Self::Sort { input, .. }
            | Self::IncrementalSort { input, .. }
            | Self::Limit { input, .. }
            | Self::Window { input, .. }
            | Self::Distinct { input, .. }
            | Self::RowPattern { input, .. }
            | Self::ParallelAggregate { input, .. }
            | Self::Gather { input, .. } => vec![input],
            Self::Join { left, right, .. }
            | Self::Union { left, right, .. }
            | Self::Intersect { left, right, .. }
            | Self::Except { left, right, .. }
            | Self::ParallelHashJoin { left, right, .. } => vec![left, right],
            Self::CTE {
                definition, body, ..
            } => vec![definition, body],
            Self::RecursiveCTE {
                base_case,
                recursive_case,
                body,
                ..
            } => vec![base_case, recursive_case, body],
            Self::Unnest { input, .. } | Self::TableFunction { input, .. } => match input {
                Some(inp) => vec![inp],
                None => vec![],
            },
            Self::BitmapAnd { inputs } | Self::BitmapOr { inputs } => {
                inputs.iter().map(std::convert::AsRef::as_ref).collect()
            }
            Self::BitmapHeapScan { bitmap, .. } => vec![bitmap],
            Self::TopK { input, .. } | Self::VectorFilter { input, .. } => vec![input],
        }
    }

    /// Collect all column references used in this expression tree.
    #[must_use]
    pub fn referenced_columns(&self) -> Vec<ColumnRef> {
        let mut cols = Vec::new();
        self.collect_columns(&mut cols);
        cols
    }

    #[expect(clippy::too_many_lines)]
    fn collect_columns(&self, out: &mut Vec<ColumnRef>) {
        match self {
            Self::Scan { .. } | Self::IndexScan { .. } | Self::ParallelScan { .. } | Self::MvScan { .. } => {}
            Self::IndexOnlyScan { predicate, .. } | Self::BitmapIndexScan { predicate, .. } => {
                collect_expr_columns(predicate, out);
            }
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
            }
            | Self::ParallelHashJoin {
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
            }
            | Self::ParallelAggregate {
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
            Self::IncrementalSort {
                prefix_keys,
                suffix_keys,
                input,
            } => {
                for key in prefix_keys {
                    collect_expr_columns(&key.expr, out);
                }
                for key in suffix_keys {
                    collect_expr_columns(&key.expr, out);
                }
                input.collect_columns(out);
            }
            Self::Limit { input, .. }
            | Self::Distinct { input, .. }
            | Self::Gather { input, .. } => {
                input.collect_columns(out);
            }
            Self::Window {
                functions, input, ..
            } => {
                for wf in functions {
                    if let Some(arg) = &wf.arg {
                        collect_expr_columns(arg, out);
                    }
                    for expr in &wf.partition_by {
                        collect_expr_columns(expr, out);
                    }
                    for key in &wf.order_by {
                        collect_expr_columns(&key.expr, out);
                    }
                }
                input.collect_columns(out);
            }
            Self::CTE {
                definition, body, ..
            } => {
                definition.collect_columns(out);
                body.collect_columns(out);
            }
            Self::RecursiveCTE {
                base_case,
                recursive_case,
                body,
                ..
            } => {
                base_case.collect_columns(out);
                recursive_case.collect_columns(out);
                body.collect_columns(out);
            }
            Self::Union { left, right, .. }
            | Self::Intersect { left, right, .. }
            | Self::Except { left, right, .. } => {
                left.collect_columns(out);
                right.collect_columns(out);
            }
            Self::Unnest { expr, input, .. } => {
                collect_expr_columns(expr, out);
                if let Some(inp) = input {
                    inp.collect_columns(out);
                }
            }
            Self::MultiUnnest { exprs, .. } => {
                for expr in exprs {
                    collect_expr_columns(expr, out);
                }
            }
            Self::TableFunction {
                args, input, ..
            } => {
                for arg in args {
                    collect_expr_columns(arg, out);
                }
                if let Some(inp) = input {
                    inp.collect_columns(out);
                }
            }
            Self::RowPattern {
                input,
                partition_by,
                order_by,
                defines,
                measures,
                ..
            } => {
                for expr in partition_by {
                    collect_expr_columns(expr, out);
                }
                for key in order_by {
                    collect_expr_columns(&key.expr, out);
                }
                for define in defines {
                    collect_expr_columns(&define.condition, out);
                }
                for measure in measures {
                    collect_expr_columns(&measure.expr, out);
                }
                input.collect_columns(out);
            }
            Self::BitmapAnd { inputs } | Self::BitmapOr { inputs } => {
                for bitmap in inputs {
                    bitmap.collect_columns(out);
                }
            }
            Self::BitmapHeapScan {
                bitmap,
                recheck_cond,
                ..
            } => {
                bitmap.collect_columns(out);
                if let Some(cond) = recheck_cond {
                    collect_expr_columns(cond, out);
                }
            }
            Self::TopK {
                input,
                vector_expr,
                query_vector,
                ..
            } => {
                collect_expr_columns(vector_expr, out);
                collect_expr_columns(query_vector, out);
                input.collect_columns(out);
            }
            Self::VectorFilter {
                input,
                vector_expr,
                query_vector,
                ..
            } => {
                collect_expr_columns(vector_expr, out);
                collect_expr_columns(query_vector, out);
                input.collect_columns(out);
            }
        }
    }
}

impl RelExpr {
    /// Check whether this expression tree references a CTE by name.
    #[must_use]
    pub fn references_cte(&self, cte_name: &str) -> bool {
        match self {
            Self::Scan { table, .. }
            | Self::IndexScan { table, .. }
            | Self::IndexOnlyScan { table, .. }
            | Self::BitmapIndexScan { table, .. }
            | Self::ParallelScan { table, .. }
            | Self::MvScan { view_name: table, .. } => table == cte_name,
            Self::Filter { input, .. }
            | Self::Project { input, .. }
            | Self::Aggregate { input, .. }
            | Self::Sort { input, .. }
            | Self::Limit { input, .. }
            | Self::Window { input, .. }
            | Self::Distinct { input, .. }
            | Self::IncrementalSort { input, .. }
            | Self::RowPattern { input, .. }
            | Self::ParallelAggregate { input, .. }
            | Self::Gather { input, .. } => {
                input.references_cte(cte_name)
            }
            Self::Join { left, right, .. }
            | Self::Union { left, right, .. }
            | Self::Intersect { left, right, .. }
            | Self::Except { left, right, .. }
            | Self::ParallelHashJoin { left, right, .. } => {
                left.references_cte(cte_name)
                    || right.references_cte(cte_name)
            }
            Self::CTE {
                definition, body, ..
            } => {
                definition.references_cte(cte_name)
                    || body.references_cte(cte_name)
            }
            Self::RecursiveCTE {
                base_case,
                recursive_case,
                body,
                ..
            } => {
                base_case.references_cte(cte_name)
                    || recursive_case.references_cte(cte_name)
                    || body.references_cte(cte_name)
            }
            Self::Values { .. }
            | Self::MultiUnnest { .. } => false,
            Self::Unnest { input, .. }
            | Self::TableFunction { input, .. } => {
                input
                    .as_ref()
                    .is_some_and(|i| i.references_cte(cte_name))
            }
            Self::BitmapAnd { inputs } | Self::BitmapOr { inputs} => {
                inputs.iter().any(|b| b.references_cte(cte_name))
            }
            Self::BitmapHeapScan { bitmap, table, .. } => {
                table == cte_name || bitmap.references_cte(cte_name)
            }
            Self::TopK { input, .. } | Self::VectorFilter { input, .. } => {
                input.references_cte(cte_name)
            }
        }
    }
}

/// Recursively collect column references from a scalar expression.
fn collect_expr_columns(expr: &Expr, out: &mut Vec<ColumnRef>) {
    match expr {
        Expr::Column(col) => out.push(col.clone()),
        Expr::Const(_) | Expr::PatternClassifier | Expr::PatternMatchNumber => {}
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
        Expr::Cast { expr, .. } | Expr::FieldAccess { expr, .. } => {
            collect_expr_columns(expr, out);
        }
        Expr::Array(elements) => {
            for elem in elements {
                collect_expr_columns(elem, out);
            }
        }
        Expr::ArrayIndex(array, index) => {
            collect_expr_columns(array, out);
            collect_expr_columns(index, out);
        }
        Expr::PatternPrev(inner, _)
        | Expr::PatternNext(inner, _)
        | Expr::PatternFirst(inner, _)
        | Expr::PatternLast(inner, _) => {
            collect_expr_columns(inner, out);
        }
        Expr::ArraySlice {
            array, start, end,
        } => {
            collect_expr_columns(array, out);
            if let Some(s) = start {
                collect_expr_columns(s, out);
            }
            if let Some(e) = end {
                collect_expr_columns(e, out);
            }
        }
        Expr::SubQuery { test_expr, .. } => {
            // Subquery columns are not considered part of outer expression
            // Only collect from the test expression if present
            if let Some(test) = test_expr {
                collect_expr_columns(test, out);
            }
        }
        Expr::FullTextMatch { columns, .. } => {
            for col in columns {
                out.push(ColumnRef {
                    table: None,
                    column: col.clone(),
                });
            }
        }
        Expr::VectorDistance { column, target, .. } => {
            collect_expr_columns(column, out);
            collect_expr_columns(target, out);
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
            Self::StdDev => "STDDEV",
            Self::Variance => "VARIANCE",
            Self::StringAgg => "STRING_AGG",
            Self::ArrayAgg => "ARRAY_AGG",
        };
        write!(f, "{name}")
    }
}

impl std::fmt::Display for WindowFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Avg => "AVG",
            Self::Sum => "SUM",
            Self::Count => "COUNT",
            Self::Min => "MIN",
            Self::Max => "MAX",
            Self::RowNumber => "ROW_NUMBER",
            Self::Rank => "RANK",
            Self::DenseRank => "DENSE_RANK",
            Self::PercentRank => "PERCENT_RANK",
            Self::Ntile => "NTILE",
            Self::Lag => "LAG",
            Self::Lead => "LEAD",
            Self::FirstValue => "FIRST_VALUE",
            Self::LastValue => "LAST_VALUE",
            Self::NthValue => "NTH_VALUE",
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
    #[expect(clippy::panic, reason = "test code uses panic for assertions")]
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
    #[expect(clippy::panic, reason = "test code uses panic for assertions")]
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
        assert_eq!(AggregateFunction::StdDev.to_string(), "STDDEV");
        assert_eq!(AggregateFunction::ArrayAgg.to_string(), "ARRAY_AGG");
    }

    #[test]
    fn window_function_display() {
        assert_eq!(WindowFunction::RowNumber.to_string(), "ROW_NUMBER");
        assert_eq!(WindowFunction::Rank.to_string(), "RANK");
        assert_eq!(WindowFunction::DenseRank.to_string(), "DENSE_RANK");
        assert_eq!(WindowFunction::Lead.to_string(), "LEAD");
        assert_eq!(WindowFunction::FirstValue.to_string(), "FIRST_VALUE");
    }

    #[test]
    #[expect(clippy::panic, reason = "test code uses panic for assertions")]
    fn distinct_builder() {
        let plan = RelExpr::scan("users").distinct();
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
        let cte = RelExpr::CTE {
            name: "temp".to_owned(),
            definition: Box::new(RelExpr::scan("source")),
            body: Box::new(RelExpr::scan("temp")),
        };
        assert_eq!(cte.children().len(), 2);
    }

    #[test]
    fn children_values_empty() {
        let vals = RelExpr::Values { rows: vec![] };
        assert!(vals.children().is_empty());
    }

    #[test]
    fn children_window_one() {
        let win = RelExpr::Window {
            functions: vec![],
            input: Box::new(RelExpr::scan("t")),
        };
        assert_eq!(win.children().len(), 1);
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
    fn children_recursive_cte_three() {
        let rcte = RelExpr::RecursiveCTE {
            name: "reachable".to_owned(),
            base_case: Box::new(RelExpr::scan("edges")),
            recursive_case: Box::new(RelExpr::scan("edges")),
            body: Box::new(RelExpr::scan("reachable")),
            cycle_detection: None,
        };
        assert_eq!(rcte.children().len(), 3);
    }

    #[test]
    fn references_cte_finds_scan() {
        let plan = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("edges")),
            right: Box::new(RelExpr::scan("reachable")),
        };
        assert!(plan.references_cte("reachable"));
        assert!(!plan.references_cte("missing"));
    }

    #[test]
    fn recursive_cte_serialize_roundtrip() {
        let rcte = RelExpr::RecursiveCTE {
            name: "r".to_owned(),
            base_case: Box::new(RelExpr::scan("t")),
            recursive_case: Box::new(RelExpr::scan("t")),
            body: Box::new(RelExpr::scan("r")),
            cycle_detection: Some(CycleDetection {
                track_columns: vec!["id".to_owned()],
                max_depth: Some(100),
                cycle_mark_column: None,
                path_column: None,
            }),
        };
        let json = serde_json::to_string(&rcte)
            .expect("serialization should succeed");
        let deserialized: RelExpr = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(rcte, deserialized);
    }

    #[test]
    fn recursive_cte_referenced_columns() {
        let rcte = RelExpr::RecursiveCTE {
            name: "r".to_owned(),
            base_case: Box::new(
                RelExpr::scan("t").filter(Expr::BinOp {
                    op: ExprBinOp::Eq,
                    left: Box::new(Expr::Column(ColumnRef::new("x"))),
                    right: Box::new(Expr::Const(Const::Int(1))),
                }),
            ),
            recursive_case: Box::new(RelExpr::scan("t")),
            body: Box::new(RelExpr::scan("r")),
            cycle_detection: None,
        };
        let cols = rcte.referenced_columns();
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].column, "x");
    }

    // -- Additional children() coverage --

    #[test]
    fn children_project_one() {
        let plan = RelExpr::Project {
            columns: vec![],
            input: Box::new(RelExpr::scan("t")),
        };
        assert_eq!(plan.children().len(), 1);
    }

    #[test]
    fn children_aggregate_one() {
        let plan = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![],
            input: Box::new(RelExpr::scan("t")),
        };
        assert_eq!(plan.children().len(), 1);
    }

    #[test]
    fn children_sort_one() {
        let plan = RelExpr::Sort {
            keys: vec![],
            input: Box::new(RelExpr::scan("t")),
        };
        assert_eq!(plan.children().len(), 1);
    }

    #[test]
    fn children_limit_one() {
        let plan = RelExpr::scan("t").limit(10, 0);
        assert_eq!(plan.children().len(), 1);
    }

    #[test]
    fn children_union_two() {
        let plan = RelExpr::Union {
            all: true,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        assert_eq!(plan.children().len(), 2);
    }

    #[test]
    fn children_intersect_two() {
        let plan = RelExpr::Intersect {
            all: false,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        assert_eq!(plan.children().len(), 2);
    }

    #[test]
    fn children_except_two() {
        let plan = RelExpr::Except {
            all: false,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        assert_eq!(plan.children().len(), 2);
    }

    #[test]
    fn children_index_scan_empty() {
        let plan = RelExpr::IndexScan {
            table: "t".into(),
            column: "id".into(),
        };
        assert!(plan.children().is_empty());
    }

    #[test]
    fn children_unnest_with_input() {
        let plan = RelExpr::Unnest {
            expr: Expr::Column(ColumnRef::new("arr")),
            alias: None,
            input: Some(Box::new(RelExpr::scan("t"))),
            with_ordinality: false,
        };
        assert_eq!(plan.children().len(), 1);
    }

    #[test]
    fn children_unnest_without_input() {
        let plan = RelExpr::Unnest {
            expr: Expr::Column(ColumnRef::new("arr")),
            alias: None,
            input: None,
            with_ordinality: false,
        };
        assert!(plan.children().is_empty());
    }

    #[test]
    fn children_mv_scan_empty() {
        let plan = RelExpr::MvScan {
            view_name: "mv".into(),
            alias: None,
        };
        assert!(plan.children().is_empty());
    }

    // -- Additional referenced_columns coverage --

    #[test]
    fn referenced_columns_scan_empty() {
        let cols = RelExpr::scan("t").referenced_columns();
        assert!(cols.is_empty());
    }

    #[test]
    fn referenced_columns_join_condition() {
        let plan = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: ExprBinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("x"))),
                right: Box::new(Expr::Column(ColumnRef::new("y"))),
            },
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let cols = plan.referenced_columns();
        assert_eq!(cols.len(), 2);
    }

    #[test]
    fn referenced_columns_values() {
        let plan = RelExpr::Values {
            rows: vec![vec![
                Expr::Column(ColumnRef::new("c1")),
                Expr::Const(Const::Int(1)),
            ]],
        };
        let cols = plan.referenced_columns();
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].column, "c1");
    }

    // -- Additional references_cte coverage --

    #[test]
    fn references_cte_scan_match() {
        assert!(RelExpr::scan("my_cte").references_cte("my_cte"));
    }

    #[test]
    fn references_cte_scan_no_match() {
        assert!(!RelExpr::scan("other").references_cte("my_cte"));
    }

    #[test]
    fn references_cte_through_filter() {
        let plan = RelExpr::scan("cte1")
            .filter(Expr::Const(Const::Bool(true)));
        assert!(plan.references_cte("cte1"));
    }

    #[test]
    fn references_cte_through_union() {
        let plan = RelExpr::Union {
            all: true,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("cte1")),
        };
        assert!(plan.references_cte("cte1"));
        assert!(!plan.references_cte("missing"));
    }

    // -- Additional builder tests --

    #[test]
    #[expect(clippy::panic, reason = "test code uses panic for assertions")]
    fn scan_creates_scan_node() {
        let plan = RelExpr::scan("users");
        if let RelExpr::Scan { table, alias } = &plan {
            assert_eq!(table, "users");
            assert!(alias.is_none());
        } else {
            panic!("expected Scan");
        }
    }

    #[test]
    #[expect(clippy::panic, reason = "test code uses panic for assertions")]
    fn filter_chains_correctly() {
        let plan = RelExpr::scan("t")
            .filter(Expr::Const(Const::Bool(true)))
            .filter(Expr::Const(Const::Bool(false)));
        if let RelExpr::Filter { input, .. } = &plan {
            assert!(matches!(input.as_ref(), RelExpr::Filter { .. }));
        } else {
            panic!("expected nested Filter");
        }
    }

    #[test]
    #[expect(clippy::panic, reason = "test code uses panic for assertions")]
    fn limit_builder_values() {
        let plan = RelExpr::scan("t").limit(5, 10);
        if let RelExpr::Limit { count, offset, .. } = &plan {
            assert_eq!(*count, 5);
            assert_eq!(*offset, 10);
        } else {
            panic!("expected Limit");
        }
    }

    #[test]
    fn join_type_display_all_variants() {
        assert_eq!(JoinType::Inner.to_string(), "INNER");
        assert_eq!(JoinType::LeftOuter.to_string(), "LEFT OUTER");
        assert_eq!(JoinType::RightOuter.to_string(), "RIGHT OUTER");
        assert_eq!(JoinType::FullOuter.to_string(), "FULL OUTER");
        assert_eq!(JoinType::Cross.to_string(), "CROSS");
        assert_eq!(JoinType::Semi.to_string(), "SEMI");
        assert_eq!(JoinType::Anti.to_string(), "ANTI");
    }

    #[test]
    fn sort_direction_display_variants() {
        assert_eq!(SortDirection::Asc.to_string(), "ASC");
        assert_eq!(SortDirection::Desc.to_string(), "DESC");
    }
}
