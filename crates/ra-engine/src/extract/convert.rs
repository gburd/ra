use egg::Id;
use ra_core::algebra::RelExpr;
use ra_core::expr::{BinOp, Expr};

use crate::egraph::{EGraphError, RelLang};

use super::helpers::{
    convert_bool_flag, convert_join_type, convert_projection_list, convert_sort_key_list,
    convert_window_expr_list, get_symbol, id,
};
use super::scalar::convert_scalar;

/// Extract a combined filter predicate from a lowered bitmap source.
fn bitmap_source_predicate(b: &RelExpr) -> Option<Expr> {
    match b {
        RelExpr::Filter { predicate, .. } => Some(predicate.clone()),
        RelExpr::BitmapAnd { inputs } => combine_bitmap_preds(inputs, BinOp::And),
        RelExpr::BitmapOr { inputs } => combine_bitmap_preds(inputs, BinOp::Or),
        _ => None,
    }
}

fn combine_bitmap_preds(inputs: &[Box<RelExpr>], op: BinOp) -> Option<Expr> {
    let mut acc: Option<Expr> = None;
    for inp in inputs {
        let p = bitmap_source_predicate(inp)?;
        acc = Some(match acc {
            None => p,
            Some(prev) => Expr::BinOp { op, left: Box::new(prev), right: Box::new(p) },
        });
    }
    acc
}

/// Convert a [`RecExpr`] back to a [`RelExpr`].
///
/// # Errors
///
/// Returns an error if the `RecExpr` contains unsupported nodes.
pub fn rec_expr_to_rel_expr(rec: &egg::RecExpr<RelLang>) -> Result<RelExpr, EGraphError> {
    let nodes = rec.as_ref();
    if nodes.is_empty() {
        return Err(EGraphError::ExtractionError("empty RecExpr".into()));
    }
    convert_node(nodes, nodes.len() - 1)
}

