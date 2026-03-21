//! Planner hook: intercepts PostgreSQL query planning to inject
//! RA optimizer advice.
//!
//! Hooks the `planner_hook` entry point. When a query arrives:
//!
//! 1. Check if the extension is enabled (GUC).
//! 2. Count base relations; bail if above threshold.
//! 3. Gather statistics from `pg_stats`.
//! 4. Build a `RelExpr` from the query string.
//! 5. (Future) Run the RA optimizer.
//! 6. If confident, apply advice via cost manipulation.
//! 7. Otherwise, fall back to the standard planner.
//!
//! Currently implements a conservative strategy: always falls back
//! to the standard PostgreSQL planner but logs its analysis. Full
//! RA optimization requires the `ra-compiler` crate integration.

use std::ffi::CStr;

use pgrx::prelude::*;

use crate::cost_mapper::CostCalibration;
use crate::extension_state::{
    RaOptimizerState, RA_ENABLED, RA_LOG_DECISIONS,
    RA_MAX_RELATIONS, RA_MIN_CONFIDENCE,
};
use crate::pg_constants::{cost_defaults, estimation};
use crate::plan_converter;
use crate::stats_bridge;

/// Saved pointer to the previous planner hook (for chaining).
static mut PREV_PLANNER_HOOK: pg_sys::planner_hook_type = None;

/// Register the planner hook on extension load.
pub fn register_hooks() {
    unsafe {
        PREV_PLANNER_HOOK = pg_sys::planner_hook;
        pg_sys::planner_hook = Some(ra_planner_hook);
    }
}

/// The main planner hook entry point.
///
/// # Safety
///
/// Called by PostgreSQL's planner infrastructure with valid pointers
/// to internal planner structures. Must chain to the previous hook
/// or the standard planner.
unsafe extern "C-unwind" fn ra_planner_hook(
    parse: *mut pg_sys::Query,
    query_string: *const std::ffi::c_char,
    cursor_options: i32,
    bound_params: *mut pg_sys::ParamListInfoData,
) -> *mut pg_sys::PlannedStmt {
    // Fast path: extension disabled.
    if !RA_ENABLED.get() {
        return call_prev_planner(
            parse,
            query_string,
            cursor_options,
            bound_params,
        );
    }

    let sql = if query_string.is_null() {
        String::new()
    } else {
        CStr::from_ptr(query_string)
            .to_string_lossy()
            .into_owned()
    };

    let _state = RaOptimizerState::new(sql.clone());

    // Parse the query to determine relation count.
    // For now, we use a heuristic: count rtable entries.
    let relation_count = count_rtable_entries(parse);
    let max_rels = RA_MAX_RELATIONS.get() as usize;

    if relation_count > max_rels {
        if RA_LOG_DECISIONS.get() {
            pgrx::log!(
                "ra_planner: skipping query with {} relations \
                 (max: {}): {}",
                relation_count,
                max_rels,
                truncate_sql(&sql, 200)
            );
        }
        return call_prev_planner(
            parse,
            query_string,
            cursor_options,
            bound_params,
        );
    }

    // Gather statistics for referenced tables.
    let table_names = extract_rtable_names(parse);
    let stats = stats_bridge::gather_all_stats(
        &table_names
            .iter()
            .map(|t| ("public".to_string(), t.clone()))
            .collect::<Vec<_>>(),
    );

    // Log the analysis if requested.
    if RA_LOG_DECISIONS.get() {
        pgrx::log!(
            "ra_planner: analyzing query with {} relations, \
             {} stats available: {}",
            relation_count,
            stats.len(),
            truncate_sql(&sql, 200)
        );
    }

    // Attempt to optimize with RA.
    let min_confidence = RA_MIN_CONFIDENCE.get();
    let calibration = CostCalibration::default_calibration();

    // Try to convert Query → RelExpr and optimize.
    match try_optimize_query(parse, &sql, stats.as_slice(), &calibration) {
        Ok(Some(optimized_plan)) => {
            // Check confidence threshold.
            if optimized_plan.confidence >= min_confidence {
                if RA_LOG_DECISIONS.get() {
                    pgrx::log!(
                        "ra_planner: using RA-optimized plan (confidence: {:.2}): {}",
                        optimized_plan.confidence,
                        truncate_sql(&sql, 100)
                    );
                }
                // Return the RA-optimized PlannedStmt.
                return optimized_plan.plan;
            } else {
                if RA_LOG_DECISIONS.get() {
                    pgrx::log!(
                        "ra_planner: RA plan confidence too low ({:.2} < {:.2}), \
                         falling back to standard planner: {}",
                        optimized_plan.confidence,
                        min_confidence,
                        truncate_sql(&sql, 100)
                    );
                }
            }
        }
        Ok(None) => {
            // No optimization possible (e.g., unsupported query type).
            if RA_LOG_DECISIONS.get() {
                pgrx::log!(
                    "ra_planner: query not optimizable by RA: {}",
                    truncate_sql(&sql, 100)
                );
            }
        }
        Err(e) => {
            // Optimization failed - log error and fall back.
            if RA_LOG_DECISIONS.get() {
                pgrx::warning!(
                    "ra_planner: optimization failed ({}), \
                     falling back to standard planner: {}",
                    e,
                    truncate_sql(&sql, 100)
                );
            }
        }
    }

    // Fall back to standard planner.
    call_prev_planner(
        parse,
        query_string,
        cursor_options,
        bound_params,
    )
}

