use egg::{Id, RecExpr};
use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering, OnConflict, ProjectionColumn,
    RelExpr, SortDirection, SortKey, WindowExpr, WindowFrame, WindowFrameBound, WindowFrameMode,
};
use ra_core::expr::{BinOp, Const, Expr, UnaryOp};

use super::errors::EGraphError;
use super::lang::RelLang;

/// Convert a [`RelExpr`] into an egg [`RecExpr`].
///
/// # Errors
///
/// Returns an error if the expression contains unsupported constructs.
pub fn to_rec_expr(expr: &RelExpr) -> Result<RecExpr<RelLang>, EGraphError> {
    let mut rec = RecExpr::default();
    add_rel_expr(&mut rec, expr)?;
    Ok(rec)
}

#[expect(clippy::too_many_lines, reason = "match over all RelExpr variants")]
fn add_rel_expr(rec: &mut RecExpr<RelLang>, expr: &RelExpr) -> Result<Id, EGraphError> {
    match expr {
        RelExpr::Scan { table, alias } => {
            let table_id = add_symbol(rec, table);
            if let Some(alias_name) = alias {
                let alias_id = add_symbol(rec, alias_name);
                Ok(rec.add(RelLang::ScanAlias([table_id, alias_id])))
            } else {
                Ok(rec.add(RelLang::Scan([table_id])))
            }
        }
        RelExpr::Filter { predicate, input } => {
            let pred_id = add_scalar_expr(rec, predicate)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Filter([pred_id, input_id])))
        }
        RelExpr::Project { columns, input } => {
            let cols_id = add_projection_list(rec, columns)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Project([cols_id, input_id])))
        }
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => {
            let jt_id = add_join_type(rec, *join_type);
            let cond_id = add_scalar_expr(rec, condition)?;
            let left_id = add_rel_expr(rec, left)?;
            let right_id = add_rel_expr(rec, right)?;
            Ok(rec.add(RelLang::Join([jt_id, cond_id, left_id, right_id])))
        }
        RelExpr::Aggregate {
            group_by,
            aggregates,
            input,
        } => {
            let groups_id = add_expr_list(rec, group_by)?;
            let aggs_id = add_aggregate_list(rec, aggregates)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Aggregate([groups_id, aggs_id, input_id])))
        }
        RelExpr::Sort { keys, input } => {
            let keys_id = add_sort_key_list(rec, keys)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Sort([keys_id, input_id])))
        }
        RelExpr::IncrementalSort {
            prefix_keys,
            suffix_keys,
            input,
        } => {
            let prefix_id = add_sort_key_list(rec, prefix_keys)?;
            let suffix_id = add_sort_key_list(rec, suffix_keys)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::IncrementalSort([prefix_id, suffix_id, input_id])))
        }
        RelExpr::Limit {
            count,
            offset,
            input,
        } => {
            let count_id = add_symbol(rec, &count.to_string());
            let offset_id = add_symbol(rec, &offset.to_string());
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Limit([count_id, offset_id, input_id])))
        }
        RelExpr::Union { all, left, right } => {
            let all_id = add_bool_flag(rec, *all);
            let left_id = add_rel_expr(rec, left)?;
            let right_id = add_rel_expr(rec, right)?;
            Ok(rec.add(RelLang::Union([all_id, left_id, right_id])))
        }
        RelExpr::Intersect { all, left, right } => {
            let all_id = add_bool_flag(rec, *all);
            let left_id = add_rel_expr(rec, left)?;
            let right_id = add_rel_expr(rec, right)?;
            Ok(rec.add(RelLang::Intersect([all_id, left_id, right_id])))
        }
        RelExpr::Except { all, left, right } => {
            let all_id = add_bool_flag(rec, *all);
            let left_id = add_rel_expr(rec, left)?;
            let right_id = add_rel_expr(rec, right)?;
            Ok(rec.add(RelLang::Except([all_id, left_id, right_id])))
        }
        RelExpr::RecursiveCTE {
            name,
            base_case,
            recursive_case,
            body,
            ..
        } => {
            let name_id = add_symbol(rec, name);
            let base_id = add_rel_expr(rec, base_case)?;
            let rec_id = add_rel_expr(rec, recursive_case)?;
            let body_id = add_rel_expr(rec, body)?;
            Ok(rec.add(RelLang::RecursiveCTE([name_id, base_id, rec_id, body_id])))
        }
        RelExpr::CTE {
            name,
            definition,
            body,
        } => {
            let name_id = add_symbol(rec, name);
            let def_id = add_rel_expr(rec, definition)?;
            let body_id = add_rel_expr(rec, body)?;
            Ok(rec.add(RelLang::CTE([name_id, def_id, body_id])))
        }
        RelExpr::Window { functions, input } => {
            let fns_id = add_window_expr_list(rec, functions)?;
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::Window([fns_id, input_id])))
        }
        RelExpr::Distinct { input } => {
            let input_id = add_rel_expr(rec, input)?;
            Ok(rec.add(RelLang::DistinctRel([input_id])))
        }
        // DISTINCT ON has no e-graph representation; it is built directly by the
        // plan builder and preserved by `optimize_preserving_distinct_on`, so it
        // should never reach conversion. Defer if it somehow does.
        RelExpr::DistinctOn { .. } => Err(EGraphError::ConversionError(
            "DISTINCT ON has no e-graph form".into(),
        )),
        RelExpr::Values { rows } => {
            let mut row_ids = Vec::with_capacity(rows.len());
            for row in rows {
                let mut cell_ids = Vec::with_capacity(row.len());
                for cell in row {
                    cell_ids.push(add_scalar_expr(rec, cell)?);
                }
                row_ids.push(rec.add(RelLang::ValuesRow(cell_ids.into_boxed_slice())));
            }
            Ok(rec.add(RelLang::Values(row_ids.into_boxed_slice())))
        }
        RelExpr::Unnest {
            expr,
            alias,
            input,
            with_ordinality,
        } => {
            let expr_id = add_scalar_expr(rec, expr)?;
            let alias_id = add_symbol(rec, alias.as_deref().unwrap_or(""));
            let ord_id = add_bool_flag(rec, *with_ordinality);
            if let Some(inp) = input {
                let input_id = add_rel_expr(rec, inp)?;
                let tag_id = add_symbol(rec, "unnest_lateral");
                let ids = vec![tag_id, expr_id, alias_id, ord_id, input_id];
                Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
            } else {
                let tag_id = add_symbol(rec, "unnest");
                let ids = vec![tag_id, expr_id, alias_id, ord_id];
                Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
            }
        }
        RelExpr::MultiUnnest {
            exprs,
            aliases,
            with_ordinality,
        } => {
            let tag_id = add_symbol(rec, "multi_unnest");
            let ord_id = add_bool_flag(rec, *with_ordinality);
            let mut ids = vec![tag_id, ord_id];
            for (expr, alias) in exprs.iter().zip(aliases.iter()) {
                ids.push(add_scalar_expr(rec, expr)?);
                ids.push(add_symbol(rec, alias.as_deref().unwrap_or("")));
            }
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::TableFunction {
            name, args, input, ..
        } => {
            let name_id = add_symbol(rec, name);
            let mut ids = vec![name_id];
            for arg in args {
                ids.push(add_scalar_expr(rec, arg)?);
            }
            if let Some(inp) = input {
                ids.push(add_rel_expr(rec, inp)?);
            }
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::BitmapIndexScan {
            table,
            index,
            predicate,
        } => {
            let table_id = add_symbol(rec, table);
            let index_id = add_symbol(rec, index);
            let pred_id = add_scalar_expr(rec, predicate)?;
            Ok(rec.add(RelLang::BitmapIndexScan([table_id, index_id, pred_id])))
        }
        RelExpr::BitmapAnd { inputs } => {
            let mut input_ids = Vec::with_capacity(inputs.len());
            for input in inputs {
                input_ids.push(add_rel_expr(rec, input)?);
            }
            Ok(rec.add(RelLang::BitmapAnd(input_ids.into_boxed_slice())))
        }
        RelExpr::BitmapOr { inputs } => {
            let mut input_ids = Vec::with_capacity(inputs.len());
            for input in inputs {
                input_ids.push(add_rel_expr(rec, input)?);
            }
            Ok(rec.add(RelLang::BitmapOr(input_ids.into_boxed_slice())))
        }
        RelExpr::BitmapHeapScan {
            table,
            bitmap,
            recheck_cond,
        } => {
            let table_id = add_symbol(rec, table);
            let bitmap_id = add_rel_expr(rec, bitmap)?;
            let recheck_id = if let Some(cond) = recheck_cond {
                add_scalar_expr(rec, cond)?
            } else {
                add_symbol(rec, "")
            };
            Ok(rec.add(RelLang::BitmapHeapScan([table_id, bitmap_id, recheck_id])))
        }
        RelExpr::IndexScan { table, column } => {
            let tag_id = add_symbol(rec, "index_scan");
            let table_id = add_symbol(rec, table);
            let col_id = add_symbol(rec, column);
            let ids = vec![tag_id, table_id, col_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::IndexOnlyScan {
            table,
            index,
            columns,
            predicate,
        } => {
            let table_id = add_symbol(rec, table);
            let index_id = add_symbol(rec, index);
            let cols_id = add_projection_list(rec, columns)?;
            let pred_id = add_scalar_expr(rec, predicate)?;
            Ok(rec.add(RelLang::IndexOnlyScan([
                table_id, index_id, cols_id, pred_id,
            ])))
        }
        RelExpr::RowPattern { input, pattern, .. } => {
            let tag_id = add_symbol(rec, "MATCH_RECOGNIZE");
            let pattern_id = add_symbol(rec, &pattern.to_string());
            let input_id = add_rel_expr(rec, input)?;
            let ids = vec![tag_id, pattern_id, input_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::ParallelScan { table, workers } => {
            let tag_id = add_symbol(rec, "parallel_scan");
            let table_id = add_symbol(rec, table);
            let workers_id = add_symbol(rec, &workers.to_string());
            let ids = vec![tag_id, table_id, workers_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::ParallelHashJoin {
            join_type,
            condition,
            left,
            right,
            workers,
        } => {
            let tag_id = add_symbol(rec, "parallel_hash_join");
            let jt_id = add_join_type(rec, *join_type);
            let cond_id = add_scalar_expr(rec, condition)?;
            let left_id = add_rel_expr(rec, left)?;
            let right_id = add_rel_expr(rec, right)?;
            let workers_id = add_symbol(rec, &workers.to_string());
            let ids = vec![tag_id, jt_id, cond_id, left_id, right_id, workers_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::ParallelAggregate {
            group_by,
            aggregates,
            input,
            workers,
        } => {
            let tag_id = add_symbol(rec, "parallel_aggregate");
            let groups_id = add_expr_list(rec, group_by)?;
            let aggs_id = add_aggregate_list(rec, aggregates)?;
            let input_id = add_rel_expr(rec, input)?;
            let workers_id = add_symbol(rec, &workers.to_string());
            let ids = vec![tag_id, groups_id, aggs_id, input_id, workers_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::Gather { input, workers } => {
            let tag_id = add_symbol(rec, "gather");
            let input_id = add_rel_expr(rec, input)?;
            let workers_id = add_symbol(rec, &workers.to_string());
            let ids = vec![tag_id, input_id, workers_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::MvScan { view_name, alias } => {
            let view_id = add_symbol(rec, view_name);
            let alias_id = add_symbol(rec, alias.as_deref().unwrap_or("auto"));
            let nil_g = rec.add(RelLang::Nil);
            let nil_a = rec.add(RelLang::Nil);
            Ok(rec.add(RelLang::MvScan([view_id, alias_id, nil_g, nil_a])))
        }
        RelExpr::TopK {
            vector_expr,
            query_vector,
            metric,
            k,
            input,
        } => {
            let tag_id = add_symbol(rec, "topk");
            let vec_expr_id = add_scalar_expr(rec, vector_expr)?;
            let query_id = add_scalar_expr(rec, query_vector)?;
            let metric_id = add_symbol(rec, &format!("{metric:?}"));
            let k_id = add_symbol(rec, &k.to_string());
            let input_id = add_rel_expr(rec, input)?;
            let ids = vec![tag_id, vec_expr_id, query_id, metric_id, k_id, input_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::VectorFilter {
            vector_expr,
            query_vector,
            metric,
            threshold,
            input,
        } => {
            let tag_id = add_symbol(rec, "vector_filter");
            let vec_expr_id = add_scalar_expr(rec, vector_expr)?;
            let query_id = add_scalar_expr(rec, query_vector)?;
            let metric_id = add_symbol(rec, &format!("{metric:?}"));
            let threshold_id = add_symbol(rec, &threshold.to_string());
            let input_id = add_rel_expr(rec, input)?;
            let ids = vec![
                tag_id,
                vec_expr_id,
                query_id,
                metric_id,
                threshold_id,
                input_id,
            ];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::Insert {
            table,
            columns,
            source,
            on_conflict,
            returning,
        } => {
            let tag_id = add_symbol(rec, "insert");
            let table_id = add_symbol(rec, table);
            let cols_id = add_string_list(rec, columns);
            let source_id = add_rel_expr(rec, source)?;
            let conflict_id = add_on_conflict(rec, on_conflict.as_ref())?;
            let returning_id = add_optional_projection(rec, returning.as_deref())?;
            let ids = vec![
                tag_id, table_id, cols_id, source_id, conflict_id, returning_id,
            ];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::Update {
            table,
            assignments,
            filter,
            from,
            returning,
        } => {
            let tag_id = add_symbol(rec, "update");
            let table_id = add_symbol(rec, table);
            let assigns_id = add_assignment_list(rec, assignments)?;
            let filter_id = add_optional_scalar(rec, filter.as_ref())?;
            let from_id = add_optional_rel(rec, from.as_deref())?;
            let returning_id = add_optional_projection(rec, returning.as_deref())?;
            let ids = vec![
                tag_id, table_id, assigns_id, filter_id, from_id, returning_id,
            ];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::Delete {
            table,
            filter,
            using,
            returning,
        } => {
            let tag_id = add_symbol(rec, "delete");
            let table_id = add_symbol(rec, table);
            let filter_id = add_optional_scalar(rec, filter.as_ref())?;
            let using_id = add_optional_rel(rec, using.as_deref())?;
            let returning_id = add_optional_projection(rec, returning.as_deref())?;
            let ids = vec![
                tag_id, table_id, filter_id, using_id, returning_id,
            ];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::Merge {
            target,
            source,
            on,
            when_clauses,
            returning,
        } => {
            // MERGE is handled by the DML fast-path (try_optimize_dml)
            // and never saturated, so this encoding only needs to be
            // structurally valid; the WHEN clauses are opaque to the
            // e-graph (recorded as a count marker).
            let tag_id = add_symbol(rec, "merge");
            let target_id = add_symbol(rec, target);
            let on_id = add_scalar_expr(rec, on)?;
            let source_id = add_rel_expr(rec, source)?;
            let when_id = add_symbol(rec, &when_clauses.len().to_string());
            let returning_id = add_optional_projection(rec, returning.as_deref())?;
            let ids = vec![
                tag_id, target_id, on_id, source_id, when_id, returning_id,
            ];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        RelExpr::GraphTable {
            graph,
            pattern,
            columns,
            alias,
        } => {
            // GRAPH_TABLE is a leaf row source handled outside the
            // e-graph; encode it structurally (the MATCH pattern is
            // opaque, recorded as an element-count marker).
            let tag_id = add_symbol(rec, "graph_table");
            let graph_id = add_symbol(rec, graph);
            let pattern_id = add_symbol(rec, &pattern.len().to_string());
            let cols_id = add_optional_projection(rec, Some(columns))?;
            let alias_id = match alias {
                Some(a) => add_symbol(rec, a),
                None => add_symbol(rec, ""),
            };
            let ids = vec![tag_id, graph_id, pattern_id, cols_id, alias_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
    }
}

#[expect(clippy::too_many_lines, reason = "match over all Expr variants")]
fn add_scalar_expr(rec: &mut RecExpr<RelLang>, expr: &Expr) -> Result<Id, EGraphError> {
    match expr {
        Expr::Column(col_ref) => {
            let col_id = add_symbol(rec, &col_ref.column);
            if let Some(table) = &col_ref.table {
                let table_id = add_symbol(rec, table);
                Ok(rec.add(RelLang::QCol([table_id, col_id])))
            } else {
                Ok(rec.add(RelLang::Col([col_id])))
            }
        }
        Expr::Const(c) => Ok(add_const(rec, c)),
        Expr::BinOp { op, left, right } => {
            let left_id = add_scalar_expr(rec, left)?;
            let right_id = add_scalar_expr(rec, right)?;
            let node = match op {
                BinOp::Add => RelLang::Add([left_id, right_id]),
                BinOp::Sub => RelLang::Sub([left_id, right_id]),
                BinOp::Mul => RelLang::Mul([left_id, right_id]),
                BinOp::Div => RelLang::Div([left_id, right_id]),
                BinOp::Mod => RelLang::Mod([left_id, right_id]),
                BinOp::Eq => RelLang::Eq([left_id, right_id]),
                BinOp::Ne => RelLang::Ne([left_id, right_id]),
                BinOp::Lt => RelLang::Lt([left_id, right_id]),
                BinOp::Le => RelLang::Le([left_id, right_id]),
                BinOp::Gt => RelLang::Gt([left_id, right_id]),
                BinOp::Ge => RelLang::Ge([left_id, right_id]),
                BinOp::And => RelLang::And([left_id, right_id]),
                BinOp::Or => RelLang::Or([left_id, right_id]),
                BinOp::Concat => RelLang::Concat([left_id, right_id]),
                BinOp::JsonAccess => RelLang::JsonAccess([left_id, right_id]),
                // IS [NOT] DISTINCT FROM treated as Ne/Eq in the e-graph
                // (NULL-safety is an execution concern, not a plan concern)
                BinOp::IsDistinctFrom => RelLang::Ne([left_id, right_id]),
                BinOp::IsNotDistinctFrom => RelLang::Eq([left_id, right_id]),
            };
            Ok(rec.add(node))
        }
        Expr::UnaryOp { op, operand } => {
            let operand_id = add_scalar_expr(rec, operand)?;
            let node = match op {
                UnaryOp::Not => RelLang::Not([operand_id]),
                UnaryOp::IsNull => RelLang::IsNull([operand_id]),
                UnaryOp::IsNotNull => RelLang::IsNotNull([operand_id]),
                UnaryOp::Neg => RelLang::Neg([operand_id]),
            };
            Ok(rec.add(node))
        }
        Expr::Function { name, args } => {
            let name_id = add_symbol(rec, name);
            let mut ids = vec![name_id];
            for arg in args {
                ids.push(add_scalar_expr(rec, arg)?);
            }
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::Case {
            operand,
            when_clauses,
            else_result,
        } => {
            // Encode as Func(["__CASE", operand_or_null, w1, t1, w2, t2, ..., else_or_null])
            // Null sentinels mark "no operand" and "no else".
            let case_tag = add_symbol(rec, "__CASE");
            let null_sentinel = rec.add(RelLang::ConstNull);

            let operand_id = if let Some(op) = operand {
                add_scalar_expr(rec, op)?
            } else {
                null_sentinel
            };

            let mut ids = vec![case_tag, operand_id];
            for (cond, result) in when_clauses {
                ids.push(add_scalar_expr(rec, cond)?);
                ids.push(add_scalar_expr(rec, result)?);
            }
            let else_id = if let Some(e) = else_result {
                add_scalar_expr(rec, e)?
            } else {
                null_sentinel
            };
            ids.push(else_id);

            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::Cast { expr, target_type } => {
            let expr_id = add_scalar_expr(rec, expr)?;
            let type_id = add_symbol(rec, target_type);
            Ok(rec.add(RelLang::Cast([expr_id, type_id])))
        }
        Expr::Array(elements) => {
            let tag_id = add_symbol(rec, "ARRAY");
            let mut ids = vec![tag_id];
            for elem in elements {
                ids.push(add_scalar_expr(rec, elem)?);
            }
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::ArrayIndex(array, index) => {
            let arr_id = add_scalar_expr(rec, array)?;
            let idx_id = add_scalar_expr(rec, index)?;
            let tag_id = add_symbol(rec, "ARRAY_INDEX");
            let ids = vec![tag_id, arr_id, idx_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::PatternPrev(inner, offset) => {
            let tag_id = add_symbol(rec, "PREV");
            let inner_id = add_scalar_expr(rec, inner)?;
            let offset_id = add_symbol(rec, &offset.to_string());
            let ids = vec![tag_id, inner_id, offset_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::PatternNext(inner, offset) => {
            let tag_id = add_symbol(rec, "NEXT");
            let inner_id = add_scalar_expr(rec, inner)?;
            let offset_id = add_symbol(rec, &offset.to_string());
            let ids = vec![tag_id, inner_id, offset_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::PatternFirst(inner, var) => {
            let tag_id = add_symbol(rec, "FIRST");
            let inner_id = add_scalar_expr(rec, inner)?;
            let var_id = add_symbol(rec, var);
            let ids = vec![tag_id, inner_id, var_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::PatternLast(inner, var) => {
            let tag_id = add_symbol(rec, "LAST");
            let inner_id = add_scalar_expr(rec, inner)?;
            let var_id = add_symbol(rec, var);
            let ids = vec![tag_id, inner_id, var_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::PatternClassifier => {
            let tag_id = add_symbol(rec, "CLASSIFIER");
            let ids = vec![tag_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::PatternMatchNumber => {
            let tag_id = add_symbol(rec, "MATCH_NUMBER");
            let ids = vec![tag_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::ArraySlice { array, start, end } => {
            let arr_id = add_scalar_expr(rec, array)?;
            let start_id = match start {
                Some(s) => add_scalar_expr(rec, s)?,
                None => rec.add(RelLang::ConstNull),
            };
            let end_id = match end {
                Some(e) => add_scalar_expr(rec, e)?,
                None => rec.add(RelLang::ConstNull),
            };
            let tag_id = add_symbol(rec, "ARRAY_SLICE");
            let ids = vec![tag_id, arr_id, start_id, end_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::FieldAccess { expr, field_name } => {
            let expr_id = add_scalar_expr(rec, expr)?;
            let field_id = add_symbol(rec, field_name);
            let tag_id = add_symbol(rec, "FIELD_ACCESS");
            let ids = vec![tag_id, expr_id, field_id];
            Ok(rec.add(RelLang::Func(ids.into_boxed_slice())))
        }
        Expr::SubQuery { .. } => Err(EGraphError::ConversionError(
            "Subquery expressions are not yet supported in the \
                 e-graph representation"
                .into(),
        )),
        Expr::FullTextMatch {
            vendor,
            columns,
            query,
            mode,
        } => {
            let vendor_id = add_symbol(rec, vendor);
            let cols_id = add_symbol(rec, &columns.join(","));
            let query_id = add_symbol(rec, query);
            let mode_id = add_symbol(rec, mode.as_deref().unwrap_or(""));
            Ok(rec.add(RelLang::FtsMatch([vendor_id, cols_id, query_id, mode_id])))
        }
        Expr::VectorDistance {
            metric,
            column,
            target,
        } => {
            let metric_id = add_symbol(rec, metric);
            let col_id = add_scalar_expr(rec, column)?;
            let target_id = add_scalar_expr(rec, target)?;
            Ok(rec.add(RelLang::VectorDistance([metric_id, col_id, target_id])))
        }
    }
}

fn add_const(rec: &mut RecExpr<RelLang>, c: &Const) -> Id {
    match c {
        Const::Null => rec.add(RelLang::ConstNull),
        Const::Bool(b) => {
            let val_id = if *b {
                rec.add(RelLang::True)
            } else {
                rec.add(RelLang::False)
            };
            rec.add(RelLang::ConstBool([val_id]))
        }
        Const::Int(i) => {
            let val_id = add_symbol(rec, &i.to_string());
            rec.add(RelLang::ConstInt([val_id]))
        }
        Const::Float(f) => {
            let val_id = add_symbol(rec, &f.to_string());
            rec.add(RelLang::ConstFloat([val_id]))
        }
        Const::String(s) => {
            let val_id = add_symbol(rec, s);
            rec.add(RelLang::ConstStr([val_id]))
        }
    }
}

fn add_symbol(rec: &mut RecExpr<RelLang>, s: &str) -> Id {
    rec.add(RelLang::Symbol(egg::Symbol::from(s)))
}

fn add_join_type(rec: &mut RecExpr<RelLang>, jt: JoinType) -> Id {
    let node = match jt {
        JoinType::Inner => RelLang::Inner,
        JoinType::LeftOuter => RelLang::LeftOuter,
        JoinType::RightOuter => RelLang::RightOuter,
        JoinType::FullOuter => RelLang::FullOuter,
        JoinType::Cross => RelLang::Cross,
        JoinType::Semi => RelLang::Semi,
        JoinType::Anti => RelLang::Anti,
    };
    rec.add(node)
}

fn add_bool_flag(rec: &mut RecExpr<RelLang>, val: bool) -> Id {
    if val {
        rec.add(RelLang::True)
    } else {
        rec.add(RelLang::False)
    }
}

fn add_projection_list(
    rec: &mut RecExpr<RelLang>,
    columns: &[ProjectionColumn],
) -> Result<Id, EGraphError> {
    let mut ids = Vec::with_capacity(columns.len());
    for col in columns {
        let expr_id = add_scalar_expr(rec, &col.expr)?;
        let proj_id = if let Some(alias) = &col.alias {
            let alias_id = add_symbol(rec, alias);
            rec.add(RelLang::ProjAlias([expr_id, alias_id]))
        } else {
            rec.add(RelLang::ProjCol([expr_id]))
        };
        ids.push(proj_id);
    }
    Ok(rec.add(RelLang::List(ids.into_boxed_slice())))
}

fn add_expr_list(rec: &mut RecExpr<RelLang>, exprs: &[Expr]) -> Result<Id, EGraphError> {
    let mut ids = Vec::with_capacity(exprs.len());
    for e in exprs {
        ids.push(add_scalar_expr(rec, e)?);
    }
    Ok(rec.add(RelLang::List(ids.into_boxed_slice())))
}

fn add_aggregate_list(
    rec: &mut RecExpr<RelLang>,
    aggs: &[AggregateExpr],
) -> Result<Id, EGraphError> {
    let mut ids = Vec::with_capacity(aggs.len());
    for agg in aggs {
        let func_node = match agg.function {
            AggregateFunction::Count => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                RelLang::Count([arg_id])
            }
            AggregateFunction::Sum => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                RelLang::Sum([arg_id])
            }
            AggregateFunction::Avg => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                RelLang::Avg([arg_id])
            }
            AggregateFunction::Min => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                RelLang::Min([arg_id])
            }
            AggregateFunction::Max => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                RelLang::Max([arg_id])
            }
            // Extended aggregates: encode as opaque Func nodes so the
            // e-graph can optimize around them without specific rules.
            AggregateFunction::StdDev => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                let tag = add_symbol(rec, "STDDEV");
                RelLang::Func(vec![tag, arg_id].into_boxed_slice())
            }
            AggregateFunction::Variance => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                let tag = add_symbol(rec, "VARIANCE");
                RelLang::Func(vec![tag, arg_id].into_boxed_slice())
            }
            AggregateFunction::StringAgg => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                let tag = add_symbol(rec, "STRING_AGG");
                RelLang::Func(vec![tag, arg_id].into_boxed_slice())
            }
            AggregateFunction::ArrayAgg => {
                let arg_id = add_agg_arg(rec, agg.arg.as_ref())?;
                let tag = add_symbol(rec, "ARRAY_AGG");
                RelLang::Func(vec![tag, arg_id].into_boxed_slice())
            }
        };
        let func_id = rec.add(func_node);
        let distinct_id = if agg.distinct {
            rec.add(RelLang::Distinct)
        } else {
            rec.add(RelLang::All)
        };
        let alias_id = if let Some(alias) = &agg.alias {
            add_symbol(rec, alias)
        } else {
            rec.add(RelLang::Nil)
        };
        let agg_id = rec.add(RelLang::AggExpr([func_id, distinct_id, alias_id]));
        ids.push(agg_id);
    }
    Ok(rec.add(RelLang::List(ids.into_boxed_slice())))
}

fn add_agg_arg(rec: &mut RecExpr<RelLang>, arg: Option<&Expr>) -> Result<Id, EGraphError> {
    match arg {
        Some(e) => add_scalar_expr(rec, e),
        None => Ok(rec.add(RelLang::Nil)),
    }
}

fn add_sort_key_list(rec: &mut RecExpr<RelLang>, keys: &[SortKey]) -> Result<Id, EGraphError> {
    let mut ids = Vec::with_capacity(keys.len());
    for key in keys {
        let expr_id = add_scalar_expr(rec, &key.expr)?;
        let dir_id = match key.direction {
            SortDirection::Asc => rec.add(RelLang::Asc),
            SortDirection::Desc => rec.add(RelLang::Desc),
        };
        let nulls_id = match key.nulls {
            NullOrdering::First => rec.add(RelLang::NullsFirst),
            NullOrdering::Last => rec.add(RelLang::NullsLast),
        };
        let key_id = rec.add(RelLang::SortKey([expr_id, dir_id, nulls_id]));
        ids.push(key_id);
    }
    Ok(rec.add(RelLang::List(ids.into_boxed_slice())))
}

fn add_window_expr_list(
    rec: &mut RecExpr<RelLang>,
    exprs: &[WindowExpr],
) -> Result<Id, EGraphError> {
    let mut ids = Vec::with_capacity(exprs.len());
    for wexpr in exprs {
        ids.push(add_window_expr(rec, wexpr)?);
    }
    Ok(rec.add(RelLang::List(ids.into_boxed_slice())))
}

fn add_window_expr(rec: &mut RecExpr<RelLang>, wexpr: &WindowExpr) -> Result<Id, EGraphError> {
    let fn_name = add_symbol(rec, &format!("{:?}", wexpr.function));
    let fn_id = rec.add(RelLang::WindowFn([fn_name]));
    let arg_id = match &wexpr.arg {
        Some(e) => add_scalar_expr(rec, e)?,
        None => rec.add(RelLang::Nil),
    };
    let part_id = add_expr_list(rec, &wexpr.partition_by)?;
    let order_id = add_sort_key_list(rec, &wexpr.order_by)?;
    let frame_id = add_window_frame(rec, wexpr.frame.as_ref())?;
    let alias_id = match &wexpr.alias {
        Some(a) => add_symbol(rec, a),
        None => rec.add(RelLang::Nil),
    };
    Ok(rec.add(RelLang::WindowExprNode([
        fn_id, arg_id, part_id, order_id, frame_id, alias_id,
    ])))
}

#[expect(clippy::unnecessary_wraps, reason = "consistent Result return with other add_ helpers")]
fn add_window_frame(
    rec: &mut RecExpr<RelLang>,
    frame: Option<&WindowFrame>,
) -> Result<Id, EGraphError> {
    let Some(f) = frame else {
        return Ok(rec.add(RelLang::Nil));
    };
    let mode_id = match f.mode {
        WindowFrameMode::Rows => rec.add(RelLang::FrameRows),
        WindowFrameMode::Range => rec.add(RelLang::FrameRange),
        WindowFrameMode::Groups => rec.add(RelLang::FrameGroups),
    };
    let start_id = add_frame_bound(rec, &f.start);
    let end_id = add_frame_bound(rec, &f.end);
    Ok(rec.add(RelLang::WindowFrameNode([mode_id, start_id, end_id])))
}

fn add_frame_bound(rec: &mut RecExpr<RelLang>, bound: &WindowFrameBound) -> Id {
    match bound {
        WindowFrameBound::UnboundedPreceding => rec.add(RelLang::FrameUnboundedPreceding),
        WindowFrameBound::Preceding(n) => {
            let n_id = add_symbol(rec, &n.to_string());
            rec.add(RelLang::FramePreceding([n_id]))
        }
        WindowFrameBound::CurrentRow => rec.add(RelLang::FrameCurrentRow),
        WindowFrameBound::Following(n) => {
            let n_id = add_symbol(rec, &n.to_string());
            rec.add(RelLang::FrameFollowing([n_id]))
        }
        WindowFrameBound::UnboundedFollowing => rec.add(RelLang::FrameUnboundedFollowing),
    }
}

// -- DML helper functions --

fn add_string_list(rec: &mut RecExpr<RelLang>, strings: &[String]) -> Id {
    let ids: Vec<Id> = strings.iter().map(|s| add_symbol(rec, s)).collect();
    rec.add(RelLang::List(ids.into_boxed_slice()))
}

fn add_on_conflict(
    rec: &mut RecExpr<RelLang>,
    on_conflict: Option<&OnConflict>,
) -> Result<Id, EGraphError> {
    let Some(oc) = on_conflict else {
        return Ok(rec.add(RelLang::Nil));
    };
    match oc {
        OnConflict::DoNothing => {
            let tag_id = add_symbol(rec, "do_nothing");
            Ok(rec.add(RelLang::Func(vec![tag_id].into_boxed_slice())))
        }
        OnConflict::DoUpdate {
            target,
            assignments,
        } => {
            let tag_id = add_symbol(rec, "do_update");
            let target_id = add_string_list(rec, target);
            let assigns_id = add_assignment_list(rec, assignments)?;
            Ok(rec.add(RelLang::Func(
                vec![tag_id, target_id, assigns_id].into_boxed_slice(),
            )))
        }
        OnConflict::DoSelect { target } => {
            let tag_id = add_symbol(rec, "do_select");
            let target_id = add_string_list(rec, target);
            Ok(rec.add(RelLang::Func(
                vec![tag_id, target_id].into_boxed_slice(),
            )))
        }
    }
}

fn add_assignment_list(
    rec: &mut RecExpr<RelLang>,
    assignments: &[(String, Expr)],
) -> Result<Id, EGraphError> {
    let mut ids = Vec::with_capacity(assignments.len());
    for (col, expr) in assignments {
        let col_id = add_symbol(rec, col);
        let expr_id = add_scalar_expr(rec, expr)?;
        let pair_id = rec.add(RelLang::Func(
            vec![col_id, expr_id].into_boxed_slice(),
        ));
        ids.push(pair_id);
    }
    Ok(rec.add(RelLang::List(ids.into_boxed_slice())))
}

fn add_optional_scalar(
    rec: &mut RecExpr<RelLang>,
    expr: Option<&Expr>,
) -> Result<Id, EGraphError> {
    match expr {
        Some(e) => add_scalar_expr(rec, e),
        None => Ok(rec.add(RelLang::Nil)),
    }
}

fn add_optional_rel(
    rec: &mut RecExpr<RelLang>,
    rel: Option<&RelExpr>,
) -> Result<Id, EGraphError> {
    match rel {
        Some(r) => add_rel_expr(rec, r),
        None => Ok(rec.add(RelLang::Nil)),
    }
}

fn add_optional_projection(
    rec: &mut RecExpr<RelLang>,
    columns: Option<&[ProjectionColumn]>,
) -> Result<Id, EGraphError> {
    match columns {
        Some(cols) => add_projection_list(rec, cols),
        None => Ok(rec.add(RelLang::Nil)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;
    use ra_core::expr::{BinOp, Const, Expr};

    #[test]
    fn case_expr_roundtrip() {
        let case_expr = Expr::Case {
            operand: None,
            when_clauses: vec![(
                Expr::BinOp {
                    op: BinOp::Gt,
                    left: Box::new(Expr::Column(ra_core::expr::ColumnRef {
                    table: None, column: "x".to_owned()
                })),
                    right: Box::new(Expr::Const(Const::Int(0))),
                },
                Expr::Const(Const::Int(1)),
            )],
            else_result: Some(Box::new(Expr::Const(Const::Int(0)))),
        };

        let plan = RelExpr::filter(RelExpr::scan("t"), case_expr);
        let result = to_rec_expr(&plan);
        assert!(result.is_ok(), "CASE should convert: {:?}", result.err());
    }

    #[test]
    fn case_in_aggregate_roundtrip() {
        use crate::egraph::optimizer::Optimizer;
        use crate::ResourceBudget;
        use ra_core::algebra::{AggregateExpr, AggregateFunction, RelExpr};
        use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

        let case_expr = Expr::Case {
            operand: None,
            when_clauses: vec![(
                Expr::BinOp {
                    op: BinOp::Gt,
                    left: Box::new(Expr::Column(ColumnRef {
                        table: None,
                        column: "x".to_owned(),
                    })),
                    right: Box::new(Expr::Const(Const::Int(0))),
                },
                Expr::Const(Const::Int(1)),
            )],
            else_result: Some(Box::new(Expr::Const(Const::Int(0)))),
        };

        let plan = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(case_expr),
                distinct: false,
                alias: None,
            }],
            input: Box::new(RelExpr::scan("t")),
        };

        let result = to_rec_expr(&plan);
        assert!(
            result.is_ok(),
            "Aggregate(SUM(CASE...)) should convert: {:?}",
            result.err()
        );

        let mut optimizer = Optimizer::new();
        optimizer.set_resource_budget(ResourceBudget::standard());
        let opt_result = optimizer.optimize_bounded(&plan);
        assert!(
            opt_result.is_ok(),
            "optimize should work: {:?}",
            opt_result.err()
        );
    }
}