#[expect(clippy::too_many_lines, reason = "match over all RelLang variants")]
pub(crate) fn convert_node(nodes: &[RelLang], idx: usize) -> Result<RelExpr, EGraphError> {
    match &nodes[idx] {
        RelLang::Scan([table_id]) => {
            let table = get_symbol(nodes, id(*table_id))?;
            Ok(RelExpr::Scan { table, alias: None })
        }
        RelLang::ScanAlias([table_id, alias_id]) => {
            let table = get_symbol(nodes, id(*table_id))?;
            let alias = get_symbol(nodes, id(*alias_id))?;
            Ok(RelExpr::Scan {
                table,
                alias: Some(alias),
            })
        }
        RelLang::Filter([pred_id, input_id]) => Ok(RelExpr::Filter {
            predicate: convert_scalar(nodes, id(*pred_id))?,
            input: Box::new(convert_node(nodes, id(*input_id))?),
        }),
        RelLang::Project([cols_id, input_id]) => Ok(RelExpr::Project {
            columns: convert_projection_list(nodes, id(*cols_id))?,
            input: Box::new(convert_node(nodes, id(*input_id))?),
        }),
        RelLang::Join(ids)
        // Physical join operators introduced during saturation extract back to
        // a logical Join; plan_builder re-selects the physical method. Mirrors
        // `from_rec`. Without these, extracting a plan whose best node is a
        // physical join fails ("unexpected relational node") and the query
        // falls back (observed on cte-multi and lateral).
        | RelLang::HashJoinOp(ids)
        | RelLang::MergeJoinOp(ids)
        | RelLang::NestLoopOp(ids)
        | RelLang::IndexNestLoopOp(ids) => convert_join(nodes, ids),
        RelLang::Aggregate(ids) => convert_aggregate(nodes, ids),
        RelLang::Sort([keys_id, input_id]) => Ok(RelExpr::Sort {
            keys: convert_sort_key_list(nodes, id(*keys_id))?,
            input: Box::new(convert_node(nodes, id(*input_id))?),
        }),
        RelLang::IncrementalSort([prefix_keys_id, suffix_keys_id, input_id]) => {
            Ok(RelExpr::IncrementalSort {
                prefix_keys: convert_sort_key_list(nodes, id(*prefix_keys_id))?,
                suffix_keys: convert_sort_key_list(nodes, id(*suffix_keys_id))?,
                input: Box::new(convert_node(nodes, id(*input_id))?),
            })
        }
        RelLang::Limit(ids) => convert_limit(nodes, ids),
        RelLang::Union([all_id, left_id, right_id]) => {
            convert_set_op(nodes, *all_id, *left_id, *right_id, "union")
        }
        RelLang::Intersect([all_id, left_id, right_id]) => {
            convert_set_op(nodes, *all_id, *left_id, *right_id, "intersect")
        }
        RelLang::Except([all_id, left_id, right_id]) => {
            convert_set_op(nodes, *all_id, *left_id, *right_id, "except")
        }
        RelLang::RecursiveCTE([name_id, base_id, rec_id, body_id]) => {
            let name = get_symbol(nodes, id(*name_id))?;
            let base_case = convert_node(nodes, id(*base_id))?;
            let recursive_case = convert_node(nodes, id(*rec_id))?;
            let body = convert_node(nodes, id(*body_id))?;
            Ok(RelExpr::RecursiveCTE {
                name,
                base_case: Box::new(base_case),
                recursive_case: Box::new(recursive_case),
                body: Box::new(body),
                cycle_detection: Some(ra_core::algebra::CycleDetection {
                    track_columns: vec![],
                    max_depth: Some(1000),
                    cycle_mark_column: None,
                    path_column: None,
                }),
            })
        }
        RelLang::CTE([name_id, def_id, body_id]) => {
            let name = get_symbol(nodes, id(*name_id))?;
            let definition = convert_node(nodes, id(*def_id))?;
            let body = convert_node(nodes, id(*body_id))?;
            Ok(RelExpr::CTE {
                name,
                definition: Box::new(definition),
                body: Box::new(body),
            })
        }
        RelLang::Window([fns_id, input_id]) => {
            let functions = convert_window_expr_list(nodes, id(*fns_id))?;
            let input = convert_node(nodes, id(*input_id))?;
            Ok(RelExpr::Window {
                functions,
                input: Box::new(input),
            })
        }
        RelLang::DistinctRel([input_id]) => {
            let input = convert_node(nodes, id(*input_id))?;
            Ok(RelExpr::Distinct {
                input: Box::new(input),
            })
        }
        RelLang::Values(row_ids) => {
            let mut rows = Vec::with_capacity(row_ids.len());
            for &row_id in row_ids {
                rows.push(convert_values_row(nodes, id(row_id))?);
            }
            Ok(RelExpr::Values { rows })
        }
        RelLang::MetadataLookup([table_id, _kind_id]) => {
            let table = get_symbol(nodes, id(*table_id))?;
            // Represent as a single-row aggregate over the table scan,
            // preserving the semantic that execution resolves the count
            // from cached metadata rather than scanning.
            Ok(RelExpr::Aggregate {
                group_by: vec![],
                aggregates: vec![ra_core::algebra::AggregateExpr {
                    function: ra_core::algebra::AggregateFunction::Count,
                    arg: None,
                    distinct: false,
                    alias: Some("count".to_string()),
                }],
                input: Box::new(RelExpr::Scan { table, alias: None }),
            })
        }
        RelLang::IndexScan([table_id, _column_id]) => {
            let table = get_symbol(nodes, id(*table_id))?;
            Ok(RelExpr::Scan { table, alias: None })
        }
        RelLang::IndexOnlyScan([table_id, index_id, cols_id, pred_id]) => {
            // Preserve the physical IndexOnlyScan so plan_builder can build a
            // faithful index-only scan (it falls back internally to a regular
            // index/seq scan when the index does not cover the query).
            let table = get_symbol(nodes, id(*table_id))?;
            let index = get_symbol(nodes, id(*index_id))?;
            let columns = convert_projection_list(nodes, id(*cols_id))?;
            let predicate = convert_scalar(nodes, id(*pred_id))?;
            Ok(RelExpr::IndexOnlyScan { table, index, columns, predicate })
        }
        RelLang::MvScan([view_id, alias_id, _, _]) => {
            let view_name = get_symbol(nodes, id(*view_id))?;
            let alias_str = get_symbol(nodes, id(*alias_id))?;
            let alias = if alias_str == "auto" || alias_str.is_empty() {
                None
            } else {
                Some(alias_str)
            };
            Ok(RelExpr::MvScan { view_name, alias })
        }
        RelLang::BitmapIndexScan([table_id, _index_id, pred_id]) => {
            let table = get_symbol(nodes, id(*table_id))?;
            let predicate = convert_scalar(nodes, id(*pred_id))?;
            Ok(RelExpr::Filter {
                predicate,
                input: Box::new(RelExpr::Scan { table, alias: None }),
            })
        }
        RelLang::BitmapAnd(input_ids) => {
            let mut inputs = Vec::with_capacity(input_ids.len());
            for &input_id in input_ids {
                inputs.push(Box::new(convert_node(nodes, id(input_id))?));
            }
            Ok(RelExpr::BitmapAnd { inputs })
        }
        RelLang::BitmapOr(input_ids) => {
            let mut inputs = Vec::with_capacity(input_ids.len());
            for &input_id in input_ids {
                inputs.push(Box::new(convert_node(nodes, id(input_id))?));
            }
            Ok(RelExpr::BitmapOr { inputs })
        }
        RelLang::BitmapHeapScan([table_id, bitmap_id, recheck_id]) => {
            let table = get_symbol(nodes, id(*table_id))?;
            let bitmap = convert_node(nodes, id(*bitmap_id))?;
            let recheck_cond = if get_symbol(nodes, id(*recheck_id)).is_ok_and(|s| s.is_empty()) {
                None
            } else {
                Some(convert_scalar(nodes, id(*recheck_id))?)
            };
            let predicate = recheck_cond.or_else(|| bitmap_source_predicate(&bitmap));
            match predicate {
                Some(predicate) => Ok(RelExpr::Filter {
                    predicate,
                    input: Box::new(RelExpr::Scan { table, alias: None }),
                }),
                None => Err(EGraphError::ConversionError(
                    "bitmap-heap-scan without a recoverable predicate".to_string(),
                )),
            }
        }
        RelLang::Func(ids) if !ids.is_empty() => convert_func_as_relational(nodes, ids),
        RelLang::VectorKNN([table_id, _col_id, _target_id, _k_id]) => {
            // Extract as annotated scan for now
            let table = get_symbol(nodes, id(*table_id))?;
            Ok(RelExpr::Scan {
                table,
                alias: Some("vector_knn_scan".to_string()),
            })
        }
        RelLang::VectorRangeScan([table_id, _col_id, _target_id, _threshold_id, _metric_id]) => {
            let table = get_symbol(nodes, id(*table_id))?;
            Ok(RelExpr::Scan {
                table,
                alias: Some("vector_range_scan".to_string()),
            })
        }
        RelLang::FtsIndexScan([table_id, _idx_id, _match_id]) => {
            let table = get_symbol(nodes, id(*table_id))?;
            Ok(RelExpr::Scan {
                table,
                alias: Some("fts_index_scan".to_string()),
            })
        }
        RelLang::FtsRankedScan([table_id, _idx_id, _query_id, _k_id, _algo_id]) => {
            let table = get_symbol(nodes, id(*table_id))?;
            Ok(RelExpr::Scan {
                table,
                alias: Some("fts_ranked_scan".to_string()),
            })
        }
        RelLang::FtsSkipListAnd([table_id, _match1_id, _match2_id]) => {
            let table = get_symbol(nodes, id(*table_id))?;
            Ok(RelExpr::Scan {
                table,
                alias: Some("fts_skip_list_and".to_string()),
            })
        }
        RelLang::HybridScan(_ids) => Ok(RelExpr::Scan {
            table: "hybrid_scan".to_string(),
            alias: Some("hybrid_scan".to_string()),
        }),
        other => Err(EGraphError::ExtractionError(format!(
            "unexpected relational node: {other:?}"
        ))),
    }
}

