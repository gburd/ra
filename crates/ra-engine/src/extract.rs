//! Cost-based plan extraction from the e-graph.
//!
//! After equality saturation explores the space of equivalent plans,
//! the extractor selects the cheapest plan using a cost model informed
//! by table statistics.

use std::collections::HashMap;
use std::hash::BuildHasher;

use egg::{Id, Language, RecExpr};
use ra_core::algebra::RelExpr;
use ra_core::statistics::Statistics;
use ra_stats::accuracy::Staleness;

use crate::analysis::RelAnalysis;
use crate::cost::IntegratedCostFn;
use crate::egraph::{EGraphError, RelLang};

/// Cost function for plan extraction from the e-graph.
///
/// Assigns a numeric cost to each node type based on hardware characteristics.
/// Costs are adjusted based on CPU speed, cache size, storage bandwidth,
/// and available SIMD instructions.
#[derive(Debug)]
pub struct RelCostFn {
    hardware: ra_hardware::HardwareProfile,
}

impl RelCostFn {
    /// Create a new cost function with the given hardware profile.
    #[must_use]
    pub fn new(hardware: ra_hardware::HardwareProfile) -> Self {
        Self { hardware }
    }
}

impl egg::CostFunction<RelLang> for RelCostFn {
    type Cost = f64;

    fn cost<C>(&mut self, enode: &RelLang, mut costs: C) -> Self::Cost
    where
        C: FnMut(Id) -> Self::Cost,
    {
        let base_cost = match enode {
            RelLang::Scan([table_id]) => {
                // Scan cost depends on storage bandwidth
                // Higher bandwidth = lower cost
                let storage_factor = 100.0 / self.hardware.storage_bandwidth_gbps;
                return costs(*table_id) + (100.0 * storage_factor);
            }
            RelLang::ScanAlias([table_id, alias_id]) => {
                let storage_factor = 100.0 / self.hardware.storage_bandwidth_gbps;
                return costs(*table_id) + costs(*alias_id) + (100.0 * storage_factor);
            }
            RelLang::Filter(_) | RelLang::Project(_) => {
                // Filter/project cost depends on SIMD width
                // Wider SIMD = lower per-row cost
                let simd_factor = 256.0 / f64::from(self.hardware.simd_width_bits);
                1.0 * simd_factor
            }
            RelLang::Join(_) => {
                // Join cost depends on cache size and memory bandwidth
                // Larger cache = better hash table performance
                #[allow(clippy::cast_precision_loss)]
                let cache_mb = self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb; // Normalize to 16 MB baseline
                500.0 * cache_factor
            }
            RelLang::Aggregate(_) => {
                // Aggregate cost depends on cache and parallelism
                #[allow(clippy::cast_precision_loss)]
                let cache_mb = self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb;
                200.0 * cache_factor
            }
            RelLang::Sort(_) => {
                // Sort cost depends on CPU cores (parallel sort)
                let parallelism_factor = 8.0 / f64::from(self.hardware.cpu_cores);
                150.0 * parallelism_factor.max(0.5) // Don't over-penalize many-core systems
            }
            RelLang::Limit(_) => 0.5,
            RelLang::Union(_) | RelLang::Intersect(_) | RelLang::Except(_) => 50.0,
            RelLang::RecursiveCTE(_) => 1000.0,
            RelLang::CTE(_) => 10.0,
            RelLang::Window(_) => {
                let parallelism_factor =
                    8.0 / f64::from(self.hardware.cpu_cores);
                200.0 * parallelism_factor.max(0.5)
            }
            RelLang::DistinctRel(_) => {
                #[allow(clippy::cast_precision_loss)]
                let cache_mb = self.hardware.l3_cache_bytes
                    as f64
                    / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb;
                150.0 * cache_factor
            }
            RelLang::Values(_) => 1.0,
            _ => 0.1,
        };

        let child_cost: f64 = enode.children().iter().map(|child| costs(*child)).sum();

        base_cost + child_cost
    }
}

