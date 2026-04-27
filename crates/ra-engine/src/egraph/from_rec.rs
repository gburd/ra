use egg::{EGraph, Id};
use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering, ProjectionColumn, RelExpr,
    SortDirection, SortKey, WindowExpr, WindowFrame, WindowFrameBound, WindowFrameMode,
    WindowFunction,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr, UnaryOp};

use crate::analysis::RelAnalysis;

use super::errors::EGraphError;
use super::lang::RelLang;

/// Convert an e-graph node (by class [`Id`]) back to a [`RelExpr`].
///
/// Extracts the best node from each e-class using the given extractor
/// function, then reconstructs the AST.
///
/// # Errors
///
/// Returns an error if the e-graph contains nodes that cannot be
/// mapped back to [`RelExpr`].
pub fn from_egraph_node(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<RelExpr, EGraphError> {
    let nodes = &egraph[id].nodes;
    let node = &nodes[0];
    from_node(egraph, node)
}

#[allow(clippy::too_many_lines)]
fn from_node(
    egraph: &EGraph<RelLang, RelAnalysis>,
    node: &RelLang,
) -> Result<RelExpr, EGraphError> {
    match node {
        RelLang::Scan([table_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            Ok(RelExpr::Scan { table, alias: None })
        }
        RelLang::ScanAlias([table_id, alias_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            let alias = extract_symbol(egraph, *alias_id)?;
            Ok(RelExpr::Scan {
                table,
                alias: Some(alias),
            })
        }
        RelLang::Filter([pred_id, input_id]) => {
            let predicate = extract_scalar_expr(egraph, *pred_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Filter {
                predicate,
                input: Box::new(input),
            })
        }
        RelLang::Project([cols_id, input_id]) => {
            let columns = extract_projection_list(egraph, *cols_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Project {
                columns,
                input: Box::new(input),
            })
        }
        RelLang::Join([jt_id, cond_id, left_id, right_id]) => {
            let join_type = extract_join_type(egraph, *jt_id)?;
            let condition = extract_scalar_expr(egraph, *cond_id)?;
            let left = from_egraph_node(egraph, *left_id)?;
            let right = from_egraph_node(egraph, *right_id)?;
            Ok(RelExpr::Join {
                join_type,
                condition,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        RelLang::Aggregate([groups_id, aggs_id, input_id]) => {
            let group_by = extract_expr_list(egraph, *groups_id)?;
            let aggregates = extract_aggregate_list(egraph, *aggs_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Aggregate {
                group_by,
                aggregates,
                input: Box::new(input),
            })
        }
        RelLang::Sort([keys_id, input_id]) => {
            let keys = extract_sort_key_list(egraph, *keys_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Sort {
                keys,
                input: Box::new(input),
            })
        }
        RelLang::IncrementalSort([prefix_id, suffix_id, input_id]) => {
            let prefix_keys = extract_sort_key_list(egraph, *prefix_id)?;
            let suffix_keys = extract_sort_key_list(egraph, *suffix_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::IncrementalSort {
                prefix_keys,
                suffix_keys,
                input: Box::new(input),
            })
        }
        RelLang::Limit([count_id, offset_id, input_id]) => {
            let count_str = extract_symbol(egraph, *count_id)?;
            let offset_str = extract_symbol(egraph, *offset_id)?;
            let count = count_str
                .parse::<u64>()
                .map_err(|e| EGraphError::ExtractionError(format!("invalid limit count: {e}")))?;
            let offset = offset_str
                .parse::<u64>()
                .map_err(|e| EGraphError::ExtractionError(format!("invalid limit offset: {e}")))?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Limit {
                count,
                offset,
                input: Box::new(input),
            })
        }
        RelLang::Union([all_id, left_id, right_id]) => {
            let all = extract_bool_flag(egraph, *all_id)?;
            let left = from_egraph_node(egraph, *left_id)?;
            let right = from_egraph_node(egraph, *right_id)?;
            Ok(RelExpr::Union {
                all,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        RelLang::Intersect([all_id, left_id, right_id]) => {
            let all = extract_bool_flag(egraph, *all_id)?;
            let left = from_egraph_node(egraph, *left_id)?;
            let right = from_egraph_node(egraph, *right_id)?;
            Ok(RelExpr::Intersect {
                all,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        RelLang::Except([all_id, left_id, right_id]) => {
            let all = extract_bool_flag(egraph, *all_id)?;
            let left = from_egraph_node(egraph, *left_id)?;
            let right = from_egraph_node(egraph, *right_id)?;
            Ok(RelExpr::Except {
                all,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        RelLang::RecursiveCTE([name_id, base_id, rec_id, body_id]) => {
            let name = extract_symbol(egraph, *name_id)?;
            let base_case = from_egraph_node(egraph, *base_id)?;
            let recursive_case = from_egraph_node(egraph, *rec_id)?;
            let body = from_egraph_node(egraph, *body_id)?;
            Ok(RelExpr::RecursiveCTE {
                name,
                base_case: Box::new(base_case),
                recursive_case: Box::new(recursive_case),
                body: Box::new(body),
                cycle_detection: None,
            })
        }
        RelLang::CTE([name_id, def_id, body_id]) => {
            let name = extract_symbol(egraph, *name_id)?;
            let definition = from_egraph_node(egraph, *def_id)?;
            let body = from_egraph_node(egraph, *body_id)?;
            Ok(RelExpr::CTE {
                name,
                definition: Box::new(definition),
                body: Box::new(body),
            })
        }
        RelLang::Window([fns_id, input_id]) => {
            let functions = extract_window_expr_list(egraph, *fns_id)?;
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Window {
                functions,
                input: Box::new(input),
            })
        }
        RelLang::DistinctRel([input_id]) => {
            let input = from_egraph_node(egraph, *input_id)?;
            Ok(RelExpr::Distinct {
                input: Box::new(input),
            })
        }
        RelLang::Values(row_ids) => {
            let mut rows = Vec::with_capacity(row_ids.len());
            for &row_id in row_ids.iter() {
                rows.push(extract_values_row(egraph, row_id)?);
            }
            Ok(RelExpr::Values { rows })
        }
        RelLang::BitmapIndexScan([table_id, index_id, pred_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            let index = extract_symbol(egraph, *index_id)?;
            let predicate = extract_scalar_expr(egraph, *pred_id)?;
            Ok(RelExpr::BitmapIndexScan {
                table,
                index,
                predicate,
            })
        }
        RelLang::BitmapAnd(input_ids) => {
            let mut inputs = Vec::with_capacity(input_ids.len());
            for &input_id in input_ids.iter() {
                inputs.push(Box::new(from_egraph_node(egraph, input_id)?));
            }
            Ok(RelExpr::BitmapAnd { inputs })
        }
        RelLang::BitmapOr(input_ids) => {
            let mut inputs = Vec::with_capacity(input_ids.len());
            for &input_id in input_ids.iter() {
                inputs.push(Box::new(from_egraph_node(egraph, input_id)?));
            }
            Ok(RelExpr::BitmapOr { inputs })
        }
        RelLang::BitmapHeapScan([table_id, bitmap_id, recheck_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            let bitmap = from_egraph_node(egraph, *bitmap_id)?;
            let recheck_str = extract_symbol(egraph, *recheck_id)?;
            let recheck_cond = if recheck_str.is_empty() {
                None
            } else {
                Some(extract_scalar_expr(egraph, *recheck_id)?)
            };
            Ok(RelExpr::BitmapHeapScan {
                table,
                bitmap: Box::new(bitmap),
                recheck_cond,
            })
        }
        RelLang::MetadataLookup([table_id, _kind_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            Ok(RelExpr::Aggregate {
                group_by: vec![],
                aggregates: vec![AggregateExpr {
                    function: AggregateFunction::Count,
                    arg: None,
                    distinct: false,
                    alias: Some("count".to_string()),
                }],
                input: Box::new(RelExpr::Scan { table, alias: None }),
            })
        }
        RelLang::VectorKNN([table_id, col_id, target_id, k_id]) => {
            // Extract as a scan with a filter annotation for now
            // TODO: Add proper VectorKNN to RelExpr enum
            let table = extract_symbol(egraph, *table_id)?;
            let _col = extract_scalar_expr(egraph, *col_id)?;
            let _target = extract_scalar_expr(egraph, *target_id)?;
            let _k = extract_symbol(egraph, *k_id)?;
            Ok(RelExpr::Scan {
                table,
                alias: Some("vector_knn_scan".to_string()),
            })
        }
        RelLang::VectorRangeScan([table_id, _col_id, _target_id, _threshold_id, _metric_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            Ok(RelExpr::Scan {
                table,
                alias: Some("vector_range_scan".to_string()),
            })
        }
        RelLang::FtsIndexScan([table_id, _idx_id, _match_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            Ok(RelExpr::Scan {
                table,
                alias: Some("fts_index_scan".to_string()),
            })
        }
        RelLang::FtsRankedScan([table_id, _idx_id, _query_id, _k_id, _algo_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            Ok(RelExpr::Scan {
                table,
                alias: Some("fts_ranked_scan".to_string()),
            })
        }
        RelLang::FtsSkipListAnd([table_id, _match1_id, _match2_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            Ok(RelExpr::Scan {
                table,
                alias: Some("fts_skip_list_and".to_string()),
            })
        }
        RelLang::HybridScan(_ids) => {
            // Placeholder for hybrid scan extraction
            Ok(RelExpr::Scan {
                table: "hybrid_scan".to_string(),
                alias: Some("hybrid_scan".to_string()),
            })
        }
        RelLang::HybridScore(_ids) => {
            // This shouldn't appear in relational context, but handle it gracefully
            Err(EGraphError::ExtractionError(
                "hybrid-score is a scalar operator, not a relational operator".into(),
            ))
        }
        other => Err(EGraphError::ExtractionError(format!(
            "unexpected relational node: {other:?}"
        ))),
    }
}

fn extract_symbol(egraph: &EGraph<RelLang, RelAnalysis>, id: Id) -> Result<String, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::Symbol(s) = node {
            return Ok(s.to_string());
        }
    }
    Err(EGraphError::ExtractionError(format!(
        "expected Symbol node at e-class {id:?}"
    )))
}

fn extract_bool_flag(egraph: &EGraph<RelLang, RelAnalysis>, id: Id) -> Result<bool, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::True => return Ok(true),
            RelLang::False => return Ok(false),
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(format!(
        "expected True/False node at e-class {id:?}"
    )))
}

fn extract_join_type(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<JoinType, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        let jt = match node {
            RelLang::Inner => JoinType::Inner,
            RelLang::LeftOuter => JoinType::LeftOuter,
            RelLang::RightOuter => JoinType::RightOuter,
            RelLang::FullOuter => JoinType::FullOuter,
            RelLang::Cross => JoinType::Cross,
            RelLang::Semi => JoinType::Semi,
            RelLang::Anti => JoinType::Anti,
            _ => continue,
        };
        return Ok(jt);
    }
    Err(EGraphError::ExtractionError(format!(
        "expected join type node at e-class {id:?}"
    )))
}

fn extract_scalar_expr(egraph: &EGraph<RelLang, RelAnalysis>, id: Id) -> Result<Expr, EGraphError> {
    let canonical = egraph.find(id);
    let node = &egraph[canonical].nodes[0];
    scalar_from_node(egraph, node)
}

#[allow(clippy::too_many_lines)]
fn scalar_from_node(
    egraph: &EGraph<RelLang, RelAnalysis>,
    node: &RelLang,
) -> Result<Expr, EGraphError> {
    match node {
        RelLang::Col([name_id]) => {
            let name = extract_symbol(egraph, *name_id)?;
            Ok(Expr::Column(ColumnRef::new(name)))
        }
        RelLang::QCol([table_id, name_id]) => {
            let table = extract_symbol(egraph, *table_id)?;
            let name = extract_symbol(egraph, *name_id)?;
            Ok(Expr::Column(ColumnRef::qualified(table, name)))
        }
        RelLang::ConstNull => Ok(Expr::Const(Const::Null)),
        RelLang::ConstBool([val_id]) => {
            let b = extract_bool_flag(egraph, *val_id)?;
            Ok(Expr::Const(Const::Bool(b)))
        }
        RelLang::ConstInt([val_id]) => {
            let s = extract_symbol(egraph, *val_id)?;
            let i = s.parse::<i64>().map_err(|e| {
                EGraphError::ExtractionError(format!("invalid integer constant: {e}"))
            })?;
            Ok(Expr::Const(Const::Int(i)))
        }
        RelLang::ConstFloat([val_id]) => {
            let s = extract_symbol(egraph, *val_id)?;
            let f = s.parse::<f64>().map_err(|e| {
                EGraphError::ExtractionError(format!("invalid float constant: {e}"))
            })?;
            Ok(Expr::Const(Const::Float(f)))
        }
        RelLang::ConstStr([val_id]) => {
            let s = extract_symbol(egraph, *val_id)?;
            Ok(Expr::Const(Const::String(s)))
        }
        RelLang::Add([l, r]) => extract_binop(egraph, BinOp::Add, *l, *r),
        RelLang::Sub([l, r]) => extract_binop(egraph, BinOp::Sub, *l, *r),
        RelLang::Mul([l, r]) => extract_binop(egraph, BinOp::Mul, *l, *r),
        RelLang::Div([l, r]) => extract_binop(egraph, BinOp::Div, *l, *r),
        RelLang::Eq([l, r]) => extract_binop(egraph, BinOp::Eq, *l, *r),
        RelLang::Ne([l, r]) => extract_binop(egraph, BinOp::Ne, *l, *r),
        RelLang::Lt([l, r]) => extract_binop(egraph, BinOp::Lt, *l, *r),
        RelLang::Le([l, r]) => extract_binop(egraph, BinOp::Le, *l, *r),
        RelLang::Gt([l, r]) => extract_binop(egraph, BinOp::Gt, *l, *r),
        RelLang::Ge([l, r]) => extract_binop(egraph, BinOp::Ge, *l, *r),
        RelLang::And([l, r]) => extract_binop(egraph, BinOp::And, *l, *r),
        RelLang::Or([l, r]) => extract_binop(egraph, BinOp::Or, *l, *r),
        RelLang::Not([operand_id]) => {
            let operand = extract_scalar_expr(egraph, *operand_id)?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            })
        }
        RelLang::IsNull([operand_id]) => {
            let operand = extract_scalar_expr(egraph, *operand_id)?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::IsNull,
                operand: Box::new(operand),
            })
        }
        RelLang::IsNotNull([operand_id]) => {
            let operand = extract_scalar_expr(egraph, *operand_id)?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::IsNotNull,
                operand: Box::new(operand),
            })
        }
        RelLang::Neg([operand_id]) => {
            let operand = extract_scalar_expr(egraph, *operand_id)?;
            Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                operand: Box::new(operand),
            })
        }
        RelLang::Func(ids) => {
            if ids.is_empty() {
                return Err(EGraphError::ExtractionError(
                    "function call with no children".into(),
                ));
            }
            let name = extract_symbol(egraph, ids[0])?;
            let mut args = Vec::with_capacity(ids.len() - 1);
            for &arg_id in &ids[1..] {
                args.push(extract_scalar_expr(egraph, arg_id)?);
            }
            Ok(Expr::Function { name, args })
        }
        RelLang::Cast([expr_id, type_id]) => {
            let expr = extract_scalar_expr(egraph, *expr_id)?;
            let target_type = extract_symbol(egraph, *type_id)?;
            Ok(Expr::Cast {
                expr: Box::new(expr),
                target_type,
            })
        }
        RelLang::VectorDistance([metric_id, col_id, target_id]) => {
            let metric = extract_symbol(egraph, *metric_id)?;
            let column = extract_scalar_expr(egraph, *col_id)?;
            let target = extract_scalar_expr(egraph, *target_id)?;
            Ok(Expr::VectorDistance {
                metric,
                column: Box::new(column),
                target: Box::new(target),
            })
        }
        RelLang::FtsMatch([vendor_id, cols_id, query_id, mode_id]) => {
            let vendor = extract_symbol(egraph, *vendor_id)?;
            let cols_str = extract_symbol(egraph, *cols_id)?;
            let columns = cols_str.split(',').map(|s| s.to_string()).collect();
            let query = extract_symbol(egraph, *query_id)?;
            let mode_str = extract_symbol(egraph, *mode_id)?;
            let mode = if mode_str.is_empty() {
                None
            } else {
                Some(mode_str)
            };
            Ok(Expr::FullTextMatch {
                vendor,
                columns,
                query,
                mode,
            })
        }
        RelLang::FtsRank([col_id, query_id, algo_id]) => {
            let col = extract_scalar_expr(egraph, *col_id)?;
            let query = extract_scalar_expr(egraph, *query_id)?;
            let algo = extract_symbol(egraph, *algo_id)?;
            // FTS rank is represented as a function call in Expr
            Ok(Expr::Function {
                name: format!("ts_rank_{}", algo),
                args: vec![col, query],
            })
        }
        other => Err(EGraphError::ExtractionError(format!(
            "unexpected scalar node: {other:?}"
        ))),
    }
}

fn extract_binop(
    egraph: &EGraph<RelLang, RelAnalysis>,
    op: BinOp,
    left_id: Id,
    right_id: Id,
) -> Result<Expr, EGraphError> {
    let left = extract_scalar_expr(egraph, left_id)?;
    let right = extract_scalar_expr(egraph, right_id)?;
    Ok(Expr::BinOp {
        op,
        left: Box::new(left),
        right: Box::new(right),
    })
}

fn extract_projection_list(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<ProjectionColumn>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::List(ids) = node {
            let mut cols = Vec::with_capacity(ids.len());
            for &child_id in ids.iter() {
                cols.push(extract_projection_column(egraph, child_id)?);
            }
            return Ok(cols);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected List node for projection columns".into(),
    ))
}

fn extract_projection_column(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<ProjectionColumn, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::ProjCol([expr_id]) => {
                let expr = extract_scalar_expr(egraph, *expr_id)?;
                return Ok(ProjectionColumn { expr, alias: None });
            }
            RelLang::ProjAlias([expr_id, alias_id]) => {
                let expr = extract_scalar_expr(egraph, *expr_id)?;
                let alias = extract_symbol(egraph, *alias_id)?;
                return Ok(ProjectionColumn {
                    expr,
                    alias: Some(alias),
                });
            }
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected ProjCol or ProjAlias node".into(),
    ))
}

fn extract_expr_list(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<Expr>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::List(ids) = node {
            let mut exprs = Vec::with_capacity(ids.len());
            for &child_id in ids.iter() {
                exprs.push(extract_scalar_expr(egraph, child_id)?);
            }
            return Ok(exprs);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected List node for expression list".into(),
    ))
}

fn extract_aggregate_list(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<AggregateExpr>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::List(ids) = node {
            let mut aggs = Vec::with_capacity(ids.len());
            for &child_id in ids.iter() {
                aggs.push(extract_agg_expr(egraph, child_id)?);
            }
            return Ok(aggs);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected List node for aggregate list".into(),
    ))
}

fn extract_agg_expr(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<AggregateExpr, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::AggExpr([func_id, distinct_id, alias_id]) = node {
            let (function, arg) = extract_agg_function(egraph, *func_id)?;
            let distinct = extract_distinct_flag(egraph, *distinct_id)?;
            let alias = extract_optional_symbol(egraph, *alias_id)?;
            return Ok(AggregateExpr {
                function,
                arg,
                distinct,
                alias,
            });
        }
    }
    Err(EGraphError::ExtractionError("expected AggExpr node".into()))
}

fn extract_agg_function(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<(AggregateFunction, Option<Expr>), EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        let (func, arg_id) = match node {
            RelLang::Count([a]) => (AggregateFunction::Count, *a),
            RelLang::Sum([a]) => (AggregateFunction::Sum, *a),
            RelLang::Avg([a]) => (AggregateFunction::Avg, *a),
            RelLang::Min([a]) => (AggregateFunction::Min, *a),
            RelLang::Max([a]) => (AggregateFunction::Max, *a),
            _ => continue,
        };
        let arg = extract_optional_expr(egraph, arg_id)?;
        return Ok((func, arg));
    }
    Err(EGraphError::ExtractionError(
        "expected aggregate function node".into(),
    ))
}

