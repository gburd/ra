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

    // Catch panics to surface as PostgreSQL ERRORs rather than crashing.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ra_planner_hook_inner(parse, query_string, cursor_options, bound_params)
    }));

    match result {
        Ok(plan) => plan,
        Err(payload) => {
            // A Ra-side panic must NEVER crash the backend: fall back to
            // the native planner. We deliberately do NOT use
            // pgrx::error!/ereport here — this hook has no #[pg_guard]
            // boundary, so raising a pgrx error panics via panic_any and,
            // with no catch on the stack, aborts the process across PG's
            // C planner() frame ("failed to initiate panic"). Log to
            // stderr (captured by the server log) and degrade gracefully.
            if RA_LOG_DECISIONS.get() {
                let msg = if let Some(s) = payload.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = payload.downcast_ref::<&str>() {
                    (*s).to_string()
                } else if let Some(e) =
                    payload.downcast_ref::<pgrx::pg_sys::panic::ErrorReportWithLevel>()
                {
                    e.message().to_string()
                } else if let Some(c) =
                    payload.downcast_ref::<pgrx::pg_sys::panic::CaughtError>()
                {
                    match c {
                        pgrx::pg_sys::panic::CaughtError::PostgresError(e)
                        | pgrx::pg_sys::panic::CaughtError::ErrorReport(e) => {
                            e.message().to_string()
                        }
                        pgrx::pg_sys::panic::CaughtError::RustPanic { ereport, .. } => {
                            ereport.message().to_string()
                        }
                    }
                } else {
                    "unknown panic".to_string()
                };
                eprintln!("ra_planner: inner panic, falling back to native planner: {msg}");
            }
            call_prev_planner(parse, query_string, cursor_options, bound_params)
        }
    }
}

