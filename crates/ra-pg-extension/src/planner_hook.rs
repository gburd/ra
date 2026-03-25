//! Planner hook: intercepts PostgreSQL query planning to inject
//! RA optimizer advice.
//!
//! Hooks the `planner_hook` entry point. When a query arrives:
//!
//! 1. Check if the extension is enabled (GUC).
//! 2. Count base relations; bail if above threshold.
//! 3. Parse the query into an RA `RelExpr` tree.
//! 4. Gather statistics from PostgreSQL catalogs (no SPI).
//! 5. Run the RA optimizer (e-graph based).
//! 6. If confident, apply advice via cost manipulation.
//! 7. If the query is unsupported by the parser, fall back to
//!    the standard planner.

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

    // Fast path: skip non-SELECT queries (INSERT, UPDATE, DELETE,
    // utility). Only SELECT queries benefit from RA optimization.
    if !parse.is_null()
        && (*parse).commandType
            != pg_sys::CmdType::CMD_SELECT
    {
        return call_prev_planner(
            parse,
            query_string,
            cursor_options,
            bound_params,
        );
    }

    // Skip utility statements that somehow reach the planner.
    if !parse.is_null() && !(*parse).utilityStmt.is_null() {
        return call_prev_planner(
            parse,
            query_string,
            cursor_options,
            bound_params,
        );
    }

    // Skip queries that reference system catalogs (pg_catalog,
    // information_schema). RA should never attempt to optimize
    // queries against PostgreSQL's internal metadata tables.
    if !parse.is_null() && references_system_catalogs(parse) {
        return call_prev_planner(
            parse,
            query_string,
            cursor_options,
            bound_params,
        );
    }

    // Delegate to the inner implementation, catching any panics
    // to prevent crashing the PostgreSQL backend process.
    let result = std::panic::catch_unwind(
        std::panic::AssertUnwindSafe(|| {
            ra_planner_hook_inner(
                parse,
                query_string,
                cursor_options,
                bound_params,
            )
        }),
    );

    match result {
        Ok(plan) => plan,
        Err(_) => {
            pgrx::warning!(
                "ra_planner: caught panic in planner hook, \
                 falling back to standard planner"
            );
            call_prev_planner(
                parse,
                query_string,
                cursor_options,
                bound_params,
            )
        }
    }
}