/// Extract the lowest-cost plan from the e-graph.
///
/// Uses both the hardware profile and table statistics to compute
/// costs. When table statistics are available, staleness adjustments
/// inflate row count estimates to bias toward robust plans.
///
/// # Errors
///
/// Returns an error if the extracted nodes cannot be converted
/// back to a [`RelExpr`].
pub fn extract_best<S: BuildHasher>(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    root: Id,
    table_stats: &HashMap<String, Statistics, S>,
    hardware: &ra_hardware::HardwareProfile,
) -> Result<RelExpr, EGraphError> {
    if table_stats.is_empty() {
        let cost_fn = RelCostFn::new(hardware.clone());
        let extractor = egg::Extractor::new(egraph, cost_fn);
        let (_, best_expr) = extractor.find_best(root);
        rec_expr_to_rel_expr(&best_expr)
    } else {
        let stats: HashMap<String, Statistics> = table_stats
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let staleness_map: HashMap<String, Staleness> = stats
            .keys()
            .map(|k| (k.clone(), Staleness::Fresh))
            .collect();
        let cost_fn = IntegratedCostFn::new(
            hardware.clone(),
            stats,
            staleness_map,
        );
        let extractor = egg::Extractor::new(egraph, cost_fn);
        let (_, best_expr) = extractor.find_best(root);
        rec_expr_to_rel_expr(&best_expr)
    }
}

/// Extract the lowest-cost plan using staleness-aware statistics.
///
/// Unlike [`extract_best`], this function accepts per-table staleness
/// information, allowing the cost function to inflate estimates for
/// tables with stale statistics.
///
/// # Errors
///
/// Returns an error if the extracted nodes cannot be converted
/// back to a [`RelExpr`].
pub fn extract_best_with_staleness<S: BuildHasher, S2: BuildHasher>(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    root: Id,
    table_stats: &HashMap<String, Statistics, S>,
    staleness_map: &HashMap<String, Staleness, S2>,
    hardware: &ra_hardware::HardwareProfile,
) -> Result<RelExpr, EGraphError> {
    let cost_fn = IntegratedCostFn::new(
        hardware.clone(),
        table_stats.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        staleness_map.iter().map(|(k, v)| (k.clone(), *v)).collect(),
    );
    let extractor = egg::Extractor::new(egraph, cost_fn);
    let (_, best_expr) = extractor.find_best(root);
    rec_expr_to_rel_expr(&best_expr)
}

/// Convert a [`RecExpr`] back to a [`RelExpr`].
///
/// # Errors
///
/// Returns an error if the `RecExpr` contains unsupported nodes.
pub fn rec_expr_to_rel_expr(rec: &RecExpr<RelLang>) -> Result<RelExpr, EGraphError> {
    let nodes = rec.as_ref();
    if nodes.is_empty() {
        return Err(EGraphError::ExtractionError("empty RecExpr".into()));
    }
    convert_node(nodes, nodes.len() - 1)
}