/// Inner planner hook: Lime parse → Ra optimize → Plan node translation.
///
/// If the parser hook already parsed this query, uses the pre-parsed
/// statement directly (no re-parsing). Otherwise falls back to parsing
/// here in the planner hook.
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
    use ra_core::algebra::Statement;

    // Refresh system fingerprint if intervals have elapsed.
    crate::monitor::maybe_refresh();

    let sql = if query_string.is_null() {
        String::new()
    } else {
        CStr::from_ptr(query_string).to_string_lossy().into_owned()
    };

    // Empty query string: nothing for Ra to do — let PG handle it.
    if sql.trim().is_empty() {
        return call_prev_planner(parse, query_string, cursor_options, bound_params);
    }

    let log = RA_LOG_DECISIONS.get();

    // ─── Step 1: Get RelExpr (from parser hook or re-parse) ───────────
    let t0 = Instant::now();

    // Check if the parser hook already parsed this query.
    let rel_expr = if let Some(stmt) = crate::parser_hook::take_parsed() {
        match stmt {
            Statement::Query(rel) | Statement::Dml(rel) => rel,
            Statement::Ddl(_) | Statement::Utility(_) | Statement::Transaction(_) => {
                // Non-optimizable statements should not reach the planner.
                // Fall back to PG's standard planner.
                return call_prev_planner(parse, query_string, cursor_options, bound_params);
            }
        }
    } else {
        // Parser hook didn't fire — parse now. A parse failure
        // means Ra can't represent this query; fall back to PG's
        // native planner rather than failing a query PG could
        // plan. Ra must be a strict drop-in: never break a
        // working query.
        //
        // EXPLAIN/EXPLAIN ANALYZE: PostgreSQL plans the *inner* query but
        // hands the planner the full "EXPLAIN ... <query>" text. Strip the
        // leading EXPLAIN clause so Lime parses the inner query; PG's EXPLAIN
        // machinery then renders Ra's returned plan in the requested format
        // (TEXT/JSON/etc., ANALYZE, BUFFERS, ...) just like any native plan.
        let parse_sql = strip_explain_prefix(&sql).unwrap_or(sql.as_str());
        match ra_parser::sql_to_relexpr(parse_sql) {
            Ok(expr) => expr,
            Err(e) => {
                if log {
                    pgrx::log!(
                        "ra_planner: parse fell back to PG: {} [query: {}]",
                        e,
                        truncate_sql(&sql, 80)
                    );
                }
                return call_prev_planner(parse, query_string, cursor_options, bound_params);
            }
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
            // Optimization failed — fall back to PG's planner
            // rather than aborting the query.
            if log {
                pgrx::log!(
                    "ra_planner: optimize fell back to PG: {} [query: {}]",
                    e,
                    truncate_sql(&sql, 80)
                );
            }
            return call_prev_planner(parse, query_string, cursor_options, bound_params);
        }
    };
    let optimize_ms = t1.elapsed().as_secs_f64() * 1000.0;

    // ─── Step 3: Translate to PostgreSQL Plan nodes ────────────────────
    let t2 = Instant::now();
    let table_map = plan_builder::build_table_map(parse);
    let mut builder = PlanBuilder::new(parse, table_map, &stats);

    // Honor user-supplied scan/join/parallelism advice by handing
    // the derived per-relation physical-strategy preferences to the
    // plan builder (RFC 0087 consumption path).
    //
    // Cost-driven defaults (`PhysicalChoices::augment_from_stats`,
    // which would pick IndexScan vs SeqScan from statistics even
    // without supplied advice) are intentionally NOT applied on the
    // production planner path. The plan builder emits a concrete
    // PG IndexScan with a fixed selectivity guess, bypassing PG's
    // own path-costing; enabling that by default could regress plan
    // quality. It stays gated to the `optimize_bounded` API (CLI,
    // tests, benchmarks) until the Ra-vs-PG plan-quality comparison
    // validates it. Supplied advice is explicit user intent, so it
    // is always honored here.
    // Physical-strategy choices handed to the plan builder:
    //  - Supplied scan/join/parallel advice (explicit user intent), and
    //  - Cost-based join-method selection (layer 2): HashJoin vs NestLoop
    //    chosen per join from cardinalities, carried here for the builder to
    //    render (it applies catalog feasibility). Advice wins over the
    //    cost-based default. Scan-strategy augmentation stays withheld on the
    //    production path (it bypasses PG's own path-costing).
    {
        use ra_engine::plan_advice_physical::PhysicalChoices;
        let mut choices = crate::extension_state::effective_plan_advice()
            .and_then(|a| ra_plan_advice::parse_advice(&a).ok())
            .map_or_else(PhysicalChoices::new, |p| PhysicalChoices::from_advice(&p));
        let stats_map: std::collections::HashMap<String, ra_core::Statistics> = stats
            .iter()
            .map(|(t, s)| (t.to_lowercase(), s.clone()))
            .collect();
        choices.augment_join_strategies_from_stats(&optimized, &stats_map);
        if !choices.is_empty() {
            builder.set_physical_choices(choices);
        }
    }

    let planned_stmt = match builder.build_planned_stmt(&optimized) {
        Ok(stmt) => stmt,
        Err(e) => {
            // Plan-builder couldn't translate this RelExpr to a PG
            // Plan (e.g. an operator variant we don't emit yet such
            // as MATCH_RECOGNIZE or a vector TopK). Fall back to
            // PG's native planner instead of failing the query.
            if log {
                pgrx::log!(
                    "ra_planner: plan-build fell back to PG: {} [query: {}]",
                    e,
                    truncate_sql(&sql, 80)
                );
            }
            return call_prev_planner(parse, query_string, cursor_options, bound_params);
        }
    };
    let translate_ms = t2.elapsed().as_secs_f64() * 1000.0;

    // Stash generated plan advice keyed on the PlannedStmt
    // pointer so the EXPLAIN(PLAN_ADVICE) hook can render it.
    // ra_planner.always_store_advice_details widens this from
    // EXPLAIN-driven to every-plan, mirroring PG's
    // pg_plan_advice.always_store_advice_details GUC. We do this
    // unconditionally for now because emit_advice is cheap; the
    // EXPLAIN side decides whether to display.
    let advice = ra_engine::plan_advice_emit::emit_advice(&optimized);
    if !advice.is_empty() {
        let rendered = ra_plan_advice::render_advice(&advice);
        crate::plan_advice_explain::stash_generated_advice(planned_stmt, rendered);
    }

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
    register_feedback(parse, &sql, &rel_expr, &optimized);

    planned_stmt
}