/// Inner planner hook implementation.
///
/// Runs the full RA optimization pipeline:
/// 1. Parse query to RelExpr
/// 2. Gather stats via catalog access (no SPI)
/// 3. Optimize with e-graph
/// 4. Apply advice via cost manipulation
///
/// Falls back to PostgreSQL planner only for unsupported query
/// types (CTEs, window functions, set operations, DML).
///
/// # Safety
///
/// Same requirements as `ra_planner_hook`.
unsafe fn ra_planner_hook_inner(
    parse: *mut pg_sys::Query,
    query_string: *const std::ffi::c_char,
    cursor_options: i32,
    bound_params: *mut pg_sys::ParamListInfoData,
) -> *mut pg_sys::PlannedStmt {
    let sql = if query_string.is_null() {
        String::new()
    } else {
        CStr::from_ptr(query_string)
            .to_string_lossy()
            .into_owned()
    };

    let mut state = RaOptimizerState::new(sql.clone());

    // Count relations and bail if above threshold.
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

    // Gather statistics from catalog (safe inside planner hook).
    let table_names: Vec<(String, String)> =
        extract_rtable_schema_names(parse);
    let stats = stats_bridge::gather_all_stats(&table_names);
    state.statistics = stats.clone();

    // Run the full optimization pipeline.
    let calibration = CostCalibration::default_calibration();

    let result = try_optimize_query(
        parse,
        &sql,
        &table_names,
        &stats,
        &calibration,
    );

    match result {
        Ok(Some(optimized)) => {
            let min_conf = RA_MIN_CONFIDENCE.get();

            if optimized.confidence >= min_conf {
                state.plan_applied = true;
                state.confidence = optimized.confidence;

                if RA_LOG_DECISIONS.get() {
                    pgrx::log!(
                        "ra_planner: applied RA plan \
                         (confidence: {:.2}, relations: {}): {}",
                        optimized.confidence,
                        relation_count,
                        truncate_sql(&sql, 200)
                    );
                }

                return optimized.plan;
            }

            // Confidence too low: fall back to PG planner.
            if RA_LOG_DECISIONS.get() {
                pgrx::log!(
                    "ra_planner: low confidence {:.2} < {:.2}, \
                     using PG planner: {}",
                    optimized.confidence,
                    min_conf,
                    truncate_sql(&sql, 100)
                );
            }
            call_prev_planner(
                parse,
                query_string,
                cursor_options,
                bound_params,
            )
        }
        Ok(None) => {
            // Unsupported query type (CTE, window, set op, DML).
            if RA_LOG_DECISIONS.get() {
                pgrx::log!(
                    "ra_planner: unsupported query shape, \
                     using PG planner: {}",
                    truncate_sql(&sql, 100)
                );
            }
            call_prev_planner(
                parse,
                query_string,
                cursor_options,
                bound_params,
            )
        }
        Err(e) => {
            // Optimization error: log and fall back.
            pgrx::warning!(
                "ra_planner: optimization failed ({}), \
                 using PG planner: {}",
                e,
                truncate_sql(&sql, 100)
            );
            call_prev_planner(
                parse,
                query_string,
                cursor_options,
                bound_params,
            )
        }
    }
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
    table_names: &[(String, String)],
    stats: &[(String, ra_core::Statistics)],
    calibration: &CostCalibration,
) -> Result<Option<OptimizedPlan>, String> {
    // Step 1: Convert PostgreSQL Query -> RA RelExpr.
    let rel_expr = match parse_query_to_relexpr(parse, sql) {
        Ok(Some(expr)) => expr,
        Ok(None) => return Ok(None), // Unsupported query type
        Err(e) => return Err(format!("Failed to parse query: {}", e)),
    };

    // Step 2: Build facts provider and run RA optimizer.
    let facts = SimpleFactsProvider::new(table_names, stats);
    let optimized_expr = match optimize_relexpr(&rel_expr, &facts) {
        Ok(expr) => expr,
        Err(e) => return Err(format!("Optimization failed: {}", e)),
    };

    // Step 3: Estimate confidence based on cost improvement and stats quality.
    let original_cost = estimate_plan_cost(&rel_expr, stats, calibration);
    let optimized_cost = estimate_plan_cost(&optimized_expr, stats, calibration);
    let improvement_ratio = if original_cost > 0.0 {
        (1.0 - (optimized_cost / original_cost)).max(0.0)
    } else {
        0.0
    };

    // Confidence is based on:
    // - 40% statistics coverage quality (column-level detail)
    // - 30% cost improvement ratio
    // - 30% table-level coverage (all referenced tables have stats)
    let detailed_coverage = facts.stats_coverage();
    let table_coverage = calculate_stats_coverage(&rel_expr, stats);
    let confidence = (
        improvement_ratio * 0.3
            + detailed_coverage * 0.4
            + table_coverage * 0.3
    )
    .clamp(0.0, 1.0);

    // Step 4: Convert optimized RelExpr -> PostgreSQL PlannedStmt.
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
    parse: *mut pg_sys::Query,
    _sql: &str,
) -> Result<Option<ra_core::algebra::RelExpr>, String> {
    crate::query_parser::parse(parse)
}