/// Result of RA optimization with confidence score.
struct OptimizedPlan {
    plan: *mut pg_sys::PlannedStmt,
    confidence: f64,
}

/// Attempt to optimize a query using RA.
///
/// # Safety
///
/// Caller must pass a valid `Query` pointer.
unsafe fn try_optimize_query(
    parse: *mut pg_sys::Query,
    sql: &str,
    stats: &[(String, ra_core::Statistics)],
    calibration: &CostCalibration,
) -> Result<Option<OptimizedPlan>, String> {
    // Step 1: Convert PostgreSQL Query → RA RelExpr.
    let rel_expr = match parse_query_to_relexpr(parse, sql) {
        Ok(Some(expr)) => expr,
        Ok(None) => return Ok(None), // Unsupported query type
        Err(e) => return Err(format!("Failed to parse query: {}", e)),
    };

    // Step 2: Run RA optimizer.
    let optimized_expr = match optimize_relexpr(&rel_expr, stats) {
        Ok(expr) => expr,
        Err(e) => return Err(format!("Optimization failed: {}", e)),
    };

    // Step 3: Estimate confidence based on cost improvement.
    let original_cost = estimate_plan_cost(&rel_expr, stats, calibration);
    let optimized_cost = estimate_plan_cost(&optimized_expr, stats, calibration);
    let improvement_ratio = if original_cost > 0.0 {
        1.0 - (optimized_cost / original_cost)
    } else {
        0.0
    };

    // Confidence is based on improvement ratio and statistics availability.
    let stats_coverage = calculate_stats_coverage(&rel_expr, stats);
    let confidence = (improvement_ratio * 0.7 + stats_coverage * 0.3).clamp(0.0, 1.0);

    // Step 4: Convert optimized RelExpr → PostgreSQL PlannedStmt.
    let planned_stmt = match plan_converter::convert_to_planned_stmt(
        &optimized_expr,
        parse,
        stats,
        calibration,
    ) {
        Ok(plan) => plan,
        Err(e) => return Err(format!("Plan conversion failed: {}", e)),
    };

    Ok(Some(OptimizedPlan {
        plan: planned_stmt,
        confidence,
    }))
}

/// Parse PostgreSQL Query to RA RelExpr.
///
/// Returns Ok(None) for unsupported query types (DDL, utility statements).
unsafe fn parse_query_to_relexpr(
    _parse: *mut pg_sys::Query,
    _sql: &str,
) -> Result<Option<ra_core::algebra::RelExpr>, String> {
    // TODO: Implement full query parsing.
    // For now, return None to indicate unsupported.
    Ok(None)
}

/// Run RA optimizer on a RelExpr.
fn optimize_relexpr(
    _rel_expr: &ra_core::algebra::RelExpr,
    _stats: &[(String, ra_core::Statistics)],
) -> Result<ra_core::algebra::RelExpr, String> {
    // TODO: Integrate with ra-engine optimizer.
    Err("RA optimizer not yet integrated".to_string())
}

/// Estimate cost of a plan using RA's cost model.
fn estimate_plan_cost(
    rel_expr: &ra_core::algebra::RelExpr,
    stats: &[(String, ra_core::Statistics)],
    calibration: &CostCalibration,
) -> f64 {
    // Get base table costs from statistics
    let base_cost = estimate_relexpr_cost(rel_expr, stats);

    // Convert RA cost to PostgreSQL cost units
    calibration.ra_to_pg_total(&base_cost)
}