/// Register pending feedback entry for the executor-end hook.
unsafe fn register_feedback(
    parse: *mut pg_sys::Query,
    sql: &str,
    original: &ra_core::algebra::RelExpr,
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

    // Predict CPU cost (ms) using the loaded BitNet model. Pre-A3 this
    // was hard-coded to 0.0, which made MAPE always equal 1.0 (max error)
    // and short-circuited the feedback loop. Now we feed real predictions
    // so the MAPE tracker measures genuine model accuracy.
    let predicted_cost = crate::extension_state::cost_model()
        .map(|m| f64::from(m.predict_cpu_ms(&features.as_array())))
        .unwrap_or(0.0);

    // Approximate which rule categories fired by comparing structural
    // counts of original vs optimized. Pre-A3 this was an empty Vec, so
    // the rule selector had no training signal at all. The classification
    // below is intentionally conservative — it labels a category as
    // "fired" only when the optimization actually changed the relevant
    // node count. For per-rule precision the planner_hook would need to
    // call `optimize_with_tracking` (with budget overhead); we keep the
    // fast path and accept category-level granularity here.
    let rules_fired = classify_rules_fired(original, optimized);
    let rules_enabled = 10; // matches NeuralRuleSelector group count

    crate::feedback_hook::register_pending(
        query_id,
        crate::feedback_hook::PendingFeedback {
            query_fingerprint: query_fp,
            plan_fingerprint: query_id,
            features,
            system_fingerprint: fp,
            predicted_cost,
            rules_fired,
            rules_enabled,
            exec_start: Instant::now(),
        },
    );
}

/// Classify which rule-group indices fired by comparing structural
/// counts of the pre- and post-optimization expressions.
///
/// Index → category mapping (matches `NeuralRuleSelector::GROUP_NAMES`):
///   0 = predicate pushdown (filter count decreased near scans)
///   1 = join reordering (join shape changed, count preserved)
///   2 = projection pruning (project count decreased)
///   3 = expression simplification (constant subexpressions removed)
///   4 = aggregate optimization (aggregate moved/merged)
///   5 = join elimination (join count decreased)
///   6 = CTE optimization (cte count decreased)
///   7 = semi-join reduction (semi join introduced)
///   8 = column pruning (project columns reduced)
///   9 = limit/sort optimization (sort eliminated or merged)
fn classify_rules_fired(
    original: &ra_core::algebra::RelExpr,
    optimized: &ra_core::algebra::RelExpr,
) -> Vec<u32> {
    use ra_engine::cost_model::extract_features;

    let f0 = extract_features(original);
    let f1 = extract_features(optimized);
    let mut fired = Vec::new();

    if f1.filter_count < f0.filter_count {
        fired.push(0); // predicate pushdown / filter merging
    }
    if f1.join_count < f0.join_count {
        fired.push(5); // join elimination
    } else if format!("{original:?}") != format!("{optimized:?}")
        && (f1.join_count - f0.join_count).abs() < f32::EPSILON
        && f0.join_count > 0.0
    {
        fired.push(1); // join reordering (shape changed, count preserved)
    }
    if f1.aggregate_count != f0.aggregate_count {
        fired.push(4); // aggregate optimization
    }
    if f1.cte_count < f0.cte_count {
        fired.push(6); // CTE optimization
    }
    if f1.subquery_count < f0.subquery_count {
        fired.push(7); // semi-join reduction (decorrelation)
    }
    if f1.order_by_count < f0.order_by_count {
        fired.push(9); // sort elimination
    }
    fired
}

