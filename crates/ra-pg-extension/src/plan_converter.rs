//! Convert RA `RelExpr` trees to PostgreSQL plan advice strings.
//!
//! Rather than directly constructing PostgreSQL `Plan` nodes (which
//! requires deep C interop), this module converts optimized RA plans
//! into advice strings that the planner hook can apply via cost
//! manipulation or, when available, `pg_plan_advice` GUC.
//!
//! The conversion extracts:
//! - Join order (left-to-right DFS of join tree)
//! - Join methods (hash / merge / nested-loop)
//! - Scan strategies (sequential / index)
//! - Parallelism hints

use ra_core::{JoinType, RelExpr};

use crate::pg_constants::{guc_names, guc_tuning, hardware_aware};

/// A complete set of plan advice extracted from an RA `RelExpr`.
#[derive(Debug, Clone)]
pub struct PlanAdviceSet {
    /// Ordered list of relation names for join ordering.
    pub join_order: Vec<String>,
    /// Per-join method hints.
    pub join_methods: Vec<JoinMethodAdvice>,
    /// Per-relation scan hints.
    pub scan_methods: Vec<ScanAdvice>,
}

/// Join method advice for a single join.
#[derive(Debug, Clone)]
pub struct JoinMethodAdvice {
    /// The relation on the inner (probe) side.
    pub inner_relation: String,
    /// Suggested join algorithm.
    pub method: JoinMethod,
}

/// Physical join algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinMethod {
    /// Hash join.
    Hash,
    /// Sort-merge join.
    Merge,
    /// Nested loop join.
    NestedLoop,
}

/// Scan method advice for a single relation.
#[derive(Debug, Clone)]
pub struct ScanAdvice {
    /// The relation being scanned.
    pub relation: String,
    /// Suggested scan strategy.
    pub method: ScanMethod,
}

/// Physical scan strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanMethod {
    /// Full sequential scan.
    Sequential,
    /// Index scan using a named index.
    Index(String),
    /// Bitmap heap scan.
    BitmapHeap,
}

impl PlanAdviceSet {
    /// Format as a `pg_plan_advice` compatible string.
    pub fn to_advice_string(&self) -> String {
        let mut parts = Vec::new();

        if self.join_order.len() >= 2 {
            parts.push(format!(
                "JOIN_ORDER({})",
                self.join_order.join(" ")
            ));
        }

        for jm in &self.join_methods {
            let keyword = match jm.method {
                JoinMethod::Hash => "HASH_JOIN",
                JoinMethod::Merge => "MERGE_JOIN",
                JoinMethod::NestedLoop => "NESTED_LOOP",
            };
            parts.push(format!(
                "{keyword}({})",
                jm.inner_relation
            ));
        }

        for sm in &self.scan_methods {
            match &sm.method {
                ScanMethod::Sequential => {
                    parts.push(format!(
                        "SEQ_SCAN({})",
                        sm.relation
                    ));
                }
                ScanMethod::Index(idx) => {
                    parts.push(format!(
                        "INDEX_SCAN({} {idx})",
                        sm.relation
                    ));
                }
                ScanMethod::BitmapHeap => {
                    parts.push(format!(
                        "BITMAP_HEAP_SCAN({})",
                        sm.relation
                    ));
                }
            }
        }

        parts.join(" ")
    }

    /// Format as `pg_hint_plan` compatible comment.
    pub fn to_pg_hint_plan(&self) -> String {
        let mut hints = Vec::new();

        if self.join_order.len() >= 2 {
            hints.push(format!(
                "Leading({})",
                self.join_order.join(" ")
            ));
        }

        for jm in &self.join_methods {
            let keyword = match jm.method {
                JoinMethod::Hash => "HashJoin",
                JoinMethod::Merge => "MergeJoin",
                JoinMethod::NestedLoop => "NestLoop",
            };
            hints.push(format!(
                "{keyword}({})",
                jm.inner_relation
            ));
        }

        for sm in &self.scan_methods {
            match &sm.method {
                ScanMethod::Sequential => {
                    hints.push(format!(
                        "SeqScan({})",
                        sm.relation
                    ));
                }
                ScanMethod::Index(idx) => {
                    hints.push(format!(
                        "IndexScan({} {idx})",
                        sm.relation
                    ));
                }
                ScanMethod::BitmapHeap => {
                    hints.push(format!(
                        "BitmapScan({})",
                        sm.relation
                    ));
                }
            }
        }

        if hints.is_empty() {
            return String::new();
        }
        format!("/*+ {} */", hints.join(" "))
    }
}