/// Recursively estimate the cost of a RelExpr tree.
fn estimate_relexpr_cost(
    expr: &ra_core::algebra::RelExpr,
    stats: &[(String, ra_core::Statistics)],
) -> ra_core::Cost {
    use ra_core::algebra::RelExpr;
    use ra_core::Cost;

    match expr {
        RelExpr::Scan { table, .. }
        | RelExpr::ParallelScan { table, .. } => {
            // Sequential scan cost: rows * page_cost
            let row_count = get_table_row_count(table, stats);
            let pages = (row_count / estimation::ROWS_PER_PAGE).max(1.0);
            let mem_bytes = (row_count * estimation::BYTES_PER_ROW).max(1.0);
            Cost::new(row_count * cost_defaults::CPU_TUPLE_COST, pages, 0.0, mem_bytes as u64)
        }
        RelExpr::IndexScan { table, .. }
        | RelExpr::IndexOnlyScan { table, .. } => {
            // Index scan cost: log(rows) * random_page_cost + rows * cpu_cost
            let row_count = get_table_row_count(table, stats);
            let index_pages = row_count.log2().max(1.0);
            let mem_bytes = (row_count * estimation::BYTES_PER_ROW).max(1.0);
            Cost::new(
                row_count * cost_defaults::CPU_INDEX_TUPLE_COST,
                index_pages * cost_defaults::RANDOM_PAGE_COST,
                0.0,
                mem_bytes as u64,
            )
        }
        RelExpr::BitmapHeapScan { table, .. } => {
            // Bitmap scan: similar to index scan but more efficient for multiple matches
            let row_count = get_table_row_count(table, stats);
            let pages = (row_count / estimation::ROWS_PER_PAGE).max(1.0) * 0.5; // More efficient than seq scan
            let mem_bytes = (row_count * estimation::BYTES_PER_ROW).max(1.0);
            Cost::new(
                row_count * cost_defaults::CPU_TUPLE_COST,
                pages * cost_defaults::RANDOM_PAGE_COST * 0.5, // Bitmap is more efficient
                0.0,
                mem_bytes as u64,
            )
        }
        RelExpr::Join { left, right, .. }
        | RelExpr::ParallelHashJoin { left, right, .. } => {
            // Join cost: left + right + approximate join CPU cost
            let left_cost = estimate_relexpr_cost(left, stats);
            let right_cost = estimate_relexpr_cost(right, stats);
            // Approximate join cost based on memory (rough cardinality estimate)
            let join_cpu = (left_cost.memory as f64 * right_cost.memory as f64).sqrt() * 0.001;
            Cost::new(
                left_cost.cpu + right_cost.cpu + join_cpu,
                left_cost.io + right_cost.io,
                left_cost.network + right_cost.network,
                left_cost.memory.max(right_cost.memory),
            )
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Gather { input, .. } => {
            // Passthrough operators: add small CPU cost proportional to memory (rows)
            let mut cost = estimate_relexpr_cost(input, stats);
            cost.cpu += cost.memory as f64 * 0.001;
            cost
        }
        RelExpr::Sort { input, .. }
        | RelExpr::IncrementalSort { input, .. } => {
            // Sort cost: n * log(n) * cpu_cost
            let mut cost = estimate_relexpr_cost(input, stats);
            let n = cost.memory as f64;
            cost.cpu += n * n.log2() * 0.002;
            cost
        }
        RelExpr::Aggregate { input, .. }
        | RelExpr::ParallelAggregate { input, .. } => {
            // Aggregate cost: hash table build + input processing
            let mut cost = estimate_relexpr_cost(input, stats);
            cost.cpu += cost.memory as f64 * 0.005;
            // Aggregation typically reduces rows significantly
            cost.memory = (cost.memory as f64 * 0.1).max(1.0) as u64;
            cost
        }
        RelExpr::Limit { input, .. } => {
            // Limit significantly reduces memory/rows
            let mut cost = estimate_relexpr_cost(input, stats);
            cost.memory = (cost.memory / 10).max(1);
            cost
        }
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            // Set operations: process both sides
            let left_cost = estimate_relexpr_cost(left, stats);
            let right_cost = estimate_relexpr_cost(right, stats);
            Cost::new(
                left_cost.cpu + right_cost.cpu,
                left_cost.io + right_cost.io,
                left_cost.network + right_cost.network,
                left_cost.memory + right_cost.memory,
            )
        }
        _ => {
            // Default: minimal cost
            Cost::new(1.0, 1.0, 0.0, 1)
        }
    }
}

