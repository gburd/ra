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
use crate::plan_converter;
use crate::stats_bridge;

/// Saved pointer to the previous planner hook (for chaining).
static mut PREV_PLANNER_HOOK: Option<
    unsafe extern "C" fn(
        parse: *mut pg_sys::Query,
        query_string: *const std::ffi::c_char,
        cursorOptions: i32,
        boundParams: *mut pg_sys::ParamListInfoData,
    ) -> *mut pg_sys::PlannedStmt,
> = None;

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
#[pg_guard]
unsafe extern "C" fn ra_planner_hook(
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

    // Conservative strategy: always use the standard planner.
    //
    // Future: run RA optimizer on the parsed query, check if the
    // resulting plan's confidence exceeds RA_MIN_CONFIDENCE, and
    // if so, apply advice to influence the planner.
    let _min_confidence = RA_MIN_CONFIDENCE.get();
    let _calibration = CostCalibration::default_calibration();

    call_prev_planner(
        parse,
        query_string,
        cursor_options,
        bound_params,
    )
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

    let length = (*rtable).length as usize;
    let mut cell = (*rtable).head;

    for _ in 0..length {
        if cell.is_null() {
            break;
        }
        let rte = (*cell).ptr_value as *mut pg_sys::RangeTblEntry;
        if !rte.is_null()
            && (*rte).rtekind == pg_sys::RTEKind::RTE_RELATION
        {
            let relid = (*rte).relid;
            let rel_name = get_rel_name(relid);
            if let Some(name) = rel_name {
                names.push(name);
            }
        }
        cell = (*cell).next;
    }
    names
}

/// Look up a relation name by OID using SPI.
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