/// Extract a full `PlanAdviceSet` from an optimized RA `RelExpr`.
pub fn extract_plan_advice(expr: &RelExpr) -> PlanAdviceSet {
    let mut join_order = Vec::new();
    let mut join_methods = Vec::new();
    let mut scan_methods = Vec::new();

    collect_advice(
        expr,
        &mut join_order,
        &mut join_methods,
        &mut scan_methods,
    );

    PlanAdviceSet {
        join_order,
        join_methods,
        scan_methods,
    }
}

fn collect_advice(
    expr: &RelExpr,
    join_order: &mut Vec<String>,
    join_methods: &mut Vec<JoinMethodAdvice>,
    scan_methods: &mut Vec<ScanAdvice>,
) {
    match expr {
        RelExpr::Scan { table, alias, .. } => {
            let name =
                alias.as_deref().unwrap_or(table).to_string();
            join_order.push(name.clone());
            scan_methods.push(ScanAdvice {
                relation: name,
                method: ScanMethod::Sequential,
            });
        }
        RelExpr::IndexScan { table, .. } => {
            join_order.push(table.clone());
            scan_methods.push(ScanAdvice {
                relation: table.clone(),
                method: ScanMethod::Index("auto".to_string()),
            });
        }
        RelExpr::IndexOnlyScan {
            table, index, ..
        } => {
            join_order.push(table.clone());
            scan_methods.push(ScanAdvice {
                relation: table.clone(),
                method: ScanMethod::Index(index.clone()),
            });
        }
        RelExpr::BitmapHeapScan {
            table, bitmap, ..
        } => {
            join_order.push(table.clone());
            scan_methods.push(ScanAdvice {
                relation: table.clone(),
                method: ScanMethod::BitmapHeap,
            });
            collect_advice(
                bitmap,
                join_order,
                join_methods,
                scan_methods,
            );
        }
        RelExpr::BitmapIndexScan { .. }
        | RelExpr::BitmapAnd { .. }
        | RelExpr::BitmapOr { .. } => {
            // Bitmap sub-plans are handled through
            // BitmapHeapScan; nothing to extract here.
        }
        RelExpr::ParallelScan { table, .. } => {
            join_order.push(table.clone());
            scan_methods.push(ScanAdvice {
                relation: table.clone(),
                method: ScanMethod::Sequential,
            });
        }
        RelExpr::Join {
            join_type,
            left,
            right,
            ..
        }
        | RelExpr::ParallelHashJoin {
            join_type,
            left,
            right,
            ..
        } => {
            collect_advice(
                left,
                join_order,
                join_methods,
                scan_methods,
            );
            collect_advice(
                right,
                join_order,
                join_methods,
                scan_methods,
            );

            if let Some(inner_name) = first_relation_name(right) {
                if let Some(method) =
                    map_join_type(*join_type)
                {
                    join_methods.push(JoinMethodAdvice {
                        inner_relation: inner_name,
                        method,
                    });
                }
            }
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::IncrementalSort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::RowPattern { input, .. }
        | RelExpr::ParallelAggregate { input, .. }
        | RelExpr::Gather { input, .. } => {
            collect_advice(
                input,
                join_order,
                join_methods,
                scan_methods,
            );
        }
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            collect_advice(
                left,
                join_order,
                join_methods,
                scan_methods,
            );
            collect_advice(
                right,
                join_order,
                join_methods,
                scan_methods,
            );
        }
        RelExpr::CTE { body, .. }
        | RelExpr::RecursiveCTE { body, .. } => {
            collect_advice(
                body,
                join_order,
                join_methods,
                scan_methods,
            );
        }
        RelExpr::Unnest { input, .. }
        | RelExpr::TableFunction { input, .. } => {
            if let Some(inp) = input {
                collect_advice(
                    inp,
                    join_order,
                    join_methods,
                    scan_methods,
                );
            }
        }
        RelExpr::Values { .. }
        | RelExpr::MultiUnnest { .. } => {}
        RelExpr::MvScan { view_name, alias, .. } => {
            let name = alias.as_deref().unwrap_or(view_name).to_string();
            join_order.push(name.clone());
            scan_methods.push(ScanAdvice {
                relation: name,
                method: ScanMethod::Sequential,
            });
        }
    }
}