/// Run RA optimizer on a RelExpr.
fn optimize_relexpr(
    rel_expr: &ra_core::algebra::RelExpr,
    facts: &dyn ra_core::FactsProvider,
) -> Result<ra_core::algebra::RelExpr, String> {
    let optimizer = ra_engine::Optimizer::new();
    optimizer
        .optimize_with_facts(rel_expr, facts)
        .map_err(|e| format!("Optimizer error: {}", e))
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

/// Extract `(schema, table)` pairs from the Query's range table.
///
/// Resolves each relation's namespace OID to a schema name using
/// `get_namespace_name`. Falls back to `"public"` when resolution
/// fails.
///
/// # Safety
///
/// Caller must pass a valid `Query` pointer.
unsafe fn extract_rtable_schema_names(
    parse: *mut pg_sys::Query,
) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    if parse.is_null() {
        return pairs;
    }
    let rtable = (*parse).rtable;
    if rtable.is_null() {
        return pairs;
    }

    let length = (*rtable).length as i32;
    for i in 0..length {
        let rte = pg_sys::list_nth(rtable, i)
            as *mut pg_sys::RangeTblEntry;
        if rte.is_null() {
            continue;
        }
        if (*rte).rtekind != pg_sys::RTEKind::RTE_RELATION {
            continue;
        }
        let relid = (*rte).relid;
        let rel_name = get_rel_name(relid);
        let schema_name = get_rel_schema_name(relid);
        if let Some(name) = rel_name {
            pairs.push((
                schema_name.unwrap_or_else(|| "public".to_string()),
                name,
            ));
        }
    }
    pairs
}

/// Look up a relation's schema name by its OID.
unsafe fn get_rel_schema_name(
    relid: pg_sys::Oid,
) -> Option<String> {
    let ns_oid = get_rel_namespace(relid);
    if ns_oid == pg_sys::InvalidOid {
        return None;
    }
    let name_ptr = pg_sys::get_namespace_name(ns_oid);
    if name_ptr.is_null() {
        return None;
    }
    Some(
        CStr::from_ptr(name_ptr)
            .to_string_lossy()
            .into_owned(),
    )
}

/// Check if any relation in the query belongs to a system catalog.
///
/// Returns true if any `RTE_RELATION` entry in the range table has
/// a namespace OID matching `pg_catalog` or `information_schema`.
/// These are PostgreSQL's internal schemas and should always be
/// planned by the standard planner.
///
/// # Safety
///
/// Caller must pass a valid `Query` pointer.
unsafe fn references_system_catalogs(
    parse: *mut pg_sys::Query,
) -> bool {
    if parse.is_null() {
        return false;
    }
    let rtable = (*parse).rtable;
    if rtable.is_null() {
        return false;
    }

    // Resolve the well-known system namespace OIDs.
    let pg_catalog_oid = pg_sys::LookupExplicitNamespace(
        c"pg_catalog".as_ptr(),
        true, // missing_ok
    );
    let info_schema_oid = pg_sys::LookupExplicitNamespace(
        c"information_schema".as_ptr(),
        true, // missing_ok
    );

    let length = (*rtable).length as i32;
    for i in 0..length {
        let rte = pg_sys::list_nth(rtable, i)
            as *mut pg_sys::RangeTblEntry;
        if rte.is_null() {
            continue;
        }
        if (*rte).rtekind != pg_sys::RTEKind::RTE_RELATION {
            continue;
        }
        let rel_ns = get_rel_namespace((*rte).relid);
        if rel_ns == pg_catalog_oid
            || rel_ns == info_schema_oid
        {
            return true;
        }
    }
    false
}

/// Look up the namespace OID of a relation.
unsafe fn get_rel_namespace(
    relid: pg_sys::Oid,
) -> pg_sys::Oid {
    let tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::RELOID as _,
        pg_sys::ObjectIdGetDatum(relid),
    );
    if tuple.is_null() {
        return pg_sys::InvalidOid;
    }
    let rel_form =
        pg_sys::GETSTRUCT(tuple) as pg_sys::Form_pg_class;
    let ns_oid = (*rel_form).relnamespace;
    pg_sys::ReleaseSysCache(tuple);
    ns_oid
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

/// FactsProvider backed by PostgreSQL catalog statistics.
///
/// Converts `ra_core::Statistics` (gathered from pg_class/pg_statistic)
/// into the `CoreTableStats`/`ColumnStats` that the RA optimizer's
/// pre-condition system expects. Integrates the detected hardware
/// profile from `extension_state`.
struct SimpleFactsProvider {
    table_stats: std::collections::HashMap<String, ra_core::CoreTableStats>,
    column_stats: std::collections::HashMap<String, std::collections::HashMap<String, ra_core::ColumnStats>>,
    schemas: std::collections::HashMap<String, ra_core::TableInfo>,
    hardware: ra_core::CoreHardwareProfile,
}