// ───────────────────────────────────────────────────────────────────────────
// Optimizer
// ───────────────────────────────────────────────────────────────────────────

/// Run Ra optimizer on a RelExpr.
fn optimize_relexpr(
    rel_expr: &ra_core::algebra::RelExpr,
    facts: &dyn ra_core::FactsProvider,
) -> Result<ra_core::algebra::RelExpr, String> {
    let mut config = ra_engine::OptimizerConfig::default();
    if let Some(s) = crate::extension_state::effective_plan_advice() {
        config.plan_advice = Some(s);
    }
    // Share the monitor's live fingerprint with the optimizer so its cost
    // model auto-tunes plan choice to the execution environment. The
    // fingerprint is an Arc<AtomicFingerprint> owned by the monitor — no
    // global mutable state crosses into the engine.
    let reader = ra_engine::state::FingerprintReader::from_shared(
        crate::monitor::fingerprint_reader().shared().clone(),
    );
    let optimizer = ra_engine::Optimizer::with_config(config).with_fingerprint_reader(reader);
    optimizer
        .optimize_with_facts(rel_expr, facts)
        .map_err(|e| format!("{e}"))
}

// Provenance via EXPLAIN: `PlanProvenance` (cost-model snapshot,
// hardware hash, rule-set hash, route, termination reason) is
// captured on every `OptimizationResult` and surfaced today via
// the CLI (`ra-cli explain --provenance`). Exposing it through a
// PG `EXPLAIN (RA_PROVENANCE)` option would mirror the existing
// `plan_advice_explain` machinery (RegisterExtensionExplainOption
// + session stash + explain_per_plan_hook). It's deferred — not
// half-built — because it changes no planning behavior and the
// EXPLAIN-option FFI is PG-version-sensitive; tracked in
// rfcs/text/0090-provenance-explain-option.md.

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

/// If `sql` is an `EXPLAIN` command, return the inner statement text.
///
/// PostgreSQL hands the planner the full `EXPLAIN ... <query>` source string
/// even though it only plans the inner query, so Ra (which re-parses text)
/// must strip the EXPLAIN clause to reach the query. Handles the parenthesized
/// option form `EXPLAIN ( ... ) stmt` (covering every option: ANALYZE,
/// VERBOSE, BUFFERS, COSTS, SETTINGS, WAL, TIMING, SUMMARY, MEMORY, SERIALIZE,
/// GENERIC_PLAN, FORMAT ...) and the legacy `EXPLAIN [ANALYZE] [VERBOSE] stmt`
/// form. Returns `None` when `sql` is not an EXPLAIN command.
fn strip_explain_prefix(sql: &str) -> Option<&str> {
    let s = sql.trim_start();
    let bytes = s.as_bytes();
    if bytes.len() < 7 || !bytes[..7].eq_ignore_ascii_case(b"explain") {
        return None;
    }
    let rest = &s[7..];
    // Require a word boundary after EXPLAIN (whitespace or the option paren).
    match rest.chars().next() {
        Some(c) if c.is_whitespace() => {}
        Some('(') => {}
        _ => return None,
    }
    let mut rest = rest.trim_start();
    if let Some(after) = rest.strip_prefix('(') {
        let mut depth = 1usize;
        for (i, c) in after.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(after[i + 1..].trim_start());
                    }
                }
                _ => {}
            }
        }
        return None; // unbalanced parens — let PG handle it
    }
    // Legacy form: skip a run of ANALYZE / VERBOSE keywords.
    loop {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = &rest[..end];
        if word.eq_ignore_ascii_case("analyze")
            || word.eq_ignore_ascii_case("analyse")
            || word.eq_ignore_ascii_case("verbose")
        {
            rest = rest[end..].trim_start();
        } else {
            break;
        }
    }
    Some(rest)
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