fn convert_node(nodes: &[RelLang], idx: usize) -> Result<RelExpr, EGraphError> {
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
        RelLang::Join(ids) => convert_join(nodes, ids),
        RelLang::Aggregate(ids) => convert_aggregate(nodes, ids),
        RelLang::Sort([keys_id, input_id]) => Ok(RelExpr::Sort {
            keys: convert_sort_key_list(nodes, id(*keys_id))?,
            input: Box::new(convert_node(nodes, id(*input_id))?),
        }),
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
                cycle_detection: Some(
                    ra_core::algebra::CycleDetection {
                        track_columns: vec![],
                        max_depth: Some(1000),
                        cycle_mark_column: None,
                        path_column: None,
                    },
                ),
            })
        }
        RelLang::CTE([name_id, def_id, body_id]) => {
            let name = get_symbol(nodes, id(*name_id))?;
            let definition =
                convert_node(nodes, id(*def_id))?;
            let body =
                convert_node(nodes, id(*body_id))?;
            Ok(RelExpr::CTE {
                name,
                definition: Box::new(definition),
                body: Box::new(body),
            })
        }
        RelLang::Window([fns_id, input_id]) => {
            let functions =
                convert_window_expr_list(nodes, id(*fns_id))?;
            let input =
                convert_node(nodes, id(*input_id))?;
            Ok(RelExpr::Window {
                functions,
                input: Box::new(input),
            })
        }
        RelLang::DistinctRel([input_id]) => {
            let input =
                convert_node(nodes, id(*input_id))?;
            Ok(RelExpr::Distinct {
                input: Box::new(input),
            })
        }
        RelLang::Values(row_ids) => {
            let mut rows =
                Vec::with_capacity(row_ids.len());
            for &row_id in row_ids.iter() {
                rows.push(convert_values_row(
                    nodes,
                    id(row_id),
                )?);
            }
            Ok(RelExpr::Values { rows })
        }
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
    Ok(RelExpr::Aggregate {
        group_by: convert_scalar_list(nodes, id(*groups_id))?,
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

fn convert_scalar(nodes: &[RelLang], idx: usize) -> Result<ra_core::expr::Expr, EGraphError> {
    use ra_core::expr::{ColumnRef, Const, Expr};

    match &nodes[idx] {
        RelLang::Col([name_id]) => {
            let name = get_symbol(nodes, id(*name_id))?;
            Ok(Expr::Column(ColumnRef::new(name)))
        }
        RelLang::QCol([table_id, name_id]) => {
            let table = get_symbol(nodes, id(*table_id))?;
            let name = get_symbol(nodes, id(*name_id))?;
            Ok(Expr::Column(ColumnRef::qualified(table, name)))
        }
        RelLang::ConstNull => Ok(Expr::Const(Const::Null)),
        RelLang::ConstBool([v]) => convert_const_bool(nodes, *v),
        RelLang::ConstInt([v]) => convert_const_int(nodes, *v),
        RelLang::ConstFloat([v]) => convert_const_float(nodes, *v),
        RelLang::ConstStr([v]) => Ok(Expr::Const(Const::String(get_symbol(nodes, id(*v))?))),
        node => convert_scalar_operator(nodes, idx, node),
    }
}

fn convert_const_bool(nodes: &[RelLang], val_id: Id) -> Result<ra_core::expr::Expr, EGraphError> {
    let b = convert_bool_flag(nodes, id(val_id))?;
    Ok(ra_core::expr::Expr::Const(ra_core::expr::Const::Bool(b)))
}

fn convert_const_int(nodes: &[RelLang], val_id: Id) -> Result<ra_core::expr::Expr, EGraphError> {
    let s = get_symbol(nodes, id(val_id))?;
    let i = s
        .parse::<i64>()
        .map_err(|e| EGraphError::ExtractionError(format!("invalid int: {e}")))?;
    Ok(ra_core::expr::Expr::Const(ra_core::expr::Const::Int(i)))
}

fn convert_const_float(nodes: &[RelLang], val_id: Id) -> Result<ra_core::expr::Expr, EGraphError> {
    let s = get_symbol(nodes, id(val_id))?;
    let f = s
        .parse::<f64>()
        .map_err(|e| EGraphError::ExtractionError(format!("invalid float: {e}")))?;
    Ok(ra_core::expr::Expr::Const(ra_core::expr::Const::Float(f)))
}

fn convert_scalar_operator(
    nodes: &[RelLang],
    idx: usize,
    node: &RelLang,
) -> Result<ra_core::expr::Expr, EGraphError> {
    use ra_core::expr::Expr;

    if let Some((op, l, r)) = try_binop(node) {
        return make_binop(nodes, op, id(l), id(r));
    }
    if let Some((op, operand)) = try_unaryop(node) {
        let inner = convert_scalar(nodes, id(operand))?;
        return Ok(Expr::UnaryOp {
            op,
            operand: Box::new(inner),
        });
    }
    if let RelLang::Func(ids) = node {
        if ids.is_empty() {
            return Err(EGraphError::ExtractionError("empty function call".into()));
        }
        let name = get_symbol(nodes, id(ids[0]))?;
        let mut args = Vec::with_capacity(ids.len() - 1);
        for &fid in &ids[1..] {
            args.push(convert_scalar(nodes, id(fid))?);
        }
        return Ok(Expr::Function { name, args });
    }
    Err(EGraphError::ExtractionError(format!(
        "unexpected scalar node at index {idx}: {node:?}"
    )))
}

fn try_binop(node: &RelLang) -> Option<(ra_core::expr::BinOp, Id, Id)> {
    use ra_core::expr::BinOp;
    let (op, l, r) = match node {
        RelLang::Add([l, r]) => (BinOp::Add, *l, *r),
        RelLang::Sub([l, r]) => (BinOp::Sub, *l, *r),
        RelLang::Mul([l, r]) => (BinOp::Mul, *l, *r),
        RelLang::Div([l, r]) => (BinOp::Div, *l, *r),
        RelLang::Mod([l, r]) => (BinOp::Mod, *l, *r),
        RelLang::Eq([l, r]) => (BinOp::Eq, *l, *r),
        RelLang::Ne([l, r]) => (BinOp::Ne, *l, *r),
        RelLang::Lt([l, r]) => (BinOp::Lt, *l, *r),
        RelLang::Le([l, r]) => (BinOp::Le, *l, *r),
        RelLang::Gt([l, r]) => (BinOp::Gt, *l, *r),
        RelLang::Ge([l, r]) => (BinOp::Ge, *l, *r),
        RelLang::And([l, r]) => (BinOp::And, *l, *r),
        RelLang::Or([l, r]) => (BinOp::Or, *l, *r),
        RelLang::Concat([l, r]) => (BinOp::Concat, *l, *r),
        RelLang::JsonAccess([l, r]) => (BinOp::JsonAccess, *l, *r),
        _ => return None,
    };
    Some((op, l, r))
}

fn try_unaryop(node: &RelLang) -> Option<(ra_core::expr::UnaryOp, Id)> {
    use ra_core::expr::UnaryOp;
    let (op, operand) = match node {
        RelLang::Not([o]) => (UnaryOp::Not, *o),
        RelLang::IsNull([o]) => (UnaryOp::IsNull, *o),
        RelLang::IsNotNull([o]) => (UnaryOp::IsNotNull, *o),
        RelLang::Neg([o]) => (UnaryOp::Neg, *o),
        _ => return None,
    };
    Some((op, operand))
}

fn make_binop(
    nodes: &[RelLang],
    op: ra_core::expr::BinOp,
    left_idx: usize,
    right_idx: usize,
) -> Result<ra_core::expr::Expr, EGraphError> {
    let left = convert_scalar(nodes, left_idx)?;
    let right = convert_scalar(nodes, right_idx)?;
    Ok(ra_core::expr::Expr::BinOp {
        op,
        left: Box::new(left),
        right: Box::new(right),
    })
}

fn id(egg_id: Id) -> usize {
    usize::from(egg_id)
}

fn get_symbol(nodes: &[RelLang], idx: usize) -> Result<String, EGraphError> {
    if let RelLang::Symbol(s) = &nodes[idx] {
        Ok(s.to_string())
    } else {
        Err(EGraphError::ExtractionError(format!(
            "expected Symbol at index {idx}, got {:?}",
            nodes[idx]
        )))
    }
}

fn convert_bool_flag(nodes: &[RelLang], idx: usize) -> Result<bool, EGraphError> {
    match &nodes[idx] {
        RelLang::True => Ok(true),
        RelLang::False => Ok(false),
        other => Err(EGraphError::ExtractionError(format!(
            "expected True/False, got {other:?}"
        ))),
    }
}

fn convert_join_type(
    nodes: &[RelLang],
    idx: usize,
) -> Result<ra_core::algebra::JoinType, EGraphError> {
    use ra_core::algebra::JoinType;
    match &nodes[idx] {
        RelLang::Inner => Ok(JoinType::Inner),
        RelLang::LeftOuter => Ok(JoinType::LeftOuter),
        RelLang::RightOuter => Ok(JoinType::RightOuter),
        RelLang::FullOuter => Ok(JoinType::FullOuter),
        RelLang::Cross => Ok(JoinType::Cross),
        RelLang::Semi => Ok(JoinType::Semi),
        RelLang::Anti => Ok(JoinType::Anti),
        other => Err(EGraphError::ExtractionError(format!(
            "expected join type, got {other:?}"
        ))),
    }
}

fn convert_projection_list(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Vec<ra_core::algebra::ProjectionColumn>, EGraphError> {
    let RelLang::List(ids) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(
            "expected List for projection columns".into(),
        ));
    };
    ids.iter()
        .map(|&child| convert_proj_col(nodes, id(child)))
        .collect()
}

fn convert_proj_col(
    nodes: &[RelLang],
    idx: usize,
) -> Result<ra_core::algebra::ProjectionColumn, EGraphError> {
    match &nodes[idx] {
        RelLang::ProjCol([expr_id]) => Ok(ra_core::algebra::ProjectionColumn {
            expr: convert_scalar(nodes, id(*expr_id))?,
            alias: None,
        }),
        RelLang::ProjAlias([expr_id, alias_id]) => Ok(ra_core::algebra::ProjectionColumn {
            expr: convert_scalar(nodes, id(*expr_id))?,
            alias: Some(get_symbol(nodes, id(*alias_id))?),
        }),
        other => Err(EGraphError::ExtractionError(format!(
            "expected ProjCol/ProjAlias, got {other:?}"
        ))),
    }
}

fn convert_scalar_list(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Vec<ra_core::expr::Expr>, EGraphError> {
    let RelLang::List(ids) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(
            "expected List for expression list".into(),
        ));
    };
    ids.iter()
        .map(|&child| convert_scalar(nodes, id(child)))
        .collect()
}

