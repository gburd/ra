//! Post-parse tree transformations.
//!
//! These transformations run after the Lime LALR parser produces a
//! `RelExpr` tree.  They detect patterns that the grammar cannot
//! express directly and rewrite the tree to use the appropriate
//! high-level operators.

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, ProjectionColumn, RelExpr, WindowExpr,
    WindowFunction,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::search_types::DistanceMetric;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Apply all post-parse transformations to `rel` and return the
/// transformed tree.
#[must_use]
pub fn apply_all(rel: RelExpr) -> RelExpr {
    let rel = transform_window_functions(rel);
    let rel = transform_scalar_aggregates(rel);
    transform_vector_search(rel)
}

// ---------------------------------------------------------------------------
// Vector search: TopK / VectorFilter
// ---------------------------------------------------------------------------

/// Map a distance function name to a `DistanceMetric`.
fn distance_metric_for(name: &str) -> Option<DistanceMetric> {
    match name.to_ascii_lowercase().as_str() {
        "vec_distance_l2" | "l2_distance" | "euclidean_distance" => {
            Some(DistanceMetric::L2)
        }
        "vec_distance_cosine" | "cosine_distance" | "cosine_similarity" => {
            Some(DistanceMetric::Cosine)
        }
        "vec_distance_ip" | "inner_product" | "dot_product" => {
            Some(DistanceMetric::InnerProduct)
        }
        _ => None,
    }
}

/// If `expr` is a call to a distance function, return
/// `Some((metric, vector_expr_arg, query_vector_arg))`.
fn extract_distance_call(expr: &Expr) -> Option<(DistanceMetric, Expr, Expr)> {
    let Expr::Function { name, args } = expr else {
        return None;
    };
    let metric = distance_metric_for(name)?;
    if args.len() < 2 {
        return None;
    }
    Some((metric, args[0].clone(), args[1].clone()))
}

/// Attempt to rewrite a `Limit(Sort(...))` into a `TopK` node.
fn try_topk(rel: RelExpr) -> RelExpr {
    // Decompose the Limit.
    let RelExpr::Limit { count, offset, input } = rel else {
        return rel;
    };
    if offset != 0 {
        return RelExpr::Limit { count, offset, input };
    }

    // Decompose the Sort inside.
    let (keys, sort_input) = match *input {
        RelExpr::Sort { keys, input } => (keys, input),
        other => {
            return RelExpr::Limit {
                count,
                offset,
                input: Box::new(other),
            }
        }
    };

    // Only rewrite if there is exactly one sort key that is a distance fn.
    if keys.len() != 1 {
        return RelExpr::Limit {
            count,
            offset,
            input: Box::new(RelExpr::Sort { keys, input: sort_input }),
        };
    }

    if let Some((metric, vector_expr, query_vector)) =
        extract_distance_call(&keys[0].expr)
    {
        RelExpr::TopK {
            vector_expr,
            query_vector,
            metric,
            k: count,
            input: sort_input,
        }
    } else {
        RelExpr::Limit {
            count,
            offset,
            input: Box::new(RelExpr::Sort { keys, input: sort_input }),
        }
    }
}

/// Attempt to rewrite `Filter(distance_fn < threshold)` into `VectorFilter`.
fn try_vector_filter(rel: RelExpr) -> RelExpr {
    let RelExpr::Filter { ref predicate, ref input } = rel else {
        return rel;
    };

    let Expr::BinOp {
        op: BinOp::Lt,
        ref left,
        ref right,
    } = *predicate
    else {
        return rel;
    };

    let Some((metric, vector_expr, query_vector)) = extract_distance_call(left)
    else {
        return rel;
    };

    let threshold = match right.as_ref() {
        Expr::Const(Const::Float(f)) => *f,
        Expr::Const(Const::Int(i)) => *i as f64,
        _ => return rel,
    };

    let input_cloned = input.clone();

    RelExpr::VectorFilter {
        vector_expr,
        query_vector,
        metric,
        threshold,
        input: input_cloned,
    }
}

/// Bubble a `VectorFilter` up through a `Project { SELECT * }` wrapper.
///
/// `SELECT * FROM t WHERE distance_fn < thr` generates
/// `Project { [*], input: VectorFilter }` after `try_vector_filter`.
/// The tests expect `VectorFilter` at the top level.
fn promote_vector_filter(rel: RelExpr) -> RelExpr {
    let RelExpr::Project { ref columns, ref input } = rel else {
        return rel;
    };
    let is_star_select = columns.len() == 1
        && matches!(
            &columns[0].expr,
            Expr::Column(ra_core::expr::ColumnRef { column, .. }) if column == "*"
        );
    if !is_star_select {
        return rel;
    }
    if matches!(input.as_ref(), RelExpr::VectorFilter { .. }) {
        return *input.clone();
    }
    rel
}