fn convert_join(
    nodes: &[RelLang],
    [jt_id, cond_id, left_id, right_id]: &[Id; 4],
) -> Result<RelExpr, EGraphError> {
    Ok(RelExpr::Join {
        join_type: convert_join_type(nodes, id(*jt_id))?,
        condition: convert_scalar(nodes, id(*cond_id))?,
        left: Box::new(convert_node(nodes, id(*left_id))?),
        right: Box::new(convert_node(nodes, id(*right_id))?),
    })
}

fn convert_aggregate(
    nodes: &[RelLang],
    [groups_id, aggs_id, input_id]: &[Id; 3],
) -> Result<RelExpr, EGraphError> {
    use super::helpers::convert_aggregate_list;

    Ok(RelExpr::Aggregate {
        group_by: super::helpers::convert_scalar_list(nodes, id(*groups_id))?,
        aggregates: convert_aggregate_list(nodes, id(*aggs_id))?,
        input: Box::new(convert_node(nodes, id(*input_id))?),
    })
}

fn convert_limit(
    nodes: &[RelLang],
    [count_id, offset_id, input_id]: &[Id; 3],
) -> Result<RelExpr, EGraphError> {
    let count = get_symbol(nodes, id(*count_id))?
        .parse::<u64>()
        .map_err(|e| EGraphError::ExtractionError(format!("invalid limit count: {e}")))?;
    let offset = get_symbol(nodes, id(*offset_id))?
        .parse::<u64>()
        .map_err(|e| EGraphError::ExtractionError(format!("invalid limit offset: {e}")))?;
    Ok(RelExpr::Limit {
        count,
        offset,
        input: Box::new(convert_node(nodes, id(*input_id))?),
    })
}

