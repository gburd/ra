//! Planner hook: full parser/planner/optimizer replacement.
//!
//! When enabled, intercepts PostgreSQL's planner hook and replaces the
//! entire planning pipeline:
//!
//! 1. **Lime parse** — raw SQL → Ra `RelExpr` (ignores PG's parse tree)
//! 2. **Ra optimize** — e-graph equality saturation
//! 3. **Translate** — optimized `RelExpr` → PostgreSQL `Plan` nodes
//!
//! PG's `Query.rtable` is used only for OID resolution (mapping table/column
//! names to catalog OIDs needed by the executor).
//!
//! Timing is measured separately for each phase and logged when
//! `ra_planner.log_decisions` is enabled.

use std::ffi::CStr;
use std::time::Instant;

use pgrx::prelude::*;

use crate::extension_state::{RA_ENABLED, RA_LOG_DECISIONS};
use crate::plan_builder::{self, PlanBuilder};
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
/// to internal planner structures.
unsafe extern "C-unwind" fn ra_planner_hook(
    parse: *mut pg_sys::Query,
    query_string: *const std::ffi::c_char,
    cursor_options: i32,
    bound_params: *mut pg_sys::ParamListInfoData,
) -> *mut pg_sys::PlannedStmt {
    // Fast path: extension disabled.
    if !RA_ENABLED.get() {
        return call_prev_planner(parse, query_string, cursor_options, bound_params);
    }

    // Skip utility statements.
    if !parse.is_null() && !(*parse).utilityStmt.is_null() {
        return call_prev_planner(parse, query_string, cursor_options, bound_params);
    }

    // Skip system catalog queries.
    if !parse.is_null() && references_system_catalogs(parse) {
        return call_prev_planner(parse, query_string, cursor_options, bound_params);
    }

    // Catch panics to prevent crashing the backend.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ra_planner_hook_inner(parse, query_string, cursor_options, bound_params)
    }));

    match result {
        Ok(plan) => plan,
        Err(_) => {
            pgrx::warning!("ra_planner: caught panic, falling back to standard planner");
            call_prev_planner(parse, query_string, cursor_options, bound_params)
        }
    }
}

/// Inner planner hook: Lime parse → Ra optimize → Plan node translation.
///
/// Falls back to the standard planner for unsupported queries.
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
    // Refresh system fingerprint if intervals have elapsed.
    crate::monitor::maybe_refresh();

    let sql = if query_string.is_null() {
        String::new()
    } else {
        CStr::from_ptr(query_string).to_string_lossy().into_owned()
    };

    // Empty query string: can't parse with Lime.
    if sql.trim().is_empty() {
        return call_prev_planner(parse, query_string, cursor_options, bound_params);
    }

    let log = RA_LOG_DECISIONS.get();

    // ─── Step 1: Lime parse (raw SQL → RelExpr) ───────────────────────
    let t0 = Instant::now();
    let rel_expr = match ra_parser::sql_to_relexpr(&sql) {
        Ok(expr) => expr,
        Err(e) => {
            if log {
                pgrx::log!(
                    "ra_planner: Lime parse failed ({}), fallback: {}",
                    e,
                    truncate_sql(&sql, 120)
                );
            }
            return call_prev_planner(parse, query_string, cursor_options, bound_params);
        }
    };
    let parse_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // ─── Step 2: Ra optimize (e-graph saturation) ─────────────────────
    let t1 = Instant::now();

    // Gather statistics for the optimizer.
    let table_names = extract_rtable_schema_names(parse);
    let stats = stats_bridge::gather_all_stats(&table_names);
    let facts = SimpleFactsProvider::new(&table_names, &stats);

    let optimized = match optimize_relexpr(&rel_expr, &facts) {
        Ok(expr) => expr,
        Err(e) => {
            if log {
                pgrx::log!(
                    "ra_planner: optimization failed ({}), fallback: {}",
                    e,
                    truncate_sql(&sql, 120)
                );
            }
            return call_prev_planner(parse, query_string, cursor_options, bound_params);
        }
    };
    let optimize_ms = t1.elapsed().as_secs_f64() * 1000.0;

    // ─── Step 3: Translate to PostgreSQL Plan nodes ────────────────────
    let t2 = Instant::now();
    let table_map = plan_builder::build_table_map(parse);
    let mut builder = PlanBuilder::new(parse, table_map);

    let planned_stmt = match builder.build_planned_stmt(&optimized) {
        Ok(stmt) => stmt,
        Err(e) => {
            if log {
                pgrx::log!(
                    "ra_planner: plan build failed ({}), fallback: {}",
                    e,
                    truncate_sql(&sql, 120)
                );
            }
            return call_prev_planner(parse, query_string, cursor_options, bound_params);
        }
    };
    let translate_ms = t2.elapsed().as_secs_f64() * 1000.0;

    // ─── Timing log ───────────────────────────────────────────────────
    if log {
        pgrx::log!(
            "ra_planner: OK parse={:.2}ms optimize={:.2}ms \
             translate={:.2}ms total={:.2}ms: {}",
            parse_ms,
            optimize_ms,
            translate_ms,
            parse_ms + optimize_ms + translate_ms,
            truncate_sql(&sql, 80)
        );
    }

    // Register feedback for executor end hook.
    register_feedback(parse, &sql, &optimized);

    planned_stmt
}