fn convert_aggregate_list(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Vec<ra_core::algebra::AggregateExpr>, EGraphError> {
    let RelLang::List(ids) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(
            "expected List for aggregate list".into(),
        ));
    };
    ids.iter()
        .map(|&child| convert_agg_expr(nodes, id(child)))
        .collect()
}

fn convert_agg_expr(
    nodes: &[RelLang],
    idx: usize,
) -> Result<ra_core::algebra::AggregateExpr, EGraphError> {
    let RelLang::AggExpr([func_id, distinct_id, alias_id]) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(format!(
            "expected AggExpr, got {:?}",
            nodes[idx]
        )));
    };
    let (function, arg) = convert_agg_function(nodes, id(*func_id))?;
    let distinct = match &nodes[id(*distinct_id)] {
        RelLang::Distinct => true,
        RelLang::All => false,
        other => {
            return Err(EGraphError::ExtractionError(format!(
                "expected Distinct/All, got {other:?}"
            )))
        }
    };
    let alias = match &nodes[id(*alias_id)] {
        RelLang::Nil => None,
        RelLang::Symbol(s) => Some(s.to_string()),
        other => {
            return Err(EGraphError::ExtractionError(format!(
                "expected Nil/Symbol for alias, got {other:?}"
            )))
        }
    };
    Ok(ra_core::algebra::AggregateExpr {
        function,
        arg,
        distinct,
        alias,
    })
}