fn transform_vector_search(rel: RelExpr) -> RelExpr {
    let rel = map_children(rel, transform_vector_search);
    let rel = try_topk(rel);
    let rel = try_vector_filter(rel);
    promote_vector_filter(rel)
}

// ---------------------------------------------------------------------------
// Window functions
// ---------------------------------------------------------------------------

fn window_function_for(marker_name: &str) -> Option<WindowFunction> {
    let base = marker_name
        .strip_prefix("__window_")
        .unwrap_or(marker_name)
        .to_ascii_lowercase();
    match base.as_str() {
        "row_number" | "rownumber" => Some(WindowFunction::RowNumber),
        "rank" => Some(WindowFunction::Rank),
        "dense_rank" | "denserank" => Some(WindowFunction::DenseRank),
        "percent_rank" | "percentrank" => Some(WindowFunction::PercentRank),
        "ntile" => Some(WindowFunction::Ntile),
        "lag" => Some(WindowFunction::Lag),
        "lead" => Some(WindowFunction::Lead),
        "first_value" | "firstvalue" => Some(WindowFunction::FirstValue),
        "last_value" | "lastvalue" => Some(WindowFunction::LastValue),
        "nth_value" | "nthvalue" => Some(WindowFunction::NthValue),
        "sum" => Some(WindowFunction::Sum),
        "avg" | "average" => Some(WindowFunction::Avg),
        "count" => Some(WindowFunction::Count),
        "min" => Some(WindowFunction::Min),
        "max" => Some(WindowFunction::Max),
        _ => None,
    }
}

/// Extract `__window_*` marker functions from projection columns.
///
/// Returns `(window_exprs, cleaned_columns)`.
fn extract_window_exprs(
    columns: Vec<ProjectionColumn>,
) -> (Vec<WindowExpr>, Vec<ProjectionColumn>) {
    let mut window_exprs = Vec::new();
    let mut clean_cols = Vec::new();

    for col in columns {
        if let Expr::Function { ref name, ref args } = col.expr {
            if name.starts_with("__window_") {
                if let Some(func) = window_function_for(name) {
                    let arg = args.first().cloned();
                    window_exprs.push(WindowExpr {
                        function: func,
                        arg,
                        partition_by: vec![],
                        order_by: vec![],
                        frame: None,
                        alias: col.alias.clone(),
                    });
                    clean_cols.push(col);
                    continue;
                }
            }
        }
        clean_cols.push(col);
    }

    (window_exprs, clean_cols)
}

fn promote_window_in_project(rel: RelExpr) -> RelExpr {
    let RelExpr::Project { columns, input } = rel else {
        return rel;
    };
    let (window_exprs, cols) = extract_window_exprs(columns);
    if window_exprs.is_empty() {
        return RelExpr::Project {
            columns: cols,
            input,
        };
    }
    let project = RelExpr::Project {
        columns: cols,
        input,
    };
    RelExpr::Window {
        functions: window_exprs,
        input: Box::new(project),
    }
}

fn transform_window_functions(rel: RelExpr) -> RelExpr {
    let rel = map_children(rel, transform_window_functions);
    promote_window_in_project(rel)
}

// ---------------------------------------------------------------------------
// Scalar aggregates (no GROUP BY)
// ---------------------------------------------------------------------------

fn aggregate_function_for(name: &str) -> Option<AggregateFunction> {
    match name.to_ascii_lowercase().as_str() {
        "count" => Some(AggregateFunction::Count),
        "sum" => Some(AggregateFunction::Sum),
        "avg" | "average" => Some(AggregateFunction::Avg),
        "min" => Some(AggregateFunction::Min),
        "max" => Some(AggregateFunction::Max),
        "stddev" | "std_dev" | "stdev" | "stddev_pop" | "stddev_samp" => {
            Some(AggregateFunction::StdDev)
        }
        "variance" | "var_pop" | "var_samp" | "var" => {
            Some(AggregateFunction::Variance)
        }
        "string_agg" => Some(AggregateFunction::StringAgg),
        "array_agg" => Some(AggregateFunction::ArrayAgg),
        _ => None,
    }
}