/// Get the first relation name from a subtree.
fn first_relation_name(expr: &RelExpr) -> Option<String> {
    match expr {
        RelExpr::Scan { table, alias, .. } => {
            Some(alias.as_deref().unwrap_or(table).to_string())
        }
        RelExpr::IndexScan { table, .. }
        | RelExpr::IndexOnlyScan { table, .. }
        | RelExpr::BitmapHeapScan { table, .. }
        | RelExpr::ParallelScan { table, .. } => {
            Some(table.clone())
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Gather { input, .. } => {
            first_relation_name(input)
        }
        RelExpr::Join { left, .. }
        | RelExpr::ParallelHashJoin { left, .. } => {
            first_relation_name(left)
        }
        _ => None,
    }
}

/// Map RA `JoinType` to a physical join method.
///
/// Returns `None` for cross joins (no specific method preference).
fn map_join_type(jt: JoinType) -> Option<JoinMethod> {
    match jt {
        JoinType::Inner
        | JoinType::LeftOuter
        | JoinType::RightOuter
        | JoinType::FullOuter => Some(JoinMethod::Hash),
        JoinType::Semi | JoinType::Anti => {
            Some(JoinMethod::NestedLoop)
        }
        JoinType::Cross => None,
    }
}

/// Count the number of base relations in a `RelExpr` tree.
pub fn count_relations(expr: &RelExpr) -> usize {
    match expr {
        RelExpr::Scan { .. }
        | RelExpr::IndexScan { .. }
        | RelExpr::IndexOnlyScan { .. }
        | RelExpr::ParallelScan { .. } => 1,
        RelExpr::Join { left, right, .. }
        | RelExpr::ParallelHashJoin { left, right, .. }
        | RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            count_relations(left) + count_relations(right)
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::IncrementalSort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::RowPattern { input, .. }
        | RelExpr::ParallelAggregate { input, .. }
        | RelExpr::Gather { input, .. } => {
            count_relations(input)
        }
        RelExpr::CTE {
            definition, body, ..
        } => count_relations(definition) + count_relations(body),
        RelExpr::RecursiveCTE {
            base_case,
            recursive_case,
            body,
            ..
        } => {
            count_relations(base_case)
                + count_relations(recursive_case)
                + count_relations(body)
        }
        RelExpr::BitmapHeapScan { bitmap, .. } => {
            count_bitmap_relations(bitmap)
        }
        RelExpr::BitmapIndexScan { .. } => 1,
        RelExpr::BitmapAnd { inputs }
        | RelExpr::BitmapOr { inputs } => {
            inputs.iter().map(|b| count_bitmap_relations(b)).sum()
        }
        RelExpr::Unnest { input, .. }
        | RelExpr::TableFunction { input, .. } => {
            input.as_ref().map_or(0, |i| count_relations(i))
        }
        RelExpr::Values { .. }
        | RelExpr::MultiUnnest { .. } => 0,
        RelExpr::MvScan { .. } => 1,
    }
}

/// Count relations in bitmap sub-plans (bitmap scans reference
/// a single table, so we count each `BitmapIndexScan` as one).
fn count_bitmap_relations(expr: &RelExpr) -> usize {
    match expr {
        RelExpr::BitmapIndexScan { .. } => 1,
        RelExpr::BitmapAnd { inputs }
        | RelExpr::BitmapOr { inputs } => {
            inputs.iter().map(|b| count_bitmap_relations(b)).sum()
        }
        // BitmapHeapScan wraps a bitmap sub-plan, count 1 for
        // the table itself.
        RelExpr::BitmapHeapScan { .. } => 1,
        _ => count_relations(expr),
    }
}

/// Extract all base-table names from an expression tree.
///
/// Returns `(schema, table)` pairs. Since the RA `RelExpr::Scan`
/// does not carry a schema, the caller should default to `"public"`.
pub fn extract_table_names(expr: &RelExpr) -> Vec<String> {
    let mut tables = Vec::new();
    collect_table_names(expr, &mut tables);
    tables.sort();
    tables.dedup();
    tables
}

fn collect_table_names(
    expr: &RelExpr,
    out: &mut Vec<String>,
) {
    match expr {
        RelExpr::Scan { table, .. }
        | RelExpr::IndexScan { table, .. }
        | RelExpr::IndexOnlyScan { table, .. }
        | RelExpr::ParallelScan { table, .. } => {
            out.push(table.clone());
        }
        RelExpr::BitmapHeapScan {
            table, bitmap, ..
        } => {
            out.push(table.clone());
            collect_table_names(bitmap, out);
        }
        RelExpr::BitmapIndexScan { table, .. } => {
            out.push(table.clone());
        }
        RelExpr::BitmapAnd { inputs }
        | RelExpr::BitmapOr { inputs } => {
            for bitmap in inputs {
                collect_table_names(bitmap, out);
            }
        }
        RelExpr::Join { left, right, .. }
        | RelExpr::ParallelHashJoin { left, right, .. }
        | RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            collect_table_names(left, out);
            collect_table_names(right, out);
        }
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::IncrementalSort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input, .. }
        | RelExpr::Window { input, .. }
        | RelExpr::Aggregate { input, .. }
        | RelExpr::RowPattern { input, .. }
        | RelExpr::ParallelAggregate { input, .. }
        | RelExpr::Gather { input, .. } => {
            collect_table_names(input, out);
        }
        RelExpr::CTE {
            definition, body, ..
        } => {
            collect_table_names(definition, out);
            collect_table_names(body, out);
        }
        RelExpr::RecursiveCTE {
            base_case,
            recursive_case,
            body,
            ..
        } => {
            collect_table_names(base_case, out);
            collect_table_names(recursive_case, out);
            collect_table_names(body, out);
        }
        RelExpr::Unnest { input, .. }
        | RelExpr::TableFunction { input, .. } => {
            if let Some(inp) = input {
                collect_table_names(inp, out);
            }
        }
        RelExpr::Values { .. }
        | RelExpr::MultiUnnest { .. } => {}
        RelExpr::MvScan { view_name, .. } => {
            out.push(view_name.clone());
        }
    }
}