fn convert_agg_function(
    nodes: &[RelLang],
    idx: usize,
) -> Result<
    (
        ra_core::algebra::AggregateFunction,
        Option<ra_core::expr::Expr>,
    ),
    EGraphError,
> {
    use ra_core::algebra::AggregateFunction;
    let (func, arg_id) = match &nodes[idx] {
        RelLang::Count([a]) => (AggregateFunction::Count, *a),
        RelLang::Sum([a]) => (AggregateFunction::Sum, *a),
        RelLang::Avg([a]) => (AggregateFunction::Avg, *a),
        RelLang::Min([a]) => (AggregateFunction::Min, *a),
        RelLang::Max([a]) => (AggregateFunction::Max, *a),
        other => {
            return Err(EGraphError::ExtractionError(format!(
                "expected aggregate function, got {other:?}"
            )))
        }
    };
    let arg_idx = id(arg_id);
    let arg = match &nodes[arg_idx] {
        RelLang::Nil => None,
        _ => Some(convert_scalar(nodes, arg_idx)?),
    };
    Ok((func, arg))
}

fn convert_window_expr_list(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Vec<ra_core::algebra::WindowExpr>, EGraphError> {
    let RelLang::List(ids) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(
            "expected List for window expressions".into(),
        ));
    };
    ids.iter()
        .map(|&child| convert_window_expr(nodes, id(child)))
        .collect()
}

fn convert_window_expr(
    nodes: &[RelLang],
    idx: usize,
) -> Result<ra_core::algebra::WindowExpr, EGraphError> {
    let RelLang::WindowExprNode([
        fn_id,
        arg_id,
        part_id,
        order_id,
        frame_id,
        alias_id,
    ]) = &nodes[idx]
    else {
        return Err(EGraphError::ExtractionError(format!(
            "expected WindowExprNode, got {:?}",
            nodes[idx]
        )));
    };
    let function = convert_window_fn(nodes, id(*fn_id))?;
    let arg = convert_optional_scalar(nodes, id(*arg_id))?;
    let partition_by =
        convert_scalar_list(nodes, id(*part_id))?;
    let order_by =
        convert_sort_key_list(nodes, id(*order_id))?;
    let frame = convert_window_frame(nodes, id(*frame_id))?;
    let alias = convert_optional_symbol(nodes, id(*alias_id))?;
    Ok(ra_core::algebra::WindowExpr {
        function,
        arg,
        partition_by,
        order_by,
        frame,
        alias,
    })
}