/// Register pending feedback entry for the executor-end hook.
unsafe fn register_feedback(
    parse: *mut pg_sys::Query,
    sql: &str,
    optimized: &ra_core::algebra::RelExpr,
) {
    let query_id = (*parse).queryId as u64;
    if query_id == 0 {
        return;
    }

    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    sql.hash(&mut hasher);
    let query_fp = hasher.finish();

    let features = ra_engine::cost_model::extract_features(optimized);
    let fp = crate::monitor::fingerprint_reader().read();

    crate::feedback_hook::register_pending(
        query_id,
        crate::feedback_hook::PendingFeedback {
            query_fingerprint: query_fp,
            plan_fingerprint: query_id,
            features,
            system_fingerprint: fp,
            predicted_cost: 0.0,
            rules_fired: Vec::new(),
            rules_enabled: 0,
            exec_start: Instant::now(),
        },
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Optimizer
// ───────────────────────────────────────────────────────────────────────────

/// Run Ra optimizer on a RelExpr.
fn optimize_relexpr(
    rel_expr: &ra_core::algebra::RelExpr,
    facts: &dyn ra_core::FactsProvider,
) -> Result<ra_core::algebra::RelExpr, String> {
    let optimizer = ra_engine::Optimizer::new();
    optimizer
        .optimize_with_facts(rel_expr, facts)
        .map_err(|e| format!("{e}"))
}

// ───────────────────────────────────────────────────────────────────────────
// PostgreSQL helpers
// ───────────────────────────────────────────────────────────────────────────

/// Chain to the previous planner hook or the standard planner.
unsafe fn call_prev_planner(
    parse: *mut pg_sys::Query,
    query_string: *const std::ffi::c_char,
    cursor_options: i32,
    bound_params: *mut pg_sys::ParamListInfoData,
) -> *mut pg_sys::PlannedStmt {
    if let Some(prev) = PREV_PLANNER_HOOK {
        prev(parse, query_string, cursor_options, bound_params)
    } else {
        pg_sys::standard_planner(parse, query_string, cursor_options, bound_params)
    }
}

/// Extract `(schema, table)` pairs from the Query's range table.
unsafe fn extract_rtable_schema_names(parse: *mut pg_sys::Query) -> Vec<(String, String)> {
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
        let rte = pg_sys::list_nth(rtable, i) as *mut pg_sys::RangeTblEntry;
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
            pairs.push((schema_name.unwrap_or_else(|| "public".to_string()), name));
        }
    }
    pairs
}

/// Look up a relation's schema name by OID.
unsafe fn get_rel_schema_name(relid: pg_sys::Oid) -> Option<String> {
    let ns_oid = get_rel_namespace(relid);
    if ns_oid == pg_sys::InvalidOid {
        return None;
    }
    let name_ptr = pg_sys::get_namespace_name(ns_oid);
    if name_ptr.is_null() {
        return None;
    }
    Some(CStr::from_ptr(name_ptr).to_string_lossy().into_owned())
}

/// Check if any relation in the query belongs to a system catalog.
unsafe fn references_system_catalogs(parse: *mut pg_sys::Query) -> bool {
    if parse.is_null() {
        return false;
    }
    let rtable = (*parse).rtable;
    if rtable.is_null() {
        return false;
    }

    let pg_catalog_oid = pg_sys::LookupExplicitNamespace(c"pg_catalog".as_ptr(), true);
    let info_schema_oid = pg_sys::LookupExplicitNamespace(c"information_schema".as_ptr(), true);

    let length = (*rtable).length as i32;
    for i in 0..length {
        let rte = pg_sys::list_nth(rtable, i) as *mut pg_sys::RangeTblEntry;
        if rte.is_null() {
            continue;
        }
        if (*rte).rtekind != pg_sys::RTEKind::RTE_RELATION {
            continue;
        }
        let rel_ns = get_rel_namespace((*rte).relid);
        if rel_ns == pg_catalog_oid || rel_ns == info_schema_oid {
            return true;
        }
    }
    false
}

/// Look up the namespace OID of a relation.
unsafe fn get_rel_namespace(relid: pg_sys::Oid) -> pg_sys::Oid {
    let tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::RELOID as _,
        pg_sys::Datum::from(relid),
    );
    if tuple.is_null() {
        return pg_sys::InvalidOid;
    }
    let rel_form = pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_class;
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
    Some(CStr::from_ptr(name_ptr).to_string_lossy().into_owned())
}

/// Truncate a SQL string for logging.
fn truncate_sql(sql: &str, max_len: usize) -> String {
    if sql.len() <= max_len {
        sql.to_string()
    } else {
        format!("{}...", &sql[..max_len])
    }
}

