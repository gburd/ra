//! Cost-based plan extraction from the e-graph.
//!
//! After equality saturation explores the space of equivalent plans,
//! the extractor selects the cheapest plan using a cost model informed
//! by table statistics.

use std::collections::HashMap;
use std::hash::BuildHasher;

use egg::{Id, Language, RecExpr};
use ra_core::algebra::RelExpr;
use ra_core::expr::Const;
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
            RelLang::IndexOnlyScan([table_id, _index_id, cols_id, pred_id]) => {
                // Index-only scan: O(log n) -- much cheaper than full table scan.
                // Models B-tree traversal to first/last key.
                let storage_factor = 100.0 / self.hardware.storage_bandwidth_gbps;
                return costs(*table_id) + costs(*cols_id) + costs(*pred_id)
                    + (5.0 * storage_factor);
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
                let cache_mb = self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb; // Normalize to 16 MB baseline
                500.0 * cache_factor
            }
            RelLang::Aggregate(_) => {
                // Aggregate cost depends on cache and parallelism
                let cache_mb = self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb;
                200.0 * cache_factor
            }
            RelLang::Sort(_) => {
                // Sort cost depends on CPU cores (parallel sort)
                let parallelism_factor = 8.0 / f64::from(self.hardware.cpu_cores);
                150.0 * parallelism_factor.max(0.5) // Don't over-penalize many-core systems
            }
            RelLang::IncrementalSort(_) => {
                // Incremental sort is cheaper than full sort: only sorts
                // within prefix groups, so cost is proportional to
                // group_size * log(group_size) instead of n * log(n).
                // Model as 40% of full sort cost (conservative estimate
                // assuming moderate prefix selectivity).
                let parallelism_factor =
                    8.0 / f64::from(self.hardware.cpu_cores);
                60.0 * parallelism_factor.max(0.5)
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
                let cache_mb = self.hardware.l3_cache_bytes
                    as f64
                    / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb;
                150.0 * cache_factor
            }
            RelLang::Values(_) => 1.0,
            RelLang::MetadataLookup(_) => {
                // O(1) metadata lookup, much cheaper than any scan
                return 1.0;
            }
            RelLang::MvScan(_) => {
                // MV scan reads pre-computed, pre-joined data.
                let storage_factor =
                    100.0 / self.hardware.storage_bandwidth_gbps;
                return costs(enode.children()[0])
                    + (15.0 * storage_factor);
            }
            RelLang::Cast(_) => {
                // Type casts are typically very cheap (often free at runtime)
                0.01
            }
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
        // Clone once to create owned HashMap (unavoidable)
        let stats: HashMap<String, Statistics> = table_stats.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        // Create staleness map (all Fresh by default)
        let staleness_map: HashMap<String, Staleness> = stats
            .keys()
            .map(|k| (k.clone(), Staleness::Fresh))
            .collect();

        // IntegratedCostFn::new wraps these in Arc internally, so subsequent
        // clones of IntegratedCostFn are cheap (just Arc reference count increments)
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