/// Build an AggregateExpr from a function call, returning `None` if it
/// is not a recognised aggregate.
fn make_agg_expr(expr: &Expr) -> Option<AggregateExpr> {
    let Expr::Function { name, args } = expr else {
        return None;
    };
    let func = aggregate_function_for(name)?;
    // COUNT(*) — the argument is the column "*" sentinel.
    let is_star = matches!(
        args.first(),
        Some(Expr::Column(ColumnRef { column, .. })) if column == "*"
    );
    let arg = if is_star || args.is_empty() {
        None
    } else {
        args.first().cloned()
    };
    Some(AggregateExpr {
        function: func,
        arg,
        distinct: false,
        alias: None,
    })
}

fn wrap_scalar_aggregate(rel: RelExpr) -> RelExpr {
    let RelExpr::Project { ref columns, ref input } = rel else {
        return rel;
    };

    // Don't double-wrap if already aggregated.
    if matches!(input.as_ref(), RelExpr::Aggregate { .. }) {
        return rel;
    }

    // Collect aggregate expressions.
    let agg_exprs: Vec<AggregateExpr> = columns
        .iter()
        .filter_map(|col| make_agg_expr(&col.expr))
        .collect();

    if agg_exprs.is_empty() {
        return rel;
    }

    let RelExpr::Project { columns, input } = rel else {
        unreachable!();
    };

    let aggregate = RelExpr::Aggregate {
        aggregates: agg_exprs,
        group_by: vec![],
        input,
    };

    RelExpr::Project {
        columns,
        input: Box::new(aggregate),
    }
}

fn transform_scalar_aggregates(rel: RelExpr) -> RelExpr {
    let rel = map_children(rel, transform_scalar_aggregates);
    wrap_scalar_aggregate(rel)
}

// ---------------------------------------------------------------------------
// Tree-walk helper
// ---------------------------------------------------------------------------

fn map_children<F>(rel: RelExpr, f: F) -> RelExpr
where
    F: Fn(RelExpr) -> RelExpr,
{
    match rel {
        // Leaf nodes — no children to transform.
        RelExpr::Scan { .. }
        | RelExpr::Values { .. }
        | RelExpr::Unnest { .. }
        | RelExpr::TableFunction { .. } => rel,

        RelExpr::Filter { predicate, input } => RelExpr::Filter {
            predicate,
            input: Box::new(f(*input)),
        },

        RelExpr::Project { columns, input } => RelExpr::Project {
            columns,
            input: Box::new(f(*input)),
        },

        RelExpr::Aggregate {
            aggregates,
            group_by,
            input,
        } => RelExpr::Aggregate {
            aggregates,
            group_by,
            input: Box::new(f(*input)),
        },

        RelExpr::Sort { keys, input } => RelExpr::Sort {
            keys,
            input: Box::new(f(*input)),
        },

        RelExpr::Limit { count, offset, input } => RelExpr::Limit {
            count,
            offset,
            input: Box::new(f(*input)),
        },

        RelExpr::Distinct { input } => RelExpr::Distinct {
            input: Box::new(f(*input)),
        },

        RelExpr::Window { functions, input } => RelExpr::Window {
            functions,
            input: Box::new(f(*input)),
        },

        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => RelExpr::Join {
            join_type,
            condition,
            left: Box::new(f(*left)),
            right: Box::new(f(*right)),
        },

        RelExpr::Union { left, right, all } => RelExpr::Union {
            left: Box::new(f(*left)),
            right: Box::new(f(*right)),
            all,
        },

        RelExpr::Intersect { left, right, all } => RelExpr::Intersect {
            left: Box::new(f(*left)),
            right: Box::new(f(*right)),
            all,
        },

        RelExpr::Except { left, right, all } => RelExpr::Except {
            left: Box::new(f(*left)),
            right: Box::new(f(*right)),
            all,
        },

        RelExpr::CTE {
            name,
            definition,
            body,
        } => RelExpr::CTE {
            name,
            definition: Box::new(f(*definition)),
            body: Box::new(f(*body)),
        },

        RelExpr::RecursiveCTE {
            name,
            base_case,
            recursive_case,
            body,
            cycle_detection,
        } => RelExpr::RecursiveCTE {
            name,
            base_case: Box::new(f(*base_case)),
            recursive_case: Box::new(f(*recursive_case)),
            body: Box::new(f(*body)),
            cycle_detection,
        },

        RelExpr::TopK {
            vector_expr,
            query_vector,
            metric,
            k,
            input,
        } => RelExpr::TopK {
            vector_expr,
            query_vector,
            metric,
            k,
            input: Box::new(f(*input)),
        },

        RelExpr::VectorFilter {
            vector_expr,
            query_vector,
            metric,
            threshold,
            input,
        } => RelExpr::VectorFilter {
            vector_expr,
            query_vector,
            metric,
            threshold,
            input: Box::new(f(*input)),
        },

        // Catch-all for any future variants: pass through unchanged.
        other => other,
    }
}