/// Convert optimized RA RelExpr to PostgreSQL PlannedStmt via advice injection.
///
/// Rather than constructing PostgreSQL Plan nodes directly (which is
/// extremely complex), this approach:
/// 1. Extracts plan advice from the RA RelExpr
/// 2. Calls the standard PostgreSQL planner
/// 3. Applies cost adjustments to guide the planner toward the RA plan
///
/// # Safety
///
/// Caller must pass a valid `Query` pointer.
///
/// # Architecture Note
///
/// Direct PlannedStmt construction would require:
/// - Allocating Plan nodes in PostgreSQL memory contexts
/// - Setting up complex Plan node relationships
/// - Computing PostgreSQL-compatible costs
/// - Managing path generation and comparison
///
/// The advice-based approach is more maintainable and robust.
pub unsafe fn convert_to_planned_stmt(
    expr: &ra_core::algebra::RelExpr,
    original_query: *mut pgrx::pg_sys::Query,
    stats: &[(String, ra_core::Statistics)],
    calibration: &crate::cost_mapper::CostCalibration,
) -> Result<*mut pgrx::pg_sys::PlannedStmt, String> {
    // Step 1: Extract plan advice from RA RelExpr
    let advice = extract_plan_advice(expr);

    // Step 2: Calculate expected cost improvement from RA optimization
    let improvement_factor = estimate_improvement_factor(expr, stats, calibration);

    // Step 3: Apply advice via cost manipulation
    // This guides PostgreSQL's standard planner toward the RA plan
    apply_plan_advice_via_costs(
        original_query,
        &advice,
        improvement_factor,
    )
}