fn extract_optional_expr(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Option<Expr>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::Nil = node {
            return Ok(None);
        }
    }
    Ok(Some(extract_scalar_expr(egraph, id)?))
}

fn extract_optional_symbol(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Option<String>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::Nil = node {
            return Ok(None);
        }
    }
    Ok(Some(extract_symbol(egraph, id)?))
}

fn extract_distinct_flag(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<bool, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::Distinct => return Ok(true),
            RelLang::All => return Ok(false),
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected Distinct/All flag".into(),
    ))
}

fn extract_window_expr_list(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<WindowExpr>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::List(ids) = node {
            let mut exprs = Vec::with_capacity(ids.len());
            for &child_id in ids.iter() {
                exprs.push(extract_window_expr(egraph, child_id)?);
            }
            return Ok(exprs);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected List node for window expressions".into(),
    ))
}

fn extract_window_expr(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<WindowExpr, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::WindowExprNode([fn_id, arg_id, part_id, order_id, frame_id, alias_id]) =
            node
        {
            let function = extract_window_function(egraph, *fn_id)?;
            let arg = extract_optional_expr(egraph, *arg_id)?;
            let partition_by = extract_expr_list(egraph, *part_id)?;
            let order_by = extract_sort_key_list(egraph, *order_id)?;
            let frame = extract_window_frame(egraph, *frame_id)?;
            let alias = extract_optional_symbol(egraph, *alias_id)?;
            return Ok(WindowExpr {
                function,
                arg,
                partition_by,
                order_by,
                frame,
                alias,
            });
        }
    }
    Err(EGraphError::ExtractionError(
        "expected WindowExprNode".into(),
    ))
}