fn convert_window_fn(
    nodes: &[RelLang],
    idx: usize,
) -> Result<ra_core::algebra::WindowFunction, EGraphError> {
    use ra_core::algebra::WindowFunction;
    let RelLang::WindowFn([name_id]) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(format!(
            "expected WindowFn, got {:?}",
            nodes[idx]
        )));
    };
    let name = get_symbol(nodes, id(*name_id))?;
    match name.as_str() {
        "RowNumber" => Ok(WindowFunction::RowNumber),
        "Rank" => Ok(WindowFunction::Rank),
        "DenseRank" => Ok(WindowFunction::DenseRank),
        "PercentRank" => Ok(WindowFunction::PercentRank),
        "Ntile" => Ok(WindowFunction::Ntile),
        "Lag" => Ok(WindowFunction::Lag),
        "Lead" => Ok(WindowFunction::Lead),
        "FirstValue" => Ok(WindowFunction::FirstValue),
        "LastValue" => Ok(WindowFunction::LastValue),
        "NthValue" => Ok(WindowFunction::NthValue),
        "Avg" => Ok(WindowFunction::Avg),
        "Sum" => Ok(WindowFunction::Sum),
        "Count" => Ok(WindowFunction::Count),
        "Min" => Ok(WindowFunction::Min),
        "Max" => Ok(WindowFunction::Max),
        other => Err(EGraphError::ExtractionError(format!(
            "unknown window function: {other}"
        ))),
    }
}