impl SimpleFactsProvider {
    fn new(
        table_names: &[(String, String)],
        stats: &[(String, ra_core::Statistics)],
    ) -> Self {
        let mut table_stats = std::collections::HashMap::new();
        let mut column_stats = std::collections::HashMap::new();
        let mut schemas = std::collections::HashMap::new();

        // Build a schema lookup from table_names for FK gathering
        let schema_for: std::collections::HashMap<&str, &str> =
            table_names
                .iter()
                .map(|(s, t)| (t.as_str(), s.as_str()))
                .collect();

        for (table_name, stat) in stats {
            // Convert Statistics -> CoreTableStats
            let avg_row_size = if stat.avg_row_size > 0 {
                stat.avg_row_size as f64
            } else {
                estimate_avg_row_size(stat)
            };
            let table_size = if stat.total_size > 0 {
                stat.total_size
            } else {
                (stat.row_count * avg_row_size) as u64
            };
            let page_count = (table_size / 8192).max(1);

            table_stats.insert(
                table_name.clone(),
                ra_core::CoreTableStats {
                    row_count: stat.row_count,
                    page_count,
                    average_row_size: avg_row_size,
                    table_size_bytes: table_size,
                    live_tuples: Some(stat.row_count),
                    dead_tuples: None,
                    last_analyzed: None,
                    confidence: compute_stats_confidence(stat),
                },
            );

            // Store column stats directly (same type)
            let mut cols = std::collections::HashMap::new();
            for (col_name, col_stat) in &stat.columns {
                cols.insert(col_name.clone(), col_stat.clone());
            }
            column_stats.insert(table_name.clone(), cols);

            // Build TableInfo for schema queries
            let columns: Vec<(String, ra_core::DataType)> = stat
                .columns
                .keys()
                .map(|col| (col.clone(), ra_core::DataType::Other("unknown".into())))
                .collect();

            let indexes: Vec<ra_core::IndexInfo> = stat
                .indexes
                .iter()
                .map(|(idx_name, idx_stat)| ra_core::IndexInfo {
                    name: idx_name.clone(),
                    index_type: idx_stat.index_type,
                    columns: idx_stat.columns.clone(),
                    included_columns: Vec::new(),
                    is_unique: idx_stat.is_unique,
                })
                .collect();

            // Detect primary key from indexes
            let primary_key: Vec<String> = stat
                .indexes
                .values()
                .find(|idx| idx.is_primary)
                .map(|idx| idx.columns.clone())
                .unwrap_or_default();

            // Gather foreign keys from pg_constraint
            let schema = schema_for
                .get(table_name.as_str())
                .copied()
                .unwrap_or("public");
            let fk_infos =
                stats_bridge::gather_foreign_keys(schema, table_name);
            let foreign_keys: Vec<ra_core::ForeignKey> = fk_infos
                .into_iter()
                .map(|fk| ra_core::ForeignKey {
                    columns: fk.columns,
                    referenced_table: fk.referenced_table,
                    referenced_columns: fk.referenced_columns,
                })
                .collect();

            schemas.insert(
                table_name.clone(),
                ra_core::TableInfo {
                    name: table_name.clone(),
                    columns,
                    primary_key,
                    foreign_keys,
                    indexes,
                },
            );
        }

        // Convert ra_hardware::HardwareProfile -> CoreHardwareProfile
        let hw = crate::extension_state::hardware_profile();
        let hardware = ra_core::CoreHardwareProfile {
            cpu_cores: hw.cpu_cores,
            available_memory: (hw.l3_cache_bytes * 64).max(8 * 1024 * 1024 * 1024),
            total_memory: (hw.l3_cache_bytes * 64).max(16 * 1024 * 1024 * 1024),
            simd_width: hw.simd_width_bits,
            has_gpu: hw.gpu_available,
            gpu_memory: if hw.gpu_available {
                Some(hw.available_gpu_memory_bytes())
            } else {
                None
            },
            l1_cache_size: 32 * 1024,
            l2_cache_size: hw.l2_cache_bytes,
            l3_cache_size: hw.l3_cache_bytes,
        };

        Self {
            table_stats,
            column_stats,
            schemas,
            hardware,
        }
    }