/// Get the row count for a table from statistics.
fn get_table_row_count(table: &str, stats: &[(String, ra_core::Statistics)]) -> f64 {
    stats
        .iter()
        .find(|(name, _)| name == table)
        .map(|(_, s)| s.row_count)
        .unwrap_or(estimation::DEFAULT_ROW_COUNT)
}

/// Calculate what fraction of tables have statistics available.
fn calculate_stats_coverage(
    rel_expr: &ra_core::algebra::RelExpr,
    stats: &[(String, ra_core::Statistics)],
) -> f64 {
    let table_names = plan_converter::extract_table_names(rel_expr);
    if table_names.is_empty() {
        return 1.0; // No tables = 100% coverage
    }

    let covered = table_names
        .iter()
        .filter(|table| {
            stats.iter().any(|(name, _)| name == *table)
        })
        .count();

    covered as f64 / table_names.len() as f64
}

/// Chain to the previous planner hook or the standard planner.
///
/// # Safety
///
/// Callers must pass valid planner arguments.
unsafe fn call_prev_planner(
    parse: *mut pg_sys::Query,
    query_string: *const std::ffi::c_char,
    cursor_options: i32,
    bound_params: *mut pg_sys::ParamListInfoData,
) -> *mut pg_sys::PlannedStmt {
    if let Some(prev) = PREV_PLANNER_HOOK {
        prev(parse, query_string, cursor_options, bound_params)
    } else {
        pg_sys::standard_planner(
            parse,
            query_string,
            cursor_options,
            bound_params,
        )
    }
}

/// Count range-table entries in a Query to estimate relation count.
///
/// # Safety
///
/// Caller must pass a valid `Query` pointer.
unsafe fn count_rtable_entries(
    parse: *mut pg_sys::Query,
) -> usize {
    if parse.is_null() {
        return 0;
    }
    let rtable = (*parse).rtable;
    if rtable.is_null() {
        return 0;
    }
    (*rtable).length as usize
}

/// Extract table names from the Query's range table.
///
/// Uses `pg_sys::list_nth` to traverse the array-based List and
/// `pg_sys::get_rel_name` to resolve OIDs to names.
///
/// # Safety
///
/// Caller must pass a valid `Query` pointer.
unsafe fn extract_rtable_names(
    parse: *mut pg_sys::Query,
) -> Vec<String> {
    let mut names = Vec::new();
    if parse.is_null() {
        return names;
    }
    let rtable = (*parse).rtable;
    if rtable.is_null() {
        return names;
    }

    let length = (*rtable).length as i32;

    for i in 0..length {
        let rte = pg_sys::list_nth(rtable, i)
            as *mut pg_sys::RangeTblEntry;
        if rte.is_null() {
            continue;
        }
        if (*rte).rtekind == pg_sys::RTEKind::RTE_RELATION {
            let relid = (*rte).relid;
            let rel_name = get_rel_name(relid);
            if let Some(name) = rel_name {
                names.push(name);
            }
        }
    }
    names
}

/// Look up a relation name by OID.
unsafe fn get_rel_name(relid: pg_sys::Oid) -> Option<String> {
    let name_ptr = pg_sys::get_rel_name(relid);
    if name_ptr.is_null() {
        return None;
    }
    Some(
        CStr::from_ptr(name_ptr)
            .to_string_lossy()
            .into_owned(),
    )
}

/// Truncate a SQL string for logging.
fn truncate_sql(sql: &str, max_len: usize) -> String {
    if sql.len() <= max_len {
        sql.to_string()
    } else {
        format!("{}...", &sql[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_string() {
        let s = truncate_sql("SELECT 1", 100);
        assert_eq!(s, "SELECT 1");
    }

    #[test]
    fn truncate_long_string() {
        let s = truncate_sql(&"x".repeat(300), 10);
        assert_eq!(s.len(), 13); // 10 + "..."
        assert!(s.ends_with("..."));
    }

    #[test]
    fn truncate_exact_boundary() {
        let s = truncate_sql("12345", 5);
        assert_eq!(s, "12345");
    }
}