fn convert_window_frame(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Option<ra_core::algebra::WindowFrame>, EGraphError>
{
    use ra_core::algebra::{
        WindowFrame, WindowFrameMode,
    };
    if let RelLang::Nil = &nodes[idx] {
        return Ok(None);
    }
    let RelLang::WindowFrameNode([
        mode_id,
        start_id,
        end_id,
    ]) = &nodes[idx]
    else {
        return Err(EGraphError::ExtractionError(format!(
            "expected WindowFrameNode, got {:?}",
            nodes[idx]
        )));
    };
    let mode = match &nodes[id(*mode_id)] {
        RelLang::FrameRows => WindowFrameMode::Rows,
        RelLang::FrameRange => WindowFrameMode::Range,
        RelLang::FrameGroups => WindowFrameMode::Groups,
        other => {
            return Err(EGraphError::ExtractionError(
                format!(
                    "expected frame mode, got {other:?}"
                ),
            ))
        }
    };
    let start = convert_frame_bound(nodes, id(*start_id))?;
    let end = convert_frame_bound(nodes, id(*end_id))?;
    Ok(Some(WindowFrame { mode, start, end }))
}

fn convert_frame_bound(
    nodes: &[RelLang],
    idx: usize,
) -> Result<ra_core::algebra::WindowFrameBound, EGraphError> {
    use ra_core::algebra::WindowFrameBound;
    match &nodes[idx] {
        RelLang::FrameUnboundedPreceding => {
            Ok(WindowFrameBound::UnboundedPreceding)
        }
        RelLang::FramePreceding([n_id]) => {
            let s = get_symbol(nodes, id(*n_id))?;
            let n = s.parse::<u64>().map_err(|e| {
                EGraphError::ExtractionError(format!(
                    "invalid frame bound: {e}"
                ))
            })?;
            Ok(WindowFrameBound::Preceding(n))
        }
        RelLang::FrameCurrentRow => {
            Ok(WindowFrameBound::CurrentRow)
        }
        RelLang::FrameFollowing([n_id]) => {
            let s = get_symbol(nodes, id(*n_id))?;
            let n = s.parse::<u64>().map_err(|e| {
                EGraphError::ExtractionError(format!(
                    "invalid frame bound: {e}"
                ))
            })?;
            Ok(WindowFrameBound::Following(n))
        }
        RelLang::FrameUnboundedFollowing => {
            Ok(WindowFrameBound::UnboundedFollowing)
        }
        other => Err(EGraphError::ExtractionError(format!(
            "expected frame bound, got {other:?}"
        ))),
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

fn convert_optional_scalar(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Option<ra_core::expr::Expr>, EGraphError> {
    if let RelLang::Nil = &nodes[idx] {
        return Ok(None);
    }
    Ok(Some(convert_scalar(nodes, idx)?))
}

fn convert_optional_symbol(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Option<String>, EGraphError> {
    match &nodes[idx] {
        RelLang::Nil => Ok(None),
        RelLang::Symbol(s) => Ok(Some(s.to_string())),
        other => Err(EGraphError::ExtractionError(format!(
            "expected Nil/Symbol, got {other:?}"
        ))),
    }
}

fn convert_sort_key_list(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Vec<ra_core::algebra::SortKey>, EGraphError> {
    let RelLang::List(ids) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(
            "expected List for sort keys".into(),
        ));
    };
    ids.iter()
        .map(|&child| convert_sort_key(nodes, id(child)))
        .collect()
}

fn convert_sort_key(
    nodes: &[RelLang],
    idx: usize,
) -> Result<ra_core::algebra::SortKey, EGraphError> {
    let RelLang::SortKey([expr_id, dir_id, nulls_id]) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(format!(
            "expected SortKey, got {:?}",
            nodes[idx]
        )));
    };
    let direction = match &nodes[id(*dir_id)] {
        RelLang::Asc => ra_core::algebra::SortDirection::Asc,
        RelLang::Desc => ra_core::algebra::SortDirection::Desc,
        other => {
            return Err(EGraphError::ExtractionError(format!(
                "expected Asc/Desc, got {other:?}"
            )))
        }
    };
    let nulls = match &nodes[id(*nulls_id)] {
        RelLang::NullsFirst => ra_core::algebra::NullOrdering::First,
        RelLang::NullsLast => ra_core::algebra::NullOrdering::Last,
        other => {
            return Err(EGraphError::ExtractionError(format!(
                "expected NullsFirst/NullsLast, got {other:?}"
            )))
        }
    };
    Ok(ra_core::algebra::SortKey {
        expr: convert_scalar(nodes, id(*expr_id))?,
        direction,
        nulls,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::egraph::{to_rec_expr, Optimizer};
    use ra_core::algebra::{
        AggregateExpr, AggregateFunction, JoinType, NullOrdering, ProjectionColumn, RelExpr,
        SortDirection, SortKey,
    };
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    #[test]
    fn extract_simple_scan() {
        let expr = RelExpr::scan("users");
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_filter() {
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_join() {
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("a", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("b", "a_id"))),
            },
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_limit() {
        let expr = RelExpr::scan("users").limit(10, 5);
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_union() {
        let expr = RelExpr::Union {
            all: true,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_project() {
        let expr = RelExpr::scan("users").project(vec![
            ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("name")),
                alias: None,
            },
            ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("age")),
                alias: Some("user_age".into()),
            },
        ]);
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_aggregate() {
        let expr = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("dept"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("cnt".into()),
            }],
            input: Box::new(RelExpr::scan("employees")),
        };
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_sort() {
        let expr = RelExpr::Sort {
            keys: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("name")),
                direction: SortDirection::Asc,
                nulls: NullOrdering::Last,
            }],
            input: Box::new(RelExpr::scan("users")),
        };
        let rec = to_rec_expr(&expr).expect("conversion should succeed");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction should succeed");
        assert_eq!(result, expr);
    }

    #[test]
    fn optimizer_selects_plan() {
        let mut optimizer = Optimizer::new();
        optimizer.add_table_stats(
            "big_table",
            ra_core::statistics::Statistics::new(1_000_000.0),
        );
        optimizer.add_table_stats("small_table", ra_core::statistics::Statistics::new(100.0));

        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("a"))),
                right: Box::new(Expr::Column(ColumnRef::new("b"))),
            },
            left: Box::new(RelExpr::scan("big_table")),
            right: Box::new(RelExpr::scan("small_table")),
        };

        let result = optimizer
            .optimize(&expr)
            .expect("optimization should succeed");
        assert!(matches!(result, RelExpr::Join { .. }));
    }
}