// ───────────────────────────────────────────────────────────────────────────
// FactsProvider implementation
// ───────────────────────────────────────────────────────────────────────────

/// FactsProvider backed by PostgreSQL catalog statistics.
struct SimpleFactsProvider {
    table_stats: std::collections::HashMap<String, ra_core::CoreTableStats>,
    column_stats:
        std::collections::HashMap<String, std::collections::HashMap<String, ra_core::ColumnStats>>,
    schemas: std::collections::HashMap<String, ra_core::TableInfo>,
    hardware: ra_core::CoreHardwareProfile,
}

impl SimpleFactsProvider {
    fn new(table_names: &[(String, String)], stats: &[(String, ra_core::Statistics)]) -> Self {
        let mut table_stats = std::collections::HashMap::new();
        let mut column_stats = std::collections::HashMap::new();
        let mut schemas = std::collections::HashMap::new();

        let schema_for: std::collections::HashMap<&str, &str> = table_names
            .iter()
            .map(|(s, t)| (t.as_str(), s.as_str()))
            .collect();

        for (table_name, stat) in stats {
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
                    estimated_modifications: 0,
                },
            );

            let mut cols = std::collections::HashMap::new();
            for (col_name, col_stat) in &stat.columns {
                cols.insert(col_name.clone(), col_stat.clone());
            }
            column_stats.insert(table_name.clone(), cols);

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

            let primary_key: Vec<String> = stat
                .indexes
                .values()
                .find(|idx| idx.is_primary)
                .map(|idx| idx.columns.clone())
                .unwrap_or_default();

            let schema = schema_for
                .get(table_name.as_str())
                .copied()
                .unwrap_or("public");
            let fk_infos = stats_bridge::gather_foreign_keys(schema, table_name);
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
                    storage_format: ra_core::facts::StorageFormat::RowBased,
                },
            );
        }

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
            cpu_architecture: ra_core::CpuArchitecture::X86_64,
        };

        Self {
            table_stats,
            column_stats,
            schemas,
            hardware,
        }
    }
}

/// Estimate average row size from column statistics.
fn estimate_avg_row_size(stat: &ra_core::Statistics) -> f64 {
    if stat.columns.is_empty() {
        return 100.0; // default bytes per row
    }
    let total: f64 = stat
        .columns
        .values()
        .map(|cs| cs.avg_length.unwrap_or(8.0))
        .sum();
    (total + 23.0).max(24.0)
}

/// Compute confidence in statistics based on data quality.
fn compute_stats_confidence(stat: &ra_core::Statistics) -> f64 {
    if stat.row_count <= 0.0 {
        return 0.0;
    }
    let mut confidence = 0.5;
    if stat.columns.is_empty() {
        return confidence;
    }
    let total_cols = stat.columns.len() as f64;
    let mut hist_count = 0;
    let mut mcv_count = 0;
    let mut corr_count = 0;
    for cs in stat.columns.values() {
        if cs.histogram.is_some() {
            hist_count += 1;
        }
        if cs.most_common_values.is_some() && cs.most_common_freqs.is_some() {
            mcv_count += 1;
        }
        if cs.correlation.is_some() {
            corr_count += 1;
        }
    }
    confidence += 0.2 * (hist_count as f64 / total_cols);
    confidence += 0.15 * (mcv_count as f64 / total_cols);
    confidence += 0.15 * (corr_count as f64 / total_cols);
    confidence.min(1.0)
}

impl ra_core::FactsProvider for SimpleFactsProvider {
    fn get_table_stats(&self, table: &str) -> Option<&ra_core::CoreTableStats> {
        self.table_stats.get(table)
    }

    fn get_column_stats(&self, table: &str, column: &str) -> Option<&ra_core::ColumnStats> {
        self.column_stats
            .get(table)
            .and_then(|cols| cols.get(column))
    }

    fn hardware_profile(&self) -> &ra_core::CoreHardwareProfile {
        &self.hardware
    }

    fn get_schema(&self, table: &str) -> Option<&ra_core::TableInfo> {
        self.schemas.get(table)
    }

    fn runtime_stats(&self, _operator_id: &str) -> Option<&ra_core::OperatorStats> {
        None
    }

    fn database_name(&self) -> &'static str {
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
        let stats = ra_core::Statistics::new(0.0);
        assert!((compute_stats_confidence(&stats) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn confidence_row_count_only() {
        let stats = ra_core::Statistics::new(1000.0);
        let conf = compute_stats_confidence(&stats);
        assert!((conf - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_avg_row_size_no_columns() {
        let stats = ra_core::Statistics::new(100.0);
        let size = estimate_avg_row_size(&stats);
        assert!((size - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_avg_row_size_with_columns() {
        let mut stats = ra_core::Statistics::new(100.0);
        let mut cs = ra_core::ColumnStats::new(10.0);
        cs.avg_length = Some(16.0);
        stats.columns.insert("col1".into(), cs);
        let size = estimate_avg_row_size(&stats);
        assert!((size - 39.0).abs() < f64::EPSILON);
    }
}