/// Extract the lowest-cost plan using cardinality-aware costing.
///
/// Uses ML-based cardinality estimation to scale operator costs
/// based on estimated intermediate result sizes. This produces more
/// accurate cost estimates than pure operator-based costing.
///
/// # Errors
///
/// Returns an error if the extracted nodes cannot be converted
/// back to a [`RelExpr`].
pub fn extract_best_with_cardinality<S: BuildHasher, S2: BuildHasher>(
    egraph: &egg::EGraph<RelLang, RelAnalysis>,
    root: Id,
    table_stats: &HashMap<String, Statistics, S>,
    staleness_map: &HashMap<String, Staleness, S2>,
    hardware: &ra_hardware::HardwareProfile,
) -> Result<RelExpr, EGraphError> {
    let cost_fn = crate::cardinality_cost::CardinalityAwareCostFn::new(
        hardware.clone(),
        table_stats.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        staleness_map.iter().map(|(k, v)| (k.clone(), *v)).collect(),
    );
    let extractor = egg::Extractor::new(egraph, cost_fn);
    let (cost, best_expr) = extractor.find_best(root);
    tracing::debug!("Extracted plan with cardinality-aware cost: {}", cost);
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
        RelLang::IncrementalSort([prefix_keys_id, suffix_keys_id, input_id]) => Ok(RelExpr::IncrementalSort {
            prefix_keys: convert_sort_key_list(nodes, id(*prefix_keys_id))?,
            suffix_keys: convert_sort_key_list(nodes, id(*suffix_keys_id))?,
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
        RelLang::IndexScan([table_id, column_id]) => {
            let table = get_symbol(nodes, id(*table_id))?;
            let column = get_symbol(nodes, id(*column_id))?;
            Ok(RelExpr::IndexScan { table, column })
        }
        RelLang::IndexOnlyScan([
            table_id,
            index_id,
            cols_id,
            pred_id,
        ]) => {
            let table =
                get_symbol(nodes, id(*table_id))?;
            let index =
                get_symbol(nodes, id(*index_id))?;
            let columns =
                convert_projection_list(nodes, id(*cols_id))?;
            let predicate =
                convert_scalar(nodes, id(*pred_id))?;
            Ok(RelExpr::IndexOnlyScan {
                table,
                index,
                columns,
                predicate,
            })
        }
        RelLang::MvScan([view_id, alias_id, _, _]) => {
            let view_name =
                get_symbol(nodes, id(*view_id))?;
            let alias_str =
                get_symbol(nodes, id(*alias_id))?;
            let alias = if alias_str == "auto" || alias_str.is_empty() {
                None
            } else {
                Some(alias_str)
            };
            Ok(RelExpr::MvScan { view_name, alias })
        }
        RelLang::BitmapIndexScan([table_id, index_id, pred_id]) => {
            let table = get_symbol(nodes, id(*table_id))?;
            let index = get_symbol(nodes, id(*index_id))?;
            let predicate =
                convert_scalar(nodes, id(*pred_id))?;
            Ok(RelExpr::BitmapIndexScan {
                table,
                index,
                predicate,
            })
        }
        RelLang::BitmapAnd(input_ids) => {
            let mut inputs =
                Vec::with_capacity(input_ids.len());
            for &input_id in input_ids.iter() {
                inputs.push(Box::new(
                    convert_node(nodes, id(input_id))?,
                ));
            }
            Ok(RelExpr::BitmapAnd { inputs })
        }
        RelLang::BitmapOr(input_ids) => {
            let mut inputs =
                Vec::with_capacity(input_ids.len());
            for &input_id in input_ids.iter() {
                inputs.push(Box::new(
                    convert_node(nodes, id(input_id))?,
                ));
            }
            Ok(RelExpr::BitmapOr { inputs })
        }
        RelLang::BitmapHeapScan([table_id, bitmap_id, recheck_id]) => {
            let table =
                get_symbol(nodes, id(*table_id))?;
            let bitmap =
                convert_node(nodes, id(*bitmap_id))?;
            let recheck_cond =
                if get_symbol(nodes, id(*recheck_id))
                    .map_or(false, |s| s.is_empty())
                {
                    None
                } else {
                    Some(convert_scalar(
                        nodes,
                        id(*recheck_id),
                    )?)
                };
            Ok(RelExpr::BitmapHeapScan {
                table,
                bitmap: Box::new(bitmap),
                recheck_cond,
            })
        }
        RelLang::Func(ids) if !ids.is_empty() => {
            convert_func_as_relational(nodes, ids)
        }
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
        RelLang::HybridScan(_ids) => {
            Ok(RelExpr::Scan {
                table: "hybrid_scan".to_string(),
                alias: Some("hybrid_scan".to_string()),
            })
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

/// Convert a `Func` e-graph node back to a relational operator.
///
/// Unnest and `TableFunction` are encoded as `Func` nodes in the
/// e-graph with a tag symbol as the first child. This function
/// dispatches on the tag to reconstruct the original `RelExpr`.
fn convert_func_as_relational(
    nodes: &[RelLang],
    ids: &[Id],
) -> Result<RelExpr, EGraphError> {
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
            let with_ordinality =
                convert_bool_flag(nodes, id(ids[3]))?;
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
                    "unnest_lateral Func node requires 5 children"
                        .into(),
                ));
            }
            let expr = convert_scalar(nodes, id(ids[1]))?;
            let alias_str = get_symbol(nodes, id(ids[2]))?;
            let alias = if alias_str.is_empty() {
                None
            } else {
                Some(alias_str)
            };
            let with_ordinality =
                convert_bool_flag(nodes, id(ids[3]))?;
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
                    "multi_unnest requires at least 4 children"
                        .into(),
                ));
            }
            let with_ordinality =
                convert_bool_flag(nodes, id(ids[1]))?;
            let mut exprs = Vec::new();
            let mut aliases = Vec::new();
            let mut i = 2;
            while i + 1 < ids.len() {
                exprs.push(convert_scalar(
                    nodes,
                    id(ids[i]),
                )?);
                let alias_str =
                    get_symbol(nodes, id(ids[i + 1]))?;
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
                match convert_scalar(nodes, id(arg_id)) {
                    Ok(expr) => args.push(expr),
                    Err(_) => {
                        // Assume it's a relational input
                        let input =
                            convert_node(nodes, id(arg_id))?;
                        return Ok(RelExpr::TableFunction {
                            name,
                            args,
                            columns: vec![],
                            input: Some(Box::new(input)),
                        });
                    }
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
    // Handle Cast operator
    if let RelLang::Cast([expr_id, type_id]) = node {
        let expr = convert_scalar(nodes, id(*expr_id))?;
        let target_type = get_symbol(nodes, id(*type_id))?;
        return Ok(Expr::Cast {
            expr: Box::new(expr),
            target_type,
        });
    }

    // Handle Vector/FTS operators
    if let RelLang::VectorDistance([metric_id, col_id, target_id]) = node {
        let metric = get_symbol(nodes, id(*metric_id))?;
        let column = convert_scalar(nodes, id(*col_id))?;
        let target = convert_scalar(nodes, id(*target_id))?;
        return Ok(Expr::VectorDistance {
            metric,
            column: Box::new(column),
            target: Box::new(target),
        });
    }
    if let RelLang::FtsMatch([vendor_id, cols_id, query_id, mode_id]) = node {
        let vendor = get_symbol(nodes, id(*vendor_id))?;
        let cols_str = get_symbol(nodes, id(*cols_id))?;
        let columns = cols_str.split(',').map(|s| s.to_string()).collect();
        let query = get_symbol(nodes, id(*query_id))?;
        let mode_str = get_symbol(nodes, id(*mode_id))?;
        let mode = if mode_str.is_empty() {
            None
        } else {
            Some(mode_str)
        };
        return Ok(Expr::FullTextMatch {
            vendor,
            columns,
            query,
            mode,
        });
    }
    if let RelLang::FtsRank([col_id, query_id, algo_id]) = node {
        let col = convert_scalar(nodes, id(*col_id))?;
        let query = convert_scalar(nodes, id(*query_id))?;
        let algo = get_symbol(nodes, id(*algo_id))?;
        // FTS rank is represented as a function call in Expr
        return Ok(Expr::Function {
            name: format!("ts_rank_{}", algo),
            args: vec![col, query],
        });
    }

    if let RelLang::Func(ids) = node {
        if ids.is_empty() {
            return Err(EGraphError::ExtractionError("empty function call".into()));
        }
        let name = get_symbol(nodes, id(ids[0]))?;
        // Reconstruct Array and ArrayIndex from their tagged Func encoding
        if name == "ARRAY" {
            let mut elements = Vec::with_capacity(ids.len() - 1);
            for &fid in &ids[1..] {
                elements.push(convert_scalar(nodes, id(fid))?);
            }
            return Ok(Expr::Array(elements));
        }
        if name == "ARRAY_INDEX" && ids.len() == 3 {
            let array = convert_scalar(nodes, id(ids[1]))?;
            let index = convert_scalar(nodes, id(ids[2]))?;
            return Ok(Expr::ArrayIndex(
                Box::new(array),
                Box::new(index),
            ));
        }
        if name == "ARRAY_SLICE" && ids.len() == 4 {
            let array = convert_scalar(nodes, id(ids[1]))?;
            let start_expr =
                convert_scalar(nodes, id(ids[2]))?;
            let end_expr =
                convert_scalar(nodes, id(ids[3]))?;
            let start = if start_expr == Expr::Const(Const::Null) {
                None
            } else {
                Some(Box::new(start_expr))
            };
            let end = if end_expr == Expr::Const(Const::Null) {
                None
            } else {
                Some(Box::new(end_expr))
            };
            return Ok(Expr::ArraySlice {
                array: Box::new(array),
                start,
                end,
            });
        }
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

    // -- Scan with alias --

    #[test]
    fn extract_scan_alias() {
        let expr = RelExpr::Scan {
            table: "users".into(),
            alias: Some("u".into()),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- Set operations: intersect and except --

    #[test]
    fn extract_intersect() {
        let expr = RelExpr::Intersect {
            all: false,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_except() {
        let expr = RelExpr::Except {
            all: true,
            left: Box::new(RelExpr::scan("x")),
            right: Box::new(RelExpr::scan("y")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_union_not_all() {
        let expr = RelExpr::Union {
            all: false,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- CTE and RecursiveCTE --

    #[test]
    fn extract_cte() {
        let expr = RelExpr::CTE {
            name: "temp".into(),
            definition: Box::new(RelExpr::scan("orders")),
            body: Box::new(RelExpr::scan("temp")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_recursive_cte() {
        let expr = RelExpr::RecursiveCTE {
            name: "ancestors".into(),
            base_case: Box::new(RelExpr::scan("people")),
            recursive_case: Box::new(RelExpr::scan("ancestors")),
            body: Box::new(RelExpr::scan("ancestors")),
            cycle_detection: Some(
                ra_core::algebra::CycleDetection {
                    track_columns: vec![],
                    max_depth: Some(1000),
                    cycle_mark_column: None,
                    path_column: None,
                },
            ),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- Distinct --

    #[test]
    fn extract_distinct() {
        let expr = RelExpr::scan("users").distinct();
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- Values --

    #[test]
    fn extract_values() {
        let expr = RelExpr::Values {
            rows: vec![
                vec![Expr::Const(Const::Int(1)), Expr::Const(Const::String("a".into()))],
                vec![Expr::Const(Const::Int(2)), Expr::Const(Const::String("b".into()))],
            ],
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- Window --

    #[test]
    fn extract_window_row_number() {
        use ra_core::algebra::{
            WindowExpr as WExpr, WindowFunction as WFn,
            WindowFrame, WindowFrameMode, WindowFrameBound,
        };
        let expr = RelExpr::Window {
            functions: vec![WExpr {
                function: WFn::RowNumber,
                arg: None,
                partition_by: vec![Expr::Column(ColumnRef::new("dept"))],
                order_by: vec![SortKey {
                    expr: Expr::Column(ColumnRef::new("salary")),
                    direction: SortDirection::Desc,
                    nulls: NullOrdering::Last,
                }],
                frame: Some(WindowFrame {
                    mode: WindowFrameMode::Rows,
                    start: WindowFrameBound::UnboundedPreceding,
                    end: WindowFrameBound::CurrentRow,
                }),
                alias: Some("rn".into()),
            }],
            input: Box::new(RelExpr::scan("employees")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_window_no_frame() {
        use ra_core::algebra::{
            WindowExpr as WExpr, WindowFunction as WFn,
        };
        let expr = RelExpr::Window {
            functions: vec![WExpr {
                function: WFn::Sum,
                arg: Some(Expr::Column(ColumnRef::new("amount"))),
                partition_by: vec![],
                order_by: vec![],
                frame: None,
                alias: None,
            }],
            input: Box::new(RelExpr::scan("sales")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_window_frame_range_following() {
        use ra_core::algebra::{
            WindowExpr as WExpr, WindowFunction as WFn,
            WindowFrame, WindowFrameMode, WindowFrameBound,
        };
        let expr = RelExpr::Window {
            functions: vec![WExpr {
                function: WFn::Avg,
                arg: Some(Expr::Column(ColumnRef::new("price"))),
                partition_by: vec![],
                order_by: vec![SortKey {
                    expr: Expr::Column(ColumnRef::new("ts")),
                    direction: SortDirection::Asc,
                    nulls: NullOrdering::First,
                }],
                frame: Some(WindowFrame {
                    mode: WindowFrameMode::Range,
                    start: WindowFrameBound::Preceding(3),
                    end: WindowFrameBound::Following(3),
                }),
                alias: Some("moving_avg".into()),
            }],
            input: Box::new(RelExpr::scan("ticks")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_window_frame_groups_unbounded() {
        use ra_core::algebra::{
            WindowExpr as WExpr, WindowFunction as WFn,
            WindowFrame, WindowFrameMode, WindowFrameBound,
        };
        let expr = RelExpr::Window {
            functions: vec![WExpr {
                function: WFn::Count,
                arg: None,
                partition_by: vec![],
                order_by: vec![],
                frame: Some(WindowFrame {
                    mode: WindowFrameMode::Groups,
                    start: WindowFrameBound::UnboundedPreceding,
                    end: WindowFrameBound::UnboundedFollowing,
                }),
                alias: None,
            }],
            input: Box::new(RelExpr::scan("t")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- All join types --

    #[test]
    fn extract_left_outer_join() {
        let cond = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Column(ColumnRef::new("fk"))),
        };
        let expr = RelExpr::Join {
            join_type: JoinType::LeftOuter,
            condition: cond,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_right_outer_join() {
        let cond = Expr::Const(Const::Bool(true));
        let expr = RelExpr::Join {
            join_type: JoinType::RightOuter,
            condition: cond,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_full_outer_join() {
        let cond = Expr::Const(Const::Bool(true));
        let expr = RelExpr::Join {
            join_type: JoinType::FullOuter,
            condition: cond,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_cross_join() {
        let cond = Expr::Const(Const::Bool(true));
        let expr = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: cond,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_semi_join() {
        let cond = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Column(ColumnRef::new("ref"))),
        };
        let expr = RelExpr::Join {
            join_type: JoinType::Semi,
            condition: cond,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_anti_join() {
        let cond = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Column(ColumnRef::new("ref"))),
        };
        let expr = RelExpr::Join {
            join_type: JoinType::Anti,
            condition: cond,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- All aggregate functions --

    #[test]
    fn extract_aggregate_sum() {
        let expr = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(Expr::Column(ColumnRef::new("amount"))),
                distinct: false,
                alias: None,
            }],
            input: Box::new(RelExpr::scan("sales")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_aggregate_avg_distinct() {
        let expr = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("category"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Avg,
                arg: Some(Expr::Column(ColumnRef::new("price"))),
                distinct: true,
                alias: Some("avg_price".into()),
            }],
            input: Box::new(RelExpr::scan("products")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_aggregate_min_max() {
        let expr = RelExpr::Aggregate {
            group_by: vec![],
            aggregates: vec![
                AggregateExpr {
                    function: AggregateFunction::Min,
                    arg: Some(Expr::Column(ColumnRef::new("created"))),
                    distinct: false,
                    alias: Some("earliest".into()),
                },
                AggregateExpr {
                    function: AggregateFunction::Max,
                    arg: Some(Expr::Column(ColumnRef::new("created"))),
                    distinct: false,
                    alias: Some("latest".into()),
                },
            ],
            input: Box::new(RelExpr::scan("events")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- All scalar expression types --

    #[test]
    fn extract_qualified_column() {
        let expr = RelExpr::scan("t").filter(
            Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("t", "id"))),
                right: Box::new(Expr::Const(Const::Int(1))),
            },
        );
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_const_null() {
        let expr = RelExpr::scan("t").filter(
            Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("x"))),
                right: Box::new(Expr::Const(Const::Null)),
            },
        );
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_const_bool() {
        let expr = RelExpr::scan("t").filter(Expr::Const(Const::Bool(false)));
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_const_float() {
        let expr = RelExpr::scan("t").filter(
            Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("price"))),
                right: Box::new(Expr::Const(Const::Float(9.99))),
            },
        );
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_const_string() {
        let expr = RelExpr::scan("t").filter(
            Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("name"))),
                right: Box::new(Expr::Const(Const::String("Alice".into()))),
            },
        );
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- All binary operators --

    #[test]
    fn extract_all_binary_operators() {
        let ops = [
            BinOp::Add, BinOp::Sub, BinOp::Mul, BinOp::Div, BinOp::Mod,
            BinOp::Eq, BinOp::Ne, BinOp::Lt, BinOp::Le, BinOp::Gt,
            BinOp::Ge, BinOp::And, BinOp::Or, BinOp::Concat,
        ];
        for op in &ops {
            let expr = RelExpr::scan("t").filter(
                Expr::BinOp {
                    op: *op,
                    left: Box::new(Expr::Column(ColumnRef::new("a"))),
                    right: Box::new(Expr::Column(ColumnRef::new("b"))),
                },
            );
            let rec = to_rec_expr(&expr)
                .unwrap_or_else(|e| panic!("to_rec_expr for {op:?}: {e}"));
            let result = rec_expr_to_rel_expr(&rec)
                .unwrap_or_else(|e| panic!("extraction for {op:?}: {e}"));
            assert_eq!(result, expr, "round-trip failed for {op:?}");
        }
    }

    #[test]
    fn extract_json_access() {
        let expr = RelExpr::scan("t").filter(
            Expr::BinOp {
                op: BinOp::JsonAccess,
                left: Box::new(Expr::Column(ColumnRef::new("data"))),
                right: Box::new(Expr::Const(Const::String("key".into()))),
            },
        );
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- Unary operators --

    #[test]
    fn extract_not() {
        let expr = RelExpr::scan("t").filter(
            Expr::UnaryOp {
                op: ra_core::expr::UnaryOp::Not,
                operand: Box::new(Expr::Column(ColumnRef::new("active"))),
            },
        );
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_is_null() {
        let expr = RelExpr::scan("t").filter(
            Expr::UnaryOp {
                op: ra_core::expr::UnaryOp::IsNull,
                operand: Box::new(Expr::Column(ColumnRef::new("x"))),
            },
        );
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_is_not_null() {
        let expr = RelExpr::scan("t").filter(
            Expr::UnaryOp {
                op: ra_core::expr::UnaryOp::IsNotNull,
                operand: Box::new(Expr::Column(ColumnRef::new("x"))),
            },
        );
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_neg() {
        let expr = RelExpr::scan("t").filter(
            Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::UnaryOp {
                    op: ra_core::expr::UnaryOp::Neg,
                    operand: Box::new(Expr::Column(ColumnRef::new("x"))),
                }),
                right: Box::new(Expr::Const(Const::Int(0))),
            },
        );
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- Function calls --

    #[test]
    fn extract_function_call() {
        let expr = RelExpr::scan("t").filter(
            Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Function {
                    name: "LENGTH".into(),
                    args: vec![Expr::Column(ColumnRef::new("name"))],
                }),
                right: Box::new(Expr::Const(Const::Int(5))),
            },
        );
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- Array and ArrayIndex --

    #[test]
    fn extract_array() {
        let expr = RelExpr::scan("t").filter(
            Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Array(vec![
                    Expr::Const(Const::Int(1)),
                    Expr::Const(Const::Int(2)),
                ])),
                right: Box::new(Expr::Column(ColumnRef::new("arr"))),
            },
        );
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_array_index() {
        let expr = RelExpr::scan("t").filter(
            Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::ArrayIndex(
                    Box::new(Expr::Column(ColumnRef::new("arr"))),
                    Box::new(Expr::Const(Const::Int(0))),
                )),
                right: Box::new(Expr::Const(Const::Int(42))),
            },
        );
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- Unnest --

    #[test]
    fn extract_unnest() {
        let expr = RelExpr::Unnest {
            expr: Expr::Column(ColumnRef::new("tags")),
            alias: Some("tag".into()),
            input: None,
            with_ordinality: false,
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_unnest_lateral() {
        let expr = RelExpr::Unnest {
            expr: Expr::Column(ColumnRef::new("items")),
            alias: None,
            input: Some(Box::new(RelExpr::scan("orders"))),
            with_ordinality: true,
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- MultiUnnest --

    #[test]
    fn extract_multi_unnest() {
        let expr = RelExpr::MultiUnnest {
            exprs: vec![
                Expr::Column(ColumnRef::new("arr1")),
                Expr::Column(ColumnRef::new("arr2")),
            ],
            aliases: vec![Some("a".into()), None],
            with_ordinality: true,
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- TableFunction --

    #[test]
    fn extract_table_function() {
        let expr = RelExpr::TableFunction {
            name: "generate_series".into(),
            args: vec![
                Expr::Const(Const::Int(1)),
                Expr::Const(Const::Int(10)),
            ],
            columns: vec![],
            input: None,
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- IndexOnlyScan --

    #[test]
    fn extract_index_only_scan() {
        let expr = RelExpr::IndexOnlyScan {
            table: "users".into(),
            index: "idx_users_email".into(),
            columns: vec![ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("email")),
                alias: None,
            }],
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("email"))),
                right: Box::new(Expr::Const(Const::String("test@example.com".into()))),
            },
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- MvScan --

    #[test]
    fn extract_mv_scan_with_alias() {
        let expr = RelExpr::MvScan {
            view_name: "sales_summary".into(),
            alias: Some("ss".into()),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_mv_scan_no_alias() {
        let expr = RelExpr::MvScan {
            view_name: "daily_totals".into(),
            alias: None,
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- Bitmap operators --

    #[test]
    fn extract_bitmap_index_scan() {
        let expr = RelExpr::BitmapIndexScan {
            table: "orders".into(),
            index: "idx_status".into(),
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("status"))),
                right: Box::new(Expr::Const(Const::String("active".into()))),
            },
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_bitmap_and() {
        let scan1 = RelExpr::BitmapIndexScan {
            table: "t".into(),
            index: "idx_a".into(),
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("a"))),
                right: Box::new(Expr::Const(Const::Int(1))),
            },
        };
        let scan2 = RelExpr::BitmapIndexScan {
            table: "t".into(),
            index: "idx_b".into(),
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("b"))),
                right: Box::new(Expr::Const(Const::Int(2))),
            },
        };
        let expr = RelExpr::BitmapAnd {
            inputs: vec![Box::new(scan1), Box::new(scan2)],
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_bitmap_or() {
        let scan1 = RelExpr::BitmapIndexScan {
            table: "t".into(),
            index: "idx_x".into(),
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("x"))),
                right: Box::new(Expr::Const(Const::Int(1))),
            },
        };
        let scan2 = RelExpr::BitmapIndexScan {
            table: "t".into(),
            index: "idx_y".into(),
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("y"))),
                right: Box::new(Expr::Const(Const::Int(2))),
            },
        };
        let expr = RelExpr::BitmapOr {
            inputs: vec![Box::new(scan1), Box::new(scan2)],
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_bitmap_heap_scan() {
        let bitmap = RelExpr::BitmapIndexScan {
            table: "orders".into(),
            index: "idx_date".into(),
            predicate: Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("date"))),
                right: Box::new(Expr::Const(Const::String("2024-01-01".into()))),
            },
        };
        let expr = RelExpr::BitmapHeapScan {
            table: "orders".into(),
            bitmap: Box::new(bitmap),
            recheck_cond: Some(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("date"))),
                right: Box::new(Expr::Const(Const::String("2024-01-01".into()))),
            }),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- Sort with multiple keys and directions --

    #[test]
    fn extract_sort_multiple_keys() {
        let expr = RelExpr::Sort {
            keys: vec![
                SortKey {
                    expr: Expr::Column(ColumnRef::new("dept")),
                    direction: SortDirection::Asc,
                    nulls: NullOrdering::First,
                },
                SortKey {
                    expr: Expr::Column(ColumnRef::new("salary")),
                    direction: SortDirection::Desc,
                    nulls: NullOrdering::Last,
                },
            ],
            input: Box::new(RelExpr::scan("employees")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- Error paths --

    #[test]
    fn rec_expr_to_rel_expr_empty() {
        let rec: RecExpr<RelLang> = RecExpr::default();
        let err = rec_expr_to_rel_expr(&rec).unwrap_err();
        match err {
            EGraphError::ExtractionError(msg) => {
                assert!(msg.contains("empty"), "expected 'empty' in: {msg}");
            }
            other => panic!("expected ExtractionError, got: {other:?}"),
        }
    }

    // -- Nested / complex expressions --

    #[test]
    fn extract_nested_filter_project_join() {
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("u", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("o", "user_id"))),
            },
            left: Box::new(
                RelExpr::Scan {
                    table: "users".into(),
                    alias: Some("u".into()),
                }
                .filter(Expr::BinOp {
                    op: BinOp::Gt,
                    left: Box::new(Expr::Column(ColumnRef::new("age"))),
                    right: Box::new(Expr::Const(Const::Int(18))),
                }),
            ),
            right: Box::new(
                RelExpr::Scan {
                    table: "orders".into(),
                    alias: Some("o".into()),
                }
                .project(vec![
                    ProjectionColumn {
                        expr: Expr::Column(ColumnRef::new("user_id")),
                        alias: None,
                    },
                    ProjectionColumn {
                        expr: Expr::Column(ColumnRef::new("total")),
                        alias: Some("order_total".into()),
                    },
                ]),
            ),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    #[test]
    fn extract_subquery_filter_aggregate_limit() {
        let inner = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("category"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(Expr::Column(ColumnRef::new("amount"))),
                distinct: false,
                alias: Some("total".into()),
            }],
            input: Box::new(RelExpr::scan("transactions")),
        };
        let expr = inner
            .filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("total"))),
                right: Box::new(Expr::Const(Const::Int(1000))),
            })
            .limit(10, 0);
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- RelCostFn tests --

    #[test]
    fn rel_cost_fn_scan_scales_with_bandwidth() {
        use egg::CostFunction;
        let slow = ra_hardware::HardwareProfile {
            storage_bandwidth_gbps: 1.0,
            ..ra_hardware::HardwareProfile::cpu_only()
        };
        let fast = ra_hardware::HardwareProfile {
            storage_bandwidth_gbps: 10.0,
            ..ra_hardware::HardwareProfile::cpu_only()
        };
        let expr = RelExpr::scan("t");
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let nodes = rec.as_ref();

        let cost_slow = {
            let mut cf = RelCostFn::new(slow);
            cf.cost(&nodes[nodes.len() - 1], |child| {
                let mut cf2 = RelCostFn::new(
                    ra_hardware::HardwareProfile {
                        storage_bandwidth_gbps: 1.0,
                        ..ra_hardware::HardwareProfile::cpu_only()
                    },
                );
                cf2.cost(&nodes[usize::from(child)], |_| 0.0)
            })
        };
        let cost_fast = {
            let mut cf = RelCostFn::new(fast);
            cf.cost(&nodes[nodes.len() - 1], |child| {
                let mut cf2 = RelCostFn::new(
                    ra_hardware::HardwareProfile {
                        storage_bandwidth_gbps: 10.0,
                        ..ra_hardware::HardwareProfile::cpu_only()
                    },
                );
                cf2.cost(&nodes[usize::from(child)], |_| 0.0)
            })
        };
        assert!(
            cost_slow > cost_fast,
            "slower bandwidth should cost more: {cost_slow} vs {cost_fast}"
        );
    }

    #[test]
    fn rel_cost_fn_join_scales_with_cache() {
        use egg::CostFunction;
        let small_cache = ra_hardware::HardwareProfile {
            l3_cache_bytes: 8 * 1024 * 1024, // 8MB
            ..ra_hardware::HardwareProfile::cpu_only()
        };
        let large_cache = ra_hardware::HardwareProfile {
            l3_cache_bytes: 64 * 1024 * 1024, // 64MB
            ..ra_hardware::HardwareProfile::cpu_only()
        };

        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let nodes = rec.as_ref();

        let cost_small = {
            let mut cf = RelCostFn::new(small_cache);
            cf.cost(&nodes[nodes.len() - 1], |_| 0.0)
        };
        let cost_large = {
            let mut cf = RelCostFn::new(large_cache);
            cf.cost(&nodes[nodes.len() - 1], |_| 0.0)
        };
        assert!(
            cost_small > cost_large,
            "small cache should cost more: {cost_small} vs {cost_large}"
        );
    }

    #[test]
    fn rel_cost_fn_sort_scales_with_cores() {
        use egg::CostFunction;
        let few_cores = ra_hardware::HardwareProfile {
            cpu_cores: 2,
            ..ra_hardware::HardwareProfile::cpu_only()
        };
        let many_cores = ra_hardware::HardwareProfile {
            cpu_cores: 32,
            ..ra_hardware::HardwareProfile::cpu_only()
        };

        let expr = RelExpr::Sort {
            keys: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("x")),
                direction: SortDirection::Asc,
                nulls: NullOrdering::Last,
            }],
            input: Box::new(RelExpr::scan("t")),
        };
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let nodes = rec.as_ref();

        let cost_few = {
            let mut cf = RelCostFn::new(few_cores);
            cf.cost(&nodes[nodes.len() - 1], |_| 0.0)
        };
        let cost_many = {
            let mut cf = RelCostFn::new(many_cores);
            cf.cost(&nodes[nodes.len() - 1], |_| 0.0)
        };
        assert!(
            cost_few > cost_many,
            "fewer cores should cost more: {cost_few} vs {cost_many}"
        );
    }

    #[test]
    fn rel_cost_fn_filter_scales_with_simd() {
        use egg::CostFunction;
        let narrow_simd = ra_hardware::HardwareProfile {
            simd_width_bits: 128,
            ..ra_hardware::HardwareProfile::cpu_only()
        };
        let wide_simd = ra_hardware::HardwareProfile {
            simd_width_bits: 512,
            ..ra_hardware::HardwareProfile::cpu_only()
        };

        let expr = RelExpr::scan("t").filter(Expr::Const(Const::Bool(true)));
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let nodes = rec.as_ref();

        let cost_narrow = {
            let mut cf = RelCostFn::new(narrow_simd);
            cf.cost(&nodes[nodes.len() - 1], |_| 0.0)
        };
        let cost_wide = {
            let mut cf = RelCostFn::new(wide_simd);
            cf.cost(&nodes[nodes.len() - 1], |_| 0.0)
        };
        assert!(
            cost_narrow > cost_wide,
            "narrow SIMD should cost more: {cost_narrow} vs {cost_wide}"
        );
    }

    // -- extract_best and extract_best_with_staleness --

    #[test]
    fn extract_best_without_stats() {
        let expr = RelExpr::scan("users").filter(
            Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(18))),
            },
        );
        let hw = ra_hardware::HardwareProfile::cpu_only();
        let mut egraph = egg::EGraph::<RelLang, RelAnalysis>::default();
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let root = egraph.add_expr(&rec);

        let stats: HashMap<String, ra_core::statistics::Statistics> = HashMap::new();
        let result = extract_best(&egraph, root, &stats, &hw)
            .expect("extraction should succeed");
        assert!(matches!(result, RelExpr::Filter { .. }));
    }

    #[test]
    fn extract_best_with_stats() {
        let expr = RelExpr::scan("users");
        let hw = ra_hardware::HardwareProfile::cpu_only();
        let mut egraph = egg::EGraph::<RelLang, RelAnalysis>::default();
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let root = egraph.add_expr(&rec);

        let mut stats = HashMap::new();
        stats.insert(
            "users".to_string(),
            ra_core::statistics::Statistics::new(10000.0),
        );
        let result = extract_best(&egraph, root, &stats, &hw)
            .expect("extraction should succeed");
        assert!(matches!(result, RelExpr::Scan { .. }));
    }

    #[test]
    fn extract_best_with_staleness_fn() {
        let expr = RelExpr::scan("users");
        let hw = ra_hardware::HardwareProfile::cpu_only();
        let mut egraph = egg::EGraph::<RelLang, RelAnalysis>::default();
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let root = egraph.add_expr(&rec);

        let mut stats = HashMap::new();
        stats.insert(
            "users".to_string(),
            ra_core::statistics::Statistics::new(5000.0),
        );
        let mut staleness = HashMap::new();
        staleness.insert("users".to_string(), Staleness::Fresh);
        let result = extract_best_with_staleness(
            &egraph, root, &stats, &staleness, &hw,
        )
        .expect("extraction should succeed");
        assert!(matches!(result, RelExpr::Scan { .. }));
    }

    // -- Window function variant coverage --

    #[test]
    fn extract_window_all_ranking_functions() {
        use ra_core::algebra::{
            WindowExpr as WExpr, WindowFunction as WFn,
        };
        let funcs = [
            WFn::Rank, WFn::DenseRank, WFn::PercentRank,
            WFn::Ntile, WFn::Lag, WFn::Lead,
            WFn::FirstValue, WFn::LastValue, WFn::NthValue,
            WFn::Min, WFn::Max,
        ];
        for wfn in &funcs {
            let expr = RelExpr::Window {
                functions: vec![WExpr {
                    function: wfn.clone(),
                    arg: Some(Expr::Column(ColumnRef::new("col"))),
                    partition_by: vec![],
                    order_by: vec![],
                    frame: None,
                    alias: None,
                }],
                input: Box::new(RelExpr::scan("t")),
            };
            let rec = to_rec_expr(&expr)
                .unwrap_or_else(|e| panic!("to_rec_expr for {wfn:?}: {e}"));
            let result = rec_expr_to_rel_expr(&rec)
                .unwrap_or_else(|e| panic!("extraction for {wfn:?}: {e}"));
            assert_eq!(result, expr, "round-trip failed for {wfn:?}");
        }
    }

    // -- Limit with zero offset --

    #[test]
    fn extract_limit_zero_offset() {
        let expr = RelExpr::scan("t").limit(100, 0);
        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }

    // -- Complex nested plan --

    #[test]
    fn extract_complex_plan() {
        let base = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("u", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("o", "uid"))),
            },
            left: Box::new(RelExpr::Scan {
                table: "users".into(),
                alias: Some("u".into()),
            }),
            right: Box::new(RelExpr::Scan {
                table: "orders".into(),
                alias: Some("o".into()),
            }),
        };
        let agg = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::qualified("u", "name"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Sum,
                arg: Some(Expr::Column(ColumnRef::qualified("o", "amount"))),
                distinct: false,
                alias: Some("total".into()),
            }],
            input: Box::new(base),
        };
        let expr = RelExpr::Sort {
            keys: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("total")),
                direction: SortDirection::Desc,
                nulls: NullOrdering::Last,
            }],
            input: Box::new(agg),
        }
        .limit(10, 0);

        let rec = to_rec_expr(&expr).expect("to_rec_expr");
        let result = rec_expr_to_rel_expr(&rec).expect("extraction");
        assert_eq!(result, expr);
    }
}