/// Estimate the improvement factor of RA's plan vs. PostgreSQL's default.
///
/// Returns a multiplier indicating how much better RA's plan is expected
/// to be (e.g., 0.5 = 2x faster, 0.2 = 5x faster).
///
/// Uses statistics coverage to scale the improvement estimate:
/// - High coverage (>80%): trust the RA plan more, allow larger improvements
/// - Low coverage (<50%): conservative, assume minimal improvement
fn estimate_improvement_factor(
    expr: &ra_core::algebra::RelExpr,
    stats: &[(String, ra_core::Statistics)],
    _calibration: &crate::cost_mapper::CostCalibration,
) -> f64 {
    let table_names = extract_table_names(expr);
    if table_names.is_empty() {
        return 1.0;
    }

    // Calculate statistics coverage
    let covered = table_names
        .iter()
        .filter(|t| stats.iter().any(|(name, _)| name == *t))
        .count();
    let coverage = covered as f64 / table_names.len() as f64;

    // Calculate column-level stats quality
    let col_quality: f64 = stats
        .iter()
        .map(|(_, s)| {
            if s.columns.is_empty() {
                0.0
            } else {
                let has_hist = s
                    .columns
                    .values()
                    .filter(|c| c.histogram.is_some())
                    .count();
                has_hist as f64 / s.columns.len() as f64
            }
        })
        .sum::<f64>()
        / stats.len().max(1) as f64;

    // Base improvement: conservative 20%
    let base_improvement = 0.8;

    // Scale improvement by stats quality:
    // Good stats -> more aggressive (down to 0.5 = 2x improvement)
    // Poor stats -> conservative (stay near 1.0)
    let stats_factor = coverage * 0.5 + col_quality * 0.5;
    let improvement = base_improvement - (0.3 * stats_factor);

    improvement.clamp(0.5, 1.0)
}

/// Apply plan advice by manipulating PostgreSQL's cost model.
///
/// This adjusts GUC parameters and relation costs to guide the standard
/// planner toward the RA-optimized plan.
///
/// # Safety
///
/// Caller must pass a valid `Query` pointer.
unsafe fn apply_plan_advice_via_costs(
    query: *mut pgrx::pg_sys::Query,
    advice: &PlanAdviceSet,
    improvement_factor: f64,
) -> Result<*mut pgrx::pg_sys::PlannedStmt, String> {
    use pgrx::pg_sys;

    // Save current GUC settings to restore later
    let _saved_settings = save_planner_gucs();

    // Adjust GUCs based on advice
    apply_advice_to_gucs(advice, improvement_factor)?;

    // Call standard planner with adjusted costs
    let planned_stmt = pg_sys::standard_planner(
        query,
        std::ptr::null(),
        0,
        std::ptr::null_mut(),
    );

    // Restore GUC settings
    // (happens automatically when _saved_settings drops)

    if planned_stmt.is_null() {
        return Err("Standard planner returned null".to_string());
    }

    Ok(planned_stmt)
}

/// Save current planner GUC settings for restoration.
struct SavedPlannerGucs {
    enable_hashjoin: bool,
    enable_mergejoin: bool,
    enable_nestloop: bool,
    enable_seqscan: bool,
    enable_indexscan: bool,
    enable_bitmapscan: bool,
    random_page_cost: f64,
}