fn extract_window_function(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<WindowFunction, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::WindowFn([name_id]) = node {
            let name = extract_symbol(egraph, *name_id)?;
            let func = match name.as_str() {
                "RowNumber" => WindowFunction::RowNumber,
                "Rank" => WindowFunction::Rank,
                "DenseRank" => WindowFunction::DenseRank,
                "PercentRank" => WindowFunction::PercentRank,
                "Ntile" => WindowFunction::Ntile,
                "Lag" => WindowFunction::Lag,
                "Lead" => WindowFunction::Lead,
                "FirstValue" => WindowFunction::FirstValue,
                "LastValue" => WindowFunction::LastValue,
                "NthValue" => WindowFunction::NthValue,
                "Avg" => WindowFunction::Avg,
                "Sum" => WindowFunction::Sum,
                "Count" => WindowFunction::Count,
                "Min" => WindowFunction::Min,
                "Max" => WindowFunction::Max,
                other => {
                    return Err(EGraphError::ExtractionError(format!(
                        "unknown window function: {other}"
                    )));
                }
            };
            return Ok(func);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected WindowFn node".into(),
    ))
}

fn extract_window_frame(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Option<WindowFrame>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::Nil = node {
            return Ok(None);
        }
        if let RelLang::WindowFrameNode([mode_id, start_id, end_id]) = node {
            let mode = extract_frame_mode(egraph, *mode_id)?;
            let start = extract_frame_bound(egraph, *start_id)?;
            let end = extract_frame_bound(egraph, *end_id)?;
            return Ok(Some(WindowFrame { mode, start, end }));
        }
    }
    Err(EGraphError::ExtractionError(
        "expected WindowFrameNode or Nil".into(),
    ))
}