    /// Calculate overall statistics coverage for confidence scoring.
    ///
    /// Returns a value in [0.0, 1.0] representing what fraction of
    /// tables have usable statistics with column-level detail.
    fn stats_coverage(&self) -> f64 {
        if self.table_stats.is_empty() {
            return 0.0;
        }

        let mut score = 0.0;
        let count = self.table_stats.len() as f64;

        for (table, ts) in &self.table_stats {
            // Base: table exists with row count
            let mut table_score = 0.5;

            // Bonus for column-level stats
            if let Some(cols) = self.column_stats.get(table) {
                if !cols.is_empty() {
                    // Scale by ratio of columns with stats
                    let col_coverage = if let Some(schema) = self.schemas.get(table) {
                        if schema.columns.is_empty() {
                            1.0
                        } else {
                            cols.len() as f64 / schema.columns.len() as f64
                        }
                    } else {
                        0.5
                    };
                    table_score += 0.3 * col_coverage;
                }
            }

            // Bonus for index information
            if let Some(schema) = self.schemas.get(table) {
                if !schema.indexes.is_empty() {
                    table_score += 0.2;
                }
            }

            // Use the stats confidence directly
            table_score *= ts.confidence;

            score += table_score;
        }

        score / count
    }
}

/// Estimate average row size from column statistics.
fn estimate_avg_row_size(stat: &ra_core::Statistics) -> f64 {
    if stat.columns.is_empty() {
        return crate::pg_constants::estimation::BYTES_PER_ROW;
    }

    let total: f64 = stat
        .columns
        .values()
        .map(|cs| cs.avg_length.unwrap_or(8.0))
        .sum();

    // Add 23 bytes for tuple header overhead
    (total + 23.0).max(24.0)
}

/// Compute confidence in statistics based on data quality.
///
/// Higher confidence when:
/// - Row count is positive (table has been analyzed)
/// - Column statistics are present
/// - Histograms and MCVs are available
fn compute_stats_confidence(stat: &ra_core::Statistics) -> f64 {
    if stat.row_count <= 0.0 {
        return 0.0;
    }

    let mut confidence = 0.6; // Base confidence for having row count

    if stat.columns.is_empty() {
        return confidence;
    }

    let mut hist_count = 0;
    let mut mcv_count = 0;
    let total_cols = stat.columns.len() as f64;

    for cs in stat.columns.values() {
        if cs.histogram.is_some() {
            hist_count += 1;
        }
        if cs.most_common_values.is_some() {
            mcv_count += 1;
        }
    }

    // Bonus for histogram coverage
    confidence += 0.2 * (hist_count as f64 / total_cols);

    // Bonus for MCV coverage
    confidence += 0.2 * (mcv_count as f64 / total_cols);

    confidence.min(1.0)
}

impl ra_core::FactsProvider for SimpleFactsProvider {
    fn get_table_stats(
        &self,
        table: &str,
    ) -> Option<&ra_core::CoreTableStats> {
        self.table_stats.get(table)
    }

    fn get_column_stats(
        &self,
        table: &str,
        column: &str,
    ) -> Option<&ra_core::ColumnStats> {
        self.column_stats
            .get(table)
            .and_then(|cols| cols.get(column))
    }

    fn hardware_profile(&self) -> &ra_core::CoreHardwareProfile {
        &self.hardware
    }

    fn get_schema(
        &self,
        table: &str,
    ) -> Option<&ra_core::TableInfo> {
        self.schemas.get(table)
    }

    fn runtime_stats(
        &self,
        _operator_id: &str,
    ) -> Option<&ra_core::OperatorStats> {
        None
    }

    fn database_name(&self) -> &str {
        "postgresql"
    }

    fn supports_feature(&self, feature: &str) -> bool {
        matches!(
            feature,
            "lateral_join"
                | "cte_recursive"
                | "window_functions"
                | "partial_index"
                | "index_only_scan"
                | "bitmap_scan"
                | "parallel_query"
                | "hash_join"
                | "merge_join"
                | "nested_loop"
        )
    }