impl Drop for SavedPlannerGucs {
    fn drop(&mut self) {
        // Restore GUC values on drop
        unsafe {
            set_guc_bool(guc_names::ENABLE_HASHJOIN, self.enable_hashjoin);
            set_guc_bool(guc_names::ENABLE_MERGEJOIN, self.enable_mergejoin);
            set_guc_bool(guc_names::ENABLE_NESTLOOP, self.enable_nestloop);
            set_guc_bool(guc_names::ENABLE_SEQSCAN, self.enable_seqscan);
            set_guc_bool(guc_names::ENABLE_INDEXSCAN, self.enable_indexscan);
            set_guc_bool(guc_names::ENABLE_BITMAPSCAN, self.enable_bitmapscan);
            set_guc_real(guc_names::RANDOM_PAGE_COST, self.random_page_cost);
        }
    }
}

fn save_planner_gucs() -> SavedPlannerGucs {
    unsafe {
        SavedPlannerGucs {
            enable_hashjoin: get_guc_bool(guc_names::ENABLE_HASHJOIN),
            enable_mergejoin: get_guc_bool(guc_names::ENABLE_MERGEJOIN),
            enable_nestloop: get_guc_bool(guc_names::ENABLE_NESTLOOP),
            enable_seqscan: get_guc_bool(guc_names::ENABLE_SEQSCAN),
            enable_indexscan: get_guc_bool(guc_names::ENABLE_INDEXSCAN),
            enable_bitmapscan: get_guc_bool(guc_names::ENABLE_BITMAPSCAN),
            random_page_cost: get_guc_real(guc_names::RANDOM_PAGE_COST),
        }
    }
}

/// Adjust PostgreSQL GUCs to favor the advised plan.
fn apply_advice_to_gucs(
    advice: &PlanAdviceSet,
    _improvement_factor: f64,
) -> Result<(), String> {
    unsafe {
        // Count method preferences in the advice
        let mut want_hash = 0;
        let mut want_merge = 0;
        let mut want_nestloop = 0;

        for jm in &advice.join_methods {
            match jm.method {
                JoinMethod::Hash => want_hash += 1,
                JoinMethod::Merge => want_merge += 1,
                JoinMethod::NestedLoop => want_nestloop += 1,
            }
        }

        // Adjust join method GUCs based on predominant method
        if want_hash > 0 || want_merge > 0 || want_nestloop > 0 {
            // Enable the method we want most, disable others
            let total = want_hash + want_merge + want_nestloop;
            set_guc_bool(guc_names::ENABLE_HASHJOIN, want_hash > total / 2);
            set_guc_bool(guc_names::ENABLE_MERGEJOIN, want_merge > total / 2);
            set_guc_bool(guc_names::ENABLE_NESTLOOP, want_nestloop > total / 2);
        }

        // Count scan method preferences
        let mut want_seqscan = 0;
        let mut want_indexscan = 0;
        let mut want_bitmapscan = 0;

        for sm in &advice.scan_methods {
            match &sm.method {
                ScanMethod::Sequential => want_seqscan += 1,
                ScanMethod::Index(_) => want_indexscan += 1,
                ScanMethod::BitmapHeap => want_bitmapscan += 1,
            }
        }

        // Adjust scan method GUCs
        if want_seqscan > 0 || want_indexscan > 0 || want_bitmapscan > 0 {
            let total = want_seqscan + want_indexscan + want_bitmapscan;

            // If we want index scans, use hardware-aware cost to favor them
            if want_indexscan > total / 2 {
                // Use detected storage type (SSD=1.0, HDD=4.0)
                let optimal_cost = hardware_aware::random_page_cost();
                set_guc_real(guc_names::RANDOM_PAGE_COST, optimal_cost);
                set_guc_bool(guc_names::ENABLE_INDEXSCAN, true);
                set_guc_bool(guc_names::ENABLE_SEQSCAN, false);
            }
            // If we want sequential scans, increase random_page_cost
            else if want_seqscan > total / 2 {
                set_guc_real(guc_names::RANDOM_PAGE_COST, guc_tuning::RANDOM_PAGE_COST_FORCE_SEQSCAN);
                set_guc_bool(guc_names::ENABLE_SEQSCAN, true);
                set_guc_bool(guc_names::ENABLE_INDEXSCAN, false);
            }

            set_guc_bool(guc_names::ENABLE_BITMAPSCAN, want_bitmapscan > 0);
        }
    }

    Ok(())
}

