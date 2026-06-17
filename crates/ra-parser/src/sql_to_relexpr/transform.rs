//! Post-parse tree transformations.
//!
//! These transformations run after the Lime LALR parser produces a
//! `RelExpr` tree.  They detect patterns that the grammar cannot
//! express directly and rewrite the tree to use the appropriate
//! high-level operators.

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, NullOrdering, ProjectionColumn, RelExpr, SortDirection,
    SortKey, WindowExpr, WindowFunction,
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
    // Normalize sub-queries nested inside expressions first. The per-node
    // transforms below recurse RelExpr children via `map_children`, but a
    // sub-query's inner query lives inside an `Expr` (a Project column,
    // Filter/Join predicate, ...) which `map_children` does not descend into.
    // Running `apply_all` on each inner query ensures sub-queries are
    // normalized identically to top-level queries (e.g. a scalar
    // `(SELECT max(x) ...)` is wrapped in an Aggregate node).
    let rel = normalize_subqueries(rel);
    let rel = transform_window_functions(rel);
    let rel = transform_scalar_aggregates(rel);
    let rel = transform_order_by_aliases(rel);
    transform_vector_search(rel)
}

/// Nearest projection that defines the output columns visible to an
/// `ORDER BY`, looking through passthrough nodes.
fn projection_below(rel: &RelExpr) -> Option<&[ProjectionColumn]> {
    match rel {
        RelExpr::Project { columns, .. } => Some(columns),
        RelExpr::Filter { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Limit { input, .. } => projection_below(input),
        _ => None,
    }
}

/// Lower `ORDER BY <output-alias>` to the underlying projected expression,
/// as `PostgreSQL` does during parse analysis. Without this the sort key
/// references an alias that has no binding once the optimizer folds the
/// projection away, producing a wrong (or no-op) sort.
fn transform_order_by_aliases(rel: RelExpr) -> RelExpr {
    let rel = map_children(rel, transform_order_by_aliases);
    let RelExpr::Sort { mut keys, input } = rel else {
        return rel;
    };
    if let Some(cols) = projection_below(&input) {
        for key in &mut keys {
            let Expr::Column(ColumnRef { table: None, column }) = &key.expr else {
                continue;
            };
            if let Some(pc) = cols
                .iter()
                .find(|c| c.alias.as_deref() == Some(column.as_str()))
            {
                key.expr = pc.expr.clone();
            }
        }
    }
    RelExpr::Sort { keys, input }
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

    // Only Float thresholds are supported for VectorFilter rewrites.
    let threshold = match right.as_ref() {
        Expr::Const(Const::Float(f)) => *f,
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
///
/// The marker function args may contain sentinel entries appended by
/// `ra_window_marker_full` to carry partition and order info:
/// - `__window_partition(exprs...)` — partition-by expressions
/// - `__window_order_asc(expr)` / `__window_order_desc(expr)` — sort keys
fn extract_window_exprs(
    columns: Vec<ProjectionColumn>,
) -> (Vec<WindowExpr>, Vec<ProjectionColumn>) {
    let mut window_exprs = Vec::new();
    let mut clean_cols = Vec::new();

    for col in columns {
        if let Expr::Function { ref name, ref args } = col.expr {
            if name.starts_with("__window_") {
                if let Some(func) = window_function_for(name) {
                    let (real_args, partition_by, order_by, has_frame) =
                        decode_window_sentinels(args.clone());
                    // Most window functions take a single argument. lag/lead/
                    // nth_value also take an offset (and optional default); the
                    // WindowExpr carries one `arg`, so preserve the extra
                    // arguments in a `__win_args(value, offset, ...)` marker
                    // that the plan builder decodes (mirrors __distinct etc.).
                    let arg = match real_args.len() {
                        0 => None,
                        1 => real_args.into_iter().next(),
                        _ => Some(Expr::Function {
                            name: "__win_args".to_string(),
                            args: real_args,
                        }),
                    };
                    window_exprs.push(WindowExpr {
                        function: func,
                        arg,
                        partition_by,
                        order_by,
                        // An explicit (non-default) frame was specified. We do
                        // not build frame semantics directly; recording it as
                        // Some makes the plan-builder defer to native PG.
                        frame: has_frame.then_some(ra_core::algebra::WindowFrame {
                            mode: ra_core::algebra::WindowFrameMode::Range,
                            start: ra_core::algebra::WindowFrameBound::UnboundedPreceding,
                            end: ra_core::algebra::WindowFrameBound::CurrentRow,
                        }),
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

/// Separate real function args from sentinel args encoding window OVER clause.
///
/// Returns `(real_args, partition_by_exprs, order_by_sort_keys)`.
fn decode_window_sentinels(
    args: Vec<Expr>,
) -> (Vec<Expr>, Vec<Expr>, Vec<SortKey>, bool) {
    let mut real_args = Vec::new();
    let mut partition_by = Vec::new();
    let mut order_by = Vec::new();
    let mut has_frame = false;

    for arg in args {
        match &arg {
            Expr::Function { name, args: inner } if name == "__window_partition" => {
                partition_by.extend(inner.iter().cloned());
            }
            Expr::Function { name, args: inner } if name == "__window_order_asc" => {
                if let Some(expr) = inner.first() {
                    order_by.push(SortKey {
                        expr: expr.clone(),
                        direction: SortDirection::Asc,
                        nulls: NullOrdering::Last,
                    });
                }
            }
            Expr::Function { name, args: inner } if name == "__window_order_desc" => {
                if let Some(expr) = inner.first() {
                    order_by.push(SortKey {
                        expr: expr.clone(),
                        direction: SortDirection::Desc,
                        nulls: NullOrdering::Last,
                    });
                }
            }
            Expr::Function { name, .. } if name == "__window_frame" => {
                has_frame = true;
            }
            _ => real_args.push(arg),
        }
    }

    (real_args, partition_by, order_by, has_frame)
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


/// Build an `AggregateExpr` from a function call, returning `None` if it
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

/// Collect every aggregate sub-expression reachable in `expr` (an aggregate's
/// own argument cannot contain another aggregate, so recursion stops once an
/// aggregate is found).
fn collect_aggs(expr: &Expr, out: &mut Vec<AggregateExpr>) {
    if let Some(agg) = make_agg_expr(expr) {
        out.push(agg);
        return;
    }
    // Ordered-set aggregate (`percentile_cont(...) WITHIN GROUP (ORDER BY ...)`,
    // encoded as a function with a `__within_group` marker arg): not in the
    // AggregateFunction enum, but it must still wrap the projection in an
    // Aggregate so the plan builder's ordered-set path runs. The placeholder
    // here is unused by the build (which reads the projection columns).
    if let Expr::Function { args, .. } = expr {
        if args
            .iter()
            .any(|a| matches!(a, Expr::Function { name, .. } if name == "__within_group"))
        {
            out.push(AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: None,
            });
            return;
        }
    }
    match expr {
        Expr::BinOp { left, right, .. } => {
            collect_aggs(left, out);
            collect_aggs(right, out);
        }
        Expr::UnaryOp { operand, .. } => collect_aggs(operand, out),
        Expr::Function { args, .. } | Expr::Array(args) => {
            for a in args {
                collect_aggs(a, out);
            }
        }
        Expr::Case { operand, when_clauses, else_result } => {
            if let Some(o) = operand {
                collect_aggs(o, out);
            }
            for (w, t) in when_clauses {
                collect_aggs(w, out);
                collect_aggs(t, out);
            }
            if let Some(el) = else_result {
                collect_aggs(el, out);
            }
        }
        Expr::Cast { expr, .. } | Expr::FieldAccess { expr, .. } => collect_aggs(expr, out),
        Expr::ArrayIndex(a, b) => {
            collect_aggs(a, out);
            collect_aggs(b, out);
        }
        _ => {}
    }
}

fn wrap_scalar_aggregate(rel: RelExpr) -> RelExpr {
    let RelExpr::Project { ref columns, ref input } = rel else {
        return rel;
    };

    // Don't double-wrap if already aggregated — including a GROUP BY
    // aggregate under a HAVING `Filter` (the projection sits above the
    // HAVING, which sits above the Aggregate).
    let already_aggregated = match input.as_ref() {
        RelExpr::Aggregate { .. } => true,
        RelExpr::Filter { input: fi, .. } => matches!(fi.as_ref(), RelExpr::Aggregate { .. }),
        _ => false,
    };
    if already_aggregated {
        return rel;
    }

    // Collect aggregate expressions reachable anywhere in the projection
    // columns — not just a column that is directly an aggregate, but also
    // aggregates nested in expressions (e.g. `max(id) - min(id)`).
    let mut agg_exprs = Vec::new();
    for col in columns {
        collect_aggs(&col.expr, &mut agg_exprs);
    }

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

/// Apply [`apply_all`] to every sub-query inner query reachable in `rel`'s own
/// expressions, recursing into `RelExpr` children via `map_children`.
fn normalize_subqueries(rel: RelExpr) -> RelExpr {
    let mut rel = map_children(rel, normalize_subqueries);
    match &mut rel {
        RelExpr::Project { columns, .. } => {
            for c in columns {
                normalize_expr_subqueries(&mut c.expr);
            }
        }
        RelExpr::Filter { predicate, .. } => normalize_expr_subqueries(predicate),
        RelExpr::Join { condition, .. } => normalize_expr_subqueries(condition),
        _ => {}
    }
    rel
}

/// Normalize the inner query of every `SubQuery` nested in `e` (in place).
fn normalize_expr_subqueries(e: &mut Expr) {
    match e {
        Expr::SubQuery { query, test_expr, .. } => {
            let inner = std::mem::replace(query.as_mut(), RelExpr::Values { rows: Vec::new() });
            *query.as_mut() = apply_all(inner);
            if let Some(t) = test_expr {
                normalize_expr_subqueries(t);
            }
        }
        Expr::BinOp { left, right, .. } => {
            normalize_expr_subqueries(left);
            normalize_expr_subqueries(right);
        }
        Expr::UnaryOp { operand, .. } => normalize_expr_subqueries(operand),
        Expr::Function { args, .. } | Expr::Array(args) => {
            for a in args {
                normalize_expr_subqueries(a);
            }
        }
        Expr::Case { operand, when_clauses, else_result } => {
            if let Some(o) = operand {
                normalize_expr_subqueries(o);
            }
            for (w, t) in when_clauses {
                normalize_expr_subqueries(w);
                normalize_expr_subqueries(t);
            }
            if let Some(el) = else_result {
                normalize_expr_subqueries(el);
            }
        }
        Expr::Cast { expr, .. } | Expr::FieldAccess { expr, .. } => normalize_expr_subqueries(expr),
        Expr::ArrayIndex(a, b) => {
            normalize_expr_subqueries(a);
            normalize_expr_subqueries(b);
        }
        Expr::ArraySlice { array, start, end } => {
            normalize_expr_subqueries(array);
            if let Some(s) = start {
                normalize_expr_subqueries(s);
            }
            if let Some(en) = end {
                normalize_expr_subqueries(en);
            }
        }
        Expr::VectorDistance { column, target, .. } => {
            normalize_expr_subqueries(column);
            normalize_expr_subqueries(target);
        }
        Expr::PatternPrev(inner, _)
        | Expr::PatternNext(inner, _)
        | Expr::PatternFirst(inner, _)
        | Expr::PatternLast(inner, _) => normalize_expr_subqueries(inner),
        Expr::Column(_)
        | Expr::Const(_)
        | Expr::FullTextMatch { .. }
        | Expr::PatternClassifier
        | Expr::PatternMatchNumber => {}
    }
}

// ---------------------------------------------------------------------------
// Tree-walk helper
// ---------------------------------------------------------------------------

#[expect(clippy::too_many_lines, reason = "exhaustive match over all RelExpr variants")]
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