fn extract_frame_mode(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<WindowFrameMode, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::FrameRows => return Ok(WindowFrameMode::Rows),
            RelLang::FrameRange => return Ok(WindowFrameMode::Range),
            RelLang::FrameGroups => return Ok(WindowFrameMode::Groups),
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected frame mode node".into(),
    ))
}

fn extract_frame_bound(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<WindowFrameBound, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::FrameUnboundedPreceding => {
                return Ok(WindowFrameBound::UnboundedPreceding);
            }
            RelLang::FramePreceding([n_id]) => {
                let s = extract_symbol(egraph, *n_id)?;
                let n = s.parse::<u64>().map_err(|e| {
                    EGraphError::ExtractionError(format!("invalid frame bound: {e}"))
                })?;
                return Ok(WindowFrameBound::Preceding(n));
            }
            RelLang::FrameCurrentRow => {
                return Ok(WindowFrameBound::CurrentRow);
            }
            RelLang::FrameFollowing([n_id]) => {
                let s = extract_symbol(egraph, *n_id)?;
                let n = s.parse::<u64>().map_err(|e| {
                    EGraphError::ExtractionError(format!("invalid frame bound: {e}"))
                })?;
                return Ok(WindowFrameBound::Following(n));
            }
            RelLang::FrameUnboundedFollowing => {
                return Ok(WindowFrameBound::UnboundedFollowing);
            }
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected frame bound node".into(),
    ))
}