/// Get a boolean GUC value.
///
/// # Safety
///
/// Calls PostgreSQL C API.
unsafe fn get_guc_bool(name: &str) -> bool {
    use pgrx::pg_sys;
    use std::ffi::CString;

    let c_name = CString::new(name).unwrap();
    let value_str = pg_sys::GetConfigOption(
        c_name.as_ptr(),
        false, // missing_ok
        false, // restrict_privileged
    );

    if value_str.is_null() {
        return true; // Default to true if not found
    }

    let value_cstr = std::ffi::CStr::from_ptr(value_str);
    let value = value_cstr.to_string_lossy();
    value == "on" || value == "true" || value == "yes" || value == "1"
}

/// Set a boolean GUC value.
///
/// # Safety
///
/// Calls PostgreSQL C API.
unsafe fn set_guc_bool(name: &str, value: bool) {
    use pgrx::pg_sys;
    use std::ffi::CString;

    let c_name = CString::new(name).unwrap();
    let c_value = CString::new(if value { "on" } else { "off" }).unwrap();

    pg_sys::SetConfigOption(
        c_name.as_ptr(),
        c_value.as_ptr(),
        pg_sys::GucContext::PGC_USERSET,
        pg_sys::GucSource::PGC_S_SESSION,
    );
}

/// Get a real (float) GUC value.
///
/// # Safety
///
/// Calls PostgreSQL C API.
unsafe fn get_guc_real(name: &str) -> f64 {
    use crate::pg_constants::cost_defaults;
    use pgrx::pg_sys;
    use std::ffi::CString;

    let c_name = CString::new(name).unwrap();
    let value_str = pg_sys::GetConfigOption(
        c_name.as_ptr(),
        false, // missing_ok
        false, // restrict_privileged
    );

    if value_str.is_null() {
        return cost_defaults::RANDOM_PAGE_COST;
    }

    let value_cstr = std::ffi::CStr::from_ptr(value_str);
    let value = value_cstr.to_string_lossy();
    value.parse::<f64>().unwrap_or(cost_defaults::RANDOM_PAGE_COST)
}