    fn sql_dialect(&self) -> ra_core::SqlDialect {
        ra_core::SqlDialect::Postgres
    }

    fn memory_limit(&self) -> Option<u64> {
        Some(self.hardware.available_memory)
    }

    fn optimizer_timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::Statistics;

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

    #[test]
    fn confidence_zero_rows() {
        let stats = Statistics::new(0.0);
        assert!((compute_stats_confidence(&stats) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn confidence_row_count_only() {
        let stats = Statistics::new(1000.0);
        let conf = compute_stats_confidence(&stats);
        assert!((conf - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn confidence_with_histograms() {
        let mut stats = Statistics::new(1000.0);
        let mut cs = ra_core::ColumnStats::new(100.0);
        cs.histogram = Some(ra_core::Histogram::EquiDepth(
            ra_core::EquiDepthHistogram {
                buckets: vec![],
                rows_per_bucket: 0.0,
            },
        ));
        cs.most_common_values = Some(vec!["a".into()]);
        cs.most_common_freqs = Some(vec![0.1]);
        stats.columns.insert("id".into(), cs);

        let conf = compute_stats_confidence(&stats);
        // 0.6 base + 0.2 (histogram) + 0.2 (MCV) = 1.0
        assert!((conf - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn confidence_partial_columns() {
        let mut stats = Statistics::new(1000.0);
        stats
            .columns
            .insert("id".into(), ra_core::ColumnStats::new(100.0));
        let mut cs = ra_core::ColumnStats::new(50.0);
        cs.histogram = Some(ra_core::Histogram::EquiDepth(
            ra_core::EquiDepthHistogram {
                buckets: vec![],
                rows_per_bucket: 0.0,
            },
        ));
        stats.columns.insert("name".into(), cs);

        let conf = compute_stats_confidence(&stats);
        // 0.6 + 0.2*(1/2) + 0.0 = 0.7
        assert!(conf > 0.6);
        assert!(conf < 1.0);
    }

    #[test]
    fn estimate_avg_row_size_no_columns() {
        let stats = Statistics::new(100.0);
        let size = estimate_avg_row_size(&stats);
        assert!((size - estimation::BYTES_PER_ROW).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_avg_row_size_with_columns() {
        let mut stats = Statistics::new(100.0);
        let mut cs = ra_core::ColumnStats::new(10.0);
        cs.avg_length = Some(16.0);
        stats.columns.insert("col1".into(), cs);

        let size = estimate_avg_row_size(&stats);
        // 16.0 + 23.0 = 39.0
        assert!((size - 39.0).abs() < f64::EPSILON);
    }

    #[test]
    fn calculate_stats_coverage_full() {
        let stats = vec![
            ("t1".into(), Statistics::new(100.0)),
            ("t2".into(), Statistics::new(200.0)),
        ];
        let expr = ra_core::algebra::RelExpr::Join {
            join_type: ra_core::JoinType::Inner,
            condition: ra_core::Expr::Const(ra_core::Const::Bool(true)),
            left: Box::new(ra_core::algebra::RelExpr::Scan {
                table: "t1".into(),
                alias: None,
            }),
            right: Box::new(ra_core::algebra::RelExpr::Scan {
                table: "t2".into(),
                alias: None,
            }),
        };
        let coverage = calculate_stats_coverage(&expr, &stats);
        assert!((coverage - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn calculate_stats_coverage_partial() {
        let stats = vec![("t1".into(), Statistics::new(100.0))];
        let expr = ra_core::algebra::RelExpr::Join {
            join_type: ra_core::JoinType::Inner,
            condition: ra_core::Expr::Const(ra_core::Const::Bool(true)),
            left: Box::new(ra_core::algebra::RelExpr::Scan {
                table: "t1".into(),
                alias: None,
            }),
            right: Box::new(ra_core::algebra::RelExpr::Scan {
                table: "t2".into(),
                alias: None,
            }),
        };
        let coverage = calculate_stats_coverage(&expr, &stats);
        assert!((coverage - 0.5).abs() < f64::EPSILON);
    }
}