fn extract_values_row(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<Expr>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::ValuesRow(ids) = node {
            let mut cells = Vec::with_capacity(ids.len());
            for &cell_id in ids.iter() {
                cells.push(extract_scalar_expr(egraph, cell_id)?);
            }
            return Ok(cells);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected ValuesRow node".into(),
    ))
}

fn extract_sort_key_list(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<Vec<SortKey>, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::List(ids) = node {
            let mut keys = Vec::with_capacity(ids.len());
            for &child_id in ids.iter() {
                keys.push(extract_sort_key(egraph, child_id)?);
            }
            return Ok(keys);
        }
    }
    Err(EGraphError::ExtractionError(
        "expected List node for sort keys".into(),
    ))
}

fn extract_sort_key(egraph: &EGraph<RelLang, RelAnalysis>, id: Id) -> Result<SortKey, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        if let RelLang::SortKey([expr_id, dir_id, nulls_id]) = node {
            let expr = extract_scalar_expr(egraph, *expr_id)?;
            let direction = extract_sort_direction(egraph, *dir_id)?;
            let nulls = extract_null_ordering(egraph, *nulls_id)?;
            return Ok(ra_core::algebra::SortKey {
                expr,
                direction,
                nulls,
            });
        }
    }
    Err(EGraphError::ExtractionError("expected SortKey node".into()))
}

fn extract_sort_direction(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<SortDirection, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::Asc => return Ok(SortDirection::Asc),
            RelLang::Desc => return Ok(SortDirection::Desc),
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected Asc/Desc node".into(),
    ))
}

fn extract_null_ordering(
    egraph: &EGraph<RelLang, RelAnalysis>,
    id: Id,
) -> Result<NullOrdering, EGraphError> {
    let canonical = egraph.find(id);
    for node in &egraph[canonical].nodes {
        match node {
            RelLang::NullsFirst => return Ok(NullOrdering::First),
            RelLang::NullsLast => return Ok(NullOrdering::Last),
            _ => {}
        }
    }
    Err(EGraphError::ExtractionError(
        "expected NullsFirst/NullsLast node".into(),
    ))
}