/// Set a real (float) GUC value.
///
/// # Safety
///
/// Calls PostgreSQL C API.
unsafe fn set_guc_real(name: &str, value: f64) {
    use pgrx::pg_sys;
    use std::ffi::CString;

    let c_name = CString::new(name).unwrap();
    let c_value = CString::new(format!("{}", value)).unwrap();

    pg_sys::SetConfigOption(
        c_name.as_ptr(),
        c_value.as_ptr(),
        pg_sys::GucContext::PGC_USERSET,
        pg_sys::GucSource::PGC_S_SESSION,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::{Const, Expr};

    fn scan(name: &str) -> RelExpr {
        RelExpr::Scan {
            table: name.to_string(),
            alias: None,
        }
    }

    fn join(left: RelExpr, right: RelExpr) -> RelExpr {
        RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn single_scan_advice() {
        let advice = extract_plan_advice(&scan("orders"));
        assert_eq!(advice.join_order, vec!["orders"]);
        assert!(advice.join_methods.is_empty());
        assert_eq!(advice.scan_methods.len(), 1);
        assert_eq!(advice.scan_methods[0].relation, "orders");
    }

    #[test]
    fn two_way_join_advice() {
        let expr = join(scan("orders"), scan("customers"));
        let advice = extract_plan_advice(&expr);
        assert_eq!(
            advice.join_order,
            vec!["orders", "customers"]
        );
        assert_eq!(advice.join_methods.len(), 1);
        assert_eq!(
            advice.join_methods[0].inner_relation,
            "customers"
        );
        assert_eq!(
            advice.join_methods[0].method,
            JoinMethod::Hash
        );
    }

    #[test]
    fn three_way_join_advice() {
        let expr = join(
            join(scan("a"), scan("b")),
            scan("c"),
        );
        let advice = extract_plan_advice(&expr);
        assert_eq!(advice.join_order, vec!["a", "b", "c"]);
        assert_eq!(advice.join_methods.len(), 2);
    }

    #[test]
    fn advice_string_format() {
        let expr = join(scan("orders"), scan("customers"));
        let advice = extract_plan_advice(&expr);
        let s = advice.to_advice_string();
        assert!(s.contains("JOIN_ORDER(orders customers)"));
        assert!(s.contains("HASH_JOIN(customers)"));
        assert!(s.contains("SEQ_SCAN(orders)"));
        assert!(s.contains("SEQ_SCAN(customers)"));
    }

    #[test]
    fn pg_hint_plan_format() {
        let expr = join(scan("a"), scan("b"));
        let advice = extract_plan_advice(&expr);
        let hint = advice.to_pg_hint_plan();
        assert!(hint.starts_with("/*+"));
        assert!(hint.ends_with("*/"));
        assert!(hint.contains("Leading(a b)"));
        assert!(hint.contains("HashJoin(b)"));
    }

    #[test]
    fn count_relations_basic() {
        assert_eq!(count_relations(&scan("t")), 1);
        assert_eq!(
            count_relations(&join(scan("a"), scan("b"))),
            2
        );
    }

    #[test]
    fn count_relations_through_filter() {
        let expr = RelExpr::Filter {
            predicate: Expr::Const(Const::Bool(true)),
            input: Box::new(join(scan("a"), scan("b"))),
        };
        assert_eq!(count_relations(&expr), 2);
    }

    #[test]
    fn extract_table_names_deduplicates() {
        let expr = join(scan("t"), scan("t"));
        let names = extract_table_names(&expr);
        assert_eq!(names, vec!["t"]);
    }

    #[test]
    fn extract_table_names_sorted() {
        let expr = join(scan("b"), scan("a"));
        let names = extract_table_names(&expr);
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn cross_join_no_method() {
        let expr = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
        };
        let advice = extract_plan_advice(&expr);
        assert!(advice.join_methods.is_empty());
    }

    #[test]
    fn semi_join_uses_nested_loop() {
        let expr = RelExpr::Join {
            join_type: JoinType::Semi,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(scan("a")),
            right: Box::new(scan("b")),
        };
        let advice = extract_plan_advice(&expr);
        assert_eq!(advice.join_methods.len(), 1);
        assert_eq!(
            advice.join_methods[0].method,
            JoinMethod::NestedLoop
        );
    }

    #[test]
    fn alias_used_in_advice() {
        let expr = RelExpr::Scan {
            table: "orders".to_string(),
            alias: Some("o".to_string()),
        };
        let advice = extract_plan_advice(&expr);
        assert_eq!(advice.join_order, vec!["o"]);
        assert_eq!(advice.scan_methods[0].relation, "o");
    }

    #[test]
    fn empty_advice_string_for_single_scan() {
        let advice = extract_plan_advice(&scan("t"));
        let s = advice.to_advice_string();
        assert!(!s.contains("JOIN_ORDER"));
        assert!(s.contains("SEQ_SCAN(t)"));
    }

    #[test]
    fn empty_pg_hint_for_no_advice() {
        let set = PlanAdviceSet {
            join_order: vec![],
            join_methods: vec![],
            scan_methods: vec![],
        };
        assert!(set.to_pg_hint_plan().is_empty());
    }

    #[test]
    fn index_scan_produces_index_advice() {
        let expr = RelExpr::IndexScan {
            table: "orders".to_string(),
            column: "id".to_string(),
        };
        let advice = extract_plan_advice(&expr);
        assert_eq!(advice.scan_methods.len(), 1);
        assert_eq!(
            advice.scan_methods[0].method,
            ScanMethod::Index("auto".to_string())
        );
    }

    #[test]
    fn parallel_scan_counted_as_relation() {
        let expr = RelExpr::ParallelScan {
            table: "big_table".to_string(),
            workers: 4,
        };
        assert_eq!(count_relations(&expr), 1);
    }
}