fn convert_set_op(
    nodes: &[RelLang],
    all_id: Id,
    left_id: Id,
    right_id: Id,
    kind: &str,
) -> Result<RelExpr, EGraphError> {
    let all = convert_bool_flag(nodes, id(all_id))?;
    let left = Box::new(convert_node(nodes, id(left_id))?);
    let right = Box::new(convert_node(nodes, id(right_id))?);
    match kind {
        "union" => Ok(RelExpr::Union { all, left, right }),
        "intersect" => Ok(RelExpr::Intersect { all, left, right }),
        "except" => Ok(RelExpr::Except { all, left, right }),
        _ => Err(EGraphError::ExtractionError(format!(
            "unknown set operation: {kind}"
        ))),
    }
}

/// Convert a `Func` e-graph node back to a relational operator.
///
/// Unnest and `TableFunction` are encoded as `Func` nodes in the
/// e-graph with a tag symbol as the first child. This function
/// dispatches on the tag to reconstruct the original `RelExpr`.
fn convert_func_as_relational(nodes: &[RelLang], ids: &[Id]) -> Result<RelExpr, EGraphError> {
    let tag = get_symbol(nodes, id(ids[0]))?;

    match tag.as_str() {
        "unnest" => {
            // [tag, expr, alias, ordinality]
            if ids.len() < 4 {
                return Err(EGraphError::ExtractionError(
                    "unnest Func node requires 4 children".into(),
                ));
            }
            let expr = convert_scalar(nodes, id(ids[1]))?;
            let alias_str = get_symbol(nodes, id(ids[2]))?;
            let alias = if alias_str.is_empty() {
                None
            } else {
                Some(alias_str)
            };
            let with_ordinality = convert_bool_flag(nodes, id(ids[3]))?;
            Ok(RelExpr::Unnest {
                expr,
                alias,
                input: None,
                with_ordinality,
            })
        }
        "unnest_lateral" => {
            // [tag, expr, alias, ordinality, input]
            if ids.len() < 5 {
                return Err(EGraphError::ExtractionError(
                    "unnest_lateral Func node requires 5 children".into(),
                ));
            }
            let expr = convert_scalar(nodes, id(ids[1]))?;
            let alias_str = get_symbol(nodes, id(ids[2]))?;
            let alias = if alias_str.is_empty() {
                None
            } else {
                Some(alias_str)
            };
            let with_ordinality = convert_bool_flag(nodes, id(ids[3]))?;
            let input = convert_node(nodes, id(ids[4]))?;
            Ok(RelExpr::Unnest {
                expr,
                alias,
                input: Some(Box::new(input)),
                with_ordinality,
            })
        }
        "multi_unnest" => {
            // [tag, ordinality, expr0, alias0, expr1, alias1, ...]
            if ids.len() < 4 {
                return Err(EGraphError::ExtractionError(
                    "multi_unnest requires at least 4 children".into(),
                ));
            }
            let with_ordinality = convert_bool_flag(nodes, id(ids[1]))?;
            let mut exprs = Vec::new();
            let mut aliases = Vec::new();
            let mut i = 2;
            while i + 1 < ids.len() {
                exprs.push(convert_scalar(nodes, id(ids[i]))?);
                let alias_str = get_symbol(nodes, id(ids[i + 1]))?;
                aliases.push(if alias_str.is_empty() {
                    None
                } else {
                    Some(alias_str)
                });
                i += 2;
            }
            Ok(RelExpr::MultiUnnest {
                exprs,
                aliases,
                with_ordinality,
            })
        }
        _ => {
            // General table function: [name, arg0, arg1, ..., optional_input]
            let name = tag;
            let mut args = Vec::new();
            for &arg_id in &ids[1..] {
                // Try to convert as scalar; if that fails,
                // the remaining child may be a relational input.
                if let Ok(expr) = convert_scalar(nodes, id(arg_id)) {
                    args.push(expr);
                } else {
                    // Assume it's a relational input
                    let input = convert_node(nodes, id(arg_id))?;
                    return Ok(RelExpr::TableFunction {
                        name,
                        args,
                        columns: vec![],
                        input: Some(Box::new(input)),
                    });
                }
            }
            Ok(RelExpr::TableFunction {
                name,
                args,
                columns: vec![],
                input: None,
            })
        }
    }
}

fn convert_values_row(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Vec<ra_core::expr::Expr>, EGraphError> {
    let RelLang::ValuesRow(ids) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(format!(
            "expected ValuesRow, got {:?}",
            nodes[idx]
        )));
    };
    ids.iter()
        .map(|&child| convert_scalar(nodes, id(child)))
        .collect()
}
