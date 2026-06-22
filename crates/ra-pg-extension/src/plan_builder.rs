//! Direct PostgreSQL `Plan` node construction from Ra `RelExpr` trees.
//!
//! This module provides [`PlanBuilder`], which translates Ra's optimized
//! relational expression tree directly into PostgreSQL internal `Plan` nodes
//! (allocating them in the current PostgreSQL memory context via palloc).
//!
//! # Architecture
//!
//! The planner hook calls [`PlanBuilder::build_planned_stmt`] to construct the
//! complete `PlannedStmt` directly, bypassing PostgreSQL's planner entirely.
//!
//! ## Plan translation map
//!
//! | Ra `RelExpr` variant       | PostgreSQL plan node           |
//! |----------------------------|-------------------------------|
//! | `Scan`                     | `SeqScan` (+ `Filter` → qual) |
//! | `Filter`                   | fold into child's `qual` list |
//! | `Project`                  | fold into child's targetlist  |
//! | `Join` (Inner/Outer)       | `HashJoin` (default)          |
//! | `Join` (Cross/Semi/Anti)   | `NestLoop`                    |
//! | `Aggregate`                | `Agg`                         |
//! | `Sort`                     | `Sort`                        |
//! | `Limit`                    | `Limit`                       |
//! | `IndexScan`                | `IndexScan`                   |
//! | `BitmapHeapScan`           | `BitmapHeapScan`              |
//! | `BitmapIndexScan`          | `BitmapIndexScan`             |
//! | `BitmapAnd` / `BitmapOr`   | `BitmapAnd` / `BitmapOr`     |
//! | `IndexOnlyScan`            | `IndexOnlyScan`               |
//! | `ParallelScan`             | `SeqScan` (parallel_aware)    |
//! | `ParallelHashJoin`         | `HashJoin` (parallel_aware)   |
//! | `ParallelAggregate`        | `Agg` (partial split)         |
//! | `Gather`                   | `Gather`                      |
//! | `Distinct`                 | `Unique`                      |
//! | `Union`                    | `Append` / `MergeAppend`      |
//! | `Intersect` / `Except`     | `SetOp`                       |
//! | `CTE` / `RecursiveCTE`     | `CteScan` / `WorkTableScan`   |
//! | `Window`                   | `WindowAgg`                   |
//! | `Unnest` / `TableFunction` | `FunctionScan`                |
//! | `Values`                   | `Result` (with Values node)   |
//! | `MvScan`                   | `SeqScan` (on MV relation)    |
//! | `IncrementalSort`          | `IncrementalSort`             |
//! | `TopK` / `VectorFilter`    | `CustomScan` (vector ops)     |
//! | `RowPattern`               | unsupported — falls back       |
//! | `MultiUnnest`              | unsupported — falls back       |
//!
//! # Safety
//!
//! All public methods are `unsafe`: they allocate PostgreSQL heap memory, access
//! relation catalogs, and dereference raw pointers. They must be called from
//! within a live PostgreSQL backend process with an active memory context.
//!
//! # Integration
//!
//! Call [`PlanBuilder::build_planned_stmt`] from the planner hook:
//!
//! ```ignore
//! let table_map = build_table_map(query);
//! let mut builder = PlanBuilder::new(query, table_map, &stats);
//! let planned = unsafe { builder.build_planned_stmt(&optimized_expr)? };
//! ```

use std::collections::HashMap;
use std::ffi::CString;

use pgrx::pg_sys;
use pgrx::prelude::*;
use tracing::debug;

use ra_core::algebra::{AggregateExpr, JoinType, OnConflict, ProjectionColumn, RelExpr, SortKey};
use ra_core::expr::{BinOp, Expr};
use ra_core::statistics::Statistics;

/// Maps a base relation's `(rtindex, attno)` to its 1-based output position in
/// a join node's targetlist, so nested-join Vars can be rewritten to
/// `OUTER_VAR`/`INNER_VAR` references (Ra bypasses the planner setrefs pass).
type JoinColMap = HashMap<(i32, i16), i32>;

use crate::expr_translator::{self, CteCol, CteScope, ExprContext};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by [`PlanBuilder`].
#[derive(Debug, thiserror::Error)]
pub enum PlanBuilderError {
    /// A table referenced in the Ra plan was not found in the PostgreSQL catalog.
    #[error("table '{0}' not found in PostgreSQL catalog")]
    TableNotFound(String),

    /// A `RelExpr` variant is not supported by the plan builder.
    /// The query will be rejected with an error to the client.
    #[error("unsupported RelExpr variant: {0}")]
    UnsupportedVariant(String),

    /// An internal invariant was violated (bug in the plan builder).
    #[error("plan builder internal error: {0}")]
    Internal(String),

    /// A null pointer was encountered where a valid pointer was expected.
    #[error("null pointer in plan builder: {0}")]
    NullPointer(String),
}

// ---------------------------------------------------------------------------
// Plan builder
// ---------------------------------------------------------------------------

/// Builds PostgreSQL `PlannedStmt` nodes directly from Ra `RelExpr` trees.
///
/// The builder maintains a range-table mapping so each scanned relation is
/// registered once and referenced by index in all scan nodes.
pub struct PlanBuilder {
    /// The original parsed query — provides the range table and target list.
    original_query: *mut pg_sys::Query,
    /// Expression translation context (OID maps).
    expr_ctx: ExprContext,
    /// Accumulated cost estimates propagated up from child nodes.
    total_cost: f64,
    /// Estimated output row count propagated from child nodes.
    plan_rows: f64,
    /// Gathered catalog statistics per table, for realistic cost estimation.
    stats: HashMap<String, Statistics>,
    /// Per-relation physical-strategy preferences derived from supplied
    /// plan advice. Empty when no advice was supplied (or the supplied
    /// advice contained no scan/join/parallel tags). Consulted in
    /// [`Self::build_seq_scan`], [`Self::build_join`], and the
    /// `Gather`-wrapping path so that `INDEX_SCAN(t i)` /
    /// `HASH_JOIN(b)` / `NO_GATHER(t)` advice actually steers the
    /// produced PG `Plan` tree.
    physical_choices: ra_engine::plan_advice_physical::PhysicalChoices,
    /// Inner plans for scalar sub-queries, in `PlannedStmt.subplans` order.
    subplans: Vec<*mut pg_sys::Plan>,
    /// SubPlan nodes for uncorrelated scalar sub-queries, hoisted to InitPlans
    /// (computed once) and attached to the top plan node's `initPlan` list.
    init_subplans: Vec<*mut pg_sys::SubPlan>,
    /// PARAM_EXEC parameter types (becomes `PlannedStmt.paramExecTypes`).
    param_types: Vec<pg_sys::Oid>,
    /// Active recursive-CTE wiring while building its recursive term / body,
    /// so a `Scan` of the CTE name builds a WorkTableScan or CteScan.
    cte_runtime: Option<CteRuntime>,
    /// Non-recursive CTEs that are referenced as a join side, keyed by
    /// lower-cased name. Such a CTE is not inlined; it is built as a
    /// SubqueryScan over its definition so the join can wire it like a scan.
    cte_join_defs: HashMap<String, CteJoinDef>,
    /// True while building any term of a recursive CTE, so the anchor's
    /// no-FROM `Scan(__dual)` becomes a one-row Result (standalone no-FROM
    /// selects keep falling back to native PG).
    in_recursive_cte: bool,
    /// While building a grouped aggregate whose input is a join, this holds the
    /// join's `(rtindex, attno) -> output position` map. When set, the
    /// aggregate's column resolution (`add_input_col`, `remap_agg_input_vars`)
    /// references the join's full passthrough output via `OUTER_VAR(position)`
    /// instead of appending to a fresh scan targetlist.
    active_join_map: Option<JoinColMap>,
    /// Catalog-resolved rtable (when built without PG Query dependency).
    catalog_rtable: Option<*mut pg_sys::List>,
    /// Catalog-resolved permission infos (when built without PG Query).
    catalog_perm_infos: Option<*mut pg_sys::List>,
}

/// How a `Scan` of the in-scope recursive CTE should be built.
enum CteScanMode {
    /// Recursive term self-reference → WorkTableScan.
    Recursive,
    /// Body reference → CteScan.
    Body,
}

/// A non-recursive CTE built as a join-side SubqueryScan: its definition (to
/// build as the subplan), the range-table index of its RTE_CTE (the scan's
/// `scanrelid`), and its output columns.
struct CteJoinDef {
    def: RelExpr,
    rtindex: pg_sys::Index,
    cols: Vec<CteCol>,
}

/// Wiring for the recursive CTE currently being built.
struct CteRuntime {
    /// CTE name (lower-cased).
    name: String,
    /// Range-table index of the RTE_CTE (shared by both scan kinds).
    rtindex: pg_sys::Index,
    /// 1-based index into `PlannedStmt.subplans` of the RecursiveUnion.
    cte_plan_id: i32,
    /// PARAM_EXEC param holding the CTE result tuplestore.
    cte_param: i32,
    /// PARAM_EXEC param holding the working-table tuplestore.
    wt_param: i32,
    /// Whether a Scan of the CTE builds a WorkTableScan or CteScan.
    mode: CteScanMode,
}

impl PlanBuilder {
    /// Create a new plan builder for the given query.
    ///
    /// `table_map` maps each base-table name (as it appears in `RelExpr::Scan`)
    /// to its 1-based index in the original query's range table and its relation
    /// OID. Build this with [`build_table_map`].
    ///
    /// `gathered_stats` provides catalog statistics gathered from the live
    /// database for realistic cost estimation.
    ///
    /// # Safety
    ///
    /// `query` must be a valid, non-null pointer to a PostgreSQL `Query` node
    /// allocated in the current memory context.
    pub unsafe fn new(
        query: *mut pg_sys::Query,
        table_map: HashMap<String, (pg_sys::Index, pg_sys::Oid)>,
        gathered_stats: &[(String, Statistics)],
    ) -> Self {
        let mut rtindex_map = HashMap::new();
        let mut rtoid_map = HashMap::new();
        for (name, (idx, oid)) in table_map {
            rtindex_map.insert(name.clone(), idx);
            rtoid_map.insert(name, oid);
        }
        let expr_ctx = ExprContext {
            rtindex_map,
            rtoid_map,
            subplans: std::cell::RefCell::new(HashMap::new()),
            cte_scope: std::cell::RefCell::new(None),
            subquery_scope: std::cell::RefCell::new(None),
            cte_join_scope: std::cell::RefCell::new(HashMap::new()),
            correlation_scope: std::cell::RefCell::new(HashMap::new()),
        };
        let stats = gathered_stats
            .iter()
            .map(|(name, s)| (name.to_lowercase(), s.clone()))
            .collect();
        Self {
            original_query: query,
            expr_ctx,
            total_cost: 0.0,
            plan_rows: 1.0,
            stats,
            physical_choices: ra_engine::plan_advice_physical::PhysicalChoices::new(),
            subplans: Vec::new(),
            init_subplans: Vec::new(),
            param_types: Vec::new(),
            cte_runtime: None,
            cte_join_defs: HashMap::new(),
            in_recursive_cte: false,
            active_join_map: None,
            catalog_rtable: None,
            catalog_perm_infos: None,
        }
    }

    /// Create a PlanBuilder from catalog-resolved tables (no PG Query dependency).
    ///
    /// # Safety
    ///
    /// `rtable` and `perm_infos` must be valid PG List pointers.
    pub unsafe fn new_from_catalog(
        table_map: HashMap<String, (pg_sys::Index, pg_sys::Oid)>,
        rtable: *mut pg_sys::List,
        perm_infos: *mut pg_sys::List,
        gathered_stats: &[(String, Statistics)],
    ) -> Self {
        let mut base = Self::new(std::ptr::null_mut(), table_map, gathered_stats);
        // Store the constructed rtable/perminfos for use in build_planned_stmt.
        base.catalog_rtable = Some(rtable);
        base.catalog_perm_infos = Some(perm_infos);
        base
    }

    /// Set the physical-strategy choices the builder should consult
    /// when picking scan / join / parallelism methods. Must be set
    /// before [`Self::build_planned_stmt`] is called.
    ///
    /// Pass an empty value (the default) to disable advice-driven
    /// selection — the builder falls back to its default choices
    /// (`SeqScan`, `HashJoin` for equi-joins, `NestLoop` otherwise).
    pub fn set_physical_choices(
        &mut self,
        choices: ra_engine::plan_advice_physical::PhysicalChoices,
    ) {
        self.physical_choices = choices;
    }

    /// Name of the first operator in `expr` that the plan_builder does
    /// not yet correctly translate, or `None` if the whole tree is
    /// supported. Makes every native-planner fallback name the exact
    /// gap so logs map directly to a coverage backlog task (see
    /// docs/planner-fallback-backlog.md).
    ///
    /// Stricter check for an Aggregate's input. A join under an aggregate is
    /// built over the join's passthrough output (`build_grouped_aggregate`'s
    /// join path), so it is admitted by recursing into the join's children;
    /// build itself defers for join shapes it cannot render.
    fn agg_input_unsupported(expr: &RelExpr) -> Option<&'static str> {
        match expr {
            RelExpr::Join { left, right, .. } => {
                Self::first_unsupported_op(left).or_else(|| Self::first_unsupported_op(right))
            }
            RelExpr::Filter { input, .. } | RelExpr::Project { input, .. } => {
                Self::agg_input_unsupported(input)
            }
            _ => Self::first_unsupported_op(expr),
        }
    }

    fn first_unsupported_op(expr: &RelExpr) -> Option<&'static str> {
        match expr {
            RelExpr::Scan { .. } => None,
            RelExpr::Filter { input, .. } => Self::first_unsupported_op(input),
            // Project over Aggregate is built as a single Agg node
            // (build_grouped_aggregate); check the aggregate's input. The
            // builder itself returns Err (→ fallback) for shapes it cannot
            // handle (HAVING's nested form, expressions over aggregates,
            // DISTINCT/unsupported aggregates).
            RelExpr::Project { input, .. } => {
                match &**input {
                RelExpr::Aggregate { input: agg_in, .. } => Self::agg_input_unsupported(agg_in),
                // Project over Join (or over a WHERE Filter over Join) is built
                // as one NestLoop; build_projected_join returns Err (→ fallback)
                // for unsupported join shapes.
                RelExpr::Join { left, right, .. } => Self::first_unsupported_op(left)
                    .or_else(|| Self::first_unsupported_op(right)),
                RelExpr::Filter { input: fi, .. } => match &**fi {
                    RelExpr::Join { left, right, .. } => Self::first_unsupported_op(left)
                        .or_else(|| Self::first_unsupported_op(right)),
                    // Project over Filter over Aggregate is HAVING — built as
                    // one Agg node with the HAVING as its qual; recurse into
                    // the aggregate's input rather than the bare Aggregate.
                    RelExpr::Aggregate { input: agg_in, .. } => {
                        Self::agg_input_unsupported(agg_in)
                    }
                    _ => Self::first_unsupported_op(input),
                },
                _ => Self::first_unsupported_op(input),
                }
            }
            // Projection-incapable passthrough nodes: they share the child
            // targetlist; ordering/limit semantics handled by the PG executor
            // node. Sort is admitted only when every key is a plain column —
            // expression keys still need ordering-operator resolution, so they
            // defer to PG.
            RelExpr::Sort { keys, input }
                if keys.iter().all(|k| {
                    matches!(k.expr, ra_core::expr::Expr::Column(_))
                        || Self::sort_output_position(&k.expr, input).is_some()
                }) =>
            {
                Self::first_unsupported_op(input)
            }
            RelExpr::Sort { .. } => Some("Sort"),
            RelExpr::IncrementalSort { .. } => Some("Sort"),
            RelExpr::Limit { input, .. } => Self::first_unsupported_op(input),
            // Window over a projection: build_window_plan handles it and Errs
            // (→ fallback) for multi-spec / non-default frame / unsupported fn.
            RelExpr::Window { input, .. } => match &**input {
                RelExpr::Project { input: pin, .. } => Self::first_unsupported_op(pin),
                _ => Some("Window"),
            },
            RelExpr::Join { left, right, .. } => {
                Self::first_unsupported_op(left).or_else(|| Self::first_unsupported_op(right))
            }
            RelExpr::Aggregate { .. } => Some("Aggregate"),
            // DISTINCT and UNION/INTERSECT/EXCEPT (incl. UNION distinct's
            // Sort+Unique over an Append) build correctly now that the Append's
            // part_prune_index is set to -1 (palloc0 left it 0, which crashed
            // ExecInitAppend on empty partition-prune info).
            RelExpr::Distinct { input } => Self::first_unsupported_op(input),
            RelExpr::DistinctOn { input, .. } => Self::first_unsupported_op(input),
            RelExpr::Union { left, right, .. }
            | RelExpr::Intersect { left, right, .. }
            | RelExpr::Except { left, right, .. } => {
                Self::first_unsupported_op(left).or_else(|| Self::first_unsupported_op(right))
            }
            // VALUES: built as an Append of one-row Result nodes;
            // build_values_result Errs (→ fallback) for non-constant rows.
            RelExpr::Values { .. } => None,
            // Non-recursive CTE is inlined (Scan(cte) → definition) and its
            // base relations flattened into the rtable; build_plan Errs (→
            // fallback) if a reference can't be resolved (e.g. CTE-qualified
            // columns, multi-relation CTE bodies).
            RelExpr::CTE { definition, body, .. } => {
                Self::first_unsupported_op(definition).or_else(|| Self::first_unsupported_op(body))
            }
            // Recursive CTE → CteScan over RecursiveUnion{anchor,
            // WorkTableScan}. Recurse into all three terms; build_plan Errs
            // (→ fallback) for shapes the bespoke wiring can't resolve.
            RelExpr::RecursiveCTE {
                base_case,
                recursive_case,
                body,
                ..
            } => Self::first_unsupported_op(base_case)
                .or_else(|| Self::first_unsupported_op(recursive_case))
                .or_else(|| Self::first_unsupported_op(body)),
            RelExpr::Unnest { .. } | RelExpr::MultiUnnest { .. } => Some("Unnest"),
            RelExpr::TableFunction { .. } => Some("TableFunction"),
            // IndexOnlyScan is built faithfully (build_index_only_scan) and
            // falls back internally when the index does not cover the query.
            RelExpr::IndexOnlyScan { .. } => None,
            RelExpr::IndexScan { .. } => Some("IndexScan"),
            RelExpr::BitmapHeapScan { .. }
            | RelExpr::BitmapIndexScan { .. }
            | RelExpr::BitmapAnd { .. }
            | RelExpr::BitmapOr { .. } => Some("BitmapScan"),
            RelExpr::MvScan { .. } => Some("MvScan"),
            RelExpr::ParallelScan { .. }
            | RelExpr::ParallelHashJoin { .. }
            | RelExpr::ParallelAggregate { .. }
            | RelExpr::Gather { .. } => Some("Parallel"),
            RelExpr::TopK { .. } | RelExpr::VectorFilter { .. } => Some("VectorSearch"),
            RelExpr::RowPattern { .. } => Some("RowPattern"),
            RelExpr::GraphTable { .. } => Some("GraphTable"),
            RelExpr::Insert { .. } => Some("Insert"),
            RelExpr::Update { .. } => Some("Update"),
            RelExpr::Delete { .. } => Some("Delete"),
            RelExpr::Merge { .. } => Some("Merge"),
        }
    }

    /// Conservative correctness gate: defer to PG for RelExpr shapes the
    /// builder/optimizer still mishandles. Currently a scalar subquery in a
    /// filter predicate (SubPlan result param not wired). Outer joins are no
    /// longer gated: the optimizer wrong-result bugs (unsound filter pushdown /
    /// join commute on `(join ?type …)` and the unguarded
    /// `duckdb-filter-through-left-join-left`) are fixed in ra-engine
    /// (references_only soundness via qualifier tracking + `is_inner_join`
    /// guards + left-side `references_only`), verified row-identical to PG.
    fn wrong_result_risk(expr: &RelExpr) -> Option<&'static str> {
        for child in expr.children() {
            if let Some(r) = Self::wrong_result_risk(child) {
                return Some(r);
            }
        }
        None
    }

    /// Build a complete `PlannedStmt` from an optimized `RelExpr` tree.
    ///
    /// Returns `Err` if the plan shape is not yet supported by the
    /// plan_builder; the planner hook then falls back to the native
    /// planner. The error names the first unsupported operator.
    ///
    /// # Safety
    ///
    /// Must be called from within a live PostgreSQL backend process.
    /// Allocates Plan nodes in the current PostgreSQL memory context.
    pub unsafe fn build_planned_stmt(
        &mut self,
        expr: &RelExpr,
    ) -> Result<*mut pg_sys::PlannedStmt, PlanBuilderError> {
        // Correctness gate (1.0 reliability invariant): only emit a Ra
        // plan for relational shapes the plan_builder is verified to
        // translate correctly. Everything else returns an error so the
        // planner hook falls back to PostgreSQL's native planner —
        // producing correct results rather than a wrong or unsafe plan.
        // The currently-verified set is single-relation Scan / Filter /
        // Project. Aggregate, Join, Sort, Limit, Distinct, set-ops, etc.
        // are NOT yet correctly emitted (they variously drop rows,
        // mistranslate aggregates, or corrupt executor memory) and must
        // stay on the native planner until each is implemented and
        // gated in via the replan-equivalence property test.
        if let Some(op) = Self::first_unsupported_op(expr) {
            return Err(PlanBuilderError::UnsupportedVariant(format!(
                "{op} not yet supported by Ra plan_builder; deferring to native \
                 planner (see docs/planner-fallback-backlog.md)"
            )));
        }

        // Correctness gate (RFC: correctness > coverage). RelExpr shapes the
        // builder currently MISBUILDS — returning wrong results without erroring
        // — must defer to PG rather than violate the prime invariant. Each is a
        // tracked bug to fix properly (then un-gate).
        if let Some(reason) = Self::wrong_result_risk(expr) {
            return Err(PlanBuilderError::UnsupportedVariant(format!(
                "{reason}; deferring to native planner for correctness \
                 (see docs/planner-fallback-backlog.md)"
            )));
        }

        // Pre-build SubPlan nodes for scalar sub-queries so expression
        // translation can resolve them; Errs here defer to native PG.
        self.prepare_subplans(expr)?;
        let plan_tree = self.build_plan(expr)?;
        // Attach hoisted InitPlans (uncorrelated scalar sub-queries) to the top
        // plan node so they are computed once before the rest of the plan runs.
        if !self.init_subplans.is_empty() && !plan_tree.is_null() {
            for sp in &self.init_subplans {
                (*plan_tree).initPlan = pg_sys::lappend((*plan_tree).initPlan, (*sp).cast());
            }
        }

        let stmt = self.alloc_node::<pg_sys::PlannedStmt>();
        if stmt.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "PlannedStmt allocation".to_string(),
            ));
        }

        // Detect command type from the expression.
        let cmd_type = match expr {
            RelExpr::Insert { .. } => pg_sys::CmdType::CMD_INSERT,
            RelExpr::Update { .. } => pg_sys::CmdType::CMD_UPDATE,
            RelExpr::Delete { .. } => pg_sys::CmdType::CMD_DELETE,
            _ => pg_sys::CmdType::CMD_SELECT,
        };

        (*stmt).type_ = pg_sys::NodeTag::T_PlannedStmt;
        (*stmt).commandType = cmd_type;
        (*stmt).planTree = plan_tree;

        // Copy range table and result relations.
        // Prefer catalog-resolved rtable (no PG Query dependency).
        if let Some(rtable) = self.catalog_rtable {
            (*stmt).rtable = rtable;
            #[cfg(not(any(feature = "pg13", feature = "pg14", feature = "pg15")))]
            if let Some(pi) = self.catalog_perm_infos {
                (*stmt).permInfos = pi;
            }
        } else if !self.original_query.is_null() {
            // Flatten any non-recursive CTE relations into a fresh rtable copy
            // (never mutate PG's parse tree — fallback would then see it). The
            // appended order matches build_table_map's index assignment.
            let cte_rtes = flatten_rtes(self.original_query);
            if cte_rtes.is_empty() {
                (*stmt).rtable = (*self.original_query).rtable;
                #[cfg(not(any(feature = "pg13", feature = "pg14", feature = "pg15")))]
                {
                    (*stmt).permInfos = (*self.original_query).rteperminfos;
                }
            } else {
                let mut rtable = pg_sys::list_copy((*self.original_query).rtable);
                #[cfg(not(any(feature = "pg13", feature = "pg14", feature = "pg15")))]
                let mut perminfos = if (*self.original_query).rteperminfos.is_null() {
                    std::ptr::null_mut()
                } else {
                    pg_sys::list_copy((*self.original_query).rteperminfos)
                };
                for fr in cte_rtes {
                    let rte_copy =
                        pg_sys::copyObjectImpl(fr.rte.cast()) as *mut pg_sys::RangeTblEntry;
                    #[cfg(not(any(feature = "pg13", feature = "pg14", feature = "pg15")))]
                    {
                        if fr.perminfo.is_null() {
                            (*rte_copy).perminfoindex = 0;
                        } else {
                            let pcopy = pg_sys::copyObjectImpl(fr.perminfo.cast());
                            perminfos = pg_sys::lappend(perminfos, pcopy);
                            (*rte_copy).perminfoindex = pg_sys::list_length(perminfos) as u32;
                        }
                    }
                    rtable = pg_sys::lappend(rtable, rte_copy.cast());
                }
                (*stmt).rtable = rtable;
                #[cfg(not(any(feature = "pg13", feature = "pg14", feature = "pg15")))]
                {
                    (*stmt).permInfos = perminfos;
                }
            }
            // PG18+ run-time pruning tracks which relids the executor may
            // open via `unprunableRelids`; relations not listed there are
            // assumed pruned and raise "trying to open a pruned relation".
            // Ra never emits run-time-pruning plans, so every range-table
            // entry is unprunable (always opened).
            #[cfg(any(feature = "pg18", feature = "pg19"))]
            {
                let n = pg_sys::list_length((*stmt).rtable);
                if n > 0 {
                    (*stmt).unprunableRelids =
                        pg_sys::bms_add_range(std::ptr::null_mut(), 1, n);
                }
            }
            (*stmt).resultRelations = std::ptr::null_mut();
        }

        // Scalar sub-query plans and their PARAM_EXEC parameter types.
        if !self.subplans.is_empty() {
            let mut sp_list: *mut pg_sys::List = std::ptr::null_mut();
            for p in &self.subplans {
                sp_list = pg_sys::lappend(sp_list, (*p).cast());
            }
            (*stmt).subplans = sp_list;
        }
        if !self.param_types.is_empty() {
            let mut pt_list: *mut pg_sys::List = std::ptr::null_mut();
            for ty in &self.param_types {
                pt_list = pg_sys::lappend_oid(pt_list, *ty);
            }
            (*stmt).paramExecTypes = pt_list;
        }

        // Propagate top-level plan cost estimate
        if !plan_tree.is_null() {
            (*stmt).planTree = plan_tree;
        }

        Ok(stmt)
    }

    // -----------------------------------------------------------------------
    // Core dispatch
    // -----------------------------------------------------------------------

    /// Recursively build a `Plan` node for any `RelExpr` variant.
    ///
    /// `Filter` and `Project` are folded into their child's `qual` /
    /// `targetlist` fields rather than generating separate plan nodes,
    /// matching PostgreSQL's standard plan representation.
    unsafe fn build_plan(&mut self, expr: &RelExpr) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        match expr {
            RelExpr::Scan { table, alias } => {
                if let Some(rt) = &self.cte_runtime {
                    if table.to_lowercase() == rt.name {
                        return self.build_cte_scan();
                    }
                }
                if self.cte_join_defs.contains_key(&table.to_lowercase()) {
                    return self.build_cte_join_subqueryscan(&table.to_lowercase());
                }
                if table.eq_ignore_ascii_case("__dual") {
                    // No-FROM single-row source. As a recursive-CTE anchor
                    // (`SELECT 1`) it is a one-row Result; standalone no-FROM
                    // selects have no real relation to scan, so defer cleanly
                    // to native PG rather than scanning a zero-OID relation.
                    return if self.in_recursive_cte {
                        self.build_result_node()
                    } else {
                        Err(PlanBuilderError::UnsupportedVariant(
                            "no-FROM SELECT; deferring to native planner".to_owned(),
                        ))
                    };
                }
                self.build_scan_with_advice(table, alias.as_deref())
            }
            RelExpr::Filter { predicate, input } => {
                // Filter directly over a Join: build the join with the
                // predicate as its post-join qual (remapped to the join's
                // OUTER/INNER frame). The generic fold below would attach a
                // base-relation-Var qual to the join node, which the join
                // executor mis-evaluates (segfault).
                if matches!(&**input, RelExpr::Join { .. }) {
                    if let RelExpr::Join {
                        join_type,
                        condition,
                        left,
                        right,
                    } = &**input
                    {
                        let (plan, _, _) = self.build_join_node(
                            *join_type,
                            condition,
                            left,
                            right,
                            Some(predicate),
                            None,
                        )?;
                        return Ok(plan);
                    }
                }
                // Fast path: Filter directly over a base Scan with a
                // single-column btree equality conjunct becomes a real
                // IndexScan (the equality moves into indexqual; any residual
                // conjuncts stay as the heap recheck qual). Strictly
                // conservative — falls through to the SeqScan path otherwise.
                if let RelExpr::Scan { table, alias } = &**input {
                    // RFC 0091 B2b: honor the cost extractor's seq-vs-index
                    // decision. It records ScanStrategy::Seq for an
                    // index-eligible scan only when it judged a sequential scan
                    // cheaper under the current live conditions (e.g. a cold /
                    // contended host) — suppress the index-scan peephole then.
                    // No recorded choice (fast-pathed / not eligible) keeps the
                    // default peephole behaviour.
                    let scan_key = alias.as_deref().unwrap_or(table);
                    let force_seq = matches!(
                        self.physical_choices.scan_for(scan_key),
                        Some(ra_engine::plan_advice_physical::ScanStrategy::Seq)
                    );
                    if !force_seq {
                        // Multi-index AND: when >=2 top-level conjuncts are
                        // each served by a DISTINCT index, a BitmapAnd of those
                        // indexes beats a single index scan that pushes one
                        // index and rechecks the rest. Gated strictly (>=2
                        // distinct indexes) so single-index ANDs still take the
                        // cheaper plain index scan below. build_bitmap_heap_for_filter
                        // Errs safely (→ fall through) if it can't build.
                        if let Ok(rel_oid) = self.rel_oid_for(table) {
                            if self.and_distinct_index_count(predicate, rel_oid) >= 2 {
                                if let Ok(plan) = self.build_bitmap_heap_for_filter(table, predicate)
                                {
                                    return Ok(plan);
                                }
                            }
                        }
                        if let Some(plan) = self.try_build_index_scan(table, predicate)? {
                            return Ok(plan);
                        }
                        // Bitmap access path: a top-level OR (or multi-index
                        // AND) of indexed-column conditions can't be served by
                        // a single index scan. Build a real BitmapIndexScan(s)
                        // → BitmapHeapScan. build_bitmap_heap_for_filter Errs if
                        // any disjunct lacks a usable index, so we fall through
                        // to the SeqScan path then (never wrong, never worse).
                        if matches!(
                            predicate,
                            Expr::BinOp { op: ra_core::expr::BinOp::Or, .. }
                        ) {
                            if let Ok(plan) = self.build_bitmap_heap_for_filter(table, predicate) {
                                return Ok(plan);
                            }
                        }
                    }
                }
                // 1.0 safety: fold the predicate into the child scan's
                // qual as a plain SeqScan filter. The previous TID_SCAN
                // / BITMAP_HEAP_SCAN advice peephole here produced
                // backend-crashing plans, so scan-strategy advice is not
                // physically honored (see build_scan_with_advice).
                let child = self.build_plan(input)?;
                let pg_expr = expr_translator::translate(predicate, &self.expr_ctx);
                // Fail-safe: an untranslatable predicate must NOT be silently
                // dropped (that returns unfiltered rows — wrong results).
                // Defer the whole query to the native planner instead.
                if pg_expr.is_null() {
                    return Err(PlanBuilderError::UnsupportedVariant(
                        "filter predicate not translatable; deferring to native \
                         planner (see docs/planner-fallback-backlog.md)"
                            .to_owned(),
                    ));
                }
                if !child.is_null() {
                    (*child).qual = pg_sys::lappend((*child).qual, pg_expr.cast());
                }
                Ok(child)
            }
            RelExpr::Project { columns, input } => {
                // Expand `SELECT *` / `t.*` into one projection column per
                // relation attribute (from the single base relation's schema)
                // before any projection path runs, so the star sentinel never
                // reaches expression translation.
                let expanded_star = self.expand_star_columns(columns, input);
                let columns: &[ProjectionColumn] =
                    expanded_star.as_deref().unwrap_or(columns);
                // Project over Aggregate is built as one Agg node whose
                // targetlist carries the group Vars and Aggref nodes.
                if let RelExpr::Aggregate {
                    group_by,
                    input: agg_input,
                    ..
                } = &**input
                {
                    return self.build_grouped_aggregate(columns, group_by, agg_input, None);
                }
                // Project over Filter over Aggregate is HAVING: the Filter
                // predicate becomes the Agg node's qual (evaluated after
                // aggregation), referencing the same Aggrefs/group Vars.
                if let RelExpr::Filter { predicate, input: fi } = &**input {
                    if let RelExpr::Aggregate {
                        group_by,
                        input: agg_input,
                        ..
                    } = &**fi
                    {
                        return self.build_grouped_aggregate(
                            columns,
                            group_by,
                            agg_input,
                            Some(predicate),
                        );
                    }
                    // Project(Filter(Project(Aggregate))) — an aggregating
                    // derived table whose computed output is filtered and
                    // projected by the outer query (a SubqueryScan in PG).
                    // Build the inner aggregate, then a Result that applies the
                    // outer filter + projection against its output columns.
                    if matches!(&**fi, RelExpr::Project { input: pp, .. }
                        if matches!(&**pp, RelExpr::Aggregate { .. }))
                    {
                        return self.build_subquery_scan(fi, Some(predicate), columns);
                    }
                }
                // Project(Project(Aggregate)) — an aggregating derived table
                // selected (no outer filter): SubqueryScan over the aggregate.
                if matches!(&**input, RelExpr::Project { input: pp, .. }
                    if matches!(&**pp, RelExpr::Aggregate { .. }))
                {
                    return self.build_subquery_scan(input, None, columns);
                }
                // Project over Join (optionally with a WHERE Filter between)
                // is built as one NestLoop with remapped OUTER/INNER refs.
                if let RelExpr::Join {
                    join_type,
                    condition,
                    left,
                    right,
                } = &**input
                {
                    return self.build_projected_join(
                        columns, None, *join_type, condition, left, right,
                    );
                }
                if let RelExpr::Filter {
                    predicate,
                    input: fi,
                } = &**input
                {
                    if let RelExpr::Join {
                        join_type,
                        condition,
                        left,
                        right,
                    } = &**fi
                    {
                        return self.build_projected_join(
                            columns,
                            Some(predicate),
                            *join_type,
                            condition,
                            left,
                            right,
                        );
                    }
                }
                // Physical access-path peephole: a single-table
                // Project(Filter(Scan)) whose covering btree index includes
                // every projected and predicate column becomes a real
                // index-only scan (no heap fetch). Strictly conservative —
                // try_build_index_only_scan returns None unless it can prove
                // coverage and build a canonical INDEX_VAR indexqual, so any
                // other shape falls through to the generic build below.
                if let RelExpr::Filter { predicate, input: fi } = &**input {
                    if let RelExpr::Scan { table, alias } = &**fi {
                        // The in-scope recursive-CTE worktable is not a catalog
                        // relation; skip the catalog-dependent index peephole
                        // (Scan(cte) builds as a WorkTableScan/CteScan below).
                        let is_cte_worktable = self
                            .cte_runtime
                            .as_ref()
                            .is_some_and(|rt| table.eq_ignore_ascii_case(&rt.name));
                        let scan_key = alias.as_deref().unwrap_or(table);
                        let force_seq = matches!(
                            self.physical_choices.scan_for(scan_key),
                            Some(ra_engine::plan_advice_physical::ScanStrategy::Seq)
                        );
                        if !force_seq && !is_cte_worktable {
                            if let Some(plan) =
                                self.try_build_index_only_scan(table, "auto", columns, predicate)?
                            {
                                return Ok(plan);
                            }
                        }
                    }
                }
                let child = self.build_plan(input)?;
                self.set_targetlist(child, columns)?;
                Ok(child)
            }
            RelExpr::Join { .. } => self.build_join_tree(expr).map(|(p, _, _)| p),
            RelExpr::Aggregate {
                group_by,
                aggregates,
                input,
            } => self.build_aggregate(group_by, aggregates, input),
            RelExpr::Sort { keys, input } => self.build_sort(keys, input),
            RelExpr::Limit {
                count,
                offset,
                input,
            } => self.build_limit(*count, *offset, input),
            RelExpr::IndexScan { table, column } => self.build_index_scan(table, column),
            RelExpr::BitmapIndexScan {
                table,
                index,
                predicate: _,
            } => self.build_bitmap_index_scan(table, index),
            RelExpr::BitmapHeapScan { table, bitmap, .. } => {
                self.build_bitmap_heap_scan(table, bitmap)
            }
            RelExpr::BitmapAnd { inputs } => self.build_bitmap_and(inputs),
            RelExpr::BitmapOr { inputs } => self.build_bitmap_or(inputs),
            RelExpr::IndexOnlyScan { table, index, columns, predicate } => {
                self.build_index_only_scan(table, index, columns, predicate)
            }
            RelExpr::ParallelScan { table, workers } => {
                self.build_parallel_seq_scan(table, *workers)
            }
            RelExpr::ParallelHashJoin {
                join_type,
                condition,
                left,
                right,
                workers,
            } => self.build_parallel_hash_join(*join_type, condition, left, right, *workers),
            RelExpr::ParallelAggregate {
                group_by,
                aggregates,
                input,
                workers,
            } => self.build_parallel_aggregate(group_by, aggregates, input, *workers),
            RelExpr::Gather { input, workers } => {
                // Honor NO_GATHER advice on the input alias (if any):
                // skip the Gather wrapper and return the inner plan
                // directly, mirroring PG's behavior when
                // pg_plan_advice's `NO_GATHER(t)` is in effect.
                if let Some(alias) = leaf_alias(input) {
                    use ra_engine::plan_advice_physical::ParallelStrategy;
                    if matches!(
                        self.physical_choices.parallel_for(&alias),
                        Some(ParallelStrategy::NoGather)
                    ) {
                        debug!(
                            alias = %alias,
                            "NO_GATHER advice honored: skipping Gather wrapper",
                        );
                        return self.build_plan(input);
                    }
                }
                self.build_gather(input, *workers)
            }
            RelExpr::Distinct { input } => self.build_unique(input),
            RelExpr::DistinctOn { on, input } => self.build_distinct_on(on, input),
            RelExpr::Union { all, left, right } => self.build_set_op_union(*all, left, right),
            RelExpr::Intersect { all, left, right } => {
                self.build_set_op_intersect(*all, left, right)
            }
            RelExpr::Except { all, left, right } => self.build_set_op_except(*all, left, right),
            RelExpr::Window { functions, input } => match &**input {
                RelExpr::Project { columns, input: proj_in } => {
                    self.build_window_plan(functions, columns, proj_in)
                }
                _ => self.build_window_agg(functions, input),
            },
            RelExpr::IncrementalSort {
                prefix_keys,
                suffix_keys,
                input,
            } => self.build_incremental_sort(prefix_keys, suffix_keys, input),
            RelExpr::CTE { name, definition, body } => {
                // A CTE referenced as a join side cannot be inlined: inlining
                // replaces `Scan(name)` with the definition, losing the
                // name → RTE_CTE link the join needs. Register it so the join
                // builds it as a SubqueryScan; otherwise inline (PG's default).
                if Self::cte_used_in_join(body, name) {
                    let lname = name.to_lowercase();
                    let (rtindex, cols) = self.find_cte_rte(&lname)?;
                    let scope_cols: Vec<CteCol> = cols
                        .iter()
                        .map(|c| CteCol {
                            name: c.name.clone(),
                            typ: c.typ,
                            typmod: c.typmod,
                            coll: c.coll,
                        })
                        .collect();
                    self.expr_ctx.cte_join_scope.borrow_mut().insert(
                        lname.clone(),
                        CteScope { name: lname.clone(), rtindex, cols: scope_cols },
                    );
                    self.cte_join_defs.insert(
                        lname,
                        CteJoinDef { def: (**definition).clone(), rtindex, cols },
                    );
                    self.build_plan(body)
                } else {
                    let inlined = inline_cte_scan(body, name, definition);
                    self.build_plan(&inlined)
                }
            }
            RelExpr::RecursiveCTE {
                name,
                base_case,
                recursive_case,
                body,
                ..
            } => self.build_recursive_cte(name, base_case, recursive_case, body),
            RelExpr::MvScan { view_name, .. } => self.build_seq_scan(view_name, None),
            RelExpr::Unnest { .. } | RelExpr::TableFunction { .. } => {
                self.build_function_scan(expr)
            }
            RelExpr::Values { rows } => self.build_values_result(rows),
            // Advanced variants not yet directly supported — fall back.
            RelExpr::MultiUnnest { .. } => Err(PlanBuilderError::UnsupportedVariant(
                "MultiUnnest".to_string(),
            )),
            RelExpr::RowPattern { .. } => Err(PlanBuilderError::UnsupportedVariant(
                "RowPattern (MATCH_RECOGNIZE)".to_string(),
            )),
            RelExpr::TopK { .. } => Err(PlanBuilderError::UnsupportedVariant(
                "TopK (vector)".to_string(),
            )),
            RelExpr::VectorFilter { .. } => Err(PlanBuilderError::UnsupportedVariant(
                "VectorFilter (vector)".to_string(),
            )),
            // DML variants — produce ModifyTable plan nodes.
            RelExpr::Insert { .. } | RelExpr::Update { .. } | RelExpr::Delete { .. } => {
                self.build_modify_table_from_dml(expr)
            }
            RelExpr::Merge { .. } => Err(PlanBuilderError::UnsupportedVariant(
                // Ra parses and models MERGE, but lowering it requires a
                // CMD_MERGE ModifyTable with mergeActionLists. PostgreSQL
                // 15+ plans MERGE natively, so we defer to the native
                // planner (the hook falls back on UnsupportedVariant)
                // rather than emit a partial plan.
                "MERGE (handled by native PostgreSQL planner)".to_owned(),
            )),
            RelExpr::GraphTable { .. } => Err(PlanBuilderError::UnsupportedVariant(
                // GRAPH_TABLE (SQL/PGQ) is parsed and modeled by Ra but
                // executed by PostgreSQL 19's native property-graph
                // machinery; defer to the native planner.
                "GRAPH_TABLE (handled by native PostgreSQL planner)".to_owned(),
            )),
        }
    }

    // -----------------------------------------------------------------------
    // Scan builders
    // -----------------------------------------------------------------------

    /// Dispatch a `RelExpr::Scan` to the right scan-method based on
    /// the supplied [`PhysicalChoices`][pc] map.
    ///
    /// Default behavior (no advice or alias not in the map) is
    /// `SeqScan`. Advice tags drive the dispatch:
    ///
    /// | Advice for `alias` | Built node | Notes |
    /// |---|---|---|
    /// | `SEQ_SCAN(alias)` | `SeqScan` | Same as default |
    /// | `INDEX_SCAN(alias name)` | `IndexScan` | Index resolved by name via [`crate::index_resolver::resolve_index_by_name`] |
    /// | `INDEX_ONLY_SCAN(alias name)` | `IndexOnlyScan` | Index name passed through |
    /// | `BITMAP_HEAP_SCAN(alias)` | `SeqScan` (fallback) | Bitmap heap scans need a bitmap subplan; supplying just the alias isn't enough to construct one. We log and fall back to seq-scan rather than synthesizing a bogus bitmap. |
    /// | `TID_SCAN(alias)` | `SeqScan` (fallback) | TID scans require a `ctid` filter we don't have here. |
    /// | `DO_NOT_SCAN(alias)` | `SeqScan` (fallback) | PG uses this for `AlternativeSubPlan` disambiguation Ra doesn't model. |
    ///
    /// [pc]: ra_engine::plan_advice_physical::PhysicalChoices
    ///
    /// When the advice asks for a scan method we can't currently
    /// produce, we fall back to `SeqScan` so the query still runs.
    /// The optimizer's [`Cost::DISABLE_PENALTY`] already applies in
    /// `validate_advice` for these cases, so EXPLAIN output flags
    /// the inapplicable advice.
    unsafe fn build_scan_with_advice(
        &mut self,
        table: &str,
        alias: Option<&str>,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        // Resolve the alias used in advice. Advice identifiers
        // come from the SQL alias (or table name when no alias
        // was provided), so we look up by alias first and fall
        // back to the table name.
        let lookup_alias = alias.unwrap_or(table);
        // Clone the strategy to release the borrow on `self.physical_choices`
        // before we recurse into other `&mut self` methods.
        let strategy = self.physical_choices.scan_for(lookup_alias).cloned();

        use ra_engine::plan_advice_physical::ScanStrategy;
        // A bare `Scan` reaching here carries no predicate, so there is no
        // index condition to build — every strategy maps to a SeqScan. The
        // physical access path (real IndexScan / covering IndexOnlyScan /
        // OR-bitmap BitmapHeapScan) is instead chosen by the Filter(Scan) and
        // Project(Filter(Scan)) peepholes in `build_plan`, which DO have the
        // predicate + projection and are verified row-identical to PG. The old
        // advice-driven scan-strategy peephole here produced backend-crashing
        // plans and is intentionally not revived.
        match strategy {
            None
            | Some(ScanStrategy::Seq)
            | Some(ScanStrategy::Index { .. })
            | Some(ScanStrategy::IndexOnly { .. })
            | Some(ScanStrategy::BitmapHeap)
            | Some(ScanStrategy::Tid)
            | Some(ScanStrategy::DoNotScan) => self.build_seq_scan(table, alias),
        }
    }

    /// Build an `IndexScan` node where the index is identified by
    /// name (rather than by column, which is what
    /// [`Self::build_index_scan`] takes). Used by the advice
    /// dispatch path; if the named index doesn't exist on the
    /// relation we fall back to `SeqScan` and log.
    unsafe fn build_index_scan_by_index_name(
        &mut self,
        table: &str,
        index_name: &str,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let rel_oid = self.rel_oid_for(table)?;
        let info = crate::index_resolver::resolve_index_by_name(rel_oid, index_name);
        match info {
            Some(idx_info) => {
                let rtindex = self.rtindex_for(table)?;
                let node = self.alloc_node::<pg_sys::IndexScan>();
                if node.is_null() {
                    return Err(PlanBuilderError::NullPointer(
                        "IndexScan allocation".to_string(),
                    ));
                }
                (*node).scan.plan.type_ = pg_sys::NodeTag::T_IndexScan;
                (*node).scan.scanrelid = rtindex;
                (*node).indexid = idx_info.oid;
                debug!(
                    table = table,
                    index = index_name,
                    index_oid = idx_info.oid.to_u32(),
                    "IndexScan: honored advice-supplied index name",
                );
                self.set_index_costs(&mut (*node).scan.plan, table, 0.1);
                Ok(&mut (*node).scan.plan as *mut pg_sys::Plan)
            }
            None => {
                debug!(
                    table = table,
                    index = index_name,
                    "INDEX_SCAN advice references unknown index; \
                     falling back to SeqScan",
                );
                self.build_seq_scan(table, None)
            }
        }
    }

    /// Build a `SeqScan` plan node for the given relation name.
    unsafe fn build_seq_scan(
        &mut self,
        table: &str,
        alias: Option<&str>,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        // Resolve the range-table index by the scan's alias when it has one:
        // build_table_map keys distinct aliases to distinct rtindexes, but the
        // base-table name maps to only one (last-wins), so a self-join
        // (orders a JOIN orders b) must use the alias to get per-instance
        // rtindexes that match the alias-resolved Vars in conditions. Fall back
        // to the table name when the alias isn't a known range-table entry.
        let rtindex = match alias {
            Some(a) => self.rtindex_for(a).or_else(|_| self.rtindex_for(table))?,
            None => self.rtindex_for(table)?,
        };
        let node = self.alloc_node::<pg_sys::SeqScan>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "SeqScan allocation".to_string(),
            ));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_SeqScan;
        (*node).scan.scanrelid = rtindex;
        self.set_costs_from_stats(&mut (*node).scan.plan, table);
        Ok(&mut (*node).scan.plan as *mut pg_sys::Plan)
    }

    /// Honor `TID_SCAN` advice by emitting a `TidScan` plan
    /// node when the supplied filter predicate has a
    /// `ctid = ...` clause. Returns `Err(reason)` when the
    /// predicate doesn't reference `ctid` so the caller can
    /// fall back to the standard Filter+SeqScan path.
    unsafe fn build_tid_scan(
        &mut self,
        table: &str,
        predicate: &Expr,
    ) -> Result<*mut pg_sys::Plan, String> {
        // Walk the predicate to find ctid-equality clauses.
        // PG's TidScan accepts:
        //   ctid = ARRAY['(blk,off)', ...]
        //   ctid = '(blk,off)'
        //   ctid IN ('(b1,o1)', '(b2,o2)', ...)
        // We honor the most common shape (one or more
        // `ctid = const`) by translating the entire predicate
        // and verifying it references `ctid`.
        if !predicate_references_ctid(predicate) {
            return Err("predicate does not reference ctid".to_string());
        }
        let rtindex = self
            .rtindex_for(table)
            .map_err(|e| format!("rtindex resolution failed: {e}"))?;
        let pg_expr = expr_translator::translate(predicate, &self.expr_ctx);
        if pg_expr.is_null() {
            return Err("predicate translation produced null".to_string());
        }
        let node = self.alloc_node::<pg_sys::TidScan>();
        if node.is_null() {
            return Err("TidScan allocation returned null".to_string());
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_TidScan;
        (*node).scan.scanrelid = rtindex;
        (*node).tidquals = pg_sys::lappend((*node).tidquals, pg_expr.cast());
        // TidScan is cheap: it's a direct heap fetch per TID.
        // Use ~1 page-lookup cost per TID; we don't know the
        // count statically, default to 1 row.
        (*node).scan.plan.startup_cost = 0.0;
        (*node).scan.plan.total_cost = 0.01;
        (*node).scan.plan.plan_rows = 1.0;
        (*node).scan.plan.plan_width = 0;
        debug!(table = %table, "TID_SCAN advice honored");
        Ok(&mut (*node).scan.plan as *mut pg_sys::Plan)
    }

    /// Honor `BITMAP_HEAP_SCAN` advice by synthesizing a
    /// Honor `BITMAP_HEAP_SCAN` advice by synthesizing a
    /// bitmap subplan + `BitmapHeapScan` when the supplied
    /// filter predicate has equality on at least one indexed
    /// column. Returns `Err(reason)` otherwise so the caller
    /// can fall back.
    ///
    /// The bitmap subplan handles three shapes:
    /// 1. Single equi-clause (`a = X`): a single
    ///    `BitmapIndexScan` covering one index.
    /// 2. AND-of-equi-clauses (`a = X AND b = Y`) with each
    ///    side having its own index: a `BitmapAnd` wrapping
    ///    the per-side `BitmapIndexScan`s.
    /// 3. OR-of-equi-clauses (`a = X OR b = Y`) with each
    ///    side having its own index: a `BitmapOr` wrapping
    ///    the per-side `BitmapIndexScan`s.
    /// Mixed shapes (some sides with index, some without)
    /// fall back to the most-coverable single index, mirroring
    /// PG's behavior of only including bitmap-eligible quals
    /// in the bitmap and re-checking the rest via
    /// `bitmapqualorig`.
    unsafe fn build_bitmap_heap_for_filter(
        &mut self,
        table: &str,
        predicate: &Expr,
    ) -> Result<*mut pg_sys::Plan, String> {
        let rel_oid = self
            .rel_oid_for(table)
            .map_err(|e| format!("rel_oid resolution failed: {e}"))?;
        let rtindex = self
            .rtindex_for(table)
            .map_err(|e| format!("rtindex resolution failed: {e}"))?;

        // Build the bitmap subplan from the predicate's
        // top-level structure.
        let bitmap_input =
            self.build_bitmap_source(predicate, rel_oid, rtindex)?;

        // The full filter predicate also goes into
        // `bitmapqualorig` so the executor re-checks any
        // tuples produced by lossy bitmap pages and any
        // un-bitmappable clauses.
        let pg_full_pred = expr_translator::translate(predicate, &self.expr_ctx);
        if pg_full_pred.is_null() {
            return Err("filter predicate translation produced null".to_string());
        }

        let bhs = self.alloc_node::<pg_sys::BitmapHeapScan>();
        if bhs.is_null() {
            return Err("BitmapHeapScan allocation returned null".to_string());
        }
        (*bhs).scan.plan.type_ = pg_sys::NodeTag::T_BitmapHeapScan;
        (*bhs).scan.scanrelid = rtindex;
        (*bhs).scan.plan.lefttree = bitmap_input;
        (*bhs).bitmapqualorig =
            pg_sys::lappend((*bhs).bitmapqualorig, pg_full_pred.cast());
        self.set_index_costs(&mut (*bhs).scan.plan, table, 0.1);
        debug!(
            table = %table,
            "BITMAP_HEAP_SCAN advice honored",
        );
        Ok(&mut (*bhs).scan.plan as *mut pg_sys::Plan)
    }

    /// Construct a bitmap-producing plan node for `predicate`.
    /// Walks AND/OR top-level structure to emit `BitmapAnd` /
    /// `BitmapOr` over per-clause `BitmapIndexScan`s. Returns
    /// `Err(reason)` if no bitmap source can be built.
    unsafe fn build_bitmap_source(
        &mut self,
        predicate: &Expr,
        rel_oid: pg_sys::Oid,
        rtindex: pg_sys::Index,
    ) -> Result<*mut pg_sys::Plan, String> {
        use ra_core::expr::BinOp as RaBinOp;
        match predicate {
            Expr::BinOp { op: RaBinOp::And, left, right } => {
                self.build_bitmap_combined(
                    &[left.as_ref(), right.as_ref()],
                    rel_oid,
                    rtindex,
                    /* is_and */ true,
                )
            }
            Expr::BinOp { op: RaBinOp::Or, left, right } => {
                self.build_bitmap_combined(
                    &[left.as_ref(), right.as_ref()],
                    rel_oid,
                    rtindex,
                    /* is_and */ false,
                )
            }
            _ => self.build_single_bitmap_index_scan(predicate, rel_oid, rtindex),
        }
    }

    /// Build a `BitmapAnd` (when `is_and == true`) or
    /// `BitmapOr` (when `is_and == false`) wrapping the per-
    /// clause bitmap subplans. Sub-clauses without a useful
    /// index are dropped from the bitmap (they get re-checked
    /// in `bitmapqualorig`); for AND this is correct PG
    /// semantics. For OR a missing-index clause means the
    /// entire bitmap is unsound — we collapse to whichever
    /// side has an index, or fail if neither does.
    unsafe fn build_bitmap_combined(
        &mut self,
        clauses: &[&Expr],
        rel_oid: pg_sys::Oid,
        rtindex: pg_sys::Index,
        is_and: bool,
    ) -> Result<*mut pg_sys::Plan, String> {
        let mut bitmap_subplans: *mut pg_sys::List = std::ptr::null_mut();
        let mut count = 0usize;
        let mut covered = 0usize;
        for clause in clauses {
            count += 1;
            match self.build_bitmap_source(clause, rel_oid, rtindex) {
                Ok(sub) => {
                    bitmap_subplans = pg_sys::lappend(bitmap_subplans, sub.cast());
                    covered += 1;
                }
                Err(_) => {
                    if !is_and {
                        // OR with un-bitmappable clause: the
                        // entire bitmap can't safely represent
                        // the disjunction. Bail out so the
                        // outer caller falls back.
                        return Err(
                            "OR clause has un-bitmappable side; cannot combine".to_string(),
                        );
                    }
                    // AND with un-bitmappable clause: skip it,
                    // bitmapqualorig will re-check.
                }
            }
        }
        if covered == 0 {
            return Err("no clauses had a useful index".to_string());
        }
        if covered == 1 {
            // Single covered clause: unwrap the singleton
            // list and return it directly. Avoids an unnecessary
            // BitmapAnd/Or wrapper.
            return Ok(pg_sys::list_nth(bitmap_subplans, 0).cast::<pg_sys::Plan>());
        }
        if is_and {
            let node = self.alloc_node::<pg_sys::BitmapAnd>();
            if node.is_null() {
                return Err("BitmapAnd allocation returned null".to_string());
            }
            (*node).plan.type_ = pg_sys::NodeTag::T_BitmapAnd;
            (*node).bitmapplans = bitmap_subplans;
            // Cost ~ sum of children, conservatively.
            (*node).plan.startup_cost = 0.0;
            (*node).plan.total_cost = (covered as f64) * 0.5;
            (*node).plan.plan_rows = 1.0;
            (*node).plan.plan_width = 0;
            debug!(
                clauses = count,
                covered = covered,
                "BitmapAnd: combining multi-index bitmap inputs",
            );
            Ok(&mut (*node).plan as *mut pg_sys::Plan)
        } else {
            let node = self.alloc_node::<pg_sys::BitmapOr>();
            if node.is_null() {
                return Err("BitmapOr allocation returned null".to_string());
            }
            (*node).plan.type_ = pg_sys::NodeTag::T_BitmapOr;
            (*node).bitmapplans = bitmap_subplans;
            (*node).plan.startup_cost = 0.0;
            (*node).plan.total_cost = (covered as f64) * 0.5;
            (*node).plan.plan_rows = 1.0;
            (*node).plan.plan_width = 0;
            debug!(
                clauses = count,
                covered = covered,
                "BitmapOr: combining multi-index bitmap inputs",
            );
            Ok(&mut (*node).plan as *mut pg_sys::Plan)
        }
    }

    /// Build a single `BitmapIndexScan` for a leaf-equality
    /// predicate. Returns `Err(reason)` if the clause isn't a
    /// column-equality test or no index covers it.
    unsafe fn build_single_bitmap_index_scan(
        &mut self,
        clause: &Expr,
        rel_oid: pg_sys::Oid,
        rtindex: pg_sys::Index,
    ) -> Result<*mut pg_sys::Plan, String> {
        // Look at the leaf clause: must be `Column = Const`
        // (or symmetric) on a column with an index.
        let column = leaf_eq_column(clause)
            .ok_or_else(|| "clause is not a column-equality test".to_string())?;
        if column.eq_ignore_ascii_case("ctid") {
            return Err("ctid clause routed via TID_SCAN".to_string());
        }
        let idx_info = crate::index_resolver::resolve_index(rel_oid, column)
            .ok_or_else(|| format!("no index covers column `{column}`"))?;

        // Per-key opfamilies of the chosen index (copied out before the
        // relcache entry is closed), for the canonical-form index condition.
        let idx_rel = pg_sys::index_open(idx_info.oid, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
        if idx_rel.is_null() || (*idx_rel).rd_opfamily.is_null() {
            if !idx_rel.is_null() {
                pg_sys::index_close(idx_rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
            }
            return Err(format!("no opfamily for index on `{column}`"));
        }
        let nkeys = if (*idx_rel).rd_index.is_null() {
            0
        } else {
            (*(*idx_rel).rd_index).indnkeyatts as usize
        };
        let mut opfamilies: Vec<pg_sys::Oid> = Vec::with_capacity(nkeys);
        for i in 0..nkeys {
            opfamilies.push(*(*idx_rel).rd_opfamily.add(i));
        }
        pg_sys::index_close(idx_rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);

        // Canonical index condition: indexqual references the index key via
        // INDEX_VAR (varattno = index column position); indexqualorig keeps the
        // heap-Var form for recheck. A heap-form indexqual would map scan keys
        // to the wrong index attribute and return wrong rows.
        let (heap_op, idx_op, _is_eq) = self
            .build_index_clause(clause, &idx_info.columns, rtindex, &opfamilies)
            .ok_or_else(|| format!("clause not a pushable btree condition on `{column}`"))?;

        let bis = self.alloc_node::<pg_sys::BitmapIndexScan>();
        if bis.is_null() {
            return Err("BitmapIndexScan allocation returned null".to_string());
        }
        (*bis).scan.plan.type_ = pg_sys::NodeTag::T_BitmapIndexScan;
        (*bis).scan.scanrelid = rtindex;
        (*bis).indexid = idx_info.oid;
        (*bis).indexqual = pg_sys::lappend((*bis).indexqual, idx_op.cast());
        (*bis).indexqualorig = pg_sys::lappend((*bis).indexqualorig, heap_op.cast());
        (*bis).scan.plan.startup_cost = 0.0;
        (*bis).scan.plan.total_cost = 0.5;
        (*bis).scan.plan.plan_rows = 1.0;
        (*bis).scan.plan.plan_width = 0;
        Ok(&mut (*bis).scan.plan as *mut pg_sys::Plan)
    }

    /// Count the distinct indexes referenced by the top-level `AND` conjuncts
    /// of `predicate` (each conjunct that is a `col = const` on an indexed
    /// column contributes its index). Used to decide whether a BitmapAnd of
    /// multiple indexes is worthwhile: a single-index AND is better served by
    /// a plain index scan (one indexqual + recheck), so the bitmap path is
    /// gated to `>= 2` DISTINCT indexes.
    unsafe fn and_distinct_index_count(&self, predicate: &Expr, rel_oid: pg_sys::Oid) -> usize {
        if !matches!(predicate, Expr::BinOp { op: ra_core::expr::BinOp::And, .. }) {
            return 0;
        }
        let mut conjuncts: Vec<&Expr> = Vec::new();
        split_conjuncts(predicate, &mut conjuncts);
        let mut oids: Vec<pg_sys::Oid> = Vec::new();
        for c in &conjuncts {
            let Some(col) = leaf_eq_column(c) else { continue };
            if col.eq_ignore_ascii_case("ctid") {
                continue;
            }
            if let Some(info) = crate::index_resolver::resolve_index(rel_oid, col) {
                if !oids.contains(&info.oid) {
                    oids.push(info.oid);
                }
            }
        }
        oids.len()
    }

    /// Try to emit a real `IndexScan` for `Filter(predicate) over Scan(table)`
    /// by pushing a single-column btree **equality** conjunct (`col = const`)
    /// into `indexqual`, leaving any other conjuncts as a recheck `qual`.
    ///
    /// Returns `Ok(Some(plan))` on success, `Ok(None)` to fall back to the
    /// standard SeqScan+qual path. Strictly conservative: it bails (→ `None`)
    /// on anything it cannot prove, so a wrong or executor-crashing index
    /// condition is never produced. The resulting node behaves exactly like
    /// the SeqScan it replaces (same `scanrelid`, same targetlist path, same
    /// residual qual) — only the equality conjunct moves into `indexqual`.
    ///
    /// DEPRECATED: With the unified e-graph pipeline (Phase 1+4), the cost
    /// extractor handles index-vs-seq selection via the `scan-to-index-scan`
    /// rule. This peephole is retained as a fallback for edge cases where
    /// the e-graph doesn't have index metadata. Will be removed once
    /// the catalog resolver feeds index info to the e-graph conditions.
    unsafe fn try_build_index_scan(
        &mut self,
        table: &str,
        predicate: &Expr,
    ) -> Result<Option<*mut pg_sys::Plan>, PlanBuilderError> {
        let (Ok(rel_oid), Ok(rtindex)) = (self.rel_oid_for(table), self.rtindex_for(table)) else {
            return Ok(None);
        };

        let mut conjuncts: Vec<&Expr> = Vec::new();
        split_conjuncts(predicate, &mut conjuncts);

        // Find a btree index whose leading column appears in a comparison
        // conjunct. The column qualifier is not checked here: a Filter over a
        // single base Scan can only reference that relation, and the
        // translated-Var `varno` check below is the authoritative guard.
        let mut found: Option<crate::index_resolver::IndexInfo> = None;
        for c in &conjuncts {
            let Some(cref) = comparison_column(c) else {
                continue;
            };
            if let Some(info) = crate::index_resolver::resolve_index(rel_oid, &cref.column) {
                if info.am_type == "btree" && !info.columns.is_empty() {
                    found = Some(info);
                    break;
                }
            }
        }
        let Some(info) = found else {
            return Ok(None);
        };

        // Operator family of the index's leading column (copied out before
        // closing the relcache entry).
        let idx_rel = pg_sys::index_open(info.oid, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
        if idx_rel.is_null() || (*idx_rel).rd_opfamily.is_null() {
            if !idx_rel.is_null() {
                pg_sys::index_close(idx_rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
            }
            return Ok(None);
        }
        // Per-key-column operator families (copied out before closing the
        // relcache entry); each conjunct is checked against its own column's
        // family so a non-leading column of a different type is handled
        // correctly. rd_opfamily has one entry per *key* column, so cap the
        // read at indnkeyatts (INCLUDE columns have no searchable family).
        let nkeys = if (*idx_rel).rd_index.is_null() {
            0
        } else {
            (*(*idx_rel).rd_index).indnkeyatts as usize
        };
        let mut opfamilies: Vec<pg_sys::Oid> = Vec::with_capacity(nkeys);
        for i in 0..nkeys {
            opfamilies.push(*(*idx_rel).rd_opfamily.add(i));
        }
        pg_sys::index_close(idx_rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);

        // Push every conjunct on an index key column into the index condition
        // (so `id >= a AND id <= b` is a bounded scan, and `a = x AND b = y`
        // on a compound index uses both keys); every other conjunct becomes a
        // heap recheck qual.
        let mut indexqual = std::ptr::null_mut::<pg_sys::List>();
        let mut indexqualorig = std::ptr::null_mut::<pg_sys::List>();
        let mut qual = std::ptr::null_mut::<pg_sys::List>();
        let mut has_unique_eq = false;
        for c in &conjuncts {
            if let Some((heap_op, idx_op, is_eq)) =
                self.build_index_clause(c, &info.columns, rtindex, &opfamilies)
            {
                indexqualorig = pg_sys::lappend(indexqualorig, heap_op.cast());
                indexqual = pg_sys::lappend(indexqual, idx_op.cast());
                has_unique_eq |= is_eq && info.is_unique;
            } else {
                // A residual conjunct must NOT be silently dropped (that would
                // return unfiltered rows); fall back if it cannot translate.
                let q = expr_translator::translate(c, &self.expr_ctx);
                if q.is_null() {
                    return Ok(None);
                }
                qual = pg_sys::lappend(qual, q.cast());
            }
        }
        if indexqual.is_null() {
            return Ok(None);
        }

        // Stats-driven access-path choice. Estimate the predicate's
        // selectivity from gathered column statistics; when the scan is
        // non-selective enough that a sequential scan is cheaper than this
        // index scan, decline the index path and let the SeqScan fold handle
        // it. A unique-index equality (matches at most one row) always wins,
        // and an explicit INDEX_SCAN / INDEX_ONLY_SCAN plan advice overrides
        // the cost comparison.
        let est_selectivity = if has_unique_eq {
            0.0
        } else {
            self.estimate_scan_selectivity(table, predicate)
        };
        let advice_forces_index = matches!(
            self.physical_choices.scan_for(table),
            Some(
                ra_engine::plan_advice_physical::ScanStrategy::Index { .. }
                    | ra_engine::plan_advice_physical::ScanStrategy::IndexOnly { .. }
            )
        );
        if !has_unique_eq && !advice_forces_index && self.seq_beats_index(table, est_selectivity) {
            return Ok(None);
        }

        let node = self.alloc_node::<pg_sys::IndexScan>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("IndexScan allocation".to_owned()));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_IndexScan;
        (*node).scan.scanrelid = rtindex;
        (*node).indexid = info.oid;
        (*node).indexqual = indexqual;
        (*node).indexqualorig = indexqualorig;
        (*node).indexorderdir = pg_sys::ScanDirection::ForwardScanDirection;
        (*node).scan.plan.qual = qual;
        self.set_index_costs(&mut (*node).scan.plan, table, est_selectivity);
        Ok(Some(&mut (*node).scan.plan as *mut pg_sys::Plan))
    }

    /// Build one canonical index condition from `clause` if it is a
    /// `col <btree-op> X` comparison (in either argument order) where `col` is
    /// one of `index_cols` and `X` references no relation column. Returns
    /// `(heap_form, index_form, is_equality)`, where `index_form` rewrites the
    /// key to an `INDEX_VAR` reference at the column's index attno. Returns
    /// `None` (→ the caller makes it a recheck qual) for anything that is not
    /// a pushable btree condition.
    unsafe fn build_index_clause(
        &self,
        clause: &Expr,
        index_cols: &[String],
        rtindex: pg_sys::Index,
        opfamilies: &[pg_sys::Oid],
    ) -> Option<(*mut pg_sys::OpExpr, *mut pg_sys::OpExpr, bool)> {
        let Expr::BinOp { op, left, right } = clause else {
            return None;
        };
        if !is_comparison(*op) {
            return None;
        }
        // The matched column must be one of the index's key columns; its
        // 1-based position is the INDEX_VAR attno to use.
        let attno_of = |cr: &ra_core::expr::ColumnRef| {
            index_cols
                .iter()
                .position(|ic| ic.eq_ignore_ascii_case(&cr.column))
                .map(|p| (p + 1) as i16)
        };
        let (col_on_left, index_attno) = match (&**left, &**right) {
            (Expr::Column(cr), _) if attno_of(cr).is_some() => (true, attno_of(cr)?),
            (_, Expr::Column(cr)) if attno_of(cr).is_some() => (false, attno_of(cr)?),
            _ => return None,
        };
        let orig = expr_translator::translate(clause, &self.expr_ctx);
        if orig.is_null() || (*orig).type_ != pg_sys::NodeTag::T_OpExpr {
            return None;
        }
        let op_node = orig.cast::<pg_sys::OpExpr>();
        if pg_sys::list_length((*op_node).args) != 2 {
            return None;
        }
        let key = pg_sys::list_nth((*op_node).args, i32::from(!col_on_left)).cast::<pg_sys::Node>();
        let other = pg_sys::list_nth((*op_node).args, i32::from(col_on_left)).cast::<pg_sys::Node>();
        if key.is_null()
            || (*key).type_ != pg_sys::NodeTag::T_Var
            || (*key.cast::<pg_sys::Var>()).varno != rtindex as i32
        {
            return None;
        }
        // The comparison value must not reference any relation column.
        if other.is_null() || pg_sys::contain_var_clause(other) {
            return None;
        }
        // Canonicalize the key to the LEFT (ExecIndexBuildScanKeys requires
        // it), commuting the operator when the column was on the right, and
        // require the operator to be a btree strategy of the index opfamily.
        let opno = if col_on_left {
            (*op_node).opno
        } else {
            pg_sys::get_commutator((*op_node).opno)
        };
        if opno == pg_sys::InvalidOid {
            return None;
        }
        let opfamily = *opfamilies.get((index_attno - 1) as usize)?;
        let strategy = pg_sys::get_op_opfamily_strategy(opno, opfamily);
        if !(1..=5).contains(&strategy) {
            return None;
        }
        if !col_on_left {
            (*op_node).opno = opno;
            (*op_node).opfuncid = pg_sys::get_opcode(opno);
            (*op_node).args = list2(key, other);
        }
        let idx = pg_sys::copyObjectImpl(op_node.cast()).cast::<pg_sys::OpExpr>();
        let idx_key = pg_sys::list_nth((*idx).args, 0).cast::<pg_sys::Var>();
        (*idx_key).varno = pg_sys::INDEX_VAR;
        (*idx_key).varattno = index_attno;
        Some((op_node, idx, strategy == pg_sys::BTEqualStrategyNumber as i32))
    }

    /// Build an `IndexScan` node for MIN/MAX index optimization.
    unsafe fn build_index_scan(
        &mut self,
        table: &str,
        column: &str,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let rtindex = self.rtindex_for(table)?;
        let rel_oid = self.rel_oid_for(table)?;
        let node = self.alloc_node::<pg_sys::IndexScan>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "IndexScan allocation".to_string(),
            ));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_IndexScan;
        (*node).scan.scanrelid = rtindex;

        // Resolve the index OID from catalog
        if let Some(info) = crate::index_resolver::resolve_index(rel_oid, column) {
            (*node).indexid = info.oid;
            debug!(
                table = table,
                column = column,
                index_oid = info.oid.to_u32(),
                "IndexScan: resolved index"
            );
        } else {
            debug!(
                table = table,
                column = column,
                "IndexScan: no suitable index found for column"
            );
        }

        self.set_index_costs(&mut (*node).scan.plan, table, 0.1);
        Ok(&mut (*node).scan.plan as *mut pg_sys::Plan)
    }

    /// Build a `BitmapIndexScan` node.
    unsafe fn build_bitmap_index_scan(
        &mut self,
        table: &str,
        index: &str,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let rtindex = self.rtindex_for(table)?;
        let rel_oid = self.rel_oid_for(table)?;
        let node = self.alloc_node::<pg_sys::BitmapIndexScan>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "BitmapIndexScan allocation".to_string(),
            ));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_BitmapIndexScan;
        (*node).scan.scanrelid = rtindex;

        // Resolve the named index to its OID
        if let Some(info) = crate::index_resolver::resolve_index_by_name(rel_oid, index) {
            (*node).indexid = info.oid;
            debug!(
                table = table,
                index = index,
                index_oid = info.oid.to_u32(),
                "BitmapIndexScan: resolved index"
            );
        } else {
            debug!(
                table = table,
                index = index,
                "BitmapIndexScan: named index not found"
            );
        }

        self.set_index_costs(&mut (*node).scan.plan, table, 0.2);
        Ok(&mut (*node).scan.plan as *mut pg_sys::Plan)
    }

    /// Build a `BitmapHeapScan` node.
    unsafe fn build_bitmap_heap_scan(
        &mut self,
        table: &str,
        bitmap_input: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let rtindex = self.rtindex_for(table)?;
        let bitmap_plan = self.build_plan(bitmap_input)?;
        let node = self.alloc_node::<pg_sys::BitmapHeapScan>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "BitmapHeapScan allocation".to_string(),
            ));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_BitmapHeapScan;
        (*node).scan.scanrelid = rtindex;
        (*node).scan.plan.lefttree = bitmap_plan;
        self.set_costs_from_stats(&mut (*node).scan.plan, table);
        Ok(&mut (*node).scan.plan as *mut pg_sys::Plan)
    }

    /// Build a `BitmapAnd` node combining multiple bitmap scans.
    unsafe fn build_bitmap_and(
        &mut self,
        inputs: &[Box<RelExpr>],
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let node = self.alloc_node::<pg_sys::BitmapAnd>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "BitmapAnd allocation".to_string(),
            ));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_BitmapAnd;
        // Build bitmapplans list by recursively translating each input
        // (each should produce a BitmapIndexScan or nested BitmapOr/And).
        let mut plans_list = std::ptr::null_mut::<pg_sys::List>();
        for input in inputs {
            match self.build_plan(input) {
                Ok(child) if !child.is_null() => {
                    plans_list = pg_sys::lappend(plans_list, child.cast());
                }
                Ok(_) => {
                    debug!("BitmapAnd: child plan translated to null, skipping");
                }
                Err(e) => {
                    debug!("BitmapAnd: child plan translation failed: {e}");
                }
            }
        }
        (*node).bitmapplans = plans_list;
        Ok(&mut (*node).plan as *mut pg_sys::Plan)
    }

    /// Build a `BitmapOr` node combining multiple bitmap scans.
    unsafe fn build_bitmap_or(
        &mut self,
        inputs: &[Box<RelExpr>],
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let node = self.alloc_node::<pg_sys::BitmapOr>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "BitmapOr allocation".to_string(),
            ));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_BitmapOr;
        // Build bitmapplans list by recursively translating each input.
        let mut plans_list = std::ptr::null_mut::<pg_sys::List>();
        for input in inputs {
            match self.build_plan(input) {
                Ok(child) if !child.is_null() => {
                    plans_list = pg_sys::lappend(plans_list, child.cast());
                }
                Ok(_) => {
                    debug!("BitmapOr: child plan translated to null, skipping");
                }
                Err(e) => {
                    debug!("BitmapOr: child plan translation failed: {e}");
                }
            }
        }
        (*node).bitmapplans = plans_list;
        Ok(&mut (*node).plan as *mut pg_sys::Plan)
    }

    /// Build an `IndexOnlyScan` node (covering index — no heap fetch).
    unsafe fn try_build_index_only_scan(
        &mut self,
        table: &str,
        index: &str,
        columns: &[ProjectionColumn],
        predicate: &Expr,
    ) -> Result<Option<*mut pg_sys::Plan>, PlanBuilderError> {
        let rtindex = self.rtindex_for(table)?;
        let rel_oid = self.rel_oid_for(table)?;
        // The index name is advisory (the optimizer emits the sentinel
        // "auto"); the access-path choice is made here by finding a btree
        // index that COVERS the query. An index-only scan is sound only when
        // every projected and predicate column is an index key column (no heap
        // fetch). Otherwise return None so the caller falls back.
        let _ = index;

        // Projected columns must all be plain Columns (an expression cannot map
        // to an index attribute). Collect them plus the predicate columns as
        // the set the index must cover.
        let mut needed: Vec<String> = Vec::with_capacity(columns.len());
        for pc in columns {
            let Expr::Column(cr) = &pc.expr else {
                return Ok(None);
            };
            needed.push(cr.column.clone());
        }
        collect_column_names(predicate, &mut needed);

        let Some(info) = crate::index_resolver::find_covering_index(rel_oid, &needed) else {
            return Ok(None);
        };
        if info.columns.is_empty() {
            return Ok(None);
        }
        // Position (1-based) of a column within the index key list.
        let key_pos = |name: &str| -> Option<i16> {
            info.columns
                .iter()
                .position(|c| c.eq_ignore_ascii_case(name))
                .map(|p| (p + 1) as i16)
        };

        // Output column → index key position (coverage guaranteed above).
        let mut out_cols: Vec<(String, i16)> = Vec::with_capacity(columns.len());
        for pc in columns {
            let Expr::Column(cr) = &pc.expr else {
                return Ok(None);
            };
            let Some(pos) = key_pos(&cr.column) else {
                return Ok(None);
            };
            out_cols.push((cr.column.clone(), pos));
        }

        // Per-key opfamilies (copied out before closing the relcache entry),
        // for the canonical-form index conditions.
        let idx_rel = pg_sys::index_open(info.oid, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
        if idx_rel.is_null() || (*idx_rel).rd_opfamily.is_null() {
            if !idx_rel.is_null() {
                pg_sys::index_close(idx_rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
            }
            return Ok(None);
        }
        let nkeys = if (*idx_rel).rd_index.is_null() {
            0
        } else {
            (*(*idx_rel).rd_index).indnkeyatts as usize
        };
        let mut opfamilies: Vec<pg_sys::Oid> = Vec::with_capacity(nkeys);
        for i in 0..nkeys {
            opfamilies.push(*(*idx_rel).rd_opfamily.add(i));
        }
        pg_sys::index_close(idx_rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);

        // Build index conditions. EVERY conjunct must be a pushable btree
        // index clause — index-only has no heap tuple to recheck an arbitrary
        // residual qual against, so a non-pushable conjunct means fall back.
        let mut conjuncts: Vec<&Expr> = Vec::new();
        split_conjuncts(predicate, &mut conjuncts);
        let mut indexqual = std::ptr::null_mut::<pg_sys::List>();
        for c in &conjuncts {
            let Some((_heap_op, idx_op, _is_eq)) =
                self.build_index_clause(c, &info.columns, rtindex, &opfamilies)
            else {
                return Ok(None);
            };
            indexqual = pg_sys::lappend(indexqual, idx_op.cast());
        }
        if indexqual.is_null() {
            return Ok(None);
        }

        let node = self.alloc_node::<pg_sys::IndexOnlyScan>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("IndexOnlyScan allocation".to_string()));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_IndexOnlyScan;
        (*node).scan.scanrelid = rtindex;
        (*node).indexid = info.oid;
        (*node).indexqual = indexqual;
        // Exact (non-lossy) btree conditions need no recheck. recheckqual would
        // be evaluated against the index-reconstructed tuple (INDEX_VAR frame),
        // not the heap, so a heap-form qual here returns wrong rows — leave it
        // empty (palloc0 default).
        (*node).indexorderdir = pg_sys::ScanDirection::ForwardScanDirection;

        // indextlist: one entry per index key column. PostgreSQL maps each
        // index position to the *underlying heap* Var (varno = scan rtindex,
        // varattno = heap attno); EXPLAIN's resolve_special_varno follows this
        // to name an INDEX_VAR. Using an INDEX_VAR here is self-referential and
        // sends resolve_special_varno into an infinite loop (EXPLAIN hangs).
        let mut indextlist = std::ptr::null_mut::<pg_sys::List>();
        for (i, colname) in info.columns.iter().enumerate() {
            let Ok(cname) = std::ffi::CString::new(colname.as_str()) else {
                return Ok(None);
            };
            let attno = pg_sys::get_attnum(rel_oid, cname.as_ptr());
            let typ = pg_sys::get_atttype(rel_oid, attno);
            let var = self.alloc_node::<pg_sys::Var>();
            (*var).xpr.type_ = pg_sys::NodeTag::T_Var;
            (*var).varno = rtindex as i32;
            (*var).varattno = attno;
            (*var).vartype = typ;
            (*var).vartypmod = -1;
            (*var).varcollid = pg_sys::get_typcollation(typ);
            (*var).varlevelsup = 0;
            let te = pg_sys::makeTargetEntry(var.cast(), (i + 1) as i16, std::ptr::null_mut(), false);
            indextlist = pg_sys::lappend(indextlist, te.cast());
        }
        (*node).indextlist = indextlist;

        // Output targetlist: each projected column as an INDEX_VAR Var at its
        // index key position (index-only reads from the index, not the heap).
        let mut out_tl = std::ptr::null_mut::<pg_sys::List>();
        for (i, (colname, pos)) in out_cols.iter().enumerate() {
            let Ok(cname) = std::ffi::CString::new(colname.as_str()) else {
                return Ok(None);
            };
            let attno = pg_sys::get_attnum(rel_oid, cname.as_ptr());
            let typ = pg_sys::get_atttype(rel_oid, attno);
            let var = self.alloc_node::<pg_sys::Var>();
            (*var).xpr.type_ = pg_sys::NodeTag::T_Var;
            (*var).varno = pg_sys::INDEX_VAR as i32;
            (*var).varattno = *pos;
            (*var).vartype = typ;
            (*var).vartypmod = -1;
            (*var).varcollid = pg_sys::get_typcollation(typ);
            (*var).varlevelsup = 0;
            let resname = std::ffi::CString::new(colname.as_str())
                .ok()
                .map_or(std::ptr::null_mut(), |c| pg_sys::pstrdup(c.as_ptr()));
            let te = pg_sys::makeTargetEntry(var.cast(), (i + 1) as i16, resname, false);
            out_tl = pg_sys::lappend(out_tl, te.cast());
        }
        (*node).scan.plan.targetlist = out_tl;

        // Index-only scans avoid the heap fetch — cheaper than a regular index
        // scan.
        self.set_index_costs(&mut (*node).scan.plan, table, 0.05);
        Ok(Some(&mut (*node).scan.plan as *mut pg_sys::Plan))
    }

    /// Build a faithful `IndexOnlyScan` when the chosen index covers the query;
    /// otherwise fall back to the standard index/seq-scan build of the
    /// equivalent `Project(Filter(Scan))` (never worse than the prior lowering).
    unsafe fn build_index_only_scan(
        &mut self,
        table: &str,
        index: &str,
        columns: &[ProjectionColumn],
        predicate: &Expr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        if let Some(plan) = self.try_build_index_only_scan(table, index, columns, predicate)? {
            return Ok(plan);
        }
        let lowered = RelExpr::Project {
            columns: columns.to_vec(),
            input: Box::new(RelExpr::Filter {
                predicate: predicate.clone(),
                input: Box::new(RelExpr::Scan { table: table.to_owned(), alias: None }),
            }),
        };
        self.build_plan(&lowered)
    }

    // -----------------------------------------------------------------------
    // Join builders
    // -----------------------------------------------------------------------

    /// Build the appropriate join plan node for the given `JoinType`.
    ///
    /// Defaults to `HashJoin` for equi-joins (`Inner`, `LeftOuter`, `RightOuter`,
    /// `FullOuter`) and `NestLoop` for `Cross`, `Semi`, and `Anti` joins.
    ///
    /// The join `condition` is translated to a PostgreSQL qual expression and
    /// wired into the appropriate field (`hashclauses` for HashJoin,
    /// `joinqual` for NestLoop).
    /// Expose every live column of a relation as the scan's output
    /// targetlist (Vars referencing `scanrelid`), returning the targetlist
    /// and a map from `attno` to output position. Used so a join's child
    /// presents a known column layout for OUTER/INNER Var remapping.
    unsafe fn expose_relation_columns(
        &self,
        scanrelid: pg_sys::Index,
        reloid: pg_sys::Oid,
    ) -> Option<(*mut pg_sys::List, HashMap<i16, i32>)> {
        let rel = pg_sys::table_open(reloid, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
        if rel.is_null() {
            return None;
        }
        let natts = (*(*rel).rd_att).natts;
        let mut tlist: *mut pg_sys::List = std::ptr::null_mut();
        let mut map: HashMap<i16, i32> = HashMap::new();
        let mut pos = 0i32;
        for attno in 1..=natts {
            let tup = pg_sys::SearchSysCache2(
                pg_sys::SysCacheIdentifier::ATTNUM as i32,
                pg_sys::Datum::from(reloid),
                pg_sys::Datum::from(attno as i16),
            );
            if tup.is_null() {
                continue;
            }
            let form = pg_sys::GETSTRUCT(tup) as *mut pg_sys::FormData_pg_attribute;
            let dropped = (*form).attisdropped;
            let (atttypid, attcoll) = ((*form).atttypid, (*form).attcollation);
            pg_sys::ReleaseSysCache(tup);
            if dropped {
                continue;
            }
            pos += 1;
            let var = self.alloc_node::<pg_sys::Var>();
            (*var).xpr.type_ = pg_sys::NodeTag::T_Var;
            (*var).varno = scanrelid as i32;
            (*var).varattno = attno as i16;
            (*var).vartype = atttypid;
            (*var).vartypmod = -1;
            (*var).varcollid = attcoll;
            (*var).varlevelsup = 0;
            let te = pg_sys::makeTargetEntry(var.cast(), pos as i16, std::ptr::null_mut(), false);
            tlist = pg_sys::lappend(tlist, te.cast());
            map.insert(attno as i16, pos);
        }
        pg_sys::table_close(rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
        Some((tlist, map))
    }

    /// Rewrite scan-relative Vars in `node` to `OUTER_VAR`/`INNER_VAR`
    /// references for a join: Vars on the left relation become `OUTER_VAR`
    /// and Vars on the right become `INNER_VAR`, with `varattno` mapped to
    /// the child's output position. Returns `false` if any Var cannot be
    /// mapped or an unhandled node type is encountered (→ caller falls back).
    unsafe fn remap_join_vars(
        &self,
        node: *mut pg_sys::Node,
        lmap: &JoinColMap,
        rmap: &JoinColMap,
    ) -> bool {
        if node.is_null() {
            return true;
        }
        match (*node).type_ {
            pg_sys::NodeTag::T_Var => {
                let var = node.cast::<pg_sys::Var>();
                let key = ((*var).varno, (*var).varattno);
                if let Some(&pos) = lmap.get(&key) {
                    (*var).varno = pg_sys::OUTER_VAR;
                    (*var).varattno = pos as i16;
                    true
                } else if let Some(&pos) = rmap.get(&key) {
                    (*var).varno = pg_sys::INNER_VAR;
                    (*var).varattno = pos as i16;
                    true
                } else {
                    false
                }
            }
            pg_sys::NodeTag::T_Const | pg_sys::NodeTag::T_Param => true,
            pg_sys::NodeTag::T_OpExpr | pg_sys::NodeTag::T_DistinctExpr => {
                self.remap_var_list((*node.cast::<pg_sys::OpExpr>()).args, lmap, rmap)
            }
            pg_sys::NodeTag::T_ScalarArrayOpExpr => self.remap_var_list(
                (*node.cast::<pg_sys::ScalarArrayOpExpr>()).args,
                lmap,
                rmap,
            ),
            pg_sys::NodeTag::T_BoolExpr => {
                self.remap_var_list((*node.cast::<pg_sys::BoolExpr>()).args, lmap, rmap)
            }
            pg_sys::NodeTag::T_FuncExpr => {
                self.remap_var_list((*node.cast::<pg_sys::FuncExpr>()).args, lmap, rmap)
            }
            pg_sys::NodeTag::T_NullTest => self.remap_join_vars(
                (*node.cast::<pg_sys::NullTest>()).arg.cast(),
                lmap,
                rmap,
            ),
            pg_sys::NodeTag::T_RelabelType => self.remap_join_vars(
                (*node.cast::<pg_sys::RelabelType>()).arg.cast(),
                lmap,
                rmap,
            ),
            pg_sys::NodeTag::T_CoerceViaIO => self.remap_join_vars(
                (*node.cast::<pg_sys::CoerceViaIO>()).arg.cast(),
                lmap,
                rmap,
            ),
            pg_sys::NodeTag::T_ArrayCoerceExpr => self.remap_join_vars(
                (*node.cast::<pg_sys::ArrayCoerceExpr>()).arg.cast(),
                lmap,
                rmap,
            ),
            pg_sys::NodeTag::T_CaseExpr => {
                let ce = node.cast::<pg_sys::CaseExpr>();
                let ok_arg = if (*ce).arg.is_null() {
                    true
                } else {
                    self.remap_join_vars((*ce).arg.cast(), lmap, rmap)
                };
                let ok_default = if (*ce).defresult.is_null() {
                    true
                } else {
                    self.remap_join_vars((*ce).defresult.cast(), lmap, rmap)
                };
                ok_arg && ok_default && self.remap_var_list((*ce).args, lmap, rmap)
            }
            pg_sys::NodeTag::T_CaseWhen => {
                let cw = node.cast::<pg_sys::CaseWhen>();
                self.remap_join_vars((*cw).expr.cast(), lmap, rmap)
                    && self.remap_join_vars((*cw).result.cast(), lmap, rmap)
            }
            pg_sys::NodeTag::T_CoalesceExpr => {
                self.remap_var_list((*node.cast::<pg_sys::CoalesceExpr>()).args, lmap, rmap)
            }
            pg_sys::NodeTag::T_SubscriptingRef => {
                let sr = node.cast::<pg_sys::SubscriptingRef>();
                let ok_expr = self.remap_join_vars((*sr).refexpr.cast(), lmap, rmap);
                let ok_upper = self.remap_var_list((*sr).refupperindexpr, lmap, rmap);
                let ok_lower = self.remap_var_list((*sr).reflowerindexpr, lmap, rmap);
                ok_expr && ok_upper && ok_lower
            }
            _ => false,
        }
    }

    unsafe fn remap_var_list(
        &self,
        list: *mut pg_sys::List,
        lmap: &JoinColMap,
        rmap: &JoinColMap,
    ) -> bool {
        if list.is_null() {
            return true;
        }
        let elements = (*list).elements;
        for i in 0..(*list).length {
            let item = (*elements.add(i as usize)).ptr_value as *mut pg_sys::Node;
            if !self.remap_join_vars(item, lmap, rmap) {
                return false;
            }
        }
        true
    }

    /// The single base-relation table name a join side reduces to (a `Scan`
    /// optionally under `Filter`/`Project` chains; the filters fold into the
    /// scan qual). `None` if it is not a single base relation.
    fn base_scan_table(expr: &RelExpr) -> Option<&str> {
        match expr {
            RelExpr::Scan { table, .. } => Some(table),
            RelExpr::Filter { input, .. } | RelExpr::Project { input, .. } => {
                Self::base_scan_table(input)
            }
            _ => None,
        }
    }

    /// Like [`Self::base_scan_table`] but returns the scan's alias when it has
    /// one, falling back to the table name. Used to look up the range-table
    /// index per scan *instance* (a self-join's two aliases map to distinct
    /// rtindexes; the bare table name does not).
    fn base_scan_ident(expr: &RelExpr) -> Option<&str> {
        match expr {
            RelExpr::Scan { table, alias } => Some(alias.as_deref().unwrap_or(table)),
            RelExpr::Filter { input, .. } | RelExpr::Project { input, .. } => {
                Self::base_scan_ident(input)
            }
            _ => None,
        }
    }

    /// Column names of `table` in attribute order, excluding dropped columns.
    /// Returns `None` if the relation can't be resolved.
    unsafe fn relation_column_names(&self, table: &str) -> Option<Vec<String>> {
        let reloid = self.rel_oid_for(table).ok()?;
        let rel = pg_sys::table_open(reloid, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
        if rel.is_null() {
            return None;
        }
        let natts = (*(*rel).rd_att).natts;
        let mut out = Vec::new();
        for attno in 1..=natts {
            let tup = pg_sys::SearchSysCache2(
                pg_sys::SysCacheIdentifier::ATTNUM as i32,
                pg_sys::Datum::from(reloid),
                pg_sys::Datum::from(attno as i16),
            );
            if tup.is_null() {
                continue;
            }
            let form = pg_sys::GETSTRUCT(tup) as *mut pg_sys::FormData_pg_attribute;
            let dropped = (*form).attisdropped;
            let name = std::ffi::CStr::from_ptr((*form).attname.data.as_ptr())
                .to_string_lossy()
                .into_owned();
            pg_sys::ReleaseSysCache(tup);
            if !dropped {
                out.push(name);
            }
        }
        pg_sys::table_close(rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
        Some(out)
    }

    /// Expand `SELECT *` / `t.*` projection columns into one column per
    /// attribute of the single base relation the input scans. Returns `None`
    /// when there is no star (caller uses the original columns) or the star
    /// can't be safely expanded (the input is not a single base relation —
    /// e.g. a join — so the caller proceeds and may defer to PG).
    fn expand_star_columns(
        &self,
        columns: &[ProjectionColumn],
        input: &RelExpr,
    ) -> Option<Vec<ProjectionColumn>> {
        use ra_core::expr::{ColumnRef, Expr};
        let is_star = |c: &ProjectionColumn| matches!(&c.expr, Expr::Column(cr) if cr.column == "*");
        if !columns.iter().any(is_star) {
            return None;
        }
        let table = Self::base_scan_table(input)?;
        let names = unsafe { self.relation_column_names(table) }?;
        if names.is_empty() {
            return None;
        }
        let mut out = Vec::with_capacity(columns.len() + names.len());
        for pc in columns {
            if is_star(pc) {
                for n in &names {
                    out.push(ProjectionColumn {
                        expr: Expr::Column(ColumnRef::new(n.as_str())),
                        alias: None,
                    });
                }
            } else {
                out.push(pc.clone());
            }
        }
        Some(out)
    }

    /// Pre-build a `SubPlan` for every scalar sub-query reachable in `expr`'s
    /// projection/filter expressions, registering each in `expr_ctx.subplans`
    /// so expression translation can resolve it.
    unsafe fn prepare_subplans(&mut self, expr: &RelExpr) -> Result<(), PlanBuilderError> {
        match expr {
            RelExpr::Project { columns, input } => {
                for pc in columns {
                    self.prepare_expr_subplans(&pc.expr)?;
                }
                self.prepare_subplans(input)?;
            }
            RelExpr::Filter { predicate, input } => {
                self.prepare_expr_subplans(predicate)?;
                self.prepare_subplans(input)?;
            }
            _ => {
                for child in expr.children() {
                    self.prepare_subplans(child)?;
                }
            }
        }
        Ok(())
    }

    /// Build a `SubPlan` for each scalar `Expr::SubQuery` in `e`.
    unsafe fn prepare_expr_subplans(&mut self, e: &Expr) -> Result<(), PlanBuilderError> {
        match e {
            Expr::SubQuery { subquery_type, query, test_expr } => {
                use ra_core::expr::SubQueryType;
                let key = std::ptr::from_ref::<RelExpr>(query.as_ref()) as usize;
                if matches!(subquery_type, SubQueryType::Scalar) {
                    if !self.expr_ctx.subplans.borrow().contains_key(&key) {
                        let sp = self.build_scalar_subplan(query)?;
                        self.expr_ctx.subplans.borrow_mut().insert(key, sp);
                    }
                } else if matches!(subquery_type, SubQueryType::In) {
                    // IN / NOT IN that survived decorrelation (NOT IN is left as
                    // a sub-query for NULL-correctness): build an ANY_SUBLINK
                    // SubPlan. Requires the test expression.
                    if let Some(t) = test_expr {
                        if !self.expr_ctx.subplans.borrow().contains_key(&key) {
                            let sp = self.build_in_subplan(query, t)?;
                            self.expr_ctx.subplans.borrow_mut().insert(key, sp);
                        }
                    }
                }
                if let Some(t) = test_expr {
                    self.prepare_expr_subplans(t)?;
                }
            }
            Expr::BinOp { left, right, .. } => {
                self.prepare_expr_subplans(left)?;
                self.prepare_expr_subplans(right)?;
            }
            Expr::UnaryOp { operand, .. } => self.prepare_expr_subplans(operand)?,
            Expr::Function { args, .. } | Expr::Array(args) => {
                for a in args {
                    self.prepare_expr_subplans(a)?;
                }
            }
            Expr::Cast { expr, .. } | Expr::FieldAccess { expr, .. } => {
                self.prepare_expr_subplans(expr)?;
            }
            Expr::Case { operand, when_clauses, else_result } => {
                if let Some(o) = operand {
                    self.prepare_expr_subplans(o)?;
                }
                for (w, t) in when_clauses {
                    self.prepare_expr_subplans(w)?;
                    self.prepare_expr_subplans(t)?;
                }
                if let Some(els) = else_result {
                    self.prepare_expr_subplans(els)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Build an `EXPR_SUBLINK` `SubPlan` from a scalar sub-query: build the
    /// inner plan, replace its references to outer (correlation) relations
    /// with `PARAM_EXEC` parameters, and register the inner plan.
    unsafe fn build_scalar_subplan(
        &mut self,
        query: &RelExpr,
    ) -> Result<*mut pg_sys::Expr, PlanBuilderError> {
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        // Nested scalar sub-queries first.
        self.prepare_subplans(query)?;
        // Inner relations (this sub-query's own scans) → their rtindexes.
        let mut inner_rtis = std::collections::HashSet::new();
        let mut tables = Vec::new();
        Self::collect_scan_tables(query, &mut tables);
        for t in &tables {
            if let Ok(rti) = self.rtindex_for(t) {
                inner_rtis.insert(rti as i32);
            }
        }
        let plan = self.build_plan(query)?;
        if plan.is_null() || (*plan).targetlist.is_null() {
            return Err(unsupported("subquery plan"));
        }
        // Replace correlation Vars with parameters.
        let mut params: Vec<(i32, *mut pg_sys::Var)> = Vec::new();
        self.paramify_plan(plan, &inner_rtis, &mut params);

        // First output column type (the scalar result type).
        let first_te = (*(*plan).targetlist).elements;
        let te0 = (*first_te.add(0)).ptr_value as *mut pg_sys::TargetEntry;
        if te0.is_null() || (*te0).expr.is_null() {
            return Err(unsupported("subquery output"));
        }
        let first_type = pg_sys::exprType((*te0).expr.cast());
        let first_typmod = pg_sys::exprTypmod((*te0).expr.cast());
        let first_coll = pg_sys::exprCollation((*te0).expr.cast());

        self.subplans.push(plan);
        let plan_id = self.subplans.len() as i32;

        let node = self.alloc_node::<pg_sys::SubPlan>();
        (*node).xpr.type_ = pg_sys::NodeTag::T_SubPlan;
        (*node).subLinkType = pg_sys::SubLinkType::EXPR_SUBLINK;
        (*node).plan_id = plan_id;
        (*node).firstColType = first_type;
        (*node).firstColTypmod = first_typmod;
        (*node).firstColCollation = first_coll;
        (*node).useHashTable = false;
        (*node).parallel_safe = false;
        if params.is_empty() {
            // Uncorrelated scalar sub-query: hoist to an InitPlan computed once
            // (PostgreSQL does the same). Without this the SubPlan is
            // re-executed for every outer row — e.g. `WHERE x < (SELECT avg(x)
            // FROM t)` re-ran the aggregate 20k times (~31s). setParam carries
            // the single result; the expression references it via a Param, and
            // the SubPlan is attached to the top plan node's initPlan list.
            let set_pid = self.alloc_param(first_type);
            (*node).setParam = pg_sys::lappend_int(std::ptr::null_mut(), set_pid);
            (*node).parParam = std::ptr::null_mut();
            (*node).args = std::ptr::null_mut();
            self.init_subplans.push(node);
            let p = self.alloc_node::<pg_sys::Param>();
            (*p).xpr.type_ = pg_sys::NodeTag::T_Param;
            (*p).paramkind = pg_sys::ParamKind::PARAM_EXEC;
            (*p).paramid = set_pid;
            (*p).paramtype = first_type;
            (*p).paramtypmod = first_typmod;
            (*p).paramcollid = first_coll;
            (*p).location = -1;
            return Ok(p.cast());
        }
        let mut par_param: *mut pg_sys::List = std::ptr::null_mut();
        let mut args: *mut pg_sys::List = std::ptr::null_mut();
        for (pid, var) in params {
            par_param = pg_sys::lappend_int(par_param, pid);
            args = pg_sys::lappend(args, var.cast());
        }
        (*node).parParam = par_param;
        (*node).args = args;
        Ok(node.cast())
    }

    /// Build an `ANY_SUBLINK` `SubPlan` for `test IN (subquery)` (and, negated
    /// by a surrounding `NOT`, for `NOT IN`). Unlike an anti-join this honors
    /// SQL NULL semantics: the executor evaluates `test = <param>` per inner
    /// row, where `<param>` is loaded with each row's first column; the ANY
    /// result is NULL when no row matches but a NULL is present, so `NOT IN`
    /// correctly yields no rows in that case.
    unsafe fn build_in_subplan(
        &mut self,
        query: &RelExpr,
        test_expr: &Expr,
    ) -> Result<*mut pg_sys::Expr, PlanBuilderError> {
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        self.prepare_subplans(query)?;
        let mut inner_rtis = std::collections::HashSet::new();
        let mut tables = Vec::new();
        Self::collect_scan_tables(query, &mut tables);
        for t in tables {
            if let Ok(rti) = self.rtindex_for(&t) {
                inner_rtis.insert(rti as i32);
            }
        }
        let plan = self.build_plan(query)?;
        if plan.is_null() || (*plan).targetlist.is_null() {
            return Err(unsupported("IN subquery plan"));
        }
        let mut params: Vec<(i32, *mut pg_sys::Var)> = Vec::new();
        self.paramify_plan(plan, &inner_rtis, &mut params);

        let first_te = (*(*plan).targetlist).elements;
        let te0 = (*first_te.add(0)).ptr_value as *mut pg_sys::TargetEntry;
        if te0.is_null() || (*te0).expr.is_null() {
            return Err(unsupported("IN subquery output"));
        }
        let first_type = pg_sys::exprType((*te0).expr.cast());
        let first_typmod = pg_sys::exprTypmod((*te0).expr.cast());
        let first_coll = pg_sys::exprCollation((*te0).expr.cast());

        self.subplans.push(plan);
        let plan_id = self.subplans.len() as i32;

        // Result param: the executor loads each inner row's first column here,
        // and the testexpr compares the outer value against it.
        let result_pid = self.alloc_param(first_type);
        let param = self.alloc_node::<pg_sys::Param>();
        (*param).xpr.type_ = pg_sys::NodeTag::T_Param;
        (*param).paramkind = pg_sys::ParamKind::PARAM_EXEC;
        (*param).paramid = result_pid;
        (*param).paramtype = first_type;
        (*param).paramtypmod = first_typmod;
        (*param).paramcollid = first_coll;
        (*param).location = -1;

        // testexpr: <test> = <param>. op_expr_from_nodes resolves the operator
        // (inserting coercions via make_op when the types differ).
        let lhs = expr_translator::translate(test_expr, &self.expr_ctx);
        if lhs.is_null() {
            return Err(unsupported("IN test expression"));
        }
        let testexpr = expr_translator::op_expr_from_nodes("=", lhs, param.cast());
        if testexpr.is_null() {
            return Err(unsupported("IN test operator"));
        }

        let node = self.alloc_node::<pg_sys::SubPlan>();
        (*node).xpr.type_ = pg_sys::NodeTag::T_SubPlan;
        (*node).subLinkType = pg_sys::SubLinkType::ANY_SUBLINK;
        (*node).testexpr = testexpr.cast();
        (*node).paramIds = pg_sys::lappend_int(std::ptr::null_mut(), result_pid);
        (*node).plan_id = plan_id;
        (*node).firstColType = first_type;
        (*node).firstColTypmod = first_typmod;
        (*node).firstColCollation = first_coll;
        // Hash the subquery result once for an uncorrelated IN/NOT IN, instead
        // of rescanning it per outer row (PG's "hashed SubPlan"). The executor
        // builds a separate null-hashtable when unknownEqFalse is false, so NOT
        // IN's NULL semantics are preserved.
        (*node).useHashTable = params.is_empty();
        // Keep UNKNOWN distinct from FALSE so NOT IN's NULL semantics hold.
        (*node).unknownEqFalse = false;
        (*node).parallel_safe = false;
        let mut par_param: *mut pg_sys::List = std::ptr::null_mut();
        let mut args: *mut pg_sys::List = std::ptr::null_mut();
        for (pid, var) in params {
            par_param = pg_sys::lappend_int(par_param, pid);
            args = pg_sys::lappend(args, var.cast());
        }
        (*node).parParam = par_param;
        (*node).args = args;
        Ok(node.cast())
    }

    fn collect_scan_tables(expr: &RelExpr, out: &mut Vec<String>) {
        if let RelExpr::Scan { table, alias } = expr {
            // Prefer the alias so a self-correlated subquery's inner scan
            // (`orders o2`) resolves to its own rtindex, not the outer's.
            out.push(alias.clone().unwrap_or_else(|| table.clone()));
        }
        for c in expr.children() {
            Self::collect_scan_tables(c, out);
        }
    }

    /// Mark every node of a subplan tree as depending on the given PARAM_EXEC
    /// parameters (`extParam`/`allParam`). Ra does not run PostgreSQL's
    /// `SS_finalize_plan`, so without this a parameterized nested-loop inner
    /// would never be rescanned when the outer correlation value changes.
    unsafe fn mark_param_deps(plan: *mut pg_sys::Plan, pids: &[i32]) {
        if plan.is_null() || pids.is_empty() {
            return;
        }
        let mut bms: *mut pg_sys::Bitmapset = std::ptr::null_mut();
        for &p in pids {
            bms = pg_sys::bms_add_member(bms, p);
        }
        (*plan).extParam = pg_sys::bms_union((*plan).extParam, bms);
        (*plan).allParam = pg_sys::bms_union((*plan).allParam, bms);
        Self::mark_param_deps((*plan).lefttree, pids);
        Self::mark_param_deps((*plan).righttree, pids);
        if (*plan).type_ == pg_sys::NodeTag::T_SubqueryScan {
            Self::mark_param_deps((*plan.cast::<pg_sys::SubqueryScan>()).subplan, pids);
        }
    }

    /// The alias (eref aliasname) of the range-table entry at `rtindex`.
    unsafe fn rte_alias_name(&self, rtindex: pg_sys::Index) -> Option<String> {
        if self.original_query.is_null() || (*self.original_query).rtable.is_null() {
            return None;
        }
        let rte = pg_sys::list_nth((*self.original_query).rtable, (rtindex - 1) as i32)
            as *mut pg_sys::RangeTblEntry;
        if rte.is_null() || (*rte).eref.is_null() || (*(*rte).eref).aliasname.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr((*(*rte).eref).aliasname)
                .to_string_lossy()
                .into_owned(),
        )
    }

    /// Locate the `RTE_CTE` for `name` in the original query's range table and
    /// extract its range-table index and output columns.
    unsafe fn find_cte_rte(
        &self,
        name: &str,
    ) -> Result<(pg_sys::Index, Vec<CteCol>), PlanBuilderError> {
        let err = || PlanBuilderError::UnsupportedVariant("recursive CTE rte".to_owned());
        if self.original_query.is_null() {
            return Err(err());
        }
        let rtable = (*self.original_query).rtable;
        if rtable.is_null() {
            return Err(err());
        }
        let e = (*rtable).elements;
        for i in 0..(*rtable).length {
            let rte = (*e.add(i as usize)).ptr_value as *mut pg_sys::RangeTblEntry;
            if rte.is_null() || (*rte).rtekind != pg_sys::RTEKind::RTE_CTE {
                continue;
            }
            if (*rte).ctename.is_null()
                || !std::ffi::CStr::from_ptr((*rte).ctename)
                    .to_string_lossy()
                    .eq_ignore_ascii_case(name)
            {
                continue;
            }
            let (types, mods, colls) = ((*rte).coltypes, (*rte).coltypmods, (*rte).colcollations);
            if types.is_null() {
                return Err(err());
            }
            let names = if (*rte).eref.is_null() {
                std::ptr::null_mut()
            } else {
                (*(*rte).eref).colnames
            };
            let mut cols = Vec::with_capacity((*types).length as usize);
            for j in 0..(*types).length as usize {
                let typ = (*(*types).elements.add(j)).oid_value;
                let typmod = (*(*mods).elements.add(j)).int_value;
                let coll = (*(*colls).elements.add(j)).oid_value;
                let name = if names.is_null() || j >= (*names).length as usize {
                    format!("column{}", j + 1)
                } else {
                    let s = (*(*names).elements.add(j)).ptr_value as *mut pg_sys::String;
                    std::ffi::CStr::from_ptr((*s).sval).to_string_lossy().to_lowercase()
                };
                cols.push(CteCol { name, typ, typmod, coll });
            }
            return Ok(((i + 1) as pg_sys::Index, cols));
        }
        Err(err())
    }

    /// One-row `Result` node (no scan); the enclosing Project sets its tlist.
    unsafe fn build_result_node(&mut self) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let node = self.alloc_node::<pg_sys::Result>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("Result".to_owned()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Result;
        (*node).plan.plan_rows = 1.0;
        Ok(node.cast())
    }

    /// Build a `WorkTableScan` (recursive term) or `CteScan` (body) over the
    /// in-scope recursive CTE, projecting the CTE's columns.
    unsafe fn build_cte_scan(&mut self) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let rt = self
            .cte_runtime
            .as_ref()
            .ok_or_else(|| PlanBuilderError::UnsupportedVariant("cte scan".to_owned()))?;
        let (rtindex, cte_plan_id, cte_param, wt_param) =
            (rt.rtindex, rt.cte_plan_id, rt.cte_param, rt.wt_param);
        let is_body = matches!(rt.mode, CteScanMode::Body);
        let tlist = self.cte_column_tlist(rtindex);
        if is_body {
            let n = self.alloc_node::<pg_sys::CteScan>();
            (*n).scan.plan.type_ = pg_sys::NodeTag::T_CteScan;
            (*n).scan.plan.targetlist = tlist;
            (*n).scan.scanrelid = rtindex;
            (*n).ctePlanId = cte_plan_id;
            (*n).cteParam = cte_param;
            Ok(n.cast())
        } else {
            let n = self.alloc_node::<pg_sys::WorkTableScan>();
            (*n).scan.plan.type_ = pg_sys::NodeTag::T_WorkTableScan;
            (*n).scan.plan.targetlist = tlist;
            (*n).scan.scanrelid = rtindex;
            (*n).wtParam = wt_param;
            Ok(n.cast())
        }
    }

    /// Targetlist of `Var`s for the in-scope CTE's columns (varno = `rtindex`).
    unsafe fn cte_column_tlist(&self, rtindex: pg_sys::Index) -> *mut pg_sys::List {
        let scope = self.expr_ctx.cte_scope.borrow();
        let mut tlist: *mut pg_sys::List = std::ptr::null_mut();
        if let Some(s) = scope.as_ref() {
            for (i, c) in s.cols.iter().enumerate() {
                let var = self.alloc_node::<pg_sys::Var>();
                (*var).xpr.type_ = pg_sys::NodeTag::T_Var;
                (*var).varno = rtindex as i32;
                (*var).varattno = (i + 1) as i16;
                (*var).vartype = c.typ;
                (*var).vartypmod = c.typmod;
                (*var).varcollid = c.coll;
                (*var).location = -1;
                let rn = match std::ffi::CString::new(c.name.as_str()) {
                    Ok(s) => s.into_raw().cast::<i8>(),
                    Err(_) => std::ptr::null_mut(),
                };
                let te = pg_sys::makeTargetEntry(var.cast(), (i + 1) as i16, rn, false);
                tlist = pg_sys::lappend(tlist, te.cast());
            }
        }
        tlist
    }

    /// Build the output targetlist for a join-side CTE's SubqueryScan: one
    /// `Var(rtindex, attno)` per CTE column, referencing the subplan output by
    /// position. Mirrors `cte_column_tlist` but takes the columns directly.
    unsafe fn cte_join_var_tlist(&self, rtindex: pg_sys::Index, cols: &[CteCol]) -> *mut pg_sys::List {
        let mut tlist: *mut pg_sys::List = std::ptr::null_mut();
        for (i, c) in cols.iter().enumerate() {
            let var = self.alloc_node::<pg_sys::Var>();
            (*var).xpr.type_ = pg_sys::NodeTag::T_Var;
            (*var).varno = rtindex as i32;
            (*var).varattno = (i + 1) as i16;
            (*var).vartype = c.typ;
            (*var).vartypmod = c.typmod;
            (*var).varcollid = c.coll;
            (*var).location = -1;
            let rn = match std::ffi::CString::new(c.name.as_str()) {
                Ok(s) => s.into_raw().cast::<i8>(),
                Err(_) => std::ptr::null_mut(),
            };
            let te = pg_sys::makeTargetEntry(var.cast(), (i + 1) as i16, rn, false);
            tlist = pg_sys::lappend(tlist, te.cast());
        }
        tlist
    }

    /// Build a non-recursive CTE that is referenced as a join side: a
    /// `SubqueryScan` over the CTE's definition, with `scanrelid` set to the
    /// CTE's RTE_CTE so the join wires it like a base scan. The qualified
    /// columns of the CTE resolve through `cte_join_scope` to this scan.
    unsafe fn build_cte_join_subqueryscan(
        &mut self,
        name: &str,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        // Copy the definition / index / columns out before building the
        // subplan, so we don't hold a borrow of `self` across `build_plan`.
        let (def, rtindex, cols) = {
            let d = self
                .cte_join_defs
                .get(name)
                .ok_or_else(|| unsupported("join-side CTE not registered"))?;
            let cols: Vec<CteCol> = d
                .cols
                .iter()
                .map(|c| CteCol { name: c.name.clone(), typ: c.typ, typmod: c.typmod, coll: c.coll })
                .collect();
            (d.def.clone(), d.rtindex, cols)
        };
        let child = self.build_plan(&def)?;
        if child.is_null() || (*child).targetlist.is_null() {
            return Err(unsupported("join-side CTE subplan"));
        }
        let tlist = self.cte_join_var_tlist(rtindex, &cols);
        let node = self.alloc_node::<pg_sys::SubqueryScan>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("SubqueryScan".to_owned()));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_SubqueryScan;
        (*node).scan.scanrelid = rtindex;
        (*node).subplan = child;
        (*node).scan.plan.targetlist = tlist;
        (*node).scan.plan.qual = std::ptr::null_mut();
        Ok(node.cast())
    }

    /// True if `Scan(name)` appears anywhere within `expr`.
    fn scan_appears(expr: &RelExpr, name: &str) -> bool {
        if let RelExpr::Scan { table, .. } = expr {
            if table.eq_ignore_ascii_case(name) {
                return true;
            }
        }
        expr.children().iter().any(|c| Self::scan_appears(c, name))
    }

    /// True if `name` is referenced as a side of a join anywhere in `expr`
    /// (recursing through nested CTEs). Such a CTE must not be inlined; it is
    /// built as a join-side SubqueryScan instead.
    fn cte_used_in_join(expr: &RelExpr, name: &str) -> bool {
        if let RelExpr::Join { left, right, .. } = expr {
            if Self::scan_appears(left, name) || Self::scan_appears(right, name) {
                return true;
            }
        }
        expr.children().iter().any(|c| Self::cte_used_in_join(c, name))
    }

    unsafe fn build_recursive_cte(
        &mut self,
        name: &str,
        base: &RelExpr,
        recursive: &RelExpr,
        body: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let lname = name.to_lowercase();
        let (rtindex, cols) = self.find_cte_rte(&lname)?;
        let cte_param = self.alloc_param(pg_sys::INTERNALOID);
        let wt_param = self.alloc_param(pg_sys::INTERNALOID);
        self.in_recursive_cte = true;
        let result = self.build_recursive_cte_inner(
            &lname, base, recursive, body, rtindex, cols, cte_param, wt_param,
        );
        self.in_recursive_cte = false;
        result
    }

    #[expect(clippy::too_many_arguments, reason = "internal recursive-CTE wiring")]
    unsafe fn build_recursive_cte_inner(
        &mut self,
        lname: &str,
        base: &RelExpr,
        recursive: &RelExpr,
        body: &RelExpr,
        rtindex: pg_sys::Index,
        cols: Vec<CteCol>,
        cte_param: i32,
        wt_param: i32,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        // Anchor (non-recursive term): no self-reference, no CTE scope.
        let base_plan = self.build_plan(base)?;
        if base_plan.is_null() || (*base_plan).targetlist.is_null() {
            return Err(PlanBuilderError::UnsupportedVariant("recursive anchor".to_owned()));
        }

        // Recursive term: Scan(name) → WorkTableScan; columns from the CTE.
        let clone_cols = |cols: &[CteCol]| {
            cols.iter()
                .map(|c| CteCol { name: c.name.clone(), typ: c.typ, typmod: c.typmod, coll: c.coll })
                .collect::<Vec<_>>()
        };
        self.cte_runtime = Some(CteRuntime {
            name: lname.to_owned(),
            rtindex,
            cte_plan_id: 0,
            cte_param,
            wt_param,
            mode: CteScanMode::Recursive,
        });
        *self.expr_ctx.cte_scope.borrow_mut() =
            Some(CteScope { name: lname.to_owned(), rtindex, cols: clone_cols(&cols) });
        let rec_result = self.build_plan(recursive);
        *self.expr_ctx.cte_scope.borrow_mut() = None;
        self.cte_runtime = None;
        let rec_plan = rec_result?;

        // RecursiveUnion (UNION ALL → numCols = 0, no dedup).
        let ru = self.alloc_node::<pg_sys::RecursiveUnion>();
        (*ru).plan.type_ = pg_sys::NodeTag::T_RecursiveUnion;
        (*ru).plan.lefttree = base_plan;
        (*ru).plan.righttree = rec_plan;
        (*ru).wtParam = wt_param;
        (*ru).numCols = 0;
        let mut ru_tlist: *mut pg_sys::List = std::ptr::null_mut();
        for (i, c) in cols.iter().enumerate() {
            let var = self.alloc_node::<pg_sys::Var>();
            (*var).xpr.type_ = pg_sys::NodeTag::T_Var;
            (*var).varno = pg_sys::OUTER_VAR;
            (*var).varattno = (i + 1) as i16;
            (*var).vartype = c.typ;
            (*var).vartypmod = c.typmod;
            (*var).varcollid = c.coll;
            (*var).location = -1;
            let te = pg_sys::makeTargetEntry(var.cast(), (i + 1) as i16, std::ptr::null_mut(), false);
            ru_tlist = pg_sys::lappend(ru_tlist, te.cast());
        }
        (*ru).plan.targetlist = ru_tlist;
        (*ru).plan.plan_rows = (*base_plan).plan_rows.max(1.0);

        self.subplans.push(ru.cast());
        let cte_plan_id = self.subplans.len() as i32;

        // Body: Scan(name) → CteScan referencing the RecursiveUnion subplan.
        self.cte_runtime = Some(CteRuntime {
            name: lname.to_owned(),
            rtindex,
            cte_plan_id,
            cte_param,
            wt_param,
            mode: CteScanMode::Body,
        });
        *self.expr_ctx.cte_scope.borrow_mut() = Some(CteScope { name: lname.to_owned(), rtindex, cols });
        let body_result = self.build_plan(body);
        *self.expr_ctx.cte_scope.borrow_mut() = None;
        self.cte_runtime = None;
        body_result
    }

    /// Allocate a `PARAM_EXEC` parameter of `ty` and return its id.
    fn alloc_param(&mut self, ty: pg_sys::Oid) -> i32 {
        let id = self.param_types.len() as i32;
        self.param_types.push(ty);
        id
    }

    /// Walk a plan tree replacing Vars that reference relations outside
    /// `inner_rtis` (correlation references) with `PARAM_EXEC` `Param` nodes;
    /// records `(param_id, original_var)` for the SubPlan's parParam/args.
    unsafe fn paramify_plan(
        &mut self,
        plan: *mut pg_sys::Plan,
        inner: &std::collections::HashSet<i32>,
        out: &mut Vec<(i32, *mut pg_sys::Var)>,
    ) {
        if plan.is_null() {
            return;
        }
        let tl = (*plan).targetlist;
        if !tl.is_null() {
            let e = (*tl).elements;
            for i in 0..(*tl).length {
                let te = (*e.add(i as usize)).ptr_value as *mut pg_sys::TargetEntry;
                if !te.is_null() {
                    (*te).expr = self.paramify_node((*te).expr.cast(), inner, out).cast();
                }
            }
        }
        self.paramify_list((*plan).qual, inner, out);
        self.paramify_plan((*plan).lefttree, inner, out);
        self.paramify_plan((*plan).righttree, inner, out);
    }

    unsafe fn paramify_list(
        &mut self,
        list: *mut pg_sys::List,
        inner: &std::collections::HashSet<i32>,
        out: &mut Vec<(i32, *mut pg_sys::Var)>,
    ) {
        if list.is_null() {
            return;
        }
        let e = (*list).elements;
        for i in 0..(*list).length {
            let slot = e.add(i as usize);
            (*slot).ptr_value = self
                .paramify_node((*slot).ptr_value.cast(), inner, out)
                .cast();
        }
    }

    unsafe fn paramify_node(
        &mut self,
        node: *mut pg_sys::Node,
        inner: &std::collections::HashSet<i32>,
        out: &mut Vec<(i32, *mut pg_sys::Var)>,
    ) -> *mut pg_sys::Node {
        if node.is_null() {
            return node;
        }
        match (*node).type_ {
            pg_sys::NodeTag::T_Var => {
                let v = node.cast::<pg_sys::Var>();
                if (*v).varno > 0 && !inner.contains(&(*v).varno) {
                    let pid = self.alloc_param((*v).vartype);
                    out.push((pid, v));
                    let p = self.alloc_node::<pg_sys::Param>();
                    (*p).xpr.type_ = pg_sys::NodeTag::T_Param;
                    (*p).paramkind = pg_sys::ParamKind::PARAM_EXEC;
                    (*p).paramid = pid;
                    (*p).paramtype = (*v).vartype;
                    (*p).paramtypmod = (*v).vartypmod;
                    (*p).paramcollid = (*v).varcollid;
                    return p.cast();
                }
                node
            }
            pg_sys::NodeTag::T_OpExpr | pg_sys::NodeTag::T_DistinctExpr => {
                self.paramify_list((*node.cast::<pg_sys::OpExpr>()).args, inner, out);
                node
            }
            pg_sys::NodeTag::T_ScalarArrayOpExpr => {
                self.paramify_list((*node.cast::<pg_sys::ScalarArrayOpExpr>()).args, inner, out);
                node
            }
            pg_sys::NodeTag::T_BoolExpr => {
                self.paramify_list((*node.cast::<pg_sys::BoolExpr>()).args, inner, out);
                node
            }
            pg_sys::NodeTag::T_FuncExpr => {
                self.paramify_list((*node.cast::<pg_sys::FuncExpr>()).args, inner, out);
                node
            }
            pg_sys::NodeTag::T_Aggref => {
                self.paramify_list((*node.cast::<pg_sys::Aggref>()).args, inner, out);
                node
            }
            pg_sys::NodeTag::T_NullTest => {
                let n = node.cast::<pg_sys::NullTest>();
                (*n).arg = self.paramify_node((*n).arg.cast(), inner, out).cast();
                node
            }
            pg_sys::NodeTag::T_RelabelType => {
                let n = node.cast::<pg_sys::RelabelType>();
                (*n).arg = self.paramify_node((*n).arg.cast(), inner, out).cast();
                node
            }
            _ => node,
        }
    }

    /// Resolve one side of a `build_projected_join` to its scan range-table
    /// index, output targetlist, and attno→position map. A side referencing
    /// the in-scope recursive CTE resolves to the CTE's WorkTableScan/CteScan
    /// (columns from the CTE scope, identity attno→position map); any other
    /// side resolves to a base relation via the catalog.
    unsafe fn join_side_cols(
        &self,
        side: &RelExpr,
    ) -> Result<(pg_sys::Index, *mut pg_sys::List, HashMap<i16, i32>), PlanBuilderError> {
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        let tab = Self::base_scan_table(side).ok_or_else(|| unsupported("join side not a scan"))?;
        if let Some(rt) = &self.cte_runtime {
            if tab.eq_ignore_ascii_case(&rt.name) {
                let rti = rt.rtindex;
                let tlist = self.cte_column_tlist(rti);
                let n = self
                    .expr_ctx
                    .cte_scope
                    .borrow()
                    .as_ref()
                    .map_or(0, |s| s.cols.len());
                let map = (1..=n as i16).map(|i| (i, i32::from(i))).collect();
                return Ok((rti, tlist, map));
            }
        }
        if let Some(d) = self.cte_join_defs.get(&tab.to_lowercase()) {
            let rti = d.rtindex;
            let tlist = self.cte_join_var_tlist(rti, &d.cols);
            let map = (1..=d.cols.len() as i16).map(|i| (i, i32::from(i))).collect();
            return Ok((rti, tlist, map));
        }
        let ident = Self::base_scan_ident(side).unwrap_or(tab);
        let rti = self
            .rtindex_for(ident)
            .or_else(|_| self.rtindex_for(tab))?;
        let reloid = self.rel_oid_for(tab)?;
        let (tlist, map) = self
            .expose_relation_columns(rti, reloid)
            .ok_or_else(|| unsupported("expose join-side columns"))?;
        Ok((rti, tlist, map))
    }

    /// Attempt an index nested-loop join: for `outer JOIN inner ON
    /// outer.X = inner.Y` where `inner` is a single base relation whose btree
    /// index leads with `Y`, and the outer side is restricted by a filter (so
    /// it drives few rows), build a `NestLoop` whose inner is a *parameterized*
    /// `IndexScan` (`Y = $param`) fed from the outer row via `nestParams`. This
    /// matches PostgreSQL's plan for selective joins and avoids hashing the
    /// whole inner relation. Returns `None` (→ caller uses hash/nestloop) for
    /// anything it cannot handle, so it can never produce a wrong plan.
    unsafe fn try_index_nestloop(
        &mut self,
        join_type: JoinType,
        condition: &Expr,
        left: &RelExpr,
        right: &RelExpr,
        where_pred: Option<&Expr>,
        out_columns: Option<&[ProjectionColumn]>,
    ) -> Result<Option<(*mut pg_sys::Plan, JoinColMap, *mut pg_sys::List)>, PlanBuilderError> {
        use ra_core::expr::{BinOp, Expr as RaExpr};
        if !matches!(join_type, JoinType::Inner) {
            return Ok(None);
        }
        let RaExpr::BinOp { op: BinOp::Eq, left: cl, right: cr } = condition else {
            return Ok(None);
        };
        let (RaExpr::Column(ca), RaExpr::Column(cb)) = (&**cl, &**cr) else {
            return Ok(None);
        };
        // Inner must be a single base relation; only drive an index NLJ when
        // the outer is filtered (small driving side — where index NLJ wins).
        let (Some(inner_tbl), Some(outer_tbl)) =
            (single_base_table(right), single_base_table(left))
        else {
            return Ok(None);
        };
        if !subtree_has_filter(left) {
            return Ok(None);
        }
        let inner_alias =
            ra_engine::plan_advice_physical::inner_join_alias(right).unwrap_or(inner_tbl.clone());
        let outer_alias =
            ra_engine::plan_advice_physical::inner_join_alias(left).unwrap_or(outer_tbl.clone());
        let belongs = |c: &ra_core::expr::ColumnRef, al: &str| {
            c.table.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(al))
        };
        let (outer_col, inner_col) = if belongs(cb, &inner_alias) && belongs(ca, &outer_alias) {
            (ca, cb)
        } else if belongs(ca, &inner_alias) && belongs(cb, &outer_alias) {
            (cb, ca)
        } else {
            return Ok(None);
        };
        let (Ok(inner_oid), Ok(inner_rti), Ok(outer_oid)) = (
            self.rel_oid_for(&inner_tbl),
            self.rtindex_for(&inner_tbl),
            self.rel_oid_for(&outer_tbl),
        ) else {
            return Ok(None);
        };
        let Some(info) = crate::index_resolver::resolve_index(inner_oid, &inner_col.column) else {
            return Ok(None);
        };
        if info.am_type != "btree"
            || info.columns.first().is_none_or(|c| !c.eq_ignore_ascii_case(&inner_col.column))
        {
            return Ok(None);
        }
        let inner_attno = pg_sys::get_attnum(
            inner_oid,
            match CString::new(inner_col.column.as_str()) {
                Ok(c) => c,
                Err(_) => return Ok(None),
            }
            .as_ptr(),
        );
        let outer_attno = pg_sys::get_attnum(
            outer_oid,
            match CString::new(outer_col.column.as_str()) {
                Ok(c) => c,
                Err(_) => return Ok(None),
            }
            .as_ptr(),
        );
        if inner_attno <= 0 || outer_attno <= 0 {
            return Ok(None);
        }
        let inner_type = pg_sys::get_atttype(inner_oid, inner_attno);
        let outer_type = pg_sys::get_atttype(outer_oid, outer_attno);
        // Same-type equi-join only (avoid index-side coercion).
        if inner_type == pg_sys::InvalidOid || inner_type != outer_type {
            return Ok(None);
        }
        // `=` operator + its btree strategy in the index's leading opfamily.
        let eq_cstr = match CString::new("=") {
            Ok(c) => c,
            Err(_) => return Ok(None),
        };
        let opname = pg_sys::lappend(
            std::ptr::null_mut(),
            pg_sys::makeString(eq_cstr.as_ptr().cast_mut()).cast(),
        );
        let eq_op = pg_sys::OpernameGetOprid(opname, inner_type, inner_type);
        if eq_op == pg_sys::InvalidOid {
            return Ok(None);
        }
        let idx_rel = pg_sys::index_open(info.oid, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
        if idx_rel.is_null() || (*idx_rel).rd_opfamily.is_null() {
            if !idx_rel.is_null() {
                pg_sys::index_close(idx_rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
            }
            return Ok(None);
        }
        let opfamily = *(*idx_rel).rd_opfamily;
        pg_sys::index_close(idx_rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
        if pg_sys::get_op_opfamily_strategy(eq_op, opfamily)
            != pg_sys::BTEqualStrategyNumber as i32
        {
            return Ok(None);
        }

        // ---- Build the outer side and locate the outer key's output column ----
        let (lplan, lmap, ltl) = self.build_join_tree(left)?;
        let Some(&outer_pos) = lmap.get(&(self.rtindex_for(&outer_tbl)? as i32, outer_attno))
        else {
            return Ok(None);
        };

        // ---- Parameter carrying outer.X into the inner index scan ----
        let pid = self.alloc_param(inner_type);
        let mk_param = || {
            let p = self.alloc_node::<pg_sys::Param>();
            (*p).xpr.type_ = pg_sys::NodeTag::T_Param;
            (*p).paramkind = pg_sys::ParamKind::PARAM_EXEC;
            (*p).paramid = pid;
            (*p).paramtype = inner_type;
            (*p).paramtypmod = -1;
            (*p).paramcollid = pg_sys::get_typcollation(inner_type);
            (*p).location = -1;
            p
        };
        let mk_eq = |key_varno: i32, key_attno: i16, rhs: *mut pg_sys::Node| {
            let key = self.alloc_node::<pg_sys::Var>();
            (*key).xpr.type_ = pg_sys::NodeTag::T_Var;
            (*key).varno = key_varno;
            (*key).varattno = key_attno;
            (*key).vartype = inner_type;
            (*key).vartypmod = -1;
            (*key).varcollid = pg_sys::get_typcollation(inner_type);
            (*key).varlevelsup = 0;
            let op = self.alloc_node::<pg_sys::OpExpr>();
            (*op).xpr.type_ = pg_sys::NodeTag::T_OpExpr;
            (*op).opno = eq_op;
            (*op).opfuncid = pg_sys::get_opcode(eq_op);
            (*op).opresulttype = pg_sys::BOOLOID;
            (*op).opretset = false;
            (*op).inputcollid = (*key).varcollid;
            (*op).args = list2(key.cast(), rhs);
            (*op).location = -1;
            op
        };
        // indexqual uses INDEX_VAR at the index's leading attno (1); indexqualorig
        // uses the heap Var (for EXPLAIN / recheck). Both compare to $param.
        let idx_op = mk_eq(pg_sys::INDEX_VAR, 1, mk_param().cast());
        let orig_op = mk_eq(inner_rti as i32, inner_attno, mk_param().cast());

        // Inner index scan exposing all the inner relation's columns.
        let (inner_tl, inner_attmap) = self
            .expose_relation_columns(inner_rti, inner_oid)
            .ok_or_else(|| PlanBuilderError::UnsupportedVariant("index NLJ inner cols".to_owned()))?;
        let mut rmap: JoinColMap = JoinColMap::new();
        for (attno, pos) in inner_attmap {
            rmap.insert((inner_rti as i32, attno), pos);
        }
        let iscan = self.alloc_node::<pg_sys::IndexScan>();
        (*iscan).scan.plan.type_ = pg_sys::NodeTag::T_IndexScan;
        (*iscan).scan.scanrelid = inner_rti;
        (*iscan).scan.plan.targetlist = inner_tl;
        (*iscan).indexid = info.oid;
        (*iscan).indexqual = pg_sys::lappend(std::ptr::null_mut(), idx_op.cast());
        (*iscan).indexqualorig = pg_sys::lappend(std::ptr::null_mut(), orig_op.cast());
        (*iscan).indexorderdir = pg_sys::ScanDirection::ForwardScanDirection;
        self.set_index_costs(&mut (*iscan).scan.plan, &inner_tbl, 0.0);

        // ---- Output targetlist (projected or full passthrough) ----
        let (out_tl, out_map) = if let Some(cols) = out_columns {
            let mut tlist: *mut pg_sys::List = std::ptr::null_mut();
            for (i, pc) in cols.iter().enumerate() {
                let e = expr_translator::translate(&pc.expr, &self.expr_ctx);
                if e.is_null() || !self.remap_join_vars(e.cast(), &lmap, &rmap) {
                    return Ok(None);
                }
                let rn = pc
                    .alias
                    .as_deref()
                    .or_else(|| crate::sort_utils::extract_column_name(&pc.expr))
                    .and_then(|n| CString::new(n).ok())
                    .map_or(std::ptr::null_mut(), |c| pg_sys::pstrdup(c.as_ptr()));
                let te = pg_sys::makeTargetEntry(e, (i + 1) as i16, rn, false);
                tlist = pg_sys::lappend(tlist, te.cast());
            }
            (tlist, JoinColMap::new())
        } else {
            self.concat_join_tlist(&lmap, ltl, &rmap, inner_tl)
        };

        // Optional residual WHERE (remapped to OUTER/INNER) on the join node.
        let mut where_q = std::ptr::null_mut::<pg_sys::Node>();
        if let Some(w) = where_pred {
            let q = expr_translator::translate(w, &self.expr_ctx);
            if q.is_null() || !self.remap_join_vars(q.cast(), &lmap, &rmap) {
                return Ok(None);
            }
            where_q = q.cast();
        }

        // ---- NestLoop with nestParams: outer.X -> $param ----
        let nlp = self.alloc_node::<pg_sys::NestLoopParam>();
        (*nlp).type_ = pg_sys::NodeTag::T_NestLoopParam;
        (*nlp).paramno = pid;
        let pv = self.alloc_node::<pg_sys::Var>();
        (*pv).xpr.type_ = pg_sys::NodeTag::T_Var;
        (*pv).varno = pg_sys::OUTER_VAR;
        (*pv).varattno = outer_pos as i16;
        (*pv).vartype = outer_type;
        (*pv).vartypmod = -1;
        (*pv).varcollid = pg_sys::get_typcollation(outer_type);
        (*pv).varlevelsup = 0;
        (*nlp).paramval = pv;

        let node = self.alloc_node::<pg_sys::NestLoop>();
        (*node).join.plan.type_ = pg_sys::NodeTag::T_NestLoop;
        (*node).join.jointype = ra_join_type_to_pg(join_type);
        (*node).join.plan.lefttree = lplan;
        (*node).join.plan.righttree = &mut (*iscan).scan.plan as *mut pg_sys::Plan;
        // The equality is enforced by the parameterized index scan, so no joinqual.
        (*node).nestParams = pg_sys::lappend(std::ptr::null_mut(), nlp.cast());
        let join_plan = &mut (*node).join.plan as *mut pg_sys::Plan;
        if !where_q.is_null() {
            (*join_plan).qual = pg_sys::lappend((*join_plan).qual, where_q.cast());
        }
        (*join_plan).targetlist = out_tl;
        self.propagate_costs_binary(&mut *join_plan, lplan, &mut (*iscan).scan.plan);
        Ok(Some((join_plan, out_map, out_tl)))
    }

    /// Split an `Expr` on top-level AND into conjuncts.
    fn and_split_expr<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
        if let Expr::BinOp { op: ra_core::expr::BinOp::And, left, right } = e {
            Self::and_split_expr(left, out);
            Self::and_split_expr(right, out);
        } else {
            out.push(e);
        }
    }

    /// True for a constant that is always true (`true` / non-zero int).
    fn expr_trivially_true(e: &Expr) -> bool {
        matches!(e, Expr::Const(ra_core::expr::Const::Bool(true)))
            || matches!(e, Expr::Const(ra_core::expr::Const::Int(n)) if *n != 0)
    }

    /// True if every column referenced by `e` is qualified with `alias` or
    /// `tbl` (and there is at least one column) — i.e. `e` depends only on the
    /// given outer relation.
    fn expr_only_qualifies(e: &Expr, alias: &str, tbl: &str) -> bool {
        let probe = RelExpr::Filter {
            predicate: e.clone(),
            input: Box::new(RelExpr::Scan { table: "__probe".to_owned(), alias: None }),
        };
        let cols = probe.referenced_columns();
        !cols.is_empty()
            && cols.iter().all(|c| {
                c.table
                    .as_deref()
                    .is_some_and(|t| t.eq_ignore_ascii_case(alias) || t.eq_ignore_ascii_case(tbl))
            })
    }

    /// Build a correlated LATERAL join `outer JOIN LATERAL (inner) t ON true`
    /// (where `inner` references outer columns) as a parameterized `NestLoop`:
    /// each referenced outer column becomes a PARAM_EXEC parameter resolved
    /// inside the inner, and `nestParams` feed the outer values in. The inner
    /// is built directly (no SubqueryScan wrapper — matching PostgreSQL's own
    /// plan for this shape). Returns `None` for anything it cannot handle.
    unsafe fn try_correlated_lateral(
        &mut self,
        join_type: JoinType,
        condition: &Expr,
        left: &RelExpr,
        right: &RelExpr,
        where_pred: Option<&Expr>,
        out_columns: Option<&[ProjectionColumn]>,
    ) -> Result<Option<(*mut pg_sys::Plan, JoinColMap, *mut pg_sys::List)>, PlanBuilderError> {
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        if !matches!(join_type, JoinType::Inner) {
            return Ok(None);
        }
        let Some(out_cols) = out_columns else {
            return Ok(None);
        };
        let Some(outer_tbl) = single_base_table(left) else {
            return Ok(None);
        };
        let outer_alias =
            ra_engine::plan_advice_physical::inner_join_alias(left).unwrap_or(outer_tbl.clone());

        // A lateral's ON condition is logically `true`, but the optimizer may
        // push outer-only WHERE predicates into it (e.g. `true AND
        // o.o_orderkey < 30`). Split it: drop trivially-true conjuncts, fold
        // outer-only conjuncts into the effective outer filter, and defer if
        // any conjunct references the lateral output (not a plain `ON true`).
        let mut cond_conjuncts: Vec<&Expr> = Vec::new();
        Self::and_split_expr(condition, &mut cond_conjuncts);
        let mut cond_outer_filters: Vec<Expr> = Vec::new();
        for c in cond_conjuncts {
            if Self::expr_trivially_true(c) {
                continue;
            }
            if Self::expr_only_qualifies(c, &outer_alias, &outer_tbl) {
                cond_outer_filters.push(c.clone());
            } else {
                return Ok(None);
            }
        }
        // Effective outer filter = the original WHERE plus any outer-only
        // predicates the optimizer pushed into the ON condition.
        let combined_where: Option<Expr> = {
            let mut parts: Vec<Expr> = Vec::new();
            if let Some(w) = where_pred {
                parts.push(w.clone());
            }
            parts.extend(cond_outer_filters);
            let mut iter = parts.into_iter();
            iter.next().map(|first| {
                iter.fold(first, |acc, p| Expr::BinOp {
                    op: ra_core::expr::BinOp::And,
                    left: Box::new(acc),
                    right: Box::new(p),
                })
            })
        };
        let where_pred: Option<&Expr> = combined_where.as_ref();

        // Correlation columns: outer-qualified columns referenced in the inner.
        let allcols: Vec<ra_core::expr::ColumnRef> = right.referenced_columns();
        let mut corr: Vec<ra_core::expr::ColumnRef> = Vec::new();
        for c in allcols {
            let is_outer = c.table.as_deref().is_some_and(|t| {
                t.eq_ignore_ascii_case(&outer_alias) || t.eq_ignore_ascii_case(&outer_tbl)
            });
            if is_outer
                && !corr
                    .iter()
                    .any(|e: &ra_core::expr::ColumnRef| e.column.eq_ignore_ascii_case(&c.column))
            {
                corr.push(c);
            }
        }
        if corr.is_empty() {
            return Ok(None);
        }
        let Some(t_rtindex) = self.first_subquery_rtindex() else {
            return Ok(None);
        };
        let Some(t_alias) = self.rte_alias_name(t_rtindex) else {
            return Ok(None);
        };

        // Build the outer; locate each correlation key's output position.
        // Push an outer-only WHERE onto the outer input so the lateral inner is
        // evaluated only for surviving outer rows (matching PG's filter
        // pushdown); a WHERE that also references the lateral output must stay
        // as the join qual.
        let where_outer_only = match where_pred {
            None => true,
            Some(w) => {
                let probe = RelExpr::Filter {
                    predicate: w.clone(),
                    input: Box::new(RelExpr::Scan {
                        table: "__probe".to_owned(),
                        alias: None,
                    }),
                };
                !probe.referenced_columns().iter().any(|c| {
                    c.table.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(&t_alias))
                })
            }
        };
        let pushed_outer;
        let outer_to_build: &RelExpr = match (where_outer_only, where_pred) {
            (true, Some(w)) => {
                pushed_outer = RelExpr::Filter {
                    predicate: w.clone(),
                    input: Box::new(left.clone()),
                };
                &pushed_outer
            }
            _ => left,
        };
        let (lplan, lmap, _ltl) = self.build_join_tree(outer_to_build)?;
        let (Ok(outer_oid), Ok(outer_rti)) =
            (self.rel_oid_for(&outer_tbl), self.rtindex_for(&outer_tbl))
        else {
            return Ok(None);
        };
        let mut nest_params: *mut pg_sys::List = std::ptr::null_mut();
        let mut corr_pids: Vec<i32> = Vec::new();
        for c in &corr {
            let cname = match CString::new(c.column.as_str()) {
                Ok(s) => s,
                Err(_) => return Ok(None),
            };
            let attno = pg_sys::get_attnum(outer_oid, cname.as_ptr());
            if attno <= 0 {
                return Ok(None);
            }
            let typ = pg_sys::get_atttype(outer_oid, attno);
            let Some(&outer_pos) = lmap.get(&(outer_rti as i32, attno)) else {
                return Ok(None);
            };
            let pid = self.alloc_param(typ);
            corr_pids.push(pid);
            let coll = pg_sys::get_typcollation(typ);
            self.expr_ctx.correlation_scope.borrow_mut().insert(
                (outer_alias.to_lowercase(), c.column.to_lowercase()),
                crate::expr_translator::CorrParam { paramid: pid, typ, typmod: -1, coll },
            );
            let nlp = self.alloc_node::<pg_sys::NestLoopParam>();
            (*nlp).type_ = pg_sys::NodeTag::T_NestLoopParam;
            (*nlp).paramno = pid;
            let pv = self.alloc_node::<pg_sys::Var>();
            (*pv).xpr.type_ = pg_sys::NodeTag::T_Var;
            (*pv).varno = pg_sys::OUTER_VAR;
            (*pv).varattno = outer_pos as i16;
            (*pv).vartype = typ;
            (*pv).vartypmod = -1;
            (*pv).varcollid = coll;
            (*pv).varlevelsup = 0;
            (*nlp).paramval = pv;
            nest_params = pg_sys::lappend(nest_params, nlp.cast());
        }

        // Build the inner directly (correlation cols resolve to Params). No
        // SubqueryScan wrapper: PostgreSQL puts the Aggregate straight under
        // the nested loop, and an extra hand-built SubqueryScan layer risks a
        // tuple-descriptor / rescan inconsistency.
        let inner_result = self.build_plan(right);
        self.expr_ctx.correlation_scope.borrow_mut().clear();
        let rplan = inner_result?;
        if rplan.is_null() || (*rplan).targetlist.is_null() {
            return Err(unsupported("lateral inner subplan"));
        }
        // Mark the inner as param-dependent so the nested loop rescans it.
        Self::mark_param_deps(rplan, &corr_pids);

        // The lateral alias resolves its output columns to the inner by
        // position (rmap keyed on a synthetic t_rtindex).
        let cols = Self::tlist_subquery_cols((*rplan).targetlist);
        let mut rmap: JoinColMap = JoinColMap::new();
        for i in 0..cols.len() {
            rmap.insert((t_rtindex as i32, (i + 1) as i16), (i + 1) as i32);
        }
        let scope_cols: Vec<CteCol> = cols
            .iter()
            .map(|c| CteCol { name: c.name.clone(), typ: c.typ, typmod: c.typmod, coll: c.coll })
            .collect();
        self.expr_ctx.cte_join_scope.borrow_mut().insert(
            t_alias.to_lowercase(),
            CteScope { name: t_alias.to_lowercase(), rtindex: t_rtindex, cols: scope_cols },
        );

        // Output targetlist (projected, remapped to OUTER/INNER).
        let cleanup = |s: &Self| {
            s.expr_ctx.cte_join_scope.borrow_mut().remove(&t_alias.to_lowercase());
        };
        let mut out_tl: *mut pg_sys::List = std::ptr::null_mut();
        for (i, pc) in out_cols.iter().enumerate() {
            let e = expr_translator::translate(&pc.expr, &self.expr_ctx);
            if e.is_null() || !self.remap_join_vars(e.cast(), &lmap, &rmap) {
                cleanup(self);
                return Ok(None);
            }
            let rn = pc
                .alias
                .as_deref()
                .or_else(|| crate::sort_utils::extract_column_name(&pc.expr))
                .and_then(|n| CString::new(n).ok())
                .map_or(std::ptr::null_mut(), |c| pg_sys::pstrdup(c.as_ptr()));
            let te = pg_sys::makeTargetEntry(e, (i + 1) as i16, rn, false);
            out_tl = pg_sys::lappend(out_tl, te.cast());
        }
        let mut where_q = std::ptr::null_mut::<pg_sys::Node>();
        if let Some(w) = where_pred {
            if !where_outer_only {
                let q = expr_translator::translate(w, &self.expr_ctx);
                if q.is_null() || !self.remap_join_vars(q.cast(), &lmap, &rmap) {
                    cleanup(self);
                    return Ok(None);
                }
                where_q = q.cast();
            }
        }
        cleanup(self);

        let node = self.alloc_node::<pg_sys::NestLoop>();
        (*node).join.plan.type_ = pg_sys::NodeTag::T_NestLoop;
        (*node).join.jointype = ra_join_type_to_pg(join_type);
        (*node).join.plan.lefttree = lplan;
        (*node).join.plan.righttree = rplan;
        (*node).nestParams = nest_params;
        let join_plan = &mut (*node).join.plan as *mut pg_sys::Plan;
        if !where_q.is_null() {
            (*join_plan).qual = pg_sys::lappend((*join_plan).qual, where_q.cast());
        }
        (*join_plan).targetlist = out_tl;
        self.propagate_costs_binary(&mut *join_plan, lplan, rplan);
        Ok(Some((join_plan, JoinColMap::new(), out_tl)))
    }

    /// Split a (translated) qual `Node` on top-level AND into its conjuncts.
    unsafe fn and_split(node: *mut pg_sys::Node, out: &mut Vec<*mut pg_sys::Node>) {
        if node.is_null() {
            return;
        }
        if (*node).type_ == pg_sys::NodeTag::T_BoolExpr {
            let b = node.cast::<pg_sys::BoolExpr>();
            if (*b).boolop == pg_sys::BoolExprType::AND_EXPR {
                let args = (*b).args;
                let e = (*args).elements;
                for i in 0..(*args).length {
                    Self::and_split((*e.add(i as usize)).ptr_value.cast(), out);
                }
                return;
            }
        }
        out.push(node);
    }

    /// Record whether an expression references OUTER_VAR / INNER_VAR Vars.
    /// Unknown node kinds set both (conservatively "mixed"), so they are never
    /// mistaken for a clean single-sided hash key.
    unsafe fn collect_sides(node: *mut pg_sys::Node, has_outer: &mut bool, has_inner: &mut bool) {
        if node.is_null() {
            return;
        }
        match (*node).type_ {
            pg_sys::NodeTag::T_Var => {
                let vno = (*node.cast::<pg_sys::Var>()).varno;
                if vno == pg_sys::OUTER_VAR as i32 {
                    *has_outer = true;
                } else if vno == pg_sys::INNER_VAR as i32 {
                    *has_inner = true;
                } else {
                    *has_outer = true;
                    *has_inner = true;
                }
            }
            pg_sys::NodeTag::T_Const | pg_sys::NodeTag::T_Param => {}
            pg_sys::NodeTag::T_RelabelType => {
                Self::collect_sides((*node.cast::<pg_sys::RelabelType>()).arg.cast(), has_outer, has_inner);
            }
            pg_sys::NodeTag::T_OpExpr => {
                Self::collect_list_sides((*node.cast::<pg_sys::OpExpr>()).args, has_outer, has_inner);
            }
            pg_sys::NodeTag::T_FuncExpr => {
                Self::collect_list_sides((*node.cast::<pg_sys::FuncExpr>()).args, has_outer, has_inner);
            }
            pg_sys::NodeTag::T_BoolExpr => {
                Self::collect_list_sides((*node.cast::<pg_sys::BoolExpr>()).args, has_outer, has_inner);
            }
            _ => {
                *has_outer = true;
                *has_inner = true;
            }
        }
    }

    unsafe fn collect_list_sides(list: *mut pg_sys::List, has_outer: &mut bool, has_inner: &mut bool) {
        if list.is_null() {
            return;
        }
        let e = (*list).elements;
        for i in 0..(*list).length {
            Self::collect_sides((*e.add(i as usize)).ptr_value.cast(), has_outer, has_inner);
        }
    }

    /// True if `node` is a hashable equality `OpExpr` whose two arguments are
    /// each purely from one join side (one OUTER, one INNER) — i.e. a usable
    /// hash-join clause.
    unsafe fn is_hash_equi_clause(node: *mut pg_sys::Node) -> bool {
        if node.is_null() || (*node).type_ != pg_sys::NodeTag::T_OpExpr {
            return false;
        }
        let op = node.cast::<pg_sys::OpExpr>();
        if pg_sys::list_length((*op).args) != 2 {
            return false;
        }
        let a0 = pg_sys::list_nth((*op).args, 0).cast::<pg_sys::Node>();
        let a1 = pg_sys::list_nth((*op).args, 1).cast::<pg_sys::Node>();
        if !pg_sys::op_hashjoinable((*op).opno, pg_sys::exprType(a0)) {
            return false;
        }
        let (mut o0, mut i0, mut o1, mut i1) = (false, false, false, false);
        Self::collect_sides(a0, &mut o0, &mut i0);
        Self::collect_sides(a1, &mut o1, &mut i1);
        let pure_outer = |o: bool, i: bool| o && !i;
        let pure_inner = |o: bool, i: bool| i && !o;
        (pure_outer(o0, i0) && pure_inner(o1, i1)) || (pure_inner(o0, i0) && pure_outer(o1, i1))
    }

    /// Build one `NestLoop` join node over two (possibly nested) join inputs,
    /// with the ON `condition` as joinqual and an optional `where_pred` as the
    /// post-join `plan.qual` (both remapped to the children's OUTER/INNER
    /// frame). Returns the node, its full-passthrough output
    /// `(rtindex,attno)->position` map, and output targetlist.
    unsafe fn build_join_node(
        &mut self,
        join_type: JoinType,
        condition: &Expr,
        left: &RelExpr,
        right: &RelExpr,
        where_pred: Option<&Expr>,
        out_columns: Option<&[ProjectionColumn]>,
    ) -> Result<(*mut pg_sys::Plan, JoinColMap, *mut pg_sys::List), PlanBuilderError> {
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        match join_type {
            JoinType::Inner | JoinType::LeftOuter | JoinType::Cross => {}
            JoinType::FullOuter | JoinType::RightOuter | JoinType::Semi | JoinType::Anti
                if out_columns.is_some() => {}
            _ => return Err(unsupported("nested join type")),
        }
        // A correlated LATERAL inner is built as a parameterized nested loop.
        if let Some(r) = self
            .try_correlated_lateral(join_type, condition, left, right, where_pred, out_columns)?
        {
            return Ok(r);
        }
        // Prefer an index nested-loop when the inner is indexed on the join key
        // and the outer is filtered (selective). Falls through to hash/nestloop
        // when not applicable.
        if let Some(r) =
            self.try_index_nestloop(join_type, condition, left, right, where_pred, out_columns)?
        {
            return Ok(r);
        }
        let (lplan, lmap, _) = self.build_join_tree(left)?;
        let (rplan, rmap, _) = self.build_join_tree(right)?;

        // Translate + remap (to OUTER/INNER) the ON condition and any WHERE.
        let mut cond_q = std::ptr::null_mut::<pg_sys::Node>();
        if !matches!(condition, Expr::Const(ra_core::expr::Const::Bool(true))) {
            let q = expr_translator::translate(condition, &self.expr_ctx);
            if q.is_null() || !self.remap_join_vars(q.cast(), &lmap, &rmap) {
                return Err(unsupported("join condition"));
            }
            cond_q = q.cast();
        }
        let mut where_q = std::ptr::null_mut::<pg_sys::Node>();
        if let Some(w) = where_pred {
            let q = expr_translator::translate(w, &self.expr_ctx);
            if q.is_null() || !self.remap_join_vars(q.cast(), &lmap, &rmap) {
                return Err(unsupported("join WHERE predicate"));
            }
            where_q = q.cast();
        }

        // Output targetlist: a custom projection (top of a SELECT) remaps the
        // chosen columns to the OUTER/INNER frame; a nested join exposes every
        // base column (full passthrough) so a parent join can reference it.
        let (out_tl, out_map) = if let Some(cols) = out_columns {
            let mut tlist: *mut pg_sys::List = std::ptr::null_mut();
            for (i, pc) in cols.iter().enumerate() {
                let e = expr_translator::translate(&pc.expr, &self.expr_ctx);
                if e.is_null() || !self.remap_join_vars(e.cast(), &lmap, &rmap) {
                    return Err(unsupported("join output column"));
                }
                let rn = pc
                    .alias
                    .as_deref()
                    .or_else(|| crate::sort_utils::extract_column_name(&pc.expr))
                    .and_then(|n| CString::new(n).ok())
                    .map_or(std::ptr::null_mut(), |c| pg_sys::pstrdup(c.as_ptr()));
                let te = pg_sys::makeTargetEntry(e, (i + 1) as i16, rn, false);
                tlist = pg_sys::lappend(tlist, te.cast());
            }
            (tlist, JoinColMap::new())
        } else {
            self.concat_join_tlist(&lmap, (*lplan).targetlist, &rmap, (*rplan).targetlist)
        };

        // Pick the join method. An equi-join on a single hashable `=` OpExpr
        // uses a HashJoin (a NestLoop is O(n*m) — unusable on large inputs and
        // impossible for FULL/RIGHT). Cross and non-equi joins use NestLoop;
        // FULL/RIGHT without a hashable condition defer to PG.
        // Pick the join method. Feasibility (is the condition a hashable `=`?)
        // needs the PG catalog and is decided here; the hash-vs-nestloop
        // *preference* is a cost decision made in layer 2 (ra-engine) and
        // carried on `physical_choices`, keyed by the inner relation's alias.
        // This builder only renders that choice, falling back to "hash when
        // feasible" when layer 2 expressed no preference.
        // Split the join condition into hashable equi-clauses (each side from
        // one relation) and a residual. Any equi-clause makes this a hash join;
        // the residual becomes the join's other quals. A bare single OpExpr and
        // an AND of (equi + filter) — what subquery decorrelation produces for
        // semi-joins — both yield hash clauses here.
        let mut conjuncts: Vec<*mut pg_sys::Node> = Vec::new();
        if !cond_q.is_null() {
            Self::and_split(cond_q, &mut conjuncts);
        }
        let mut hash_ops: Vec<*mut pg_sys::OpExpr> = Vec::new();
        let mut residual: Vec<*mut pg_sys::Node> = Vec::new();
        for c in conjuncts {
            if Self::is_hash_equi_clause(c) {
                hash_ops.push(c.cast());
            } else {
                residual.push(c);
            }
        }
        let is_equi = !hash_ops.is_empty();
        let needs_hash = matches!(join_type, JoinType::FullOuter | JoinType::RightOuter);
        if needs_hash && !is_equi {
            return Err(unsupported("full/right join needs a hashable equi-condition"));
        }
        let cost_prefers_nestloop = ra_engine::plan_advice_physical::inner_join_alias(right)
            .and_then(|a| self.physical_choices.join_for(&a).cloned())
            .is_some_and(|s| {
                use ra_engine::plan_advice_physical::JoinInnerStrategy::{
                    NestedLoopMaterialize, NestedLoopMemoize, NestedLoopPlain,
                };
                matches!(s, NestedLoopPlain | NestedLoopMaterialize | NestedLoopMemoize)
            });
        let use_hash = is_equi
            && !matches!(join_type, JoinType::Cross)
            && (needs_hash || !cost_prefers_nestloop);
        let join_plan = if use_hash {
            let node = self.alloc_node::<pg_sys::HashJoin>();
            if node.is_null() {
                return Err(PlanBuilderError::NullPointer("HashJoin".to_owned()));
            }
            let hash = self.alloc_node::<pg_sys::Hash>();
            if hash.is_null() {
                return Err(PlanBuilderError::NullPointer("Hash".to_owned()));
            }
            (*hash).plan.type_ = pg_sys::NodeTag::T_Hash;
            (*hash).plan.lefttree = rplan;
            (*hash).plan.targetlist = (*rplan).targetlist;
            // The executor sizes the hash table from the Hash node's own
            // plan_rows/plan_width (ExecChooseHashTableSize). Without the
            // build-side estimate it allocates a single bucket, collapsing the
            // probe into an O(n*m) chain walk. Propagate the build child's
            // cardinality so the table is sized correctly.
            (*hash).plan.plan_rows = (*rplan).plan_rows;
            (*hash).plan.plan_width = (*rplan).plan_width;
            (*hash).plan.startup_cost = (*rplan).total_cost;
            (*hash).plan.total_cost = (*rplan).total_cost;
            (*node).join.plan.type_ = pg_sys::NodeTag::T_HashJoin;
            (*node).join.jointype = ra_join_type_to_pg(join_type);
            (*node).join.plan.lefttree = lplan;
            (*node).join.plan.righttree = &mut (*hash).plan as *mut pg_sys::Plan;
            // Wire every hashable equi-clause: hashclauses (per-match recheck),
            // hashoperators/hashcollations, the outer hash keys (HashJoin) and
            // the inner hash keys (Hash node). Without per-key wiring every row
            // hashes to one bucket -> O(n*m) (correct via recheck, but slow).
            let is_inner = |n: *mut pg_sys::Node| {
                !n.is_null()
                    && (*n).type_ == pg_sys::NodeTag::T_Var
                    && (*n.cast::<pg_sys::Var>()).varno == pg_sys::INNER_VAR as i32
            };
            // A purely-inner argument (may be an expression, not just a Var).
            let arg_is_inner = |n: *mut pg_sys::Node| {
                let (mut o, mut i) = (false, false);
                Self::collect_sides(n, &mut o, &mut i);
                i && !o
            };
            for op in &hash_ops {
                let op = *op;
                let a0 = pg_sys::list_nth((*op).args, 0).cast::<pg_sys::Node>();
                let a1 = pg_sys::list_nth((*op).args, 1).cast::<pg_sys::Node>();
                let (outer_key, inner_key) = if arg_is_inner(a0) { (a1, a0) } else { (a0, a1) };
                (*node).hashclauses = pg_sys::lappend((*node).hashclauses, op.cast());
                (*node).hashoperators = pg_sys::lappend_oid((*node).hashoperators, (*op).opno);
                (*node).hashcollations =
                    pg_sys::lappend_oid((*node).hashcollations, (*op).inputcollid);
                (*node).hashkeys = pg_sys::lappend((*node).hashkeys, outer_key.cast());
                // ExecHash evaluates the Hash node's hashkeys against its child
                // tuple, which sits in the *outer* slot of the Hash's expr
                // context — rewrite INNER_VAR -> OUTER_VAR (same attno) on a
                // copy, so the shared recheck clause is not corrupted.
                let hk = if is_inner(inner_key) {
                    let v = self.alloc_node::<pg_sys::Var>();
                    *v = *inner_key.cast::<pg_sys::Var>();
                    (*v).varno = pg_sys::OUTER_VAR as i32;
                    v.cast::<pg_sys::Node>()
                } else {
                    inner_key
                };
                (*hash).hashkeys = pg_sys::lappend((*hash).hashkeys, hk.cast());
            }
            // Residual (non-hash) conditions become the join's other quals.
            for r in &residual {
                (*node).join.joinqual = pg_sys::lappend((*node).join.joinqual, (*r).cast());
            }
            &mut (*node).join.plan as *mut pg_sys::Plan
        } else {
            let node = self.alloc_node::<pg_sys::NestLoop>();
            if node.is_null() {
                return Err(PlanBuilderError::NullPointer("NestLoop".to_owned()));
            }
            (*node).join.plan.type_ = pg_sys::NodeTag::T_NestLoop;
            (*node).join.jointype = ra_join_type_to_pg(join_type);
            (*node).join.plan.lefttree = lplan;
            (*node).join.plan.righttree = rplan;
            if !cond_q.is_null() {
                (*node).join.joinqual = pg_sys::lappend(std::ptr::null_mut(), cond_q.cast());
            }
            &mut (*node).join.plan as *mut pg_sys::Plan
        };
        if !where_q.is_null() {
            (*join_plan).qual = pg_sys::lappend((*join_plan).qual, where_q.cast());
        }
        (*join_plan).targetlist = out_tl;
        self.propagate_costs_binary(&mut *join_plan, lplan, rplan);
        Ok((join_plan, out_map, out_tl))
    }

    /// Build a join input that may be a single base relation OR a (possibly
    /// nested) join. Returns the plan, a `(rtindex, attno) -> output-position`
    /// map, and the full passthrough output targetlist so a parent join can
    /// reference every base column. Recurses for nested joins, emitting
    /// `OUTER_VAR`/`INNER_VAR` refs at each level (Ra bypasses setrefs).
    unsafe fn build_join_tree(
        &mut self,
        expr: &RelExpr,
    ) -> Result<(*mut pg_sys::Plan, JoinColMap, *mut pg_sys::List), PlanBuilderError> {
        if let RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } = expr
        {
            self.build_join_node(*join_type, condition, left, right, None, None)
        } else {
            // Leaf: a single base relation (scan / Filter-over-scan / CTE).
            let (rti, tlist, attmap) = self.join_side_cols(expr)?;
            let plan = self.build_plan(expr)?;
            (*plan).targetlist = tlist;
            let map = attmap
                .into_iter()
                .map(|(att, pos)| ((rti as i32, att), pos))
                .collect();
            Ok((plan, map, tlist))
        }
    }

    /// Build a join node's output targetlist (left child columns as
    /// `OUTER_VAR` refs, then right child columns as `INNER_VAR` refs) plus
    /// the combined `(rtindex, attno) -> position` map (left positions, then
    /// right positions offset by the left column count).
    unsafe fn concat_join_tlist(
        &self,
        lmap: &JoinColMap,
        ltl: *mut pg_sys::List,
        rmap: &JoinColMap,
        rtl: *mut pg_sys::List,
    ) -> (*mut pg_sys::List, JoinColMap) {
        let llen = pg_sys::list_length(ltl);
        let mut out: *mut pg_sys::List = std::ptr::null_mut();
        for (varno, src, tl, len) in [
            (pg_sys::OUTER_VAR, ltl, ltl, llen),
            (pg_sys::INNER_VAR, rtl, rtl, pg_sys::list_length(rtl)),
        ] {
            let _ = src;
            for p in 1..=len {
                let te = pg_sys::list_nth(tl, p - 1).cast::<pg_sys::TargetEntry>();
                let var = self.alloc_node::<pg_sys::Var>();
                (*var).xpr.type_ = pg_sys::NodeTag::T_Var;
                (*var).varno = varno;
                (*var).varattno = p as i16;
                (*var).vartype = pg_sys::exprType((*te).expr.cast());
                (*var).vartypmod = pg_sys::exprTypmod((*te).expr.cast());
                (*var).varcollid = pg_sys::exprCollation((*te).expr.cast());
                (*var).varlevelsup = 0;
                let resno = (pg_sys::list_length(out) + 1) as i16;
                let nt = pg_sys::makeTargetEntry(var.cast(), resno, (*te).resname, false);
                out = pg_sys::lappend(out, nt.cast());
            }
        }
        let mut map: JoinColMap = HashMap::new();
        for (&k, &pos) in lmap {
            map.insert(k, pos);
        }
        for (&k, &pos) in rmap {
            map.insert(k, llen + pos);
        }
        (out, map)
    }

    /// Build a `Project` over a `Join` (possibly nested, with an optional
    /// WHERE) as a join plan node whose targetlist is the projected columns.
    /// Inner/Left/Full/Right/Semi/Anti and Cross are supported; the method and
    /// Var remapping are handled by `build_join_node`.
    unsafe fn build_projected_join(
        &mut self,
        out_columns: &[ProjectionColumn],
        where_pred: Option<&Expr>,
        join_type: JoinType,
        condition: &Expr,
        left: &RelExpr,
        right: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        self.build_join_node(join_type, condition, left, right, where_pred, Some(out_columns))
            .map(|(p, _, _)| p)
    }

    unsafe fn build_join(
        &mut self,
        join_type: JoinType,
        condition: &Expr,
        left: &RelExpr,
        right: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let left_plan = self.build_plan(left)?;
        let right_plan = self.build_plan(right)?;

        // Translate the join condition to a PostgreSQL expression node.
        // A trivial TRUE condition (cross join) produces a null pg_expr,
        // which is correct — no qual means unconditional join.
        let pg_condition = self.translate_join_condition(condition);

        // Honor join-method advice when the inner-side alias has
        // a JoinInnerStrategy mapped in physical_choices. Cloned
        // off the borrow so the recursion can re-borrow `self`.
        use ra_engine::plan_advice_physical::JoinInnerStrategy;
        let inner_alias = leaf_alias(right);
        let join_strategy = inner_alias
            .as_deref()
            .and_then(|a| self.physical_choices.join_for(a))
            .cloned();
        match (join_type, join_strategy) {
            // Hash join: explicit advice or default for inner / outer joins.
            (
                JoinType::Inner | JoinType::LeftOuter | JoinType::RightOuter | JoinType::FullOuter,
                Some(JoinInnerStrategy::Hash) | None,
            ) => self.build_hash_join(join_type, left_plan, right_plan, pg_condition),

            // Hash advice on a join type that defaults to nestloop.
            // PG allows hash joins on cross/semi/anti when the
            // condition is hashable; we honor the advice and emit
            // a HashJoin.
            (
                JoinType::Cross | JoinType::Semi | JoinType::Anti,
                Some(JoinInnerStrategy::Hash),
            ) => self.build_hash_join(join_type, left_plan, right_plan, pg_condition),

            // Nested-loop variants: explicit advice OR cross/semi/anti default.
            (
                JoinType::Cross | JoinType::Semi | JoinType::Anti,
                None
                | Some(JoinInnerStrategy::NestedLoopPlain)
                | Some(JoinInnerStrategy::NestedLoopMaterialize)
                | Some(JoinInnerStrategy::NestedLoopMemoize),
            ) => self.build_nested_loop(join_type, left_plan, right_plan, pg_condition),

            // Nested-loop advice on a join type that defaults to hash.
            (
                _,
                Some(JoinInnerStrategy::NestedLoopPlain)
                | Some(JoinInnerStrategy::NestedLoopMaterialize)
                | Some(JoinInnerStrategy::NestedLoopMemoize),
            ) => self.build_nested_loop(join_type, left_plan, right_plan, pg_condition),

            // Merge-join advice — emit T_MergeJoin when we can
            // extract a single-clause equi-join with column-ref
            // operands; fall back to hash join otherwise (a true
            // MergeJoin requires sorted inputs and resolved
            // opfamilies; if either lookup fails we'd produce
            // an executor-invalid plan, so degrade gracefully).
            (
                JoinType::Inner | JoinType::LeftOuter | JoinType::RightOuter | JoinType::FullOuter,
                Some(JoinInnerStrategy::MergeJoinPlain)
                | Some(JoinInnerStrategy::MergeJoinMaterialize),
            ) => match self.build_merge_join(
                join_type,
                left,
                right,
                condition,
                left_plan,
                right_plan,
            ) {
                Ok(plan) => Ok(plan),
                Err(reason) => {
                    debug!(
                        inner_alias = ?inner_alias,
                        %reason,
                        "MERGE_JOIN advice could not be honored; falling back to HashJoin",
                    );
                    self.build_hash_join(join_type, left_plan, right_plan, pg_condition)
                }
            },
            (
                _,
                Some(JoinInnerStrategy::MergeJoinPlain)
                | Some(JoinInnerStrategy::MergeJoinMaterialize),
            ) => self.build_nested_loop(join_type, left_plan, right_plan, pg_condition),

            // Foreign-join advice: requires FDW pushdown which the
            // plan-builder doesn't synthesize today.
            (_, Some(JoinInnerStrategy::ForeignJoin)) => {
                debug!(
                    inner_alias = ?inner_alias,
                    "FOREIGN_JOIN advice cannot be honored at this layer; \
                     falling back to HashJoin/NestLoop default",
                );
                match join_type {
                    JoinType::Cross | JoinType::Semi | JoinType::Anti => {
                        self.build_nested_loop(join_type, left_plan, right_plan, pg_condition)
                    }
                    _ => self.build_hash_join(join_type, left_plan, right_plan, pg_condition),
                }
            }
        }
    }

    /// Translate a Ra join condition `Expr` into a PostgreSQL expression node.
    ///
    /// Returns null if the condition is trivial (e.g., `Const(Bool(true))`)
    /// or if translation fails for a complex expression. A null return means
    /// "no join qualification" which is safe (worst case: larger result set).
    unsafe fn translate_join_condition(&self, condition: &Expr) -> *mut pg_sys::Expr {
        // Trivial true condition means unconditional join (cross product).
        if matches!(condition, Expr::Const(ra_core::expr::Const::Bool(true))) {
            return std::ptr::null_mut();
        }

        let pg_expr = expr_translator::translate(condition, &self.expr_ctx);
        if pg_expr.is_null() {
            debug!(
                "join condition translation returned null; \
                 condition will be omitted (unqualified join)"
            );
        }
        pg_expr
    }

    unsafe fn build_hash_join(
        &mut self,
        join_type: JoinType,
        left_plan: *mut pg_sys::Plan,
        right_plan: *mut pg_sys::Plan,
        condition: *mut pg_sys::Expr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let node = self.alloc_node::<pg_sys::HashJoin>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "HashJoin allocation".to_string(),
            ));
        }
        (*node).join.plan.type_ = pg_sys::NodeTag::T_HashJoin;
        (*node).join.jointype = ra_join_type_to_pg(join_type);
        (*node).join.plan.lefttree = left_plan;

        // Wrap right child in a Hash node as required by PostgreSQL's HashJoin.
        let hash_node = self.alloc_node::<pg_sys::Hash>();
        if hash_node.is_null() {
            return Err(PlanBuilderError::NullPointer("Hash allocation".to_string()));
        }
        (*hash_node).plan.type_ = pg_sys::NodeTag::T_Hash;
        (*hash_node).plan.lefttree = right_plan;
        (*node).join.plan.righttree = &mut (*hash_node).plan as *mut pg_sys::Plan;

        // Wire the join condition into hashclauses. PostgreSQL expects
        // hashclauses to contain OpExpr nodes (equality operators) for
        // hash-compatible join conditions. For non-hashable conditions the
        // executor would need a different join strategy, but since Ra's
        // optimizer selected HashJoin it should have ensured equi-join
        // compatibility. If the condition translates to a non-OpExpr
        // (e.g., BoolExpr AND of multiple clauses), we place it in
        // joinqual as a fallback — the executor applies it as a filter
        // after the hash probe.
        if !condition.is_null() {
            if (*condition).type_ == pg_sys::NodeTag::T_OpExpr {
                (*node).hashclauses = pg_sys::lappend((*node).hashclauses, condition.cast());
            } else {
                // Non-OpExpr condition (e.g., AND of multiple conditions):
                // place in joinqual where the executor evaluates it post-match.
                debug!(
                    "HashJoin condition is not a simple OpExpr (tag={:?}); \
                     placing in joinqual instead of hashclauses",
                    (*condition).type_
                );
                (*node).join.joinqual = pg_sys::lappend((*node).join.joinqual, condition.cast());
            }
        }

        self.propagate_costs_binary(&mut (*node).join.plan, left_plan, right_plan);
        Ok(&mut (*node).join.plan as *mut pg_sys::Plan)
    }

    unsafe fn build_nested_loop(
        &mut self,
        join_type: JoinType,
        left_plan: *mut pg_sys::Plan,
        right_plan: *mut pg_sys::Plan,
        condition: *mut pg_sys::Expr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let node = self.alloc_node::<pg_sys::NestLoop>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "NestLoop allocation".to_string(),
            ));
        }
        (*node).join.plan.type_ = pg_sys::NodeTag::T_NestLoop;
        (*node).join.jointype = ra_join_type_to_pg(join_type);
        (*node).join.plan.lefttree = left_plan;
        (*node).join.plan.righttree = right_plan;

        // Wire the join condition into joinqual. The NestLoop executor
        // evaluates joinqual for every (outer, inner) tuple pair.
        if !condition.is_null() {
            (*node).join.joinqual = pg_sys::lappend((*node).join.joinqual, condition.cast());
        }

        self.propagate_costs_binary(&mut (*node).join.plan, left_plan, right_plan);
        Ok(&mut (*node).join.plan as *mut pg_sys::Plan)
    }

    /// Build a `MergeJoin` plan node honoring `MERGE_JOIN_*`
    /// advice.
    ///
    /// MergeJoin requires (a) sorted children, (b) a list of
    /// equi-join `OpExpr`s in `mergeclauses`, (c) parallel
    /// arrays describing the btree opfamily / collation /
    /// reversal / nulls-first per clause. We extract each piece
    /// from the supplied join condition and `RelExpr` children;
    /// if any extraction fails we return `Err(reason)` and the
    /// caller falls back to `HashJoin`.
    ///
    /// Caveat: handles the most common case — equi-join clauses
    /// with column-ref operands on both sides where the
    /// operator OID has registered btree opfamilies. Compound
    /// non-column-ref operands (function calls, expressions)
    /// fall back. This is honest production-correct behavior
    /// for the most common shape; growing it covers the long
    /// tail.
    unsafe fn build_merge_join(
        &mut self,
        join_type: JoinType,
        left_rel: &RelExpr,
        right_rel: &RelExpr,
        condition: &Expr,
        left_plan: *mut pg_sys::Plan,
        right_plan: *mut pg_sys::Plan,
    ) -> Result<*mut pg_sys::Plan, String> {
        // 1. Extract column-pair list from the join condition.
        //    Each pair is (left_col_name, right_col_name).
        let mut pairs: Vec<(String, String)> = Vec::new();
        if !collect_equi_pairs(condition, left_rel, right_rel, &mut pairs) {
            return Err("join condition is not a pure equi-join over column references".to_string());
        }
        if pairs.is_empty() {
            return Err("no equi-join clauses found".to_string());
        }

        // 2. Build SortKey lists for each side and wrap the
        //    children in Sort nodes. Both sides must be sorted
        //    on the join columns in matching order.
        let left_keys: Vec<SortKey> = pairs
            .iter()
            .map(|(lcol, _)| sort_key_asc(lcol))
            .collect();
        let right_keys: Vec<SortKey> = pairs
            .iter()
            .map(|(_, rcol)| sort_key_asc(rcol))
            .collect();
        let left_oid = self.first_rel_oid(left_rel);
        let right_oid = self.first_rel_oid(right_rel);
        let sorted_left = self
            .wrap_in_sort(left_plan, &left_keys, left_oid)
            .ok_or_else(|| "failed to build Sort wrapper on left input".to_string())?;
        let sorted_right = self
            .wrap_in_sort(right_plan, &right_keys, right_oid)
            .ok_or_else(|| "failed to build Sort wrapper on right input".to_string())?;

        // 3. Translate the join condition to a list of OpExpr
        //    nodes for `mergeclauses`. Reuses the join-condition
        //    translator; the result must decompose into one or
        //    more T_OpExpr.
        let pg_condition = self.translate_join_condition(condition);
        if pg_condition.is_null() {
            return Err("translation of join condition produced null".to_string());
        }
        let opexprs = expr_to_opexpr_list(pg_condition);
        if opexprs.is_empty() {
            return Err("translated condition is not OpExpr-shaped".to_string());
        }
        if opexprs.len() != pairs.len() {
            return Err(format!(
                "extracted {} equi-pairs but {} OpExprs after translation",
                pairs.len(),
                opexprs.len(),
            ));
        }

        // 4. Resolve mergeFamilies per OpExpr via the PG catalog.
        //    `get_mergejoin_opfamilies(opno)` returns a List* of
        //    btree opfamily OIDs the operator belongs to.
        let n = opexprs.len();
        let merge_families = pg_sys::palloc(n * std::mem::size_of::<pg_sys::Oid>())
            .cast::<pg_sys::Oid>();
        let merge_collations = pg_sys::palloc(n * std::mem::size_of::<pg_sys::Oid>())
            .cast::<pg_sys::Oid>();
        let merge_reversals =
            pg_sys::palloc(n * std::mem::size_of::<bool>()).cast::<bool>();
        let merge_nulls_first =
            pg_sys::palloc(n * std::mem::size_of::<bool>()).cast::<bool>();
        let mut mergeclause_list: *mut pg_sys::List = std::ptr::null_mut();
        for (i, opexpr) in opexprs.iter().enumerate() {
            let opno = (**opexpr).opno;
            let families = pg_sys::get_mergejoin_opfamilies(opno);
            if families.is_null() || (*families).length == 0 {
                return Err(format!(
                    "operator OID {opno:?} has no btree opfamilies for merge join",
                ));
            }
            // Take the first opfamily; for typical equality
            // operators there's exactly one btree opfamily.
            let first_oid = pg_sys::list_nth(families, 0).cast::<pg_sys::Oid>();
            *merge_families.add(i) = *first_oid;
            *merge_collations.add(i) = (**opexpr).inputcollid;
            *merge_reversals.add(i) = false; // Asc
            *merge_nulls_first.add(i) = false; // PG default
            mergeclause_list =
                pg_sys::lappend(mergeclause_list, opexpr.cast());
        }

        // 5. Allocate the MergeJoin node and wire fields.
        let node = self.alloc_node::<pg_sys::MergeJoin>();
        if node.is_null() {
            return Err("MergeJoin allocation returned null".to_string());
        }
        (*node).join.plan.type_ = pg_sys::NodeTag::T_MergeJoin;
        (*node).join.jointype = ra_join_type_to_pg(join_type);
        (*node).join.plan.lefttree = sorted_left;
        (*node).join.plan.righttree = sorted_right;
        (*node).mergeclauses = mergeclause_list;
        (*node).mergeFamilies = merge_families;
        (*node).mergeCollations = merge_collations;
        (*node).mergeReversals = merge_reversals;
        (*node).mergeNullsFirst = merge_nulls_first;
        // skip_mark_restore = false: safe default; PG sets true
        // only when the inner side has the unique-key property
        // ensuring no duplicate matches.
        (*node).skip_mark_restore = false;

        self.propagate_costs_binary(&mut (*node).join.plan, sorted_left, sorted_right);
        Ok(&mut (*node).join.plan as *mut pg_sys::Plan)
    }

    /// Wrap an existing `Plan*` in a `Sort` node sorted on the
    /// supplied keys. Returns `None` if sort metadata can't be
    /// built (e.g., column resolution fails). Used by
    /// `build_merge_join` to prepare children for merging.
    unsafe fn wrap_in_sort(
        &self,
        child: *mut pg_sys::Plan,
        keys: &[SortKey],
        rel_oid: pg_sys::Oid,
    ) -> Option<*mut pg_sys::Plan> {
        if child.is_null() {
            return None;
        }
        let node = self.alloc_node::<pg_sys::Sort>();
        if node.is_null() {
            return None;
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Sort;
        (*node).plan.lefttree = child;

        let child_tlist = (*child).targetlist;
        let arrays = crate::sort_utils::build_sort_arrays(keys, child_tlist, rel_oid)?;
        (*node).numCols = arrays.num_cols;
        (*node).sortColIdx = arrays.col_idx;
        (*node).sortOperators = arrays.operators;
        (*node).collations = arrays.collations;
        (*node).nullsFirst = arrays.nulls_first;

        // Cost: child + n*log(n) comparison cost
        let n = (*child).plan_rows.max(1.0);
        (*node).plan.startup_cost = (*child).total_cost + n * n.ln().max(1.0) * 0.001;
        (*node).plan.total_cost = (*node).plan.startup_cost;
        (*node).plan.plan_rows = (*child).plan_rows;
        (*node).plan.plan_width = (*child).plan_width;
        Some(&mut (*node).plan as *mut pg_sys::Plan)
    }

    // -----------------------------------------------------------------------
    // Aggregate, Sort, Limit
    // -----------------------------------------------------------------------

    /// Build a `Var` referencing the lefttree output at position `pos`
    /// (`OUTER_VAR`). Upper plan nodes reference their child's output this
    /// way; Ra bypasses the planner's setrefs pass so we emit it directly.
    unsafe fn outer_var(
        &self,
        pos: i32,
        vartype: pg_sys::Oid,
        varcollid: pg_sys::Oid,
    ) -> *mut pg_sys::Expr {
        let var = self.alloc_node::<pg_sys::Var>();
        (*var).xpr.type_ = pg_sys::NodeTag::T_Var;
        (*var).varno = pg_sys::OUTER_VAR;
        (*var).varattno = pos as i16;
        (*var).vartype = vartype;
        (*var).vartypmod = -1;
        (*var).varcollid = varcollid;
        (*var).varlevelsup = 0;
        var.cast()
    }

    /// Add `col` (a plain column) to the aggregate input targetlist if not
    /// already present, returning `(position, type, collation)` of the
    /// column in that targetlist. Returns `None` if `col` is not a Var.
    unsafe fn add_input_col(
        &self,
        col: &Expr,
        in_tlist: &mut *mut pg_sys::List,
        colmap: &mut Vec<(i16, i32, pg_sys::Oid, pg_sys::Oid)>,
    ) -> Option<(i32, pg_sys::Oid, pg_sys::Oid)> {
        let v = expr_translator::translate(col, &self.expr_ctx);
        if v.is_null() || (*v).type_ != pg_sys::NodeTag::T_Var {
            return None;
        }
        let var = v.cast::<pg_sys::Var>();
        // Aggregate over a join: the column already exists in the join's full
        // passthrough output; resolve it to that position (OUTER_VAR) rather
        // than appending to a scan targetlist.
        if let Some(jm) = &self.active_join_map {
            let pos = *jm.get(&((*var).varno, (*var).varattno))?;
            return Some((pos, (*var).vartype, (*var).varcollid));
        }
        let attno = (*var).varattno;
        if let Some(&(_, pos, ty, coll)) = colmap.iter().find(|(a, ..)| *a == attno) {
            return Some((pos, ty, coll));
        }
        let pos = colmap.len() as i32 + 1;
        let te = pg_sys::makeTargetEntry(v.cast(), pos as i16, std::ptr::null_mut(), false);
        *in_tlist = pg_sys::lappend(*in_tlist, te.cast());
        let (ty, coll) = ((*var).vartype, (*var).varcollid);
        colmap.push((attno, pos, ty, coll));
        Some((pos, ty, coll))
    }

    /// Build an `Aggref` for a supported aggregate (`count`/`sum`/`avg`/
    /// `min`/`max`). `arg_var`/`arg_type` describe the (already-resolved
    /// OUTER) argument, or `None` for `count(*)`.
    /// Remap a translated aggregate-argument expression so its column `Var`s
    /// reference the aggregate input tlist via `OUTER_VAR` (matched by attno
    /// to its position in `colmap`). Used for expression arguments (a CASE
    /// from `FILTER (WHERE ...)`, arithmetic like `a*b`). Returns false on an
    /// unhandled node type so the caller defers to PG.
    unsafe fn remap_agg_input_vars(
        &self,
        node: *mut pg_sys::Node,
        colmap: &[(i16, i32, pg_sys::Oid, pg_sys::Oid)],
    ) -> bool {
        if node.is_null() {
            return true;
        }
        let list = |this: &Self, l: *mut pg_sys::List| -> bool {
            if l.is_null() {
                return true;
            }
            let el = (*l).elements;
            for i in 0..(*l).length {
                let item = (*el.add(i as usize)).ptr_value as *mut pg_sys::Node;
                if !this.remap_agg_input_vars(item, colmap) {
                    return false;
                }
            }
            true
        };
        match (*node).type_ {
            pg_sys::NodeTag::T_Var => {
                let var = node.cast::<pg_sys::Var>();
                // Aggregate over a join: remap to the join output position.
                if let Some(jm) = &self.active_join_map {
                    if let Some(&pos) = jm.get(&((*var).varno, (*var).varattno)) {
                        (*var).varno = pg_sys::OUTER_VAR;
                        (*var).varattno = pos as i16;
                        return true;
                    }
                    return false;
                }
                if let Some(&(_, pos, _, _)) =
                    colmap.iter().find(|(a, ..)| *a == (*var).varattno)
                {
                    (*var).varno = pg_sys::OUTER_VAR;
                    (*var).varattno = pos as i16;
                    true
                } else {
                    false
                }
            }
            pg_sys::NodeTag::T_Const | pg_sys::NodeTag::T_Param => true,
            pg_sys::NodeTag::T_OpExpr | pg_sys::NodeTag::T_DistinctExpr => {
                list(self, (*node.cast::<pg_sys::OpExpr>()).args)
            }
            pg_sys::NodeTag::T_ScalarArrayOpExpr => {
                list(self, (*node.cast::<pg_sys::ScalarArrayOpExpr>()).args)
            }
            pg_sys::NodeTag::T_BoolExpr => list(self, (*node.cast::<pg_sys::BoolExpr>()).args),
            pg_sys::NodeTag::T_FuncExpr => list(self, (*node.cast::<pg_sys::FuncExpr>()).args),
            pg_sys::NodeTag::T_CoalesceExpr => {
                list(self, (*node.cast::<pg_sys::CoalesceExpr>()).args)
            }
            pg_sys::NodeTag::T_NullTest => {
                self.remap_agg_input_vars((*node.cast::<pg_sys::NullTest>()).arg.cast(), colmap)
            }
            pg_sys::NodeTag::T_RelabelType => {
                self.remap_agg_input_vars((*node.cast::<pg_sys::RelabelType>()).arg.cast(), colmap)
            }
            pg_sys::NodeTag::T_CaseExpr => {
                let ce = node.cast::<pg_sys::CaseExpr>();
                if !(*ce).arg.is_null()
                    && !self.remap_agg_input_vars((*ce).arg.cast(), colmap)
                {
                    return false;
                }
                if !(*ce).args.is_null() {
                    let el = (*(*ce).args).elements;
                    for i in 0..(*(*ce).args).length {
                        let cw = (*el.add(i as usize)).ptr_value as *mut pg_sys::CaseWhen;
                        if !self.remap_agg_input_vars((*cw).expr.cast(), colmap)
                            || !self.remap_agg_input_vars((*cw).result.cast(), colmap)
                        {
                            return false;
                        }
                    }
                }
                if (*ce).defresult.is_null() {
                    true
                } else {
                    self.remap_agg_input_vars((*ce).defresult.cast(), colmap)
                }
            }
            _ => false,
        }
    }

    unsafe fn build_aggref(
        &self,
        name: &str,
        args: &[(*mut pg_sys::Expr, pg_sys::Oid, pg_sys::Oid)],
        aggno: i32,
        distinct: bool,
    ) -> Result<*mut pg_sys::Expr, PlanBuilderError> {
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        let lower = name.to_lowercase();
        let nargs = args.len() as i32;
        let mut argtypes: Vec<pg_sys::Oid> = args.iter().map(|&(_, ty, _)| ty).collect();
        // `count(col)` resolves to the polymorphic `count("any")` aggregate;
        // LookupFuncName needs the declared ANYOID, not the column's type
        // (type-specific aggregates like max/sum match their exact type).
        if lower == "count" && !args.is_empty() {
            argtypes[0] = pg_sys::ANYOID;
        }
        let fname = CString::new(lower.as_str()).map_err(|_| unsupported("agg name"))?;
        let name_node = pg_sys::makeString(fname.as_ptr().cast_mut());
        let fname_list = pg_sys::lappend(std::ptr::null_mut(), name_node.cast());
        let aggfnoid = pg_sys::LookupFuncName(fname_list, nargs, argtypes.as_ptr(), true);
        if aggfnoid == pg_sys::InvalidOid
            || pg_sys::get_func_prokind(aggfnoid) != pg_sys::PROKIND_AGGREGATE as i8
        {
            return Err(unsupported("aggregate function lookup"));
        }
        // aggtranstype from pg_aggregate (catalog column 17,
        // Anum_pg_aggregate_aggtranstype — stable across PG13-19). The Form
        // struct is not generated by pgrx, so read the attribute directly.
        let aggtup = pg_sys::SearchSysCache1(
            pg_sys::SysCacheIdentifier::AGGFNOID as i32,
            pg_sys::Datum::from(aggfnoid),
        );
        if aggtup.is_null() {
            return Err(unsupported("pg_aggregate lookup"));
        }
        let mut isnull = false;
        let transtype_datum = pg_sys::SysCacheGetAttr(
            pg_sys::SysCacheIdentifier::AGGFNOID as i32,
            aggtup,
            17,
            &mut isnull,
        );
        pg_sys::ReleaseSysCache(aggtup);
        if isnull {
            return Err(unsupported("aggregate transtype"));
        }
        let aggtranstype = pg_sys::Oid::from(transtype_datum.value() as u32);

        let rettype = pg_sys::get_func_rettype(aggfnoid);
        // Resolve a polymorphic array return (e.g. array_agg's `anyarray`) to
        // the concrete array type of the first argument.
        let aggtype = if rettype == pg_sys::ANYARRAYOID && !args.is_empty() {
            pg_sys::get_array_type(args[0].1)
        } else {
            rettype
        };
        let node = self.alloc_node::<pg_sys::Aggref>();
        (*node).xpr.type_ = pg_sys::NodeTag::T_Aggref;
        (*node).aggfnoid = aggfnoid;
        (*node).aggtype = aggtype;
        (*node).aggcollid = pg_sys::get_typcollation(aggtype);
        (*node).aggtranstype = aggtranstype;
        (*node).aggstar = args.is_empty();
        (*node).aggkind = b'n' as i8; // AGGKIND_NORMAL
        (*node).aggsplit = pg_sys::AggSplit::AGGSPLIT_SIMPLE;
        (*node).aggno = aggno;
        (*node).aggtransno = aggno;
        (*node).agglevelsup = 0;
        if let Some(&(_, _, coll)) = args.first() {
            (*node).inputcollid = coll;
        }
        for (i, &(arg_expr, ty, _)) in args.iter().enumerate() {
            (*node).aggargtypes = pg_sys::lappend_oid((*node).aggargtypes, ty);
            let te = pg_sys::makeTargetEntry(arg_expr, (i + 1) as i16, std::ptr::null_mut(), false);
            if distinct {
                // DISTINCT aggregate: tag each argument with a sort/group ref
                // and build the matching SortGroupClause list in aggdistinct,
                // so the executor de-duplicates the aggregate input. Mirrors
                // PostgreSQL's representation of count(DISTINCT x).
                (*te).ressortgroupref = (i + 1) as pg_sys::Index;
            }
            (*node).args = pg_sys::lappend((*node).args, te.cast());
        }
        if distinct {
            let mut dlist: *mut pg_sys::List = std::ptr::null_mut();
            for (i, &(_, ty, _)) in args.iter().enumerate() {
                let mut lt = pg_sys::InvalidOid;
                let mut eq = pg_sys::InvalidOid;
                let mut gt = pg_sys::InvalidOid;
                let mut hashable = false;
                pg_sys::get_sort_group_operators(
                    ty, true, true, false, &mut lt, &mut eq, &mut gt, &mut hashable,
                );
                if eq == pg_sys::InvalidOid {
                    return Err(unsupported("DISTINCT aggregate: no equality operator"));
                }
                let sgc = self.alloc_node::<pg_sys::SortGroupClause>();
                (*sgc).type_ = pg_sys::NodeTag::T_SortGroupClause;
                (*sgc).tleSortGroupRef = (i + 1) as pg_sys::Index;
                (*sgc).eqop = eq;
                (*sgc).sortop = lt;
                (*sgc).nulls_first = false;
                (*sgc).hashable = hashable;
                dlist = pg_sys::lappend(dlist, sgc.cast());
            }
            (*node).aggdistinct = dlist;
        }
        Ok(node.cast())
    }

    /// Build an ordered-set `Aggref` for `percentile_cont`/`percentile_disc`
    /// (`agg(fraction) WITHIN GROUP (ORDER BY col)`), encoded by the parser as
    /// `agg(fraction, __within_group(col))`. The fraction and the ordered
    /// column are coerced to `float8` (matching PostgreSQL's own coercion of a
    /// numeric input), so the `(float8, float8)` catalog aggregate is used.
    /// Returns `None` (→ caller defers) for any other ordered-set aggregate,
    /// arity, or unresolvable column/cast.
    unsafe fn build_ordered_set_aggref(
        &self,
        name: &str,
        direct_args: &[Expr],
        order_args: &[Expr],
        in_tlist: &mut *mut pg_sys::List,
        colmap: &mut Vec<(i16, i32, pg_sys::Oid, pg_sys::Oid)>,
        aggno: &mut i32,
    ) -> Option<*mut pg_sys::Expr> {
        let lower = name.to_lowercase();
        // Only percentile_cont is built: it has a concrete (float8, float8)
        // signature and returns float8. percentile_disc/mode return the input
        // type (polymorphic) and need different handling, so they defer to PG.
        if lower != "percentile_cont" {
            return None;
        }
        if direct_args.len() != 1 || order_args.len() != 1 {
            return None;
        }
        let to_float8 = |e: &Expr| Expr::Cast {
            expr: Box::new(e.clone()),
            target_type: "float8".to_owned(),
        };
        // Direct arg (the fraction): a constant, coerced to float8.
        let direct_e = expr_translator::translate(&to_float8(&direct_args[0]), &self.expr_ctx);
        if direct_e.is_null() {
            return None;
        }
        // Ordered arg (the column): register it on the aggregate input, then
        // build its float8 cast referencing the input frame (OUTER_VAR).
        self.add_input_col(&order_args[0], in_tlist, colmap)?;
        let order_e = expr_translator::translate(&to_float8(&order_args[0]), &self.expr_ctx);
        if order_e.is_null() || !self.remap_agg_input_vars(order_e.cast(), colmap) {
            return None;
        }
        let order_type = pg_sys::exprType(order_e.cast());
        let order_coll = pg_sys::exprCollation(order_e.cast());

        let argtypes = [pg_sys::FLOAT8OID, order_type];
        let fname = CString::new(lower.as_str()).ok()?;
        let name_node = pg_sys::makeString(fname.as_ptr().cast_mut());
        let fname_list = pg_sys::lappend(std::ptr::null_mut(), name_node.cast());
        let aggfnoid = pg_sys::LookupFuncName(fname_list, 2, argtypes.as_ptr(), true);
        if aggfnoid == pg_sys::InvalidOid
            || pg_sys::get_func_prokind(aggfnoid) != pg_sys::PROKIND_AGGREGATE as i8
        {
            return None;
        }
        let aggtup = pg_sys::SearchSysCache1(
            pg_sys::SysCacheIdentifier::AGGFNOID as i32,
            pg_sys::Datum::from(aggfnoid),
        );
        if aggtup.is_null() {
            return None;
        }
        let mut isnull = false;
        let transtype_datum = pg_sys::SysCacheGetAttr(
            pg_sys::SysCacheIdentifier::AGGFNOID as i32,
            aggtup,
            17,
            &mut isnull,
        );
        pg_sys::ReleaseSysCache(aggtup);
        if isnull {
            return None;
        }
        let aggtranstype = pg_sys::Oid::from(transtype_datum.value() as u32);
        let rettype = pg_sys::get_func_rettype(aggfnoid);

        // Sort/equality operators for the ordered column (aggorder).
        let mut lt = pg_sys::InvalidOid;
        let mut eq = pg_sys::InvalidOid;
        let mut gt = pg_sys::InvalidOid;
        let mut hashable = false;
        pg_sys::get_sort_group_operators(order_type, true, true, false, &mut lt, &mut eq, &mut gt, &mut hashable);
        if eq == pg_sys::InvalidOid || lt == pg_sys::InvalidOid {
            return None;
        }

        let node = self.alloc_node::<pg_sys::Aggref>();
        (*node).xpr.type_ = pg_sys::NodeTag::T_Aggref;
        (*node).aggfnoid = aggfnoid;
        (*node).aggtype = rettype;
        (*node).aggcollid = pg_sys::get_typcollation(rettype);
        (*node).inputcollid = order_coll;
        (*node).aggtranstype = aggtranstype;
        (*node).aggdirectargs = pg_sys::lappend(std::ptr::null_mut(), direct_e.cast());
        let te = pg_sys::makeTargetEntry(order_e, 1, std::ptr::null_mut(), false);
        (*te).ressortgroupref = 1;
        (*node).args = pg_sys::lappend(std::ptr::null_mut(), te.cast());
        let sgc = self.alloc_node::<pg_sys::SortGroupClause>();
        (*sgc).type_ = pg_sys::NodeTag::T_SortGroupClause;
        (*sgc).tleSortGroupRef = 1;
        (*sgc).eqop = eq;
        (*sgc).sortop = lt;
        (*sgc).nulls_first = false;
        (*sgc).hashable = hashable;
        (*node).aggorder = pg_sys::lappend(std::ptr::null_mut(), sgc.cast());
        // aggargtypes: direct then aggregated arg types.
        (*node).aggargtypes = pg_sys::lappend_oid((*node).aggargtypes, pg_sys::FLOAT8OID);
        (*node).aggargtypes = pg_sys::lappend_oid((*node).aggargtypes, order_type);
        (*node).aggkind = b'o' as i8; // AGGKIND_ORDERED_SET
        (*node).aggstar = false;
        (*node).aggsplit = pg_sys::AggSplit::AGGSPLIT_SIMPLE;
        (*node).aggno = *aggno;
        (*node).aggtransno = *aggno;
        (*node).agglevelsup = 0;
        (*node).location = -1;
        *aggno += 1;
        Some(node.cast())
    }

    /// Register the input columns referenced by any aggregate argument
    unsafe fn register_agg_args(
        &self,
        expr: &Expr,
        in_tlist: &mut *mut pg_sys::List,
        colmap: &mut Vec<(i16, i32, pg_sys::Oid, pg_sys::Oid)>,
    ) -> Result<(), PlanBuilderError> {
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        if let Expr::Function { name, args } = expr {
            if Self::is_supported_agg(name) {
                for a in args {
                    // Unwrap a DISTINCT marker so the underlying column is
                    // registered in the aggregate input tlist.
                    let real = match a {
                        Expr::Function { name: n, args: ia }
                            if n == "__distinct" && ia.len() == 1 =>
                        {
                            &ia[0]
                        }
                        _ => a,
                    };
                    match real {
                        Expr::Column(c) if c.column != "*" => {
                            if self.add_input_col(real, in_tlist, colmap).is_none() {
                                return Err(unsupported("aggregate arg not a column"));
                            }
                        }
                        Expr::Column(_) => {} // count(*): no input column
                        // Expression argument (CASE from FILTER, arithmetic
                        // like a*b): register the columns it references.
                        other => self.register_agg_args(other, in_tlist, colmap)?,
                    }
                }
                return Ok(());
            }
        }
        match expr {
            Expr::BinOp { left, right, .. } => {
                self.register_agg_args(left, in_tlist, colmap)?;
                self.register_agg_args(right, in_tlist, colmap)?;
            }
            Expr::UnaryOp { operand, .. } => self.register_agg_args(operand, in_tlist, colmap)?,
            Expr::Function { args, .. } | Expr::Array(args) => {
                for a in args {
                    self.register_agg_args(a, in_tlist, colmap)?;
                }
            }
            Expr::Cast { expr, .. } | Expr::FieldAccess { expr, .. } => {
                self.register_agg_args(expr, in_tlist, colmap)?;
            }
            Expr::Case { operand, when_clauses, else_result } => {
                if let Some(o) = operand {
                    self.register_agg_args(o, in_tlist, colmap)?;
                }
                for (w, t) in when_clauses {
                    self.register_agg_args(w, in_tlist, colmap)?;
                    self.register_agg_args(t, in_tlist, colmap)?;
                }
                if let Some(el) = else_result {
                    self.register_agg_args(el, in_tlist, colmap)?;
                }
            }
            // A column reached while recursing through an expression argument
            // (e.g. inside a CASE or arithmetic) must be added to the input
            // tlist so the remapped aggregate arg can reference it.
            Expr::Column(c) if c.column != "*" => {
                let _ = self.add_input_col(expr, in_tlist, colmap);
            }
            _ => {}
        }
        Ok(())
    }

    /// Build an aggregate-context output/qual expression: aggregate calls
    /// become `Aggref`s, group/scan columns become OUTER Vars, constants pass
    /// through, and binary operators combine recursively-built operands.
    /// `None` (→ caller Errs → native planner) for any other shape.
    unsafe fn build_agg_out_expr(
        &self,
        expr: &Expr,
        in_tlist: &mut *mut pg_sys::List,
        colmap: &mut Vec<(i16, i32, pg_sys::Oid, pg_sys::Oid)>,
        aggno: &mut i32,
    ) -> Option<*mut pg_sys::Expr> {
        match expr {
            // Ordered-set aggregate: agg(direct...) WITHIN GROUP (ORDER BY ...)
            // is encoded by the parser as agg(direct..., __within_group(cols)).
            Expr::Function { name, args }
                if args.iter().any(|a| {
                    matches!(a, Expr::Function { name: n, .. } if n == "__within_group")
                }) =>
            {
                let mut direct: Vec<Expr> = Vec::new();
                let mut order: &[Expr] = &[];
                for a in args {
                    if let Expr::Function { name: n, args: oa } = a {
                        if n == "__within_group" {
                            order = oa;
                            continue;
                        }
                    }
                    direct.push(a.clone());
                }
                self.build_ordered_set_aggref(name, &direct, order, in_tlist, colmap, aggno)
            }
            Expr::Function { name, args } if Self::is_supported_agg(name) => {
                // Build each argument: a column becomes an input-tlist column
                // referenced by OUTER_VAR; a constant (e.g. string_agg's
                // delimiter) is translated in place. `count(*)` has no args.
                let mut arglist: Vec<(*mut pg_sys::Expr, pg_sys::Oid, pg_sys::Oid)> = Vec::new();
                let is_star = matches!(args.as_slice(), [Expr::Column(c)] if c.column == "*");
                let mut distinct = false;
                if !is_star {
                    for a in args {
                        // count(DISTINCT x) parses as count(__distinct(x)); the
                        // parser wraps a DISTINCT argument in a "__distinct"
                        // marker function. Unwrap it and flag the aggregate.
                        let real = match a {
                            Expr::Function { name: n, args: ia }
                                if n == "__distinct" && ia.len() == 1 =>
                            {
                                distinct = true;
                                &ia[0]
                            }
                            _ => a,
                        };
                        match real {
                            Expr::Column(_) => {
                                let (pos, ty, coll) = self.add_input_col(real, in_tlist, colmap)?;
                                arglist.push((self.outer_var(pos, ty, coll), ty, coll));
                            }
                            Expr::Const(_) => {
                                let e = expr_translator::translate(real, &self.expr_ctx);
                                if e.is_null() {
                                    return None;
                                }
                                arglist.push((e, pg_sys::exprType(e.cast()), pg_sys::exprCollation(e.cast())));
                            }
                            _ => {
                                // Expression argument (CASE from FILTER,
                                // arithmetic like a*b): its columns were
                                // registered by register_agg_args, so translate
                                // it and remap the column Vars to OUTER_VAR
                                // references into the aggregate input tlist.
                                let e = expr_translator::translate(real, &self.expr_ctx);
                                if e.is_null() || !self.remap_agg_input_vars(e.cast(), colmap) {
                                    return None;
                                }
                                arglist.push((
                                    e,
                                    pg_sys::exprType(e.cast()),
                                    pg_sys::exprCollation(e.cast()),
                                ));
                            }
                        }
                    }
                }
                let aggref = self.build_aggref(name, &arglist, *aggno, distinct).ok()?;
                *aggno += 1;
                Some(aggref)
            }
            Expr::Column(_) => {
                let (pos, ty, coll) = self.add_input_col(expr, in_tlist, colmap)?;
                Some(self.outer_var(pos, ty, coll))
            }
            Expr::Const(_) => {
                let e = expr_translator::translate(expr, &self.expr_ctx);
                (!e.is_null()).then_some(e)
            }
            Expr::BinOp { op: ra_core::expr::BinOp::And, left, right } => {
                let l = self.build_agg_out_expr(left, in_tlist, colmap, aggno)?;
                let r = self.build_agg_out_expr(right, in_tlist, colmap, aggno)?;
                let e = expr_translator::bool_expr_from_nodes(
                    pg_sys::BoolExprType::AND_EXPR,
                    l,
                    r,
                );
                (!e.is_null()).then_some(e)
            }
            Expr::BinOp { op: ra_core::expr::BinOp::Or, left, right } => {
                let l = self.build_agg_out_expr(left, in_tlist, colmap, aggno)?;
                let r = self.build_agg_out_expr(right, in_tlist, colmap, aggno)?;
                let e = expr_translator::bool_expr_from_nodes(
                    pg_sys::BoolExprType::OR_EXPR,
                    l,
                    r,
                );
                (!e.is_null()).then_some(e)
            }
            Expr::BinOp { op, left, right } => {
                let op_str = expr_translator::binop_op_str(op)?;
                let l = self.build_agg_out_expr(left, in_tlist, colmap, aggno)?;
                let r = self.build_agg_out_expr(right, in_tlist, colmap, aggno)?;
                let e = expr_translator::op_expr_from_nodes(op_str, l, r);
                (!e.is_null()).then_some(e)
            }
            _ => None,
        }
    }
    /// Output-column scope for an inlined subquery, derived from a child plan's
    /// targetlist (1-based positions aligned with the targetlist so
    /// `Var(scanrelid, pos)` references resolve correctly).
    unsafe fn tlist_subquery_cols(
        tlist: *mut pg_sys::List,
    ) -> Vec<crate::expr_translator::SubqueryCol> {
        let mut cols = Vec::new();
        let n = pg_sys::list_length(tlist);
        for i in 0..n {
            let te = pg_sys::list_nth(tlist, i).cast::<pg_sys::TargetEntry>();
            let (name, typ, coll) = if te.is_null() || (*te).expr.is_null() {
                (String::new(), pg_sys::InvalidOid, pg_sys::InvalidOid)
            } else {
                let name = if (*te).resname.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr((*te).resname)
                        .to_string_lossy()
                        .to_lowercase()
                };
                (
                    name,
                    pg_sys::exprType((*te).expr.cast()),
                    pg_sys::exprCollation((*te).expr.cast()),
                )
            };
            cols.push(crate::expr_translator::SubqueryCol { name, typ, typmod: -1, coll });
        }
        cols
    }

    /// 1-based range-table index of the first `RTE_SUBQUERY` or `RTE_CTE` in
    /// the original query — the derived table / CTE RTE used as a
    /// SubqueryScan's `scanrelid` when building an inlined aggregating derived
    /// table or CTE.
    unsafe fn first_subquery_rtindex(&self) -> Option<pg_sys::Index> {
        let q = self.original_query;
        if q.is_null() || (*q).rtable.is_null() {
            return None;
        }
        let rt = (*q).rtable;
        let e = (*rt).elements;
        for i in 0..(*rt).length {
            let rte = (*e.add(i as usize)).ptr_value as *mut pg_sys::RangeTblEntry;
            if !rte.is_null()
                && matches!(
                    (*rte).rtekind,
                    pg_sys::RTEKind::RTE_SUBQUERY | pg_sys::RTEKind::RTE_CTE
                )
            {
                return Some((i + 1) as pg_sys::Index);
            }
        }
        None
    }

    /// Build a `SubqueryScan` over an inlined derived table (an aggregating /
    /// computing FROM sub-query) whose computed output is filtered and/or
    /// projected by the outer query. Builds the inner subplan, then a
    /// SubqueryScan whose `scanrelid` is the derived table's RTE and whose
    /// qual/targetlist reference the subquery output via `Var(scanrelid, pos)`
    /// (resolved by name through the subquery scope). Mirrors PostgreSQL.
    unsafe fn build_subquery_scan(
        &mut self,
        inner: &RelExpr,
        filter_pred: Option<&Expr>,
        out_columns: &[ProjectionColumn],
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        let scanrelid = self
            .first_subquery_rtindex()
            .ok_or_else(|| unsupported("derived-table RTE not found"))?;
        let child = self.build_plan(inner)?;
        if child.is_null() || (*child).targetlist.is_null() {
            return Err(unsupported("subquery scan input"));
        }
        *self.expr_ctx.subquery_scope.borrow_mut() =
            Some(crate::expr_translator::SubqueryScope {
                rtindex: scanrelid,
                cols: Self::tlist_subquery_cols((*child).targetlist),
            });

        let node = self.alloc_node::<pg_sys::SubqueryScan>();
        if node.is_null() {
            self.expr_ctx.subquery_scope.replace(None);
            return Err(PlanBuilderError::NullPointer("SubqueryScan".to_owned()));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_SubqueryScan;
        (*node).scan.scanrelid = scanrelid;
        (*node).subplan = child;
        let plan_ptr = &mut (*node).scan.plan as *mut pg_sys::Plan;
        self.set_targetlist(plan_ptr, out_columns)?;
        if let Some(pred) = filter_pred {
            let q = expr_translator::translate(pred, &self.expr_ctx);
            if q.is_null() {
                self.expr_ctx.subquery_scope.replace(None);
                return Err(unsupported("subquery scan filter not translatable"));
            }
            (*node).scan.plan.qual = pg_sys::lappend((*node).scan.plan.qual, q.cast());
        }
        (*node).scan.plan.plan_rows = (*child).plan_rows;
        (*node).scan.plan.startup_cost = (*child).startup_cost;
        (*node).scan.plan.total_cost = (*child).total_cost;
        self.expr_ctx.subquery_scope.replace(None);
        Ok(plan_ptr)
    }

    /// Build an `Agg` plan node for `Project(out_columns)` over
    /// `Aggregate(group_by)`. Output columns may be group columns, supported
    /// aggregates, or expressions combining them; aggregate arguments and
    /// group keys must be plain columns of a single-relation input. Any other
    /// shape returns `Err` (→ native
    /// planner): expressions over aggregates, `DISTINCT` aggregates,
    /// `HAVING`'s nested form, unsupported functions, multi-relation input.
    unsafe fn build_grouped_aggregate(
        &mut self,
        out_columns: &[ProjectionColumn],
        group_by: &[Expr],
        agg_input: &RelExpr,
        having: Option<&Expr>,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        // An aggregate over a join builds the join with full passthrough output
        // and resolves group/aggregate columns to the join's output positions
        // (active_join_map). Otherwise the input is a scan/filter whose
        // targetlist we set to exactly the referenced columns.
        let (child, is_join) =
            if let Some((jt, cond, l, r, wp)) = Self::agg_join_components(agg_input) {
                let (jplan, jmap, _) = self.build_join_node(jt, cond, l, r, wp, None)?;
                self.active_join_map = Some(jmap);
                (jplan, true)
            } else {
                (self.build_plan(agg_input)?, false)
            };
        if child.is_null() {
            self.active_join_map = None;
            return Err(unsupported("aggregate input"));
        }
        let mut in_tlist: *mut pg_sys::List = std::ptr::null_mut();
        let mut colmap: Vec<(i16, i32, pg_sys::Oid, pg_sys::Oid)> = Vec::new();

        // Pass 1: register every input column referenced by group_by and by
        // aggregate arguments so the scan exposes them. For a join the columns
        // already exist in the passthrough output, so only the group keys are
        // resolved (to positions) and the join targetlist is left intact.
        let mut grp_pos: Vec<(i32, pg_sys::Oid, pg_sys::Oid)> = Vec::new();
        for g in group_by {
            match self.add_input_col(g, &mut in_tlist, &mut colmap) {
                Some(t) => grp_pos.push(t),
                None => {
                    self.active_join_map = None;
                    return Err(unsupported("group key not a column"));
                }
            }
        }
        if !is_join {
            for pc in out_columns {
                self.register_agg_args(&pc.expr, &mut in_tlist, &mut colmap)?;
            }
            if let Some(h) = having {
                self.register_agg_args(h, &mut in_tlist, &mut colmap)?;
            }
            (*child).targetlist = in_tlist;
        }

        let node = self.alloc_node::<pg_sys::Agg>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("Agg".to_owned()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Agg;
        (*node).plan.lefttree = child;
        (*node).aggsplit = pg_sys::AggSplit::AGGSPLIT_SIMPLE;

        // Pass 2: build the Agg output targetlist (group Vars + Aggrefs).
        let mut out_tlist: *mut pg_sys::List = std::ptr::null_mut();
        let mut aggno = 0;
        for (i, pc) in out_columns.iter().enumerate() {
            let entry = self
                .build_agg_out_expr(&pc.expr, &mut in_tlist, &mut colmap, &mut aggno)
                .ok_or_else(|| unsupported("aggregate output expression"))?;
            let resname = pc
                .alias
                .as_deref()
                .or_else(|| crate::sort_utils::extract_column_name(&pc.expr));
            let rn = match resname {
                Some(n) => CString::new(n).map(|c| pg_sys::pstrdup(c.as_ptr())).unwrap_or(std::ptr::null_mut()),
                None => std::ptr::null_mut(),
            };
            let te = pg_sys::makeTargetEntry(entry, (i + 1) as i16, rn, false);
            out_tlist = pg_sys::lappend(out_tlist, te.cast());
        }
        (*node).plan.targetlist = out_tlist;

        // HAVING → the Agg node's qual (evaluated after aggregation),
        // continuing aggno so its Aggrefs are distinct from output ones.
        if let Some(h) = having {
            let q = self
                .build_agg_out_expr(h, &mut in_tlist, &mut colmap, &mut aggno)
                .ok_or_else(|| unsupported("HAVING expression"))?;
            (*node).plan.qual = pg_sys::lappend(std::ptr::null_mut(), q.cast());
        }

        let ngroups: f64 = if group_by.is_empty() {
            (*node).aggstrategy = pg_sys::AggStrategy::AGG_PLAIN;
            1.0
        } else {
            (*node).aggstrategy = pg_sys::AggStrategy::AGG_HASHED;
            let n = grp_pos.len();
            let col_idx = pg_sys::palloc(n * std::mem::size_of::<pg_sys::AttrNumber>())
                as *mut pg_sys::AttrNumber;
            let ops = pg_sys::palloc(n * std::mem::size_of::<pg_sys::Oid>()) as *mut pg_sys::Oid;
            let colls = pg_sys::palloc(n * std::mem::size_of::<pg_sys::Oid>()) as *mut pg_sys::Oid;
            for (i, &(pos, ty, coll)) in grp_pos.iter().enumerate() {
                *col_idx.add(i) = pos as i16;
                *ops.add(i) = crate::sort_utils::resolve_equality_op(ty);
                *colls.add(i) = coll;
            }
            (*node).numCols = n as i32;
            (*node).grpColIdx = col_idx;
            (*node).grpOperators = ops;
            (*node).grpCollations = colls;
            (*child).plan_rows.max(1.0).sqrt().clamp(1.0, 1_000_000.0)
        };
        // numGroups: i64 on pg13..pg18, Cardinality (f64) on pg19+.
        #[cfg(not(feature = "pg19"))]
        {
            (*node).numGroups = ngroups as i64;
        }
        #[cfg(feature = "pg19")]
        {
            (*node).numGroups = ngroups;
        }
        (*node).plan.plan_rows = ngroups;
        (*node).plan.total_cost = (*child).total_cost + (*child).plan_rows * 0.01;
        (*node).plan.startup_cost = (*node).plan.total_cost;
        // Clear the join column map so nodes built above this aggregate resolve
        // columns normally.
        self.active_join_map = None;
        Ok(&mut (*node).plan as *mut pg_sys::Plan)
    }

    /// If an aggregate's input is a join (optionally under a WHERE `Filter`),
    /// return its `(join_type, condition, left, right, where_predicate)` so the
    /// aggregate can be built over the join's passthrough output. `None` for a
    /// non-join input (the scan/filter path).
    fn agg_join_components(
        expr: &RelExpr,
    ) -> Option<(JoinType, &Expr, &RelExpr, &RelExpr, Option<&Expr>)> {
        match expr {
            RelExpr::Join {
                join_type,
                condition,
                left,
                right,
            } => Some((*join_type, condition, left, right, None)),
            RelExpr::Filter { predicate, input } => {
                if let RelExpr::Join {
                    join_type,
                    condition,
                    left,
                    right,
                } = &**input
                {
                    Some((*join_type, condition, left, right, Some(predicate)))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn is_supported_agg(name: &str) -> bool {
        matches!(
            name.to_lowercase().as_str(),
            "count" | "sum" | "avg" | "min" | "max" | "string_agg" | "array_agg"
        )
    }

    unsafe fn build_aggregate(
        &mut self,
        group_by: &[ra_core::expr::Expr],
        _aggregates: &[AggregateExpr],
        input: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let child = self.build_plan(input)?;
        let node = self.alloc_node::<pg_sys::Agg>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("Agg allocation".to_string()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Agg;
        (*node).plan.lefttree = child;
        (*node).aggsplit = pg_sys::AggSplit::AGGSPLIT_SIMPLE;

        if group_by.is_empty() {
            (*node).aggstrategy = pg_sys::AggStrategy::AGG_PLAIN;
        } else {
            (*node).aggstrategy = pg_sys::AggStrategy::AGG_HASHED;

            // Build group-by column metadata arrays
            let child_tlist = if child.is_null() {
                std::ptr::null_mut()
            } else {
                (*child).targetlist
            };
            let rel_oid = self.first_rel_oid(input);

            if let Some(arrays) =
                crate::sort_utils::build_group_arrays(group_by, child_tlist, rel_oid)
            {
                (*node).numCols = arrays.num_cols;
                (*node).grpColIdx = arrays.col_idx;
                (*node).grpOperators = arrays.operators;
                (*node).grpCollations = arrays.collations;
            } else {
                (*node).numCols = group_by.len() as i32;
            }
        }

        // Estimate numGroups from statistics
        let input_rows = if child.is_null() {
            1000.0
        } else {
            (*child).plan_rows.max(1.0)
        };
        // numGroups type: i64 on pg13..pg18, Cardinality (f64) on pg19+.
        #[cfg(not(feature = "pg19"))]
        {
            (*node).numGroups = if group_by.is_empty() {
                1
            } else {
                // Conservative estimate: sqrt(input_rows), clamped.
                // PG 18 changed `Result.numHashes` (and similar) from u64 to i64.
                (input_rows.sqrt() as i64).clamp(10, 1_000_000)
            };
        }
        #[cfg(feature = "pg19")]
        {
            (*node).numGroups = if group_by.is_empty() {
                1.0
            } else {
                input_rows.sqrt().clamp(10.0, 1_000_000.0)
            };
        }

        // Cost: child cost + per-row hashing/comparison cost
        if !child.is_null() {
            let hash_cost = input_rows * 0.01; // cpu_operator_cost per tuple
            (*node).plan.startup_cost = (*child).total_cost + hash_cost;
            (*node).plan.total_cost = (*node).plan.startup_cost;
            (*node).plan.plan_rows = if group_by.is_empty() {
                1.0
            } else {
                (*node).numGroups as f64
            };
            (*node).plan.plan_width = (*child).plan_width;
        }
        Ok(&mut (*node).plan as *mut pg_sys::Plan)
    }

    /// Resolve each sort key to its output column position in `child_tlist`.
    ///
    /// Sort keys reference the underlying column (the parser lowers
    /// `ORDER BY <alias>` to the source column), so a `resname`-based lookup
    /// misses aliased outputs. We translate each key to its PG `Var` and match
    /// it against the child targetlist by `varattno`/`varno`, robust to
    /// projection folding and aliasing.
    ///
    /// Returns `false` if any key does not correspond to an output column
    /// (e.g. `ORDER BY <column-not-in-SELECT>`, which PG handles with a
    /// resjunk targetlist entry that Ra does not yet build). The caller must
    /// then defer to the native planner — a dangling sort index reads past the
    /// tuple slot (wrong results or a backend crash).
    /// 1-based output position of a sort key that appears verbatim among the
    /// input `Project`'s output columns — e.g. `ORDER BY rev` where `rev` is a
    /// selected aggregate, so the parser resolves the key to the same
    /// `sum(...)` expression that the Project outputs. `None` when the input is
    /// not a `Project` or the key is not one of its output columns (the caller
    /// then uses Var/name resolution or defers).
    fn sort_output_position(key_expr: &ra_core::expr::Expr, input: &RelExpr) -> Option<i16> {
        if let RelExpr::Project { columns, .. } = input {
            for (j, c) in columns.iter().enumerate() {
                if c.expr == *key_expr {
                    return i16::try_from(j + 1).ok();
                }
            }
        }
        None
    }

    unsafe fn resolve_sort_indices(
        &self,
        keys: &[SortKey],
        positions: &[Option<i16>],
        child_tlist: *mut pg_sys::List,
        col_idx: *mut pg_sys::AttrNumber,
        num_cols: i32,
    ) -> bool {
        if child_tlist.is_null() || col_idx.is_null() {
            return false;
        }
        for (i, key) in keys.iter().enumerate() {
            if i as i32 >= num_cols {
                break;
            }
            // A key that matches an output column verbatim (e.g. ORDER BY an
            // aggregate that is also selected) resolves directly to its
            // position; the operator/collation are fixed up by the caller.
            if let Some(p) = positions.get(i).copied().flatten() {
                *col_idx.add(i) = p;
                continue;
            }
            let key_expr = expr_translator::translate(&key.expr, &self.expr_ctx);
            if key_expr.is_null() || (*key_expr).type_ != pg_sys::NodeTag::T_Var {
                return false;
            }
            let kv = key_expr.cast::<pg_sys::Var>();
            let elements = (*child_tlist).elements;
            let mut resolved = false;
            for pos in 0..(*child_tlist).length {
                let te = (*elements.add(pos as usize)).ptr_value as *mut pg_sys::TargetEntry;
                if te.is_null() || (*te).expr.is_null() {
                    continue;
                }
                if (*(*te).expr).type_ != pg_sys::NodeTag::T_Var {
                    continue;
                }
                let tv = (*te).expr.cast::<pg_sys::Var>();
                if (*tv).varattno == (*kv).varattno && (*tv).varno == (*kv).varno {
                    *col_idx.add(i) = (*te).resno;
                    resolved = true;
                    break;
                }
            }
            // Fallback: match by output column name. Needed when the child is
            // an Agg whose output columns are OUTER_VAR references (their
            // varno/varattno don't match the key's scan-relative Var), but the
            // TargetEntry carries the group column's resname.
            if !resolved {
                if let Some(name) = crate::sort_utils::extract_column_name(&key.expr) {
                    if let Some(resno) =
                        crate::sort_utils::find_attr_in_targetlist(child_tlist, name)
                    {
                        *col_idx.add(i) = resno;
                        resolved = true;
                    }
                }
            }
            if !resolved {
                return false;
            }
        }
        true
    }

    /// If `input` is a `Project` missing some ORDER BY *column* keys from its
    /// output, return an augmented `Project` that also selects those columns
    /// (so the Sort can reference them), plus the count of appended columns.
    /// The columns are built through the normal `set_targetlist` path — which
    /// sorts correctly — and the caller marks the appended trailing tlist
    /// entries resjunk so PostgreSQL's junk filter drops them from the result.
    /// Returns `(input.clone(), 0)` when nothing needs augmenting (or the input
    /// is not a Project), leaving the existing behaviour unchanged.
    fn augment_sort_input(&self, keys: &[SortKey], input: &RelExpr) -> (RelExpr, usize) {
        use ra_core::expr::Expr;
        let RelExpr::Project { columns, input: inner } = input else {
            return (input.clone(), 0);
        };
        let col_present = |kc: &ra_core::expr::ColumnRef| {
            columns.iter().any(|c| {
                matches!(&c.expr, Expr::Column(cc)
                    if cc.column.eq_ignore_ascii_case(&kc.column)
                        && cc.table == kc.table)
            })
        };
        let mut missing: Vec<ProjectionColumn> = Vec::new();
        for k in keys {
            if let Expr::Column(kc) = &k.expr {
                let dup = missing.iter().any(|c| {
                    matches!(&c.expr, Expr::Column(cc)
                        if cc.column.eq_ignore_ascii_case(&kc.column) && cc.table == kc.table)
                });
                if !col_present(kc) && !dup {
                    missing.push(ProjectionColumn { expr: k.expr.clone(), alias: None });
                }
            }
        }
        if missing.is_empty() {
            return (input.clone(), 0);
        }
        let n = missing.len();
        let mut cols = columns.clone();
        cols.extend(missing);
        (RelExpr::Project { columns: cols, input: inner.clone() }, n)
    }

    unsafe fn build_sort(
        &mut self,
        keys: &[SortKey],
        input: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        // ORDER BY may reference a column not in the SELECT list. If the input
        // is a Project, select those columns too (via the normal targetlist
        // path) and mark the appended trailing entries resjunk so PostgreSQL's
        // junk filter removes them from the query result.
        let (build_input, n_resjunk) = self.augment_sort_input(keys, input);
        let child = self.build_plan(&build_input)?;
        if n_resjunk > 0 && !child.is_null() {
            let tl = (*child).targetlist;
            let len = pg_sys::list_length(tl);
            for p in (len - n_resjunk as i32)..len {
                let te = pg_sys::list_nth(tl, p).cast::<pg_sys::TargetEntry>();
                if !te.is_null() {
                    (*te).resjunk = true;
                }
            }
        }
        let node = self.alloc_node::<pg_sys::Sort>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("Sort allocation".to_string()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Sort;
        (*node).plan.lefttree = child;
        // Sort passes tuples through unchanged: share the child targetlist
        // verbatim (PG's make_sort does `plan->targetlist = lefttree->targetlist`).
        // Without this the executor reads an empty targetlist and writes past
        // the tuple slot ("write past chunk end").
        if !child.is_null() {
            (*node).plan.targetlist = (*child).targetlist;
        }

        // Build sort column metadata arrays using sort_utils
        let child_tlist = if child.is_null() {
            std::ptr::null_mut()
        } else {
            (*child).targetlist
        };
        let rel_oid = self.first_rel_oid(input);

        if let Some(arrays) = crate::sort_utils::build_sort_arrays(keys, child_tlist, rel_oid) {
            // Resolve keys that name an output column verbatim (e.g. ORDER BY a
            // selected aggregate). For those, fix the column index, operator and
            // collation from the child targetlist entry — build_sort_arrays
            // leaves a non-column key with an InvalidOid operator.
            let positions: Vec<Option<i16>> = keys
                .iter()
                .map(|k| Self::sort_output_position(&k.expr, input))
                .collect();
            for (i, key) in keys.iter().enumerate() {
                let Some(p) = positions[i] else { continue };
                let te = pg_sys::list_nth(child_tlist, i32::from(p) - 1).cast::<pg_sys::TargetEntry>();
                if te.is_null() || (*te).expr.is_null() {
                    continue;
                }
                let ty = pg_sys::exprType((*te).expr.cast());
                let asc = matches!(key.direction, ra_core::algebra::SortDirection::Asc);
                *arrays.col_idx.add(i) = p;
                *arrays.operators.add(i) = crate::sort_utils::resolve_sort_operator(ty, asc);
                *arrays.collations.add(i) = pg_sys::exprCollation((*te).expr.cast());
            }
            // Every sort key must map to an output column; otherwise the sort
            // index dangles (ORDER BY a non-selected column needs a resjunk
            // targetlist entry Ra does not build). Defer to native PG.
            if !self.resolve_sort_indices(keys, &positions, child_tlist, arrays.col_idx, arrays.num_cols) {
                return Err(PlanBuilderError::UnsupportedVariant(
                    "ORDER BY references a column not in the output; deferring to \
                     native planner (see docs/planner-fallback-backlog.md)"
                        .to_owned(),
                ));
            }
            (*node).numCols = arrays.num_cols;
            (*node).sortColIdx = arrays.col_idx;
            (*node).sortOperators = arrays.operators;
            (*node).collations = arrays.collations;
            (*node).nullsFirst = arrays.nulls_first;
        } else {
            (*node).numCols = keys.len() as i32;
        }

        // Cost: n*log(n) comparison cost on top of child
        if !child.is_null() {
            let n = (*child).plan_rows.max(1.0);
            (*node).plan.startup_cost = (*child).total_cost + n * n.ln().max(1.0) * 0.001;
            (*node).plan.total_cost = (*node).plan.startup_cost;
            (*node).plan.plan_rows = (*child).plan_rows;
            (*node).plan.plan_width = (*child).plan_width;
        }
        Ok(&mut (*node).plan as *mut pg_sys::Plan)
    }

    unsafe fn build_limit(
        &mut self,
        count: u64,
        offset: u64,
        input: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let child = self.build_plan(input)?;
        let node = self.alloc_node::<pg_sys::Limit>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "Limit allocation".to_string(),
            ));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Limit;
        (*node).plan.lefttree = child;
        // Limit passes tuples through unchanged: share the child targetlist.
        if !child.is_null() {
            (*node).plan.targetlist = (*child).targetlist;
        }

        // Build Const nodes for limitCount and limitOffset. count == u64::MAX
        // is the "no count limit" sentinel (OFFSET without LIMIT) → leave
        // limitCount null so the executor returns all rows past the offset.
        if count != u64::MAX {
            (*node).limitCount = make_int8_const(count as i64).cast();
        }
        if offset > 0 {
            (*node).limitOffset = make_int8_const(offset as i64).cast();
        }

        if !child.is_null() {
            (*node).plan.total_cost = (*child).total_cost;
            (*node).plan.plan_rows = if count == u64::MAX {
                (*child).plan_rows
            } else {
                count as f64
            };
        }
        Ok(&mut (*node).plan as *mut pg_sys::Plan)
    }

    // -----------------------------------------------------------------------
    // Parallel variants
    // -----------------------------------------------------------------------

    unsafe fn build_parallel_seq_scan(
        &mut self,
        table: &str,
        _workers: usize,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let plan = self.build_seq_scan(table, None)?;
        if !plan.is_null() {
            (*plan).parallel_aware = true;
            (*plan).parallel_safe = true;
        }
        Ok(plan)
    }

    unsafe fn build_parallel_hash_join(
        &mut self,
        join_type: JoinType,
        condition: &Expr,
        left: &RelExpr,
        right: &RelExpr,
        _workers: usize,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let plan = self.build_join(join_type, condition, left, right)?;
        if !plan.is_null() {
            (*plan).parallel_aware = true;
            (*plan).parallel_safe = true;
        }
        Ok(plan)
    }

    unsafe fn build_parallel_aggregate(
        &mut self,
        group_by: &[ra_core::expr::Expr],
        aggregates: &[AggregateExpr],
        input: &RelExpr,
        workers: usize,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let plan = self.build_aggregate(group_by, aggregates, input)?;
        if !plan.is_null() {
            (*plan).parallel_aware = true;
            // Partial aggregation split for parallel execution
            let agg = plan as *mut pg_sys::Agg;
            (*agg).aggsplit = pg_sys::AggSplit::AGGSPLIT_INITIAL_SERIAL;
            let _ = workers;
        }
        Ok(plan)
    }

    unsafe fn build_gather(
        &mut self,
        input: &RelExpr,
        workers: usize,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let child = self.build_plan(input)?;
        let node = self.alloc_node::<pg_sys::Gather>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "Gather allocation".to_string(),
            ));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Gather;
        (*node).plan.lefttree = child;
        (*node).num_workers = workers as i32;
        if !child.is_null() {
            (*node).plan.total_cost = (*child).total_cost;
            (*node).plan.plan_rows = (*child).plan_rows;
        }
        Ok(&mut (*node).plan as *mut pg_sys::Plan)
    }

    // -----------------------------------------------------------------------
    // Set operations, Distinct, Window
    // -----------------------------------------------------------------------

    /// Build a `Sort` over `child` ordered by all of `child`'s output
    /// columns (ascending, NULLS LAST). Used to give `Unique` the sorted
    /// input it requires (PG's Sort+Unique strategy for `SELECT DISTINCT`).
    unsafe fn build_sort_all_columns(
        &mut self,
        child: *mut pg_sys::Plan,
    ) -> *mut pg_sys::Plan {
        if child.is_null() || (*child).targetlist.is_null() {
            return child;
        }
        let ncols = (*(*child).targetlist).length;
        if ncols == 0 {
            return child;
        }
        let node = self.alloc_node::<pg_sys::Sort>();
        if node.is_null() {
            return child;
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Sort;
        (*node).plan.lefttree = child;
        (*node).plan.targetlist = (*child).targetlist;
        let col_idx = pg_sys::palloc(ncols as usize * std::mem::size_of::<pg_sys::AttrNumber>())
            as *mut pg_sys::AttrNumber;
        let operators =
            pg_sys::palloc(ncols as usize * std::mem::size_of::<pg_sys::Oid>()) as *mut pg_sys::Oid;
        let collations =
            pg_sys::palloc(ncols as usize * std::mem::size_of::<pg_sys::Oid>()) as *mut pg_sys::Oid;
        let nulls_first = pg_sys::palloc(ncols as usize * std::mem::size_of::<bool>()) as *mut bool;
        let elements = (*(*child).targetlist).elements;
        for i in 0..ncols as usize {
            let te = (*elements.add(i)).ptr_value as *mut pg_sys::TargetEntry;
            let (resno, type_oid, coll) = if te.is_null() || (*te).expr.is_null() {
                ((i + 1) as pg_sys::AttrNumber, pg_sys::INT4OID, pg_sys::InvalidOid)
            } else {
                (
                    (*te).resno,
                    pg_sys::exprType((*te).expr.cast()),
                    pg_sys::exprCollation((*te).expr.cast()),
                )
            };
            *col_idx.add(i) = resno;
            *operators.add(i) = crate::sort_utils::resolve_sort_operator(type_oid, true);
            *collations.add(i) = coll;
            *nulls_first.add(i) = false;
        }
        (*node).numCols = ncols;
        (*node).sortColIdx = col_idx;
        (*node).sortOperators = operators;
        (*node).collations = collations;
        (*node).nullsFirst = nulls_first;
        (*node).plan.total_cost = (*child).total_cost;
        (*node).plan.plan_rows = (*child).plan_rows;
        (*node).plan.plan_width = (*child).plan_width;
        &mut (*node).plan as *mut pg_sys::Plan
    }

    unsafe fn build_unique(
        &mut self,
        input: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let raw_child = self.build_plan(input)?;
        Ok(self.dedup_plan(raw_child))
    }

    /// Build `DISTINCT ON (keys)` as a `Unique` on the key columns over the
    /// already-sorted input (the `DistinctOn` input is the ORDER BY `Sort`, so
    /// rows sharing the keys are adjacent). Keys must be plain columns present
    /// in the output; otherwise defer to PG.
    unsafe fn build_distinct_on(
        &mut self,
        on: &[Expr],
        input: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        if on.is_empty() {
            return Err(unsupported("DISTINCT ON without keys"));
        }
        let child = self.build_plan(input)?;
        if child.is_null() {
            return Err(unsupported("DISTINCT ON input"));
        }
        let child_tlist = (*child).targetlist;
        let n = on.len();
        let col_idx =
            pg_sys::palloc(n * std::mem::size_of::<pg_sys::AttrNumber>()) as *mut pg_sys::AttrNumber;
        let operators = pg_sys::palloc(n * std::mem::size_of::<pg_sys::Oid>()) as *mut pg_sys::Oid;
        let collations = pg_sys::palloc(n * std::mem::size_of::<pg_sys::Oid>()) as *mut pg_sys::Oid;
        for (i, key) in on.iter().enumerate() {
            let name = crate::sort_utils::extract_column_name(key)
                .ok_or_else(|| unsupported("DISTINCT ON key not a column"))?;
            let resno = crate::sort_utils::find_attr_in_targetlist(child_tlist, name)
                .ok_or_else(|| unsupported("DISTINCT ON key not in output"))?;
            let te = pg_sys::list_nth(child_tlist, i32::from(resno) - 1).cast::<pg_sys::TargetEntry>();
            let (ty, coll) = if te.is_null() || (*te).expr.is_null() {
                (pg_sys::INT4OID, pg_sys::InvalidOid)
            } else {
                (
                    pg_sys::exprType((*te).expr.cast()),
                    pg_sys::exprCollation((*te).expr.cast()),
                )
            };
            *col_idx.add(i) = resno;
            *operators.add(i) = crate::sort_utils::resolve_equality_op(ty);
            *collations.add(i) = coll;
        }
        let node = self.alloc_node::<pg_sys::Unique>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("Unique".to_owned()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Unique;
        (*node).plan.lefttree = child;
        // Unique passes tuples through unchanged: share the child targetlist.
        (*node).plan.targetlist = child_tlist;
        (*node).numCols = n as i32;
        (*node).uniqColIdx = col_idx;
        (*node).uniqOperators = operators;
        (*node).uniqCollations = collations;
        (*node).plan.total_cost = (*child).total_cost;
        (*node).plan.plan_rows = ((*child).plan_rows * 0.5).max(1.0);
        (*node).plan.plan_width = (*child).plan_width;
        Ok(&mut (*node).plan as *mut pg_sys::Plan)
    }

    /// Deduplicate a plan's rows on all output columns: sort by every output
    /// column then apply `Unique` (PG's Sort+Unique strategy). Used by both
    /// `SELECT DISTINCT` and `UNION` (distinct).
    unsafe fn dedup_plan(&mut self, raw_child: *mut pg_sys::Plan) -> *mut pg_sys::Plan {
        // Unique only collapses *adjacent* equal rows, so its input must be
        // sorted on the distinct columns. Sort by all output columns first.
        let child = self.build_sort_all_columns(raw_child);
        let node = self.alloc_node::<pg_sys::Unique>();
        if node.is_null() {
            return raw_child;
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Unique;
        (*node).plan.lefttree = child;
        // Unique passes tuples through unchanged: share the child targetlist.
        if !child.is_null() {
            (*node).plan.targetlist = (*child).targetlist;
        }

        // Build unique column arrays from child's targetlist
        // Unique operates on ALL output columns (SELECT DISTINCT)
        if !child.is_null() && !(*child).targetlist.is_null() {
            let ncols = (*(*child).targetlist).length;

            if ncols > 0 {
                let col_idx =
                    pg_sys::palloc(ncols as usize * std::mem::size_of::<pg_sys::AttrNumber>())
                        as *mut pg_sys::AttrNumber;
                let operators = pg_sys::palloc(ncols as usize * std::mem::size_of::<pg_sys::Oid>())
                    as *mut pg_sys::Oid;
                let collations = pg_sys::palloc(ncols as usize * std::mem::size_of::<pg_sys::Oid>())
                    as *mut pg_sys::Oid;

                let elements = (*(*child).targetlist).elements;
                for i in 0..ncols as usize {
                    let cell = elements.add(i);
                    let te = (*cell).ptr_value as *mut pg_sys::TargetEntry;
                    let resno = if te.is_null() {
                        (i + 1) as pg_sys::AttrNumber
                    } else {
                        (*te).resno
                    };
                    *col_idx.add(i) = resno;

                    // Get type info for equality operator
                    let type_oid = if !te.is_null() && !(*te).expr.is_null() {
                        pg_sys::exprType((*te).expr as *mut pg_sys::Node)
                    } else {
                        pg_sys::INT4OID
                    };
                    *operators.add(i) = crate::sort_utils::resolve_equality_op(type_oid);
                    *collations.add(i) = pg_sys::exprCollation(if !te.is_null() {
                        (*te).expr as *mut pg_sys::Node
                    } else {
                        std::ptr::null_mut()
                    });
                }

                (*node).numCols = ncols;
                (*node).uniqColIdx = col_idx;
                (*node).uniqOperators = operators;
                (*node).uniqCollations = collations;
            }

            (*node).plan.total_cost = (*child).total_cost;
            (*node).plan.plan_rows = ((*child).plan_rows * 0.75).max(1.0);
            (*node).plan.plan_width = (*child).plan_width;
        }
        &mut (*node).plan as *mut pg_sys::Plan
    }

    /// Build a fresh "dummy" targetlist projecting a child plan's output
    /// unchanged: one `OUTER_VAR` `Var` per child column (`varattno` =
    /// position), copying type/typmod/collation/resname/resjunk from the
    /// child's targetlist entry.
    ///
    /// This mirrors PostgreSQL's `set_dummy_tlist_references`, which the core
    /// planner applies to nodes that return their child's tuples verbatim
    /// (`Append`, `MergeAppend`, `SetOp`, `Sort`, `Unique`, ...). Ra bypasses
    /// the planner's setrefs pass, so assigning the child's targetlist
    /// directly leaves the parent's output `Var`s carrying the child's own
    /// (scan/special) varnos. For a parent whose first child *owns* that same
    /// list (an `Append` aliasing its first subplan's tlist), an `OUTER_VAR`
    /// entry then resolves — in the parent's deparse context, where
    /// `outer_plan` is that first child — straight back into the same list,
    /// producing a self-referential `resolve_special_varno` cycle that
    /// recurses until `check_stack_depth` (observed as ~200ms of set-op
    /// planning). Emitting fresh `OUTER_VAR` refs resolves each column one
    /// level down to the first child and terminates.
    unsafe fn dummy_outer_tlist(&self, child_tlist: *mut pg_sys::List) -> *mut pg_sys::List {
        let len = pg_sys::list_length(child_tlist);
        let mut out: *mut pg_sys::List = std::ptr::null_mut();
        for p in 1..=len {
            let te = pg_sys::list_nth(child_tlist, p - 1).cast::<pg_sys::TargetEntry>();
            if te.is_null() || (*te).expr.is_null() {
                continue;
            }
            let var = self.alloc_node::<pg_sys::Var>();
            (*var).xpr.type_ = pg_sys::NodeTag::T_Var;
            (*var).varno = pg_sys::OUTER_VAR;
            (*var).varattno = p as i16;
            (*var).vartype = pg_sys::exprType((*te).expr.cast());
            (*var).vartypmod = pg_sys::exprTypmod((*te).expr.cast());
            (*var).varcollid = pg_sys::exprCollation((*te).expr.cast());
            (*var).varlevelsup = 0;
            let nt = pg_sys::makeTargetEntry(var.cast(), p as i16, (*te).resname, (*te).resjunk);
            out = pg_sys::lappend(out, nt.cast());
        }
        out
    }

    /// Collect the leaf branches of a `UNION ALL` spine. Nested `UNION ALL`
    /// nodes are flattened; any other node (including `UNION DISTINCT`) is a
    /// leaf. Flattening lets [`Self::unify_setop_null_types_global`] resolve
    /// column types across every branch at once — NULL pads are retypable
    /// `Const`s at the leaves, but become opaque `Var`s once wrapped in a
    /// nested `Append`, so pairwise resolution is fragile to branch ordering
    /// (which the e-graph may permute).
    fn collect_union_all_leaves<'a>(expr: &'a RelExpr, out: &mut Vec<&'a RelExpr>) {
        if let RelExpr::Union {
            all: true,
            left,
            right,
        } = expr
        {
            Self::collect_union_all_leaves(left, out);
            Self::collect_union_all_leaves(right, out);
        } else {
            out.push(expr);
        }
    }

    /// Unify column types across all arms of a set operation, order-independent.
    /// For each column, the concrete (non-NULL) type found in any arm is the
    /// resolved type; every NULL `Const` in that column (a grouping-set NULL
    /// pad, or a literal NULL) is retyped to it, so the Append's output
    /// descriptor (taken from the first arm) is correct and all arms agree.
    /// Returns `false` if two arms carry different concrete types for a column
    /// (irreconcilable here — defer to PG).
    unsafe fn unify_setop_null_types_global(&self, plans: &[*mut pg_sys::Plan]) -> bool {
        let Some(&first) = plans.first() else {
            return true;
        };
        let ncols = pg_sys::list_length((*first).targetlist);
        for i in 0..ncols {
            let mut concrete: Option<(pg_sys::Oid, i32, pg_sys::Oid)> = None;
            for &p in plans {
                let te = pg_sys::list_nth((*p).targetlist, i).cast::<pg_sys::TargetEntry>();
                if te.is_null() {
                    continue;
                }
                let e = (*te).expr;
                if !Self::is_null_const(e) {
                    let t = pg_sys::exprType(e.cast());
                    match concrete {
                        None => {
                            concrete = Some((
                                t,
                                pg_sys::exprTypmod(e.cast()),
                                pg_sys::exprCollation(e.cast()),
                            ));
                        }
                        Some((ct, _, _)) if ct != t => return false,
                        _ => {}
                    }
                }
            }
            if let Some((t, tm, coll)) = concrete {
                for &p in plans {
                    let te = pg_sys::list_nth((*p).targetlist, i).cast::<pg_sys::TargetEntry>();
                    if !te.is_null() && Self::is_null_const((*te).expr) {
                        Self::retype_null_const((*te).expr, t, tm, coll);
                    }
                }
            }
        }
        true
    }

    /// True if `expr` is a NULL `Const` node.
    unsafe fn is_null_const(expr: *mut pg_sys::Expr) -> bool {
        !expr.is_null()
            && (*expr).type_ == pg_sys::NodeTag::T_Const
            && (*expr.cast::<pg_sys::Const>()).constisnull
    }

    /// Retype a NULL `Const` in place to the given type/typmod/collation.
    unsafe fn retype_null_const(expr: *mut pg_sys::Expr, typ: pg_sys::Oid, typmod: i32, coll: pg_sys::Oid) {
        let c = expr.cast::<pg_sys::Const>();
        let mut typlen: i16 = 0;
        let mut typbyval = false;
        pg_sys::get_typlenbyval(typ, &mut typlen, &mut typbyval);
        (*c).consttype = typ;
        (*c).consttypmod = typmod;
        (*c).constcollid = coll;
        (*c).constlen = i32::from(typlen);
        (*c).constbyval = typbyval;
    }

    unsafe fn build_set_op_union(
        &mut self,
        all: bool,
        left: &RelExpr,
        right: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        // Flatten a UNION ALL spine into all leaf branches so column types can
        // be resolved globally; UNION DISTINCT stays a 2-way Append + dedup.
        let mut leaves: Vec<&RelExpr> = Vec::new();
        if all {
            Self::collect_union_all_leaves(left, &mut leaves);
            Self::collect_union_all_leaves(right, &mut leaves);
        } else {
            leaves.push(left);
            leaves.push(right);
        }
        let mut child_plans: Vec<*mut pg_sys::Plan> = Vec::with_capacity(leaves.len());
        for leaf in &leaves {
            let p = self.build_plan(leaf)?;
            if p.is_null() {
                return Err(PlanBuilderError::UnsupportedVariant("union child".to_owned()));
            }
            child_plans.push(p);
        }
        // Set-operation column type unification: NULL Consts (grouping-set NULL
        // pads, literal NULLs) adopt the concrete type found in any sibling
        // branch, so the Append's output descriptor — taken from the first
        // branch — is correctly typed and every branch agrees. An irreconcilable
        // mismatch between two concrete types defers to PG.
        if !self.unify_setop_null_types_global(&child_plans) {
            return Err(PlanBuilderError::UnsupportedVariant(
                "set-op column type mismatch".to_owned(),
            ));
        }
        let node = self.alloc_node::<pg_sys::Append>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "Append allocation".to_string(),
            ));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Append;
        // No run-time partition pruning: PG uses -1 here, but palloc0 zeroes it
        // to 0, which makes ExecInitAppend index an empty es_part_prune_infos[0]
        // and crash (even under EXPLAIN's EXEC_FLAG_EXPLAIN_ONLY init). Set -1.
        (*node).part_prune_index = -1;
        let mut plans_list = std::ptr::null_mut::<pg_sys::List>();
        let mut total_cost = 0.0;
        let mut total_rows = 0.0;
        for &p in &child_plans {
            plans_list = pg_sys::lappend(plans_list, p.cast());
            total_cost += (*p).total_cost;
            total_rows += (*p).plan_rows;
        }
        (*node).appendplans = plans_list;
        // All subplans are non-partial (Ra emits no parallel-aware Append), so
        // the first partial plan is past the end. palloc0 leaves this 0, which
        // would make the executor treat every subplan as partial.
        (*node).first_partial_plan = pg_sys::list_length(plans_list);
        (*node).plan.total_cost = total_cost;
        (*node).plan.plan_rows = total_rows;
        // Append returns child slots directly; its targetlist supplies the
        // result tuple descriptor (column types) and the deparse/plan-ref
        // column references. Emit fresh OUTER_VAR refs (not the first child's
        // aliased tlist) so PG resolves each output column one level down to
        // the first subplan — see `dummy_outer_tlist`.
        let first = child_plans[0];
        (*node).plan.targetlist = self.dummy_outer_tlist((*first).targetlist);
        (*node).plan.plan_width = (*first).plan_width;
        let append_plan = &mut (*node).plan as *mut pg_sys::Plan;

        if all {
            Ok(append_plan)
        } else {
            // UNION (distinct): deduplicate the appended rows.
            Ok(self.dedup_plan(append_plan))
        }
    }

    unsafe fn build_set_op_intersect(
        &mut self,
        all: bool,
        left: &RelExpr,
        right: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let cmd = if all {
            pg_sys::SetOpCmd::SETOPCMD_INTERSECT_ALL
        } else {
            pg_sys::SetOpCmd::SETOPCMD_INTERSECT
        };
        self.build_setop_node(cmd, left, right)
    }

    unsafe fn build_set_op_except(
        &mut self,
        all: bool,
        left: &RelExpr,
        right: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let cmd = if all {
            pg_sys::SetOpCmd::SETOPCMD_EXCEPT_ALL
        } else {
            pg_sys::SetOpCmd::SETOPCMD_EXCEPT
        };
        self.build_setop_node(cmd, left, right)
    }

    /// Build a PG18 `SetOp` (INTERSECT/EXCEPT [ALL]). PG18's hashed SetOp
    /// takes two children directly and compares all output columns with
    /// equality operators — no flag column or sorted input required.
    unsafe fn build_setop_node(
        &mut self,
        cmd: pg_sys::SetOpCmd::Type,
        left: &RelExpr,
        right: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let left_plan = self.build_plan(left)?;
        let right_plan = self.build_plan(right)?;
        if left_plan.is_null() || right_plan.is_null() || (*left_plan).targetlist.is_null() {
            return Err(PlanBuilderError::UnsupportedVariant("setop child".to_owned()));
        }
        let node = self.alloc_node::<pg_sys::SetOp>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("SetOp".to_owned()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_SetOp;
        (*node).cmd = cmd;
        (*node).strategy = pg_sys::SetOpStrategy::SETOP_HASHED;
        (*node).plan.lefttree = left_plan;
        (*node).plan.righttree = right_plan;
        // Fresh OUTER_VAR refs (see `dummy_outer_tlist`); the cmp arrays below
        // still index the left child's own targetlist by resno.
        (*node).plan.targetlist = self.dummy_outer_tlist((*left_plan).targetlist);
        (*node).plan.plan_width = (*left_plan).plan_width;

        // Compare on all output columns with equality operators.
        let tlist = (*left_plan).targetlist;
        let ncols = (*tlist).length;
        let col_idx = pg_sys::palloc(ncols as usize * std::mem::size_of::<pg_sys::AttrNumber>())
            as *mut pg_sys::AttrNumber;
        let ops = pg_sys::palloc(ncols as usize * std::mem::size_of::<pg_sys::Oid>())
            as *mut pg_sys::Oid;
        let colls = pg_sys::palloc(ncols as usize * std::mem::size_of::<pg_sys::Oid>())
            as *mut pg_sys::Oid;
        let nulls = pg_sys::palloc(ncols as usize * std::mem::size_of::<bool>()) as *mut bool;
        let elements = (*tlist).elements;
        for i in 0..ncols as usize {
            let te = (*elements.add(i)).ptr_value as *mut pg_sys::TargetEntry;
            let (resno, ty, coll) = if te.is_null() || (*te).expr.is_null() {
                ((i + 1) as pg_sys::AttrNumber, pg_sys::INT4OID, pg_sys::InvalidOid)
            } else {
                (
                    (*te).resno,
                    pg_sys::exprType((*te).expr.cast()),
                    pg_sys::exprCollation((*te).expr.cast()),
                )
            };
            *col_idx.add(i) = resno;
            *ops.add(i) = crate::sort_utils::resolve_equality_op(ty);
            *colls.add(i) = coll;
            *nulls.add(i) = false;
        }
        (*node).numCols = ncols;
        (*node).cmpColIdx = col_idx;
        (*node).cmpOperators = ops;
        (*node).cmpCollations = colls;
        (*node).cmpNullsFirst = nulls;
        let ngroups = (*left_plan).plan_rows.max(1.0).clamp(1.0, 1_000_000.0);
        // SetOp.numGroups: c_long on pg13..pg18, Cardinality (f64) on pg19+.
        #[cfg(not(feature = "pg19"))]
        {
            (*node).numGroups = ngroups as std::os::raw::c_long;
        }
        #[cfg(feature = "pg19")]
        {
            (*node).numGroups = ngroups;
        }
        (*node).plan.plan_rows = (*left_plan).plan_rows;
        (*node).plan.total_cost = (*left_plan).total_cost + (*right_plan).total_cost;
        Ok(&mut (*node).plan as *mut pg_sys::Plan)
    }

    /// Build a `WindowFunc` node for a supported window function. `arg`
    /// describes the single OUTER argument (or `None` for no-arg /
    /// star functions). `winref` links it to the `WindowAgg`.
    unsafe fn build_window_func(
        &self,
        func: &ra_core::algebra::WindowFunction,
        arg: Option<(*mut pg_sys::Expr, pg_sys::Oid, pg_sys::Oid)>,
        extra_args: &[(*mut pg_sys::Expr, pg_sys::Oid)],
        winref: pg_sys::Index,
    ) -> Result<*mut pg_sys::Expr, PlanBuilderError> {
        use ra_core::algebra::WindowFunction as Wf;
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        let name = match func {
            Wf::RowNumber => "row_number",
            Wf::Rank => "rank",
            Wf::DenseRank => "dense_rank",
            Wf::PercentRank => "percent_rank",
            Wf::Ntile => "ntile",
            Wf::Lag => "lag",
            Wf::Lead => "lead",
            Wf::FirstValue => "first_value",
            Wf::LastValue => "last_value",
            Wf::Sum => "sum",
            Wf::Count => "count",
            Wf::Avg => "avg",
            Wf::Min => "min",
            Wf::Max => "max",
            _ => return Err(unsupported("window function")),
        };
        let actual_arg_ty = arg.map(|(_, ty, _)| ty);
        // The value window functions (lag/lead/first_value/last_value) have a
        // polymorphic `anyelement` first parameter in the catalog, so a lookup
        // by the actual argument type (e.g. numeric) finds no exact match. Look
        // them up by the polymorphic signature and resolve the result type from
        // the actual argument below. lag/lead additionally take an integer
        // offset (and optional default); the offset is carried in `extra_args`
        // (decoded from the parser's __win_args marker), so the correct arity
        // is built rather than silently dropping it.
        let polymorphic_value_fn =
            matches!(func, Wf::FirstValue | Wf::LastValue | Wf::Lag | Wf::Lead);
        let mut argtypes: Vec<pg_sys::Oid> = Vec::with_capacity(1 + extra_args.len());
        if let Some((_, ty, _)) = arg {
            argtypes.push(if polymorphic_value_fn {
                pg_sys::ANYELEMENTOID
            } else {
                ty
            });
        }
        for &(_, ty) in extra_args {
            argtypes.push(ty);
        }
        let nargs = argtypes.len() as i32;
        let fname = CString::new(name).map_err(|_| unsupported("window fn name"))?;
        let name_node = pg_sys::makeString(fname.as_ptr().cast_mut());
        let fname_list = pg_sys::lappend(std::ptr::null_mut(), name_node.cast());
        let winfnoid = pg_sys::LookupFuncName(fname_list, nargs, argtypes.as_ptr(), true);
        if winfnoid == pg_sys::InvalidOid {
            return Err(unsupported("window fn lookup"));
        }
        let prokind = pg_sys::get_func_prokind(winfnoid);
        let winagg = prokind == pg_sys::PROKIND_AGGREGATE as i8;
        if !winagg && prokind != pg_sys::PROKIND_WINDOW as i8 {
            return Err(unsupported("not a window/agg function"));
        }
        // A polymorphic return type resolves to the actual argument type.
        let mut wintype = pg_sys::get_func_rettype(winfnoid);
        if matches!(
            wintype,
            pg_sys::ANYELEMENTOID | pg_sys::ANYCOMPATIBLEOID | pg_sys::ANYARRAYOID
        ) {
            if let Some(ty) = actual_arg_ty {
                wintype = ty;
            }
        }
        let node = self.alloc_node::<pg_sys::WindowFunc>();
        (*node).xpr.type_ = pg_sys::NodeTag::T_WindowFunc;
        (*node).winfnoid = winfnoid;
        (*node).wintype = wintype;
        (*node).wincollid = pg_sys::get_typcollation(wintype);
        (*node).winref = winref;
        (*node).winstar = arg.is_none() && matches!(func, Wf::Count);
        (*node).winagg = winagg;
        if let Some((arg_expr, _, coll)) = arg {
            (*node).inputcollid = coll;
            let mut args = pg_sys::lappend(std::ptr::null_mut(), arg_expr.cast());
            for &(e, _) in extra_args {
                args = pg_sys::lappend(args, e.cast());
            }
            (*node).args = args;
        }
        Ok(node.cast())
    }

    /// Build a `WindowAgg` for `Window([wf])` over `Project(out_columns)`.
    /// Exposes the referenced columns on the scan, sorts by
    /// PARTITION BY ++ ORDER BY (WindowAgg requires sorted input), then
    /// builds the WindowAgg whose targetlist carries passthrough Vars and
    /// the WindowFunc. Returns `Err` (→ native planner) for multiple window
    /// specs, non-default frames, unsupported functions, or non-scan input.
    unsafe fn build_window_plan(
        &mut self,
        functions: &[ra_core::algebra::WindowExpr],
        out_columns: &[ProjectionColumn],
        input: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        let [wf] = functions else {
            return Err(unsupported("multiple window functions"));
        };
        if wf.frame.is_some() {
            return Err(unsupported("non-default window frame"));
        }
        // Order keys must be plain columns (operator/index resolution).
        if !wf.order_by.iter().all(|k| matches!(k.expr, Expr::Column(_)))
            || !wf.partition_by.iter().all(|e| matches!(e, Expr::Column(_)))
        {
            return Err(unsupported("window partition/order not plain columns"));
        }
        let child = self.build_plan(input)?;
        if child.is_null() {
            return Err(unsupported("window input"));
        }
        let mut in_tlist: *mut pg_sys::List = std::ptr::null_mut();
        let mut colmap: Vec<(i16, i32, pg_sys::Oid, pg_sys::Oid)> = Vec::new();

        // Pass 1: register all needed input columns (passthrough outputs,
        // partition, order, window arg) so the scan exposes them.
        for pc in out_columns {
            if !Self::is_window_marker(&pc.expr) {
                if self.add_input_col(&pc.expr, &mut in_tlist, &mut colmap).is_none() {
                    return Err(unsupported("window passthrough column"));
                }
            }
        }
        let mut sort_keys: Vec<(i32, pg_sys::Oid, pg_sys::Oid, bool, bool)> = Vec::new();
        let mut part_pos: Vec<(i32, pg_sys::Oid, pg_sys::Oid)> = Vec::new();
        for p in &wf.partition_by {
            let t = self
                .add_input_col(p, &mut in_tlist, &mut colmap)
                .ok_or_else(|| unsupported("partition col"))?;
            part_pos.push(t);
            sort_keys.push((t.0, t.1, t.2, true, false));
        }
        let mut ord_pos: Vec<(i32, pg_sys::Oid, pg_sys::Oid)> = Vec::new();
        for k in &wf.order_by {
            let t = self
                .add_input_col(&k.expr, &mut in_tlist, &mut colmap)
                .ok_or_else(|| unsupported("order col"))?;
            ord_pos.push(t);
            let asc = matches!(k.direction, ra_core::algebra::SortDirection::Asc);
            let nf = matches!(k.nulls, ra_core::algebra::NullOrdering::First);
            sort_keys.push((t.0, t.1, t.2, asc, nf));
        }
        let (arg_outer, extra_args) = match &wf.arg {
            // lag/lead carry their offset (and optional default) in a
            // __win_args(value, offset, ...) marker. The value becomes the
            // window input column; the offset/default must be constants
            // (translated without a column frame), else defer to PG.
            Some(Expr::Function { name, args }) if name == "__win_args" => {
                let value = args.first().ok_or_else(|| unsupported("window arg"))?;
                let (pos, ty, coll) = self
                    .add_input_col(value, &mut in_tlist, &mut colmap)
                    .ok_or_else(|| unsupported("window arg"))?;
                let mut extra: Vec<(*mut pg_sys::Expr, pg_sys::Oid)> = Vec::new();
                if args.len() - 1 > 1 {
                    // offset + default (3-arg lag/lead) not yet supported.
                    return Err(unsupported("window default arg"));
                }
                for a in &args[1..] {
                    let e = expr_translator::translate(a, &self.expr_ctx);
                    if e.is_null() || (*e).type_ != pg_sys::NodeTag::T_Const {
                        return Err(unsupported("window offset not constant"));
                    }
                    extra.push((e, pg_sys::exprType(e.cast())));
                }
                (Some((self.outer_var(pos, ty, coll), ty, coll)), extra)
            }
            Some(a) => {
                let (pos, ty, coll) = self
                    .add_input_col(a, &mut in_tlist, &mut colmap)
                    .ok_or_else(|| unsupported("window arg"))?;
                (
                    Some((self.outer_var(pos, ty, coll), ty, coll)),
                    Vec::new(),
                )
            }
            None => (None, Vec::new()),
        };
        (*child).targetlist = in_tlist;

        // WindowAgg requires its input sorted on PARTITION BY ++ ORDER BY.
        let sorted = self.build_keyed_sort(child, &sort_keys);

        let node = self.alloc_node::<pg_sys::WindowAgg>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("WindowAgg".to_owned()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_WindowAgg;
        (*node).plan.lefttree = sorted;
        (*node).winref = 1;
        // EXPLAIN's show_window_def calls quote_identifier(winname); a NULL
        // winname (PG always names the window, e.g. "w1") segfaults the
        // deparse. Name it to match the single winref we emit.
        if let Ok(wn) = std::ffi::CString::new("w1") {
            (*node).winname = pg_sys::pstrdup(wn.as_ptr());
        }
        (*node).frameOptions = pg_sys::FRAMEOPTION_DEFAULTS as i32;

        let set_cols = |n: usize,
                        positions: &[(i32, pg_sys::Oid, pg_sys::Oid)],
                        eq: bool|
         -> (*mut pg_sys::AttrNumber, *mut pg_sys::Oid, *mut pg_sys::Oid) {
            let col_idx = pg_sys::palloc(n * std::mem::size_of::<pg_sys::AttrNumber>())
                as *mut pg_sys::AttrNumber;
            let ops = pg_sys::palloc(n * std::mem::size_of::<pg_sys::Oid>()) as *mut pg_sys::Oid;
            let colls = pg_sys::palloc(n * std::mem::size_of::<pg_sys::Oid>()) as *mut pg_sys::Oid;
            for (i, &(pos, ty, coll)) in positions.iter().enumerate() {
                *col_idx.add(i) = pos as i16;
                *ops.add(i) = if eq {
                    crate::sort_utils::resolve_equality_op(ty)
                } else {
                    crate::sort_utils::resolve_sort_operator(ty, true)
                };
                *colls.add(i) = coll;
            }
            (col_idx, ops, colls)
        };
        if !part_pos.is_empty() {
            let (ci, op, co) = set_cols(part_pos.len(), &part_pos, true);
            (*node).partNumCols = part_pos.len() as i32;
            (*node).partColIdx = ci;
            (*node).partOperators = op;
            (*node).partCollations = co;
        }
        if !ord_pos.is_empty() {
            let (ci, op, co) = set_cols(ord_pos.len(), &ord_pos, true);
            (*node).ordNumCols = ord_pos.len() as i32;
            (*node).ordColIdx = ci;
            (*node).ordOperators = op;
            (*node).ordCollations = co;
        }

        // Targetlist: passthrough Vars + the single WindowFunc.
        let mut out_tlist: *mut pg_sys::List = std::ptr::null_mut();
        for (i, pc) in out_columns.iter().enumerate() {
            let entry: *mut pg_sys::Expr = if Self::is_window_marker(&pc.expr) {
                self.build_window_func(&wf.function, arg_outer, &extra_args, 1)?
            } else {
                let (pos, ty, coll) = self
                    .add_input_col(&pc.expr, &mut in_tlist, &mut colmap)
                    .ok_or_else(|| unsupported("window output column"))?;
                self.outer_var(pos, ty, coll)
            };
            let rn = pc
                .alias
                .as_deref()
                .or_else(|| crate::sort_utils::extract_column_name(&pc.expr))
                .and_then(|n| CString::new(n).ok())
                .map_or(std::ptr::null_mut(), |c| pg_sys::pstrdup(c.as_ptr()));
            let te = pg_sys::makeTargetEntry(entry, (i + 1) as i16, rn, false);
            out_tlist = pg_sys::lappend(out_tlist, te.cast());
        }
        (*node).plan.targetlist = out_tlist;
        if !sorted.is_null() {
            (*node).plan.plan_rows = (*sorted).plan_rows;
            (*node).plan.total_cost = (*sorted).total_cost;
            (*node).plan.startup_cost = (*sorted).total_cost;
        }
        Ok(&mut (*node).plan as *mut pg_sys::Plan)
    }

    /// Is `expr` a Window-function placeholder (`__window_*`) emitted by the
    /// parser into the projection?
    fn is_window_marker(expr: &Expr) -> bool {
        matches!(expr, Expr::Function { name, .. } if name.starts_with("__window_"))
    }

    /// Build a `Sort` over `child` with explicit keys
    /// `(position, type, collation, ascending, nulls_first)`.
    unsafe fn build_keyed_sort(
        &mut self,
        child: *mut pg_sys::Plan,
        keys: &[(i32, pg_sys::Oid, pg_sys::Oid, bool, bool)],
    ) -> *mut pg_sys::Plan {
        if child.is_null() || keys.is_empty() {
            return child;
        }
        let node = self.alloc_node::<pg_sys::Sort>();
        if node.is_null() {
            return child;
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Sort;
        (*node).plan.lefttree = child;
        (*node).plan.targetlist = (*child).targetlist;
        let n = keys.len();
        let col_idx =
            pg_sys::palloc(n * std::mem::size_of::<pg_sys::AttrNumber>()) as *mut pg_sys::AttrNumber;
        let ops = pg_sys::palloc(n * std::mem::size_of::<pg_sys::Oid>()) as *mut pg_sys::Oid;
        let colls = pg_sys::palloc(n * std::mem::size_of::<pg_sys::Oid>()) as *mut pg_sys::Oid;
        let nulls = pg_sys::palloc(n * std::mem::size_of::<bool>()) as *mut bool;
        for (i, &(pos, ty, coll, asc, nf)) in keys.iter().enumerate() {
            *col_idx.add(i) = pos as i16;
            *ops.add(i) = crate::sort_utils::resolve_sort_operator(ty, asc);
            *colls.add(i) = coll;
            *nulls.add(i) = nf;
        }
        (*node).numCols = n as i32;
        (*node).sortColIdx = col_idx;
        (*node).sortOperators = ops;
        (*node).collations = colls;
        (*node).nullsFirst = nulls;
        (*node).plan.plan_rows = (*child).plan_rows;
        (*node).plan.total_cost = (*child).total_cost;
        (*node).plan.plan_width = (*child).plan_width;
        &mut (*node).plan as *mut pg_sys::Plan
    }

    unsafe fn build_window_agg(
        &mut self,
        functions: &[ra_core::algebra::WindowExpr],
        input: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let child = self.build_plan(input)?;
        let node = self.alloc_node::<pg_sys::WindowAgg>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "WindowAgg allocation".to_string(),
            ));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_WindowAgg;
        (*node).plan.lefttree = child;

        // Use the first window function's partition/order spec
        // (PostgreSQL requires one WindowAgg node per distinct window spec)
        if let Some(wf) = functions.first() {
            let child_tlist = if child.is_null() {
                std::ptr::null_mut()
            } else {
                (*child).targetlist
            };
            let rel_oid = self.first_rel_oid(input);

            // Build PARTITION BY column arrays
            if !wf.partition_by.is_empty() {
                if let Some(part_arrays) =
                    crate::sort_utils::build_group_arrays(&wf.partition_by, child_tlist, rel_oid)
                {
                    (*node).partNumCols = part_arrays.num_cols;
                    (*node).partColIdx = part_arrays.col_idx;
                    (*node).partOperators = part_arrays.operators;
                    (*node).partCollations = part_arrays.collations;
                }
            }

            // Build ORDER BY column arrays
            if !wf.order_by.is_empty() {
                if let Some(ord_arrays) =
                    crate::sort_utils::build_sort_arrays(&wf.order_by, child_tlist, rel_oid)
                {
                    (*node).ordNumCols = ord_arrays.num_cols;
                    (*node).ordColIdx = ord_arrays.col_idx;
                    (*node).ordOperators = ord_arrays.operators;
                    (*node).ordCollations = ord_arrays.collations;
                    // Note: PG 17/18 WindowAgg does NOT store per-key
                    // nulls-first flags; those are encoded in the
                    // SortGroupOperators by `get_sort_group_operators`
                    // and applied by the upstream Sort node, not
                    // re-applied here. The unused
                    // `ord_arrays.nulls_first` field is retained for
                    // diagnostics; suppress it explicitly so the
                    // unused-field warning doesn't fire.
                    let _ = ord_arrays.nulls_first;
                }
            }

            // Set frame options from the window frame spec
            (*node).frameOptions = self.translate_frame_options(&wf.frame);
        }

        // winref defaults to 0 (first window clause)
        (*node).winref = 0;

        if !child.is_null() {
            (*node).plan.total_cost = (*child).total_cost + (*child).plan_rows * 0.01;
            (*node).plan.plan_rows = (*child).plan_rows;
            (*node).plan.plan_width = (*child).plan_width;
        }
        Ok(&mut (*node).plan as *mut pg_sys::Plan)
    }

    /// Translate a Ra `WindowFrame` to PostgreSQL's FRAMEOPTION bitmask.
    fn translate_frame_options(&self, frame: &Option<ra_core::algebra::WindowFrame>) -> i32 {
        use ra_core::algebra::{WindowFrameBound, WindowFrameMode};

        // PG frame option constants (from nodes.h)
        const FRAMEOPTION_NONDEFAULT: i32 = 0x00001;
        const FRAMEOPTION_RANGE: i32 = 0x00002;
        const FRAMEOPTION_ROWS: i32 = 0x00004;
        const FRAMEOPTION_GROUPS: i32 = 0x00008;
        const FRAMEOPTION_BETWEEN: i32 = 0x00010;
        const FRAMEOPTION_START_UNBOUNDED_PRECEDING: i32 = 0x00020;
        const FRAMEOPTION_END_UNBOUNDED_PRECEDING: i32 = 0x00040;
        const FRAMEOPTION_START_UNBOUNDED_FOLLOWING: i32 = 0x00080;
        const FRAMEOPTION_END_UNBOUNDED_FOLLOWING: i32 = 0x00100;
        const FRAMEOPTION_START_CURRENT_ROW: i32 = 0x00200;
        const FRAMEOPTION_END_CURRENT_ROW: i32 = 0x00400;
        const FRAMEOPTION_START_OFFSET_PRECEDING: i32 = 0x00800;
        const FRAMEOPTION_END_OFFSET_PRECEDING: i32 = 0x01000;
        const FRAMEOPTION_START_OFFSET_FOLLOWING: i32 = 0x02000;
        const FRAMEOPTION_END_OFFSET_FOLLOWING: i32 = 0x04000;

        let Some(frame) = frame else {
            // Default: RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
            return FRAMEOPTION_NONDEFAULT
                | FRAMEOPTION_RANGE
                | FRAMEOPTION_BETWEEN
                | FRAMEOPTION_START_UNBOUNDED_PRECEDING
                | FRAMEOPTION_END_CURRENT_ROW;
        };

        let mut opts = FRAMEOPTION_NONDEFAULT | FRAMEOPTION_BETWEEN;

        // Mode
        opts |= match frame.mode {
            WindowFrameMode::Rows => FRAMEOPTION_ROWS,
            WindowFrameMode::Range => FRAMEOPTION_RANGE,
            WindowFrameMode::Groups => FRAMEOPTION_GROUPS,
        };

        // Start bound
        opts |= match &frame.start {
            WindowFrameBound::UnboundedPreceding => FRAMEOPTION_START_UNBOUNDED_PRECEDING,
            WindowFrameBound::CurrentRow => FRAMEOPTION_START_CURRENT_ROW,
            WindowFrameBound::Preceding(_) => FRAMEOPTION_START_OFFSET_PRECEDING,
            WindowFrameBound::Following(_) => FRAMEOPTION_START_OFFSET_FOLLOWING,
            WindowFrameBound::UnboundedFollowing => FRAMEOPTION_START_UNBOUNDED_FOLLOWING,
        };

        // End bound
        opts |= match &frame.end {
            WindowFrameBound::UnboundedFollowing => FRAMEOPTION_END_UNBOUNDED_FOLLOWING,
            WindowFrameBound::CurrentRow => FRAMEOPTION_END_CURRENT_ROW,
            WindowFrameBound::Preceding(_) => FRAMEOPTION_END_OFFSET_PRECEDING,
            WindowFrameBound::Following(_) => FRAMEOPTION_END_OFFSET_FOLLOWING,
            WindowFrameBound::UnboundedPreceding => FRAMEOPTION_END_UNBOUNDED_PRECEDING,
        };

        opts
    }

    unsafe fn build_incremental_sort(
        &mut self,
        prefix_keys: &[SortKey],
        suffix_keys: &[SortKey],
        input: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let child = self.build_plan(input)?;
        let node = self.alloc_node::<pg_sys::IncrementalSort>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "IncrementalSort allocation".to_string(),
            ));
        }
        (*node).sort.plan.type_ = pg_sys::NodeTag::T_IncrementalSort;
        (*node).sort.plan.lefttree = child;
        // IncrementalSort passes tuples through unchanged: share child targetlist.
        if !child.is_null() {
            (*node).sort.plan.targetlist = (*child).targetlist;
        }

        // Total keys = prefix (already sorted) + suffix (to sort within groups)
        let all_keys: Vec<SortKey> = prefix_keys
            .iter()
            .chain(suffix_keys.iter())
            .cloned()
            .collect();
        (*node).nPresortedCols = prefix_keys.len() as i32;

        // Build sort arrays for ALL keys (prefix + suffix)
        let child_tlist = if child.is_null() {
            std::ptr::null_mut()
        } else {
            (*child).targetlist
        };
        let rel_oid = self.first_rel_oid(input);

        if let Some(arrays) = crate::sort_utils::build_sort_arrays(&all_keys, child_tlist, rel_oid)
        {
            (*node).sort.numCols = arrays.num_cols;
            (*node).sort.sortColIdx = arrays.col_idx;
            (*node).sort.sortOperators = arrays.operators;
            (*node).sort.collations = arrays.collations;
            (*node).sort.nullsFirst = arrays.nulls_first;
        } else {
            (*node).sort.numCols = all_keys.len() as i32;
        }

        // Cost: cheaper than full sort since prefix is pre-sorted
        if !child.is_null() {
            let n = (*child).plan_rows.max(1.0);
            let group_size = (n / 100.0).max(2.0); // estimated group size
            let sort_cost = n * group_size.ln().max(1.0) * 0.001;
            (*node).sort.plan.startup_cost = (*child).total_cost + sort_cost;
            (*node).sort.plan.total_cost = (*node).sort.plan.startup_cost;
            (*node).sort.plan.plan_rows = (*child).plan_rows;
            (*node).sort.plan.plan_width = (*child).plan_width;
        }
        Ok(&mut (*node).sort.plan as *mut pg_sys::Plan)
    }

    // -----------------------------------------------------------------------
    // Function scan and Values
    // -----------------------------------------------------------------------

    unsafe fn build_function_scan(
        &mut self,
        expr: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let node = self.alloc_node::<pg_sys::FunctionScan>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "FunctionScan allocation".to_string(),
            ));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_FunctionScan;

        // Extract function info from the RelExpr
        match expr {
            RelExpr::TableFunction { name, args, .. } => {
                // Build a FuncExpr for the function call
                let func_expr = Expr::Function {
                    name: name.clone(),
                    args: args.clone(),
                };
                let pg_expr = expr_translator::translate(&func_expr, &self.expr_ctx);
                if !pg_expr.is_null() {
                    // Wrap in a RangeTblFunction and attach to scan
                    let rtfunc = pg_sys::palloc0(std::mem::size_of::<pg_sys::RangeTblFunction>())
                        as *mut pg_sys::RangeTblFunction;
                    (*rtfunc).type_ = pg_sys::NodeTag::T_RangeTblFunction;
                    (*rtfunc).funcexpr = pg_expr as *mut pg_sys::Node;
                    (*node).functions = pg_sys::lappend(
                        std::ptr::null_mut(),
                        rtfunc.cast::<core::ffi::c_void>(),
                    );
                }
                debug!(
                    function = name.as_str(),
                    "FunctionScan: built function call"
                );
            }
            RelExpr::Unnest {
                with_ordinality, ..
            } => {
                (*node).funcordinality = *with_ordinality;
                debug!("FunctionScan: unnest expression");
            }
            _ => {
                debug!("FunctionScan: unrecognized expression type");
            }
        }

        (*node).scan.plan.plan_rows = 100.0;
        (*node).scan.plan.total_cost = 10.0;
        Ok(&mut (*node).scan.plan as *mut pg_sys::Plan)
    }

    /// Build a `ValuesScan` for a `VALUES` clause, referencing the
    /// `RTE_VALUES` PostgreSQL already built in the original query's range
    /// table (reused verbatim as the PlannedStmt rtable). Falls back if no
    /// RTE_VALUES is present.
    unsafe fn build_values_result(
        &mut self,
        rows: &[Vec<Expr>],
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        if rows.is_empty() || self.original_query.is_null() {
            return Err(unsupported("VALUES"));
        }
        // Locate the RTE_VALUES in PG's range table.
        let rtable = (*self.original_query).rtable;
        if rtable.is_null() {
            return Err(unsupported("VALUES rtable"));
        }
        let mut scanrelid: pg_sys::Index = 0;
        let mut rte: *mut pg_sys::RangeTblEntry = std::ptr::null_mut();
        let elements = (*rtable).elements;
        for i in 0..(*rtable).length {
            let r = (*elements.add(i as usize)).ptr_value as *mut pg_sys::RangeTblEntry;
            if !r.is_null() && (*r).rtekind == pg_sys::RTEKind::RTE_VALUES {
                scanrelid = (i + 1) as pg_sys::Index;
                rte = r;
                break;
            }
        }
        if rte.is_null() || (*rte).values_lists.is_null() {
            return Err(unsupported("no RTE_VALUES"));
        }
        let first = pg_sys::list_nth((*rte).values_lists, 0) as *mut pg_sys::List;
        if first.is_null() {
            return Err(unsupported("empty VALUES row"));
        }
        let ncols = (*first).length;
        let node = self.alloc_node::<pg_sys::ValuesScan>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("ValuesScan".to_owned()));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_ValuesScan;
        (*node).scan.scanrelid = scanrelid;
        (*node).values_lists = (*rte).values_lists;
        // Output Vars reference the RTE's columns; types come from the
        // (already type-unified) first row's expressions.
        let row_elems = (*first).elements;
        let mut tlist: *mut pg_sys::List = std::ptr::null_mut();
        for j in 0..ncols {
            let e = (*row_elems.add(j as usize)).ptr_value as *mut pg_sys::Node;
            let var = self.alloc_node::<pg_sys::Var>();
            (*var).xpr.type_ = pg_sys::NodeTag::T_Var;
            (*var).varno = scanrelid as i32;
            (*var).varattno = (j + 1) as i16;
            (*var).vartype = if e.is_null() { pg_sys::INT4OID } else { pg_sys::exprType(e) };
            (*var).vartypmod = if e.is_null() { -1 } else { pg_sys::exprTypmod(e) };
            (*var).varcollid = if e.is_null() {
                pg_sys::InvalidOid
            } else {
                pg_sys::exprCollation(e)
            };
            (*var).varlevelsup = 0;
            let rn = CString::new(format!("column{}", j + 1))
                .map(|c| pg_sys::pstrdup(c.as_ptr()))
                .unwrap_or(std::ptr::null_mut());
            let te = pg_sys::makeTargetEntry(var.cast(), (j + 1) as i16, rn, false);
            tlist = pg_sys::lappend(tlist, te.cast());
        }
        (*node).scan.plan.targetlist = tlist;
        (*node).scan.plan.plan_rows = rows.len() as f64;
        (*node).scan.plan.total_cost = host_cost_params().2 * rows.len() as f64;
        Ok(&mut (*node).scan.plan as *mut pg_sys::Plan)
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Look up the range-table index for a table by name.
    fn rtindex_for(&self, table: &str) -> Result<pg_sys::Index, PlanBuilderError> {
        self.expr_ctx
            .rtindex_map
            .get(&table.to_lowercase())
            .copied()
            .ok_or_else(|| PlanBuilderError::TableNotFound(table.to_string()))
    }

    /// Look up the relation OID for a table by name.
    fn rel_oid_for(&self, table: &str) -> Result<pg_sys::Oid, PlanBuilderError> {
        self.expr_ctx
            .rtoid_map
            .get(&table.to_lowercase())
            .copied()
            .ok_or_else(|| PlanBuilderError::TableNotFound(table.to_string()))
    }

    /// Extract the first relation OID from an expression tree.
    ///
    /// Walks down the tree to find a Scan node and returns its OID.
    /// Used for type resolution when building sort/group arrays.
    fn first_rel_oid(&self, expr: &RelExpr) -> pg_sys::Oid {
        match expr {
            RelExpr::Scan { table, .. }
            | RelExpr::IndexScan { table, .. }
            | RelExpr::IndexOnlyScan { table, .. }
            | RelExpr::BitmapIndexScan { table, .. }
            | RelExpr::BitmapHeapScan { table, .. }
            | RelExpr::ParallelScan { table, .. } => self
                .expr_ctx
                .rtoid_map
                .get(&table.to_lowercase())
                .copied()
                .unwrap_or(pg_sys::InvalidOid),
            _ => {
                // Recurse into first child
                if let Some(child) = expr.children().first() {
                    self.first_rel_oid(child)
                } else {
                    pg_sys::InvalidOid
                }
            }
        }
    }

    /// Set the targetlist on a plan node from Ra projection columns.
    ///
    /// Fail-safe: if any column expression cannot be translated, returns
    /// `Err` so the whole query defers to the native planner rather than
    /// emitting a plan with a missing output column (wrong results).
    unsafe fn set_targetlist(
        &self,
        plan: *mut pg_sys::Plan,
        columns: &[ProjectionColumn],
    ) -> Result<(), PlanBuilderError> {
        if plan.is_null() {
            return Ok(());
        }
        // A projection defines the complete output, so replace any existing
        // targetlist (a nested Project from CTE inlining would otherwise
        // double the columns by appending).
        (*plan).targetlist = std::ptr::null_mut();
        for (i, pc) in columns.iter().enumerate() {
            let pg_expr = expr_translator::translate(&pc.expr, &self.expr_ctx);
            if pg_expr.is_null() {
                return Err(PlanBuilderError::UnsupportedVariant(
                    "projection column not translatable; deferring to native \
                     planner (see docs/planner-fallback-backlog.md)"
                        .to_owned(),
                ));
            }
            let te = self.alloc_node::<pg_sys::TargetEntry>();
            if te.is_null() {
                return Err(PlanBuilderError::NullPointer("TargetEntry".to_owned()));
            }
            (*te).xpr.type_ = pg_sys::NodeTag::T_TargetEntry;
            (*te).expr = pg_expr;
            (*te).resno = (i + 1) as pg_sys::AttrNumber;
            // Name the output column: explicit alias, else the source column
            // name (matches PG — `SELECT id` yields a column named "id"). The
            // name lets sort/group key resolution find this column by name.
            let resname = pc
                .alias
                .as_deref()
                .or_else(|| crate::sort_utils::extract_column_name(&pc.expr));
            if let Some(name) = resname {
                if let Ok(cs) = CString::new(name) {
                    (*te).resname = pg_sys::pstrdup(cs.as_ptr());
                }
            }
            (*plan).targetlist = pg_sys::lappend((*plan).targetlist, te.cast());
        }
        Ok(())
    }

    /// Set cost estimates on a scan plan node from gathered catalog statistics.
    ///
    /// Uses the PostgreSQL cost model:
    /// - SeqScan: `startup = 0`, `total = relpages * seq_page_cost + reltuples * cpu_tuple_cost`
    /// - `plan_rows = reltuples`
    /// - `plan_width = sum(avg_width)` across all columns
    ///
    /// Falls back to conservative defaults when statistics are unavailable.
    unsafe fn set_costs_from_stats(&self, plan: &mut pg_sys::Plan, table: &str) {
        // Host-calibrated cost parameters (not hardcoded GUC defaults).
        let (seq_page_cost, _, cpu_tuple_cost, _) = host_cost_params();

        let table_lower = table.to_lowercase();
        if let Some(stats) = self.stats.get(&table_lower) {
            let reltuples = stats.row_count.max(1.0);
            let relpages = if stats.total_size > 0 {
                (stats.total_size as f64 / 8192.0).max(1.0)
            } else {
                (reltuples / 50.0).max(1.0) // estimate ~50 tuples per page
            };

            plan.startup_cost = 0.0;
            plan.total_cost = relpages * seq_page_cost + reltuples * cpu_tuple_cost;
            plan.plan_rows = reltuples;

            // Sum avg_width from column stats, or default to 100
            let width: i32 = if stats.columns.is_empty() {
                100
            } else {
                stats
                    .columns
                    .values()
                    .map(|c| c.avg_length.unwrap_or(8.0) as i32)
                    .sum::<i32>()
                    .max(1)
            };
            plan.plan_width = width;
        } else {
            // No statistics available — use conservative defaults
            plan.startup_cost = 0.0;
            plan.total_cost = 100.0;
            plan.plan_rows = 1000.0;
            plan.plan_width = 100;
        }
    }

    /// Set cost estimates for an index scan using selectivity and random I/O.
    ///
    /// Index scans have higher per-page cost (random I/O) but read fewer pages.
    unsafe fn set_index_costs(&self, plan: &mut pg_sys::Plan, table: &str, selectivity: f64) {
        let (_, random_page_cost, cpu_tuple_cost, cpu_index_tuple_cost) = host_cost_params();

        let table_lower = table.to_lowercase();
        if let Some(stats) = self.stats.get(&table_lower) {
            let reltuples = stats.row_count.max(1.0);
            let relpages = if stats.total_size > 0 {
                (stats.total_size as f64 / 8192.0).max(1.0)
            } else {
                (reltuples / 50.0).max(1.0)
            };

            let selected_tuples = (reltuples * selectivity).max(1.0);
            let pages_fetched = (relpages * selectivity).max(1.0);

            plan.startup_cost = 0.1; // Index startup cost
            plan.total_cost = pages_fetched * random_page_cost
                + selected_tuples * (cpu_index_tuple_cost + cpu_tuple_cost);
            plan.plan_rows = selected_tuples;
            plan.plan_width = 100;
        } else {
            plan.startup_cost = 0.1;
            plan.total_cost = 50.0;
            plan.plan_rows = 100.0;
            plan.plan_width = 100;
        }
    }

    /// Estimate the fraction of `table`'s rows selected by `predicate` using
    /// gathered column statistics. Conjuncts are combined assuming
    /// independence (product of per-conjunct selectivities). Falls back to a
    /// neutral 0.1 when nothing can be estimated; the result is clamped to
    /// `[1/row_count, 1.0]`.
    fn estimate_scan_selectivity(&self, table: &str, predicate: &Expr) -> f64 {
        let Some(stats) = self.stats.get(&table.to_lowercase()) else {
            return 0.1;
        };
        let mut conjuncts: Vec<&Expr> = Vec::new();
        split_conjuncts(predicate, &mut conjuncts);
        let mut selectivity = 1.0_f64;
        let mut estimated_any = false;
        for c in &conjuncts {
            if let Some(s) = Self::conjunct_selectivity(c, stats) {
                selectivity *= s;
                estimated_any = true;
            }
        }
        if !estimated_any {
            return 0.1;
        }
        let floor = 1.0 / stats.row_count.max(1.0);
        selectivity.clamp(floor, 1.0)
    }

    /// Estimate the selectivity of a single `col <op> const` comparison (in
    /// either argument order). Equality uses `1/ndv`; range predicates
    /// interpolate the constant against the column's min/max. Returns `None`
    /// when the clause is not an estimable column-vs-constant comparison.
    fn conjunct_selectivity(clause: &Expr, stats: &Statistics) -> Option<f64> {
        let Expr::BinOp { op, left, right } = clause else {
            return None;
        };
        if !is_comparison(*op) {
            return None;
        }
        // Normalise to `column <op> constant`, flipping the operator when the
        // constant is on the left.
        let (cref, konst, op) = match (&**left, &**right) {
            (Expr::Column(c), Expr::Const(k)) => (c, k, *op),
            (Expr::Const(k), Expr::Column(c)) => (c, k, flip_comparison(*op)),
            _ => return None,
        };
        let col = stats.columns.get(&cref.column.to_lowercase())?;
        match op {
            BinOp::Eq => Some(col.equality_selectivity()),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let k = const_to_f64(konst)?;
                let lo = col.min_value.as_ref()?.parse::<f64>().ok()?;
                let hi = col.max_value.as_ref()?.parse::<f64>().ok()?;
                if hi <= lo {
                    return Some(0.5);
                }
                let frac_below = ((k - lo) / (hi - lo)).clamp(0.0, 1.0);
                Some(match op {
                    BinOp::Lt | BinOp::Le => frac_below,
                    _ => 1.0 - frac_below,
                })
            }
            _ => None,
        }
    }

    /// True when a sequential scan of `table` is strictly cheaper than an
    /// index scan at the given `selectivity`, under the host-calibrated cost
    /// parameters. Mirrors the formulas in `set_costs_from_stats` and
    /// `set_index_costs`.
    fn seq_beats_index(&self, table: &str, selectivity: f64) -> bool {
        let Some(stats) = self.stats.get(&table.to_lowercase()) else {
            return false;
        };
        let (seq_page_cost, random_page_cost, cpu_tuple_cost, cpu_index_tuple_cost) =
            host_cost_params();
        let reltuples = stats.row_count.max(1.0);
        let relpages = if stats.total_size > 0 {
            (stats.total_size as f64 / 8192.0).max(1.0)
        } else {
            (reltuples / 50.0).max(1.0)
        };
        let seq_cost = relpages * seq_page_cost + reltuples * cpu_tuple_cost;
        let selected = (reltuples * selectivity).max(1.0);
        let pages_fetched = (relpages * selectivity).max(1.0);
        let index_cost =
            pages_fetched * random_page_cost + selected * (cpu_index_tuple_cost + cpu_tuple_cost);
        seq_cost < index_cost
    }

    /// Propagate costs from two child nodes to a join node.
    unsafe fn propagate_costs_binary(
        &self,
        plan: &mut pg_sys::Plan,
        left: *mut pg_sys::Plan,
        right: *mut pg_sys::Plan,
    ) {
        let left_cost = if left.is_null() {
            0.0
        } else {
            (*left).total_cost
        };
        let right_cost = if right.is_null() {
            0.0
        } else {
            (*right).total_cost
        };
        let left_rows = if left.is_null() {
            1.0
        } else {
            (*left).plan_rows
        };
        let right_rows = if right.is_null() {
            1.0
        } else {
            (*right).plan_rows
        };
        plan.startup_cost = left_cost;
        plan.total_cost = left_cost + right_cost + left_rows * right_rows * 0.01;
        plan.plan_rows = (left_rows * right_rows * 0.1).max(1.0);
        plan.plan_width = 200;
    }

    // -----------------------------------------------------------------------
    // DML plan nodes
    // -----------------------------------------------------------------------

    /// Build a `ModifyTable` plan from a DML `RelExpr` variant.
    ///
    /// Extracts subplan, WHERE filter, assignments (UPDATE), ON CONFLICT
    /// (INSERT), and RETURNING from the RelExpr and translates each to
    /// the corresponding PostgreSQL `ModifyTable` fields.
    unsafe fn build_modify_table_from_dml(
        &mut self,
        expr: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        match expr {
            RelExpr::Insert {
                table,
                source,
                on_conflict,
                returning,
                ..
            } => self.build_modify_table_insert(
                table,
                source,
                on_conflict.as_ref(),
                returning.as_deref(),
            ),
            RelExpr::Update {
                table,
                assignments,
                filter,
                from,
                returning,
            } => self.build_modify_table_update(
                table,
                assignments,
                filter.as_ref(),
                from.as_deref(),
                returning.as_deref(),
            ),
            RelExpr::Delete {
                table,
                filter,
                using,
                returning,
            } => self.build_modify_table_delete(
                table,
                filter.as_ref(),
                using.as_deref(),
                returning.as_deref(),
            ),
            _ => Err(PlanBuilderError::Internal(
                "build_modify_table_from_dml called with non-DML expr".to_string(),
            )),
        }
    }

    /// Build a `ModifyTable` plan for INSERT.
    unsafe fn build_modify_table_insert(
        &mut self,
        table: &str,
        source: &RelExpr,
        on_conflict: Option<&OnConflict>,
        returning: Option<&[ProjectionColumn]>,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let subplan = self.build_plan(source)?;
        let mt = self.alloc_modify_table(table, pg_sys::CmdType::CMD_INSERT, subplan)?;

        if let Some(oc) = on_conflict {
            self.apply_on_conflict(mt, oc);
        }
        if let Some(ret_cols) = returning {
            let ret_list = self.build_returning_list(ret_cols);
            if !ret_list.is_null() {
                (*mt).returningLists = pg_sys::lappend(std::ptr::null_mut(), ret_list.cast());
            }
        }

        Ok(mt.cast())
    }

    /// Build a `ModifyTable` plan for UPDATE.
    unsafe fn build_modify_table_update(
        &mut self,
        table: &str,
        assignments: &[(String, Expr)],
        filter: Option<&Expr>,
        from: Option<&RelExpr>,
        returning: Option<&[ProjectionColumn]>,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let scan_fallback = RelExpr::Scan {
            table: table.to_owned(),
            alias: None,
        };
        let sub_expr = from.unwrap_or(&scan_fallback);
        let subplan = self.build_plan(sub_expr)?;

        // Apply WHERE filter as a qual on the subplan.
        if let Some(pred) = filter {
            if !subplan.is_null() {
                let pg_expr = expr_translator::translate(pred, &self.expr_ctx);
                if !pg_expr.is_null() {
                    (*subplan).qual = pg_sys::lappend((*subplan).qual, pg_expr.cast());
                }
            }
        }

        let mt = self.alloc_modify_table(table, pg_sys::CmdType::CMD_UPDATE, subplan)?;

        // Apply SET assignments as targetlist entries on the subplan.
        self.apply_update_assignments(subplan, assignments);

        if let Some(ret_cols) = returning {
            let ret_list = self.build_returning_list(ret_cols);
            if !ret_list.is_null() {
                (*mt).returningLists = pg_sys::lappend(std::ptr::null_mut(), ret_list.cast());
            }
        }

        Ok(mt.cast())
    }

    /// Build a `ModifyTable` plan for DELETE.
    unsafe fn build_modify_table_delete(
        &mut self,
        table: &str,
        filter: Option<&Expr>,
        using: Option<&RelExpr>,
        returning: Option<&[ProjectionColumn]>,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let scan_fallback = RelExpr::Scan {
            table: table.to_owned(),
            alias: None,
        };
        let sub_expr = using.unwrap_or(&scan_fallback);
        let subplan = self.build_plan(sub_expr)?;

        // Apply WHERE filter as a qual on the subplan.
        if let Some(pred) = filter {
            if !subplan.is_null() {
                let pg_expr = expr_translator::translate(pred, &self.expr_ctx);
                if !pg_expr.is_null() {
                    (*subplan).qual = pg_sys::lappend((*subplan).qual, pg_expr.cast());
                }
            }
        }

        let mt = self.alloc_modify_table(table, pg_sys::CmdType::CMD_DELETE, subplan)?;

        if let Some(ret_cols) = returning {
            let ret_list = self.build_returning_list(ret_cols);
            if !ret_list.is_null() {
                (*mt).returningLists = pg_sys::lappend(std::ptr::null_mut(), ret_list.cast());
            }
        }

        Ok(mt.cast())
    }

    /// Allocate and initialize a `ModifyTable` node with common fields.
    unsafe fn alloc_modify_table(
        &mut self,
        table: &str,
        cmd_type: pg_sys::CmdType::Type,
        subplan: *mut pg_sys::Plan,
    ) -> Result<*mut pg_sys::ModifyTable, PlanBuilderError> {
        let mt = self.alloc_node::<pg_sys::ModifyTable>();
        if mt.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "ModifyTable allocation".to_string(),
            ));
        }

        (*mt).plan.type_ = pg_sys::NodeTag::T_ModifyTable;
        (*mt).operation = cmd_type;
        (*mt).canSetTag = true;

        // Resolve target relation RT index.
        let rtindex = self.rtindex_for(table)?;
        (*mt).nominalRelation = rtindex as u32;
        (*mt).rootRelation = rtindex as u32;
        (*mt).resultRelations = pg_sys::lappend_int(std::ptr::null_mut(), rtindex as i32);

        // Attach subplan.
        (*mt).plan.lefttree = subplan;

        // Propagate cost from subplan.
        if !subplan.is_null() {
            (*mt).plan.startup_cost = (*subplan).startup_cost;
            (*mt).plan.total_cost = (*subplan).total_cost + 10.0;
            (*mt).plan.plan_rows = (*subplan).plan_rows;
            (*mt).plan.plan_width = (*subplan).plan_width;
        }

        debug!(
            "build_modify_table: {} on '{}' (rtindex={})",
            match cmd_type {
                pg_sys::CmdType::CMD_INSERT => "INSERT",
                pg_sys::CmdType::CMD_UPDATE => "UPDATE",
                pg_sys::CmdType::CMD_DELETE => "DELETE",
                _ => "UNKNOWN",
            },
            table,
            rtindex
        );

        Ok(mt)
    }

    /// Build a RETURNING target list from projection columns.
    ///
    /// Returns a PostgreSQL `List*` of `TargetEntry` nodes.
    unsafe fn build_returning_list(&self, columns: &[ProjectionColumn]) -> *mut pg_sys::List {
        let mut list: *mut pg_sys::List = std::ptr::null_mut();
        for (i, pc) in columns.iter().enumerate() {
            let pg_expr = expr_translator::translate(&pc.expr, &self.expr_ctx);
            if pg_expr.is_null() {
                continue;
            }
            let te = self.alloc_node::<pg_sys::TargetEntry>();
            if te.is_null() {
                continue;
            }
            (*te).xpr.type_ = pg_sys::NodeTag::T_TargetEntry;
            (*te).expr = pg_expr;
            (*te).resno = (i + 1) as pg_sys::AttrNumber;
            if let Some(alias) = &pc.alias {
                if let Ok(cs) = CString::new(alias.as_str()) {
                    (*te).resname = pg_sys::pstrdup(cs.as_ptr());
                }
            }
            list = pg_sys::lappend(list, te.cast());
        }
        list
    }

    /// Apply UPDATE SET assignments as a targetlist on the subplan.
    ///
    /// Each assignment `(column_name, value_expr)` becomes a `TargetEntry`
    /// on the subplan's targetlist, producing the new column values.
    unsafe fn apply_update_assignments(
        &self,
        subplan: *mut pg_sys::Plan,
        assignments: &[(String, Expr)],
    ) {
        if subplan.is_null() {
            return;
        }
        for (i, (col_name, value_expr)) in assignments.iter().enumerate() {
            let pg_expr = expr_translator::translate(value_expr, &self.expr_ctx);
            if pg_expr.is_null() {
                continue;
            }
            let te = self.alloc_node::<pg_sys::TargetEntry>();
            if te.is_null() {
                continue;
            }
            (*te).xpr.type_ = pg_sys::NodeTag::T_TargetEntry;
            (*te).expr = pg_expr;
            (*te).resno = (i + 1) as pg_sys::AttrNumber;
            if let Ok(cs) = CString::new(col_name.as_str()) {
                (*te).resname = pg_sys::pstrdup(cs.as_ptr());
            }
            (*subplan).targetlist = pg_sys::lappend((*subplan).targetlist, te.cast());
        }
    }

    /// Apply ON CONFLICT clause to a ModifyTable node.
    unsafe fn apply_on_conflict(&self, mt: *mut pg_sys::ModifyTable, on_conflict: &OnConflict) {
        match on_conflict {
            OnConflict::DoNothing => {
                (*mt).onConflictAction = pg_sys::OnConflictAction::ONCONFLICT_NOTHING;
            }
            OnConflict::DoUpdate { assignments, .. } => {
                (*mt).onConflictAction = pg_sys::OnConflictAction::ONCONFLICT_UPDATE;

                // Build the SET targetlist for the conflict update.
                let mut set_list: *mut pg_sys::List = std::ptr::null_mut();
                for (i, (_col_name, value_expr)) in assignments.iter().enumerate() {
                    let pg_expr = expr_translator::translate(value_expr, &self.expr_ctx);
                    if pg_expr.is_null() {
                        continue;
                    }
                    let te = self.alloc_node::<pg_sys::TargetEntry>();
                    if te.is_null() {
                        continue;
                    }
                    (*te).xpr.type_ = pg_sys::NodeTag::T_TargetEntry;
                    (*te).expr = pg_expr;
                    (*te).resno = (i + 1) as pg_sys::AttrNumber;
                    if let Ok(cs) = CString::new(_col_name.as_str()) {
                        (*te).resname = pg_sys::pstrdup(cs.as_ptr());
                    }
                    set_list = pg_sys::lappend(set_list, te.cast());
                }
                (*mt).onConflictSet = set_list;
            }
            OnConflict::DoSelect { .. } => {
                // ON CONFLICT DO SELECT is a PostgreSQL 19 feature.
                // pgrx 0.17 (pg13–pg18) has no ONCONFLICT_SELECT
                // action, so we cannot emit an executable plan node
                // for it here. The Lime parser and RelExpr model
                // understand DO SELECT (parse coverage), but
                // execution requires PG19, which the extension
                // toolchain does not yet target. Leave the default
                // action; the planner hook's fallback handles the
                // (currently unreachable on pg18) execution path.
                debug!(
                    "ON CONFLICT DO SELECT recognized but not executable on pg<=18 \
                     (PG19 feature); leaving default conflict action",
                );
            }
        }
    }

    /// Allocate a zeroed PostgreSQL node of type `T` in the current memory context.
    unsafe fn alloc_node<T>(&self) -> *mut T {
        pg_sys::palloc0(std::mem::size_of::<T>()).cast()
    }
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Collect the top-level AND conjuncts of `expr` into `out` (a non-AND

/// The single base relation a subtree scans (descending through pass-through
/// nodes), or `None` if it is a join, set-op, or otherwise not one relation.
fn single_base_table(expr: &RelExpr) -> Option<String> {
    match expr {
        RelExpr::Scan { table, .. } => Some(table.clone()),
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input } => single_base_table(input),
        _ => None,
    }
}

/// True if a single-relation subtree carries a Filter (a restricting WHERE),
/// used as a cheap proxy for "the driving side is selective" when deciding to
/// use an index nested-loop join.
fn subtree_has_filter(expr: &RelExpr) -> bool {
    match expr {
        RelExpr::Filter { .. } => true,
        RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input } => subtree_has_filter(input),
        _ => false,
    }
}
/// expression yields a single element). Used to isolate an indexable
/// equality conjunct from the rest of a `WHERE` clause.

/// Collect the names of all columns referenced anywhere in `expr` (for the
/// index-only-scan coverage check).
fn collect_column_names(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Column(cr) => out.push(cr.column.clone()),
        Expr::BinOp { left, right, .. } => {
            collect_column_names(left, out);
            collect_column_names(right, out);
        }
        Expr::UnaryOp { operand, .. } => collect_column_names(operand, out),
        Expr::Function { args, .. } | Expr::Array(args) => {
            for a in args {
                collect_column_names(a, out);
            }
        }
        Expr::Cast { expr, .. } | Expr::FieldAccess { expr, .. } => {
            collect_column_names(expr, out);
        }
        Expr::Case { operand, when_clauses, else_result } => {
            if let Some(o) = operand {
                collect_column_names(o, out);
            }
            for (c, r) in when_clauses {
                collect_column_names(c, out);
                collect_column_names(r, out);
            }
            if let Some(e) = else_result {
                collect_column_names(e, out);
            }
        }
        Expr::ArrayIndex(a, b) => {
            collect_column_names(a, out);
            collect_column_names(b, out);
        }
        _ => {}
    }
}

fn split_conjuncts<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::BinOp {
        op: BinOp::And,
        left,
        right,
    } = expr
    {
        split_conjuncts(left, out);
        split_conjuncts(right, out);
    } else {
        out.push(expr);
    }
}

/// PostgreSQL cost parameters derived from the host's calibrated hardware
/// model and the live monitoring dataflow — no estimated constants. The
/// static hardware capability (storage bandwidth, CPU speed) sets the base
/// rates; the continuously-monitored `SystemFingerprint` then auto-tunes
/// them to current conditions: page I/O is paid only on the measured
/// cache-miss fraction and scaled by measured I/O saturation, and CPU cost
/// is scaled by measured CPU load. Returns
/// `(seq_page_cost, random_page_cost, cpu_tuple_cost, cpu_index_tuple_cost)`.
fn host_cost_params() -> (f64, f64, f64, f64) {
    let cal = crate::extension_state::calibrated_cost_model();
    let seq_mbps = cal.measurements.sequential_read_mbps.max(1.0);
    // Nanoseconds to read one 8 KiB page sequentially on this host.
    let page_seq_ns = 8192.0 * 1000.0 / seq_mbps;
    let base_seq = 1.0; // unit: one sequential page read
    let base_random = cal.random_io_ratio.max(1.0); // measured random:seq ratio
    let base_cpu = (cal.measurements.cpu_tuple_cost_ns / page_seq_ns).max(0.0);

    // Live host conditions from the monitoring dataflow.
    let fp = crate::monitor::current_fingerprint();
    let miss = (1.0 - f64::from(fp.shared_buffers_hit_rate)).clamp(0.0, 1.0);
    let io_factor = miss * (1.0 + f64::from(fp.io_saturation));
    let cpu_factor = 1.0 + f64::from(fp.cpu_load_fraction);

    (
        base_seq * io_factor,
        base_random * io_factor,
        base_cpu * cpu_factor,
        base_cpu * 0.5 * cpu_factor, // index tuples are narrower
    )
}

/// Flip a comparison operator so `const <op> col` is rewritten as the
/// equivalent `col <flipped-op> const`.
fn flip_comparison(op: BinOp) -> BinOp {
    match op {
        BinOp::Lt => BinOp::Gt,
        BinOp::Le => BinOp::Ge,
        BinOp::Gt => BinOp::Lt,
        BinOp::Ge => BinOp::Le,
        other => other,
    }
}

/// Interpret a constant as an `f64` for range-selectivity interpolation.
/// Numeric strings are parsed; non-numeric constants return `None`.
fn const_to_f64(k: &ra_core::expr::Const) -> Option<f64> {
    match k {
        ra_core::expr::Const::Int(i) => Some(*i as f64),
        ra_core::expr::Const::Float(f) => Some(*f),
        ra_core::expr::Const::String(s) => s.parse::<f64>().ok(),
        ra_core::expr::Const::Bool(_) | ra_core::expr::Const::Null => None,
    }
}

/// True if `op` is a comparison that can map to a btree index strategy.
fn is_comparison(op: BinOp) -> bool {
    matches!(
        op,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
    )
}

/// The column reference of a `Col <cmp> x` / `x <cmp> Col` comparison, if the
/// clause is such a comparison with a column on one side.
fn comparison_column(clause: &Expr) -> Option<&ra_core::expr::ColumnRef> {
    let Expr::BinOp { op, left, right } = clause else {
        return None;
    };
    if !is_comparison(*op) {
        return None;
    }
    match (&**left, &**right) {
        (Expr::Column(cr), _) | (_, Expr::Column(cr)) => Some(cr),
        _ => None,
    }
}

/// Build a single-element PostgreSQL `List` of node pointers.
unsafe fn list1(a: *mut pg_sys::Node) -> *mut pg_sys::List {
    pg_sys::lappend(std::ptr::null_mut(), a.cast())
}

/// Build a two-element PostgreSQL `List` of node pointers.
unsafe fn list2(a: *mut pg_sys::Node, b: *mut pg_sys::Node) -> *mut pg_sys::List {
    pg_sys::lappend(list1(a), b.cast())
}

/// Convert a Ra `JoinType` to the corresponding PostgreSQL `JoinType`.
fn ra_join_type_to_pg(jt: JoinType) -> pg_sys::JoinType::Type {
    match jt {
        JoinType::Inner => pg_sys::JoinType::JOIN_INNER,
        JoinType::LeftOuter => pg_sys::JoinType::JOIN_LEFT,
        JoinType::RightOuter => pg_sys::JoinType::JOIN_RIGHT,
        JoinType::FullOuter => pg_sys::JoinType::JOIN_FULL,
        JoinType::Cross => pg_sys::JoinType::JOIN_INNER,
        JoinType::Semi => pg_sys::JoinType::JOIN_SEMI,
        JoinType::Anti => pg_sys::JoinType::JOIN_ANTI,
    }
}

/// Build a PostgreSQL `Const` node holding an INT8 value.
unsafe fn make_int8_const(val: i64) -> *mut pg_sys::Expr {
    let node = pg_sys::palloc0(std::mem::size_of::<pg_sys::Const>()) as *mut pg_sys::Const;
    (*node).xpr.type_ = pg_sys::NodeTag::T_Const;
    (*node).consttype = pg_sys::INT8OID;
    (*node).consttypmod = -1;
    (*node).constlen = 8;
    (*node).constvalue = pg_sys::Datum::from(val);
    (*node).constisnull = false;
    (*node).constbyval = true;
    node as *mut pg_sys::Expr
}

/// Build a `HashMap<String, (pg_sys::Index, pg_sys::Oid)>` from a PostgreSQL
/// range table list.
///
/// Maps each relation name (lowercase) to its 1-based range-table index and
/// its relation OID.  Pass the returned map to [`PlanBuilder::new`].
///
/// True when `pred` contains a reference to the system column
/// `ctid` (equality test with another expression).
fn predicate_references_ctid(pred: &Expr) -> bool {
    use ra_core::expr::BinOp as RaBinOp;
    match pred {
        Expr::BinOp { op: RaBinOp::Eq, left, right } => {
            is_ctid_column(left) || is_ctid_column(right)
        }
        Expr::BinOp { op: RaBinOp::And, left, right } => {
            predicate_references_ctid(left) || predicate_references_ctid(right)
        }
        Expr::Function { name, args } if name.eq_ignore_ascii_case("__in") => {
            // CTID IN (...) lowering: __in(col, list) shape.
            args.first().is_some_and(is_ctid_column)
        }
        _ => false,
    }
}

fn is_ctid_column(e: &Expr) -> bool {
    if let Expr::Column(c) = e {
        c.column.eq_ignore_ascii_case("ctid")
    } else {
        false
    }
}

/// Collect column names appearing in equality tests against
/// non-column operands within `pred`. Skips `ctid` (handled by
/// TidScan). Restricted to AND-conjunctions so every collected
/// column is genuinely constrained when the bitmap fires. The
/// `_table` parameter is reserved for future qualified-name
/// filtering (it's intentionally unused today since Filter
/// predicates above a single Scan are unambiguous).
fn collect_eq_columns_for_table(pred: &Expr, _table: &str) -> Vec<String> {
    use ra_core::expr::BinOp as RaBinOp;
    let mut out = Vec::new();
    fn walk(pred: &Expr, out: &mut Vec<String>) {
        match pred {
            Expr::BinOp {
                op: ra_core::expr::BinOp::Eq,
                left,
                right,
            } => {
                let l_col = as_named_column(left);
                let r_col = as_named_column(right);
                match (l_col, r_col) {
                    (Some(c), None) if !c.eq_ignore_ascii_case("ctid") => {
                        out.push(c.to_string());
                    }
                    (None, Some(c)) if !c.eq_ignore_ascii_case("ctid") => {
                        out.push(c.to_string());
                    }
                    _ => {}
                }
            }
            Expr::BinOp {
                op: RaBinOp::And,
                left,
                right,
            } => {
                walk(left, out);
                walk(right, out);
            }
            _ => {}
        }
    }
    walk(pred, &mut out);
    out
}

fn as_named_column(e: &Expr) -> Option<&str> {
    if let Expr::Column(c) = e {
        Some(&c.column)
    } else {
        None
    }
}

/// Extract the column name from a leaf equality clause
/// `Column = Expr` (or `Expr = Column`). Returns `None` for
/// non-equality, two-column-side, or two-non-column-side
/// shapes. Used by the bitmap-source builder to decide
/// whether a leaf clause is bitmap-eligible.
fn leaf_eq_column(clause: &Expr) -> Option<&str> {
    use ra_core::expr::BinOp as RaBinOp;
    if let Expr::BinOp { op: RaBinOp::Eq, left, right } = clause {
        let l = as_named_column(left);
        let r = as_named_column(right);
        match (l, r) {
            (Some(c), None) | (None, Some(c)) => Some(c),
            _ => None,
        }
    } else {
        None
    }
}

/// Walk a [`RelExpr`] subtree and return the leaf-most alias
/// reachable on the left-most-then-deepest path. Used to identify
/// the inner-side alias for join-method advice lookup: PG's
/// `HASH_JOIN(b)` advice means "the join touching `b` should be
/// a hash join with `b` on the inner side", so we look up the
/// leaf alias of the right (inner) child.
///
/// Returns `None` for non-Scan inner subtrees we can't easily
/// disambiguate (e.g. inner is itself a subquery). Callers
/// fall back to default join-method selection in that case.
fn leaf_alias(expr: &RelExpr) -> Option<String> {
    match expr {
        RelExpr::Scan { table, alias } => {
            Some(alias.clone().unwrap_or_else(|| table.clone()))
        }
        // Pass-through wrappers: descend.
        RelExpr::Filter { input, .. }
        | RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input } => leaf_alias(input),
        _ => None,
    }
}

/// Build an ascending NULLS-LAST [`SortKey`] over a single
/// column reference. Used by [`PlanBuilder::build_merge_join`]
/// to construct sort-key lists for the merge inputs.
fn sort_key_asc(column: &str) -> SortKey {
    SortKey {
        expr: Expr::Column(ra_core::expr::ColumnRef::new(column)),
        direction: ra_core::algebra::SortDirection::Asc,
        nulls: ra_core::algebra::NullOrdering::Last,
    }
}

/// Walk `condition` collecting `(left_col, right_col)` equi-join
/// pairs where the left column belongs to `left_rel`'s scope and
/// the right column belongs to `right_rel`'s scope.
///
/// Returns `false` if any clause isn't a pure equi-join over
/// column references (so callers can degrade gracefully). On
/// success, all clauses in an AND-of-equalities are appended to
/// `out` in left-to-right order.
fn collect_equi_pairs(
    condition: &Expr,
    left_rel: &RelExpr,
    right_rel: &RelExpr,
    out: &mut Vec<(String, String)>,
) -> bool {
    use ra_core::expr::BinOp as RaBinOp;
    match condition {
        Expr::BinOp { op: RaBinOp::Eq, left, right } => {
            let (Expr::Column(lc), Expr::Column(rc)) = (left.as_ref(), right.as_ref()) else {
                return false;
            };
            // Determine which column belongs to which side.
            let left_aliases = collect_aliases(left_rel);
            let right_aliases = collect_aliases(right_rel);
            let l_in_left = column_belongs_to(lc, &left_aliases);
            let l_in_right = column_belongs_to(lc, &right_aliases);
            let r_in_left = column_belongs_to(rc, &left_aliases);
            let r_in_right = column_belongs_to(rc, &right_aliases);
            // Prefer matches that are unambiguously on opposite
            // sides. With qualified columns this resolves
            // cleanly; with unqualified columns we accept the
            // first plausible orientation.
            if l_in_left && r_in_right && !(l_in_right && r_in_left) {
                out.push((lc.column.clone(), rc.column.clone()));
                true
            } else if l_in_right && r_in_left && !(l_in_left && r_in_right) {
                out.push((rc.column.clone(), lc.column.clone()));
                true
            } else if l_in_left && r_in_right {
                // Both unqualified — orient as (left, right)
                // since `lc` appeared on the left of `=`.
                out.push((lc.column.clone(), rc.column.clone()));
                true
            } else {
                false
            }
        }
        Expr::BinOp { op: RaBinOp::And, left, right } => {
            collect_equi_pairs(left, left_rel, right_rel, out)
                && collect_equi_pairs(right, left_rel, right_rel, out)
        }
        _ => false,
    }
}

/// Collect the set of relation aliases reachable in a `RelExpr`
/// subtree. Used to disambiguate which side of a join an
/// equi-clause column belongs to.
fn collect_aliases(expr: &RelExpr) -> Vec<String> {
    let mut out = Vec::new();
    walk_aliases(expr, &mut out);
    out
}

fn walk_aliases(expr: &RelExpr, out: &mut Vec<String>) {
    if let RelExpr::Scan { table, alias } = expr {
        out.push(alias.clone().unwrap_or_else(|| table.clone()));
    }
    for child in expr.children() {
        walk_aliases(child, out);
    }
}

/// True if a `ColumnRef` refers to a relation in `aliases`.
/// An unqualified column always returns true (we can't tell —
/// caller's responsibility to handle ambiguity).
fn column_belongs_to(col: &ra_core::expr::ColumnRef, aliases: &[String]) -> bool {
    if let Some(table) = &col.table {
        aliases.iter().any(|a| a.eq_ignore_ascii_case(table))
    } else {
        // Unqualified — caller decides.
        true
    }
}

/// Decompose a translated PG condition expression into a list
/// of `OpExpr` pointers. Handles a single `OpExpr`, an AND-of-
/// `OpExpr`s (`BoolExpr` with `AND_EXPR`), and returns an empty
/// list for any other shape.
unsafe fn expr_to_opexpr_list(
    expr: *mut pg_sys::Expr,
) -> Vec<*mut pg_sys::OpExpr> {
    let mut out = Vec::new();
    if expr.is_null() {
        return out;
    }
    match (*expr).type_ {
        pg_sys::NodeTag::T_OpExpr => {
            out.push(expr.cast::<pg_sys::OpExpr>());
        }
        pg_sys::NodeTag::T_BoolExpr => {
            let bool_expr = expr.cast::<pg_sys::BoolExpr>();
            if (*bool_expr).boolop != pg_sys::BoolExprType::AND_EXPR {
                return Vec::new();
            }
            let mut cell = (*(*bool_expr).args).elements;
            let n = (*(*bool_expr).args).length;
            for _ in 0..n {
                let inner = (*cell).ptr_value.cast::<pg_sys::Expr>();
                if inner.is_null() || (*inner).type_ != pg_sys::NodeTag::T_OpExpr {
                    return Vec::new();
                }
                out.push(inner.cast::<pg_sys::OpExpr>());
                cell = cell.add(1);
            }
        }
        _ => {}
    }
    out
}

/// # Safety
///
/// `query` must be a valid, non-null pointer to a PostgreSQL `Query` node.
pub unsafe fn build_table_map(
    query: *mut pg_sys::Query,
) -> HashMap<String, (pg_sys::Index, pg_sys::Oid)> {
    let mut map = HashMap::new();
    if query.is_null() {
        return map;
    }
    let rtable = (*query).rtable;
    if rtable.is_null() {
        return map;
    }
    let length = (*rtable).length;
    let elements = (*rtable).elements;
    for i in 0..length {
        let cell = elements.add(i as usize);
        let rte = (*cell).ptr_value as *mut pg_sys::RangeTblEntry;
        if rte.is_null() {
            continue;
        }
        if (*rte).rtekind != pg_sys::RTEKind::RTE_RELATION {
            continue;
        }
        let relid = (*rte).relid;
        let relname = pg_sys::get_rel_name(relid);
        if relname.is_null() {
            continue;
        }
        let name = std::ffi::CStr::from_ptr(relname)
            .to_string_lossy()
            .to_lowercase();
        let rtindex = (i + 1) as pg_sys::Index;
        map.insert(name, (rtindex, relid));
        // Also map the table alias (FROM peq p → "p") so alias-qualified
        // columns resolve.
        if !(*rte).eref.is_null() && !(*(*rte).eref).aliasname.is_null() {
            let alias = std::ffi::CStr::from_ptr((*(*rte).eref).aliasname)
                .to_string_lossy()
                .to_lowercase();
            map.entry(alias).or_insert((rtindex, relid));
        }
    }
    // Flatten non-recursive CTE relations: assign each the next range-table
    // index past the original rtable (build_planned_stmt appends them to the
    // PlannedStmt rtable in the same order). Keeps the rtindex_map and the
    // executed rtable consistent without mutating PG's parse tree.
    let mut next = length + 1;
    for fr in flatten_rtes(query) {
        let relid = (*fr.rte).relid;
        let relname = pg_sys::get_rel_name(relid);
        if relname.is_null() {
            continue;
        }
        let name = std::ffi::CStr::from_ptr(relname)
            .to_string_lossy()
            .to_lowercase();
        map.entry(name).or_insert((next as pg_sys::Index, relid));
        // Derived-table alias resolves to the same (passthrough) relation.
        if let Some(a) = fr.alias {
            map.entry(a).or_insert((next as pg_sys::Index, relid));
        }
        next += 1;
    }
    map
}

/// Inline a non-recursive CTE: replace `Scan(cte_name)` references in `body`
/// with the CTE `definition`. PostgreSQL inlines non-recursive CTEs by
/// default; doing it here lets the flattened base relations resolve. Only
/// recurses through the operator shapes Ra builds; in any other node a
/// `Scan(cte_name)` survives and the build then defers to native PG.
fn inline_cte_scan(body: &RelExpr, name: &str, def: &RelExpr) -> RelExpr {
    let rec = |e: &RelExpr| Box::new(inline_cte_scan(e, name, def));
    match body {
        RelExpr::Scan { table, alias } => {
            let refs = table.eq_ignore_ascii_case(name)
                || alias.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(name));
            if refs {
                def.clone()
            } else {
                body.clone()
            }
        }
        RelExpr::Filter { predicate, input } => RelExpr::Filter {
            predicate: predicate.clone(),
            input: rec(input),
        },
        RelExpr::Project { columns, input } => RelExpr::Project {
            columns: columns.clone(),
            input: rec(input),
        },
        RelExpr::Aggregate { aggregates, group_by, input } => RelExpr::Aggregate {
            aggregates: aggregates.clone(),
            group_by: group_by.clone(),
            input: rec(input),
        },
        RelExpr::Sort { keys, input } => RelExpr::Sort {
            keys: keys.clone(),
            input: rec(input),
        },
        RelExpr::Limit { count, offset, input } => RelExpr::Limit {
            count: *count,
            offset: *offset,
            input: rec(input),
        },
        RelExpr::Distinct { input } => RelExpr::Distinct { input: rec(input) },
        RelExpr::Window { functions, input } => RelExpr::Window {
            functions: functions.clone(),
            input: rec(input),
        },
        RelExpr::Join { join_type, condition, left, right } => RelExpr::Join {
            join_type: *join_type,
            condition: condition.clone(),
            left: rec(left),
            right: rec(right),
        },
        RelExpr::Union { left, right, all } => RelExpr::Union {
            left: rec(left),
            right: rec(right),
            all: *all,
        },
        RelExpr::Intersect { left, right, all } => RelExpr::Intersect {
            left: rec(left),
            right: rec(right),
            all: *all,
        },
        // Nested CTE: inline `name` into the inner CTE's definition and body so
        // a multi-CTE query (`WITH a AS (...), b AS (...) ...`) resolves
        // references to `a` that appear inside `b`'s scope or the final body.
        // If the inner CTE shadows `name`, its body refers to the inner
        // definition, so do not inline the outer one there.
        RelExpr::CTE { name: cname, definition, body } => {
            let new_body = if cname.eq_ignore_ascii_case(name) {
                body.clone()
            } else {
                rec(body)
            };
            RelExpr::CTE {
                name: cname.clone(),
                definition: rec(definition),
                body: new_body,
            }
        }
        RelExpr::Except { left, right, all } => RelExpr::Except {
            left: rec(left),
            right: rec(right),
            all: *all,
        },
        other => other.clone(),
    }
}

/// Range-table entries from non-recursive CTE definitions, flattened in a
/// deterministic order so the rtindex_map and the PlannedStmt rtable agree.
/// Returns `(rte, perminfo)` pairs; the perminfo (from the CTE sub-query's
/// rteperminfos) must be copied into the main permInfos so permission checks
/// are preserved. Returns empty unless every CTE is non-recursive and its
/// sub-query's range table is entirely base relations (the simple,
/// inline-able shape); anything else defers the whole query to native PG.
/// A relation pulled up from a CTE definition or a derived-table (FROM
/// sub-query) so it can be referenced in the flat PlannedStmt range table.
pub struct FlatRel {
    pub rte: *mut pg_sys::RangeTblEntry,
    pub perminfo: *mut pg_sys::RTEPermissionInfo,
    /// Sub-query alias to also map to this relation (derived tables only).
    pub alias: Option<String>,
}

/// The single base relation a derived-table sub-query scans, as a `FlatRel`
/// (alias `None`, so only the relation name is mapped — the derived table's
/// own alias maps to its computed output, not to this relation). Returns
/// `None` unless the sub-query's range table is exactly one base relation.
/// Used to pull the scan of an aggregating/computing derived table up into the
/// flat range table so the inlined scan resolves.
unsafe fn subquery_single_base(rte: *mut pg_sys::RangeTblEntry) -> Option<FlatRel> {
    let sq = (*rte).subquery;
    if sq.is_null() || (*sq).rtable.is_null() {
        return None;
    }
    let srt = (*sq).rtable;
    if srt.is_null() {
        return None;
    }
    // Find the single base relation among the sub-query's range table. A
    // GROUP BY adds an RTE_GROUP entry (PG 16+) and aggregation may add an
    // RTE_RESULT, so the range table is not necessarily length 1; require
    // exactly one RTE_RELATION.
    let sre = (*srt).elements;
    let mut base: *mut pg_sys::RangeTblEntry = std::ptr::null_mut();
    let mut relation_count = 0;
    for i in 0..(*srt).length {
        let e = (*sre.add(i as usize)).ptr_value as *mut pg_sys::RangeTblEntry;
        if !e.is_null() && (*e).rtekind == pg_sys::RTEKind::RTE_RELATION {
            relation_count += 1;
            base = e;
        }
    }
    if relation_count != 1 || base.is_null() {
        return None;
    }
    let inner = base;
    let perminfo = if (*inner).perminfoindex > 0 && !(*sq).rteperminfos.is_null() {
        pg_sys::list_nth((*sq).rteperminfos, ((*inner).perminfoindex - 1) as i32)
            as *mut pg_sys::RTEPermissionInfo
    } else {
        std::ptr::null_mut()
    };
    // Carry the inner relation's alias (e.g. lineitem aliased `l` inside a
    // LATERAL subquery) so alias-qualified columns of the inner resolve.
    let alias = if !(*inner).eref.is_null() && !(*(*inner).eref).aliasname.is_null() {
        Some(
            std::ffi::CStr::from_ptr((*(*inner).eref).aliasname)
                .to_string_lossy()
                .to_lowercase(),
        )
    } else {
        None
    };
    Some(FlatRel { rte: inner, perminfo, alias })
}

/// Verify a derived-table sub-query is a single-relation *passthrough*
/// (`SELECT <cols> FROM rel [WHERE ...]`, no rename/compute/aggregate/limit),
/// returning the inner base relation and its perminfo. Only then is mapping
/// the derived-table alias to the inner relation correct.
unsafe fn subquery_passthrough(
    rte: *mut pg_sys::RangeTblEntry,
) -> Option<(*mut pg_sys::RangeTblEntry, *mut pg_sys::RTEPermissionInfo)> {
    let sq = (*rte).subquery;
    if sq.is_null()
        || (*sq).hasAggs
        || !(*sq).groupClause.is_null()
        || !(*sq).distinctClause.is_null()
        || !(*sq).havingQual.is_null()
        || !(*sq).windowClause.is_null()
        || !(*sq).setOperations.is_null()
        || !(*sq).limitCount.is_null()
        || !(*sq).limitOffset.is_null()
    {
        return None;
    }
    let srt = (*sq).rtable;
    if srt.is_null() || (*srt).length != 1 {
        return None;
    }
    let inner = (*(*srt).elements.add(0)).ptr_value as *mut pg_sys::RangeTblEntry;
    if inner.is_null() || (*inner).rtekind != pg_sys::RTEKind::RTE_RELATION {
        return None;
    }
    let relid = (*inner).relid;
    // Every non-junk output column must be a passthrough Var of the inner
    // relation whose name matches the underlying column (no rename/compute),
    // so `alias.col` resolves to the same column as `inner.col`.
    let tl = (*sq).targetList;
    if !tl.is_null() {
        let elems = (*tl).elements;
        for i in 0..(*tl).length {
            let te = (*elems.add(i as usize)).ptr_value as *mut pg_sys::TargetEntry;
            if te.is_null() || (*te).resjunk {
                continue;
            }
            let e = (*te).expr;
            if e.is_null() || (*e).type_ != pg_sys::NodeTag::T_Var {
                return None;
            }
            let var = e.cast::<pg_sys::Var>();
            if (*var).varno != 1 {
                return None;
            }
            let real = pg_sys::get_attname(relid, (*var).varattno, true);
            if real.is_null() || (*te).resname.is_null() {
                return None;
            }
            if std::ffi::CStr::from_ptr(real) != std::ffi::CStr::from_ptr((*te).resname) {
                return None;
            }
        }
    }
    let pinfo = if (*inner).perminfoindex > 0 && !(*sq).rteperminfos.is_null() {
        pg_sys::list_nth((*sq).rteperminfos, ((*inner).perminfoindex - 1) as i32)
            as *mut pg_sys::RTEPermissionInfo
    } else {
        std::ptr::null_mut()
    };
    Some((inner, pinfo))
}

/// Recursively collect base (`RTE_RELATION`) entries from `q`'s range table,
/// descending into `RTE_SUBQUERY` entries (set-operation arms of a recursive
/// CTE nest their FROM relations inside arm sub-queries). `RTE_CTE`
/// self-references and other kinds are skipped.
unsafe fn collect_query_base_rtes(q: *mut pg_sys::Query, out: &mut Vec<FlatRel>) {
    if q.is_null() || (*q).rtable.is_null() {
        return;
    }
    let rt = (*q).rtable;
    let e = (*rt).elements;
    for i in 0..(*rt).length {
        let rte = (*e.add(i as usize)).ptr_value as *mut pg_sys::RangeTblEntry;
        if rte.is_null() {
            continue;
        }
        match (*rte).rtekind {
            pg_sys::RTEKind::RTE_RELATION => {
                let perminfo = if (*rte).perminfoindex > 0 && !(*q).rteperminfos.is_null() {
                    pg_sys::list_nth((*q).rteperminfos, ((*rte).perminfoindex - 1) as i32)
                        as *mut pg_sys::RTEPermissionInfo
                } else {
                    std::ptr::null_mut()
                };
                let alias = if !(*rte).eref.is_null() && !(*(*rte).eref).aliasname.is_null() {
                    Some(
                        std::ffi::CStr::from_ptr((*(*rte).eref).aliasname)
                            .to_string_lossy()
                            .to_lowercase(),
                    )
                } else {
                    None
                };
                out.push(FlatRel { rte, perminfo, alias });
            }
            pg_sys::RTEKind::RTE_SUBQUERY => collect_query_base_rtes((*rte).subquery, out),
            _ => {}
        }
    }
}

/// Relations to pull up into the flat range table: non-recursive CTE
/// definitions and simple passthrough derived tables. Deterministic order
/// (CTEs then main-rtable sub-queries) so build_table_map and
/// build_planned_stmt assign matching indices. Empty if any CTE is not
/// inline-able.
pub unsafe fn flatten_rtes(query: *mut pg_sys::Query) -> Vec<FlatRel> {
    let mut out = Vec::new();
    if query.is_null() {
        return out;
    }
    // CTEs.
    if !(*query).cteList.is_null() {
        let ctes = (*query).cteList;
        let cte_elems = (*ctes).elements;
        for c in 0..(*ctes).length {
            let cte = (*cte_elems.add(c as usize)).ptr_value as *mut pg_sys::CommonTableExpr;
            if cte.is_null() {
                return Vec::new();
            }
            let cq = (*cte).ctequery as *mut pg_sys::Query;
            if cq.is_null() {
                return Vec::new();
            }
            // Recursive CTE: its base relations live in the recursive term,
            // nested inside set-operation arm sub-queries. Pull them up so the
            // WorkTableScan-side joins can resolve them.
            if (*cte).cterecursive {
                collect_query_base_rtes(cq, &mut out);
                continue;
            }
            if (*cq).rtable.is_null() {
                return Vec::new();
            }
            let crt = (*cq).rtable;
            let cre = (*crt).elements;
            for i in 0..(*crt).length {
                let rte = (*cre.add(i as usize)).ptr_value as *mut pg_sys::RangeTblEntry;
                if rte.is_null() {
                    continue;
                }
                match (*rte).rtekind {
                    pg_sys::RTEKind::RTE_RELATION => {}
                    // GROUP BY / aggregation add bookkeeping entries (PG16+);
                    // they carry no scan, so skip them rather than bailing.
                    pg_sys::RTEKind::RTE_GROUP | pg_sys::RTEKind::RTE_RESULT => continue,
                    // A nested subquery/join/etc. in the CTE is beyond the
                    // base-relation shape that flattens cleanly: defer.
                    _ => return Vec::new(),
                }
                let perminfo = if (*rte).perminfoindex > 0 && !(*cq).rteperminfos.is_null() {
                    pg_sys::list_nth((*cq).rteperminfos, ((*rte).perminfoindex - 1) as i32)
                        as *mut pg_sys::RTEPermissionInfo
                } else {
                    std::ptr::null_mut()
                };
                out.push(FlatRel { rte, perminfo, alias: None });
            }
        }
    }
    // Derived tables (FROM sub-queries) in the main range table.
    if !(*query).rtable.is_null() {
        let rt = (*query).rtable;
        let re = (*rt).elements;
        for i in 0..(*rt).length {
            let rte = (*re.add(i as usize)).ptr_value as *mut pg_sys::RangeTblEntry;
            if rte.is_null() || (*rte).rtekind != pg_sys::RTEKind::RTE_SUBQUERY {
                continue;
            }
            let Some((inner, perminfo)) = subquery_passthrough(rte) else {
                // Not a passthrough (e.g. an aggregating derived table
                // `(SELECT k, sum(x) s FROM r GROUP BY k) t`). Pull up the
                // single base relation it scans so the inlined scan resolves;
                // the derived table's computed output columns (t.s) are
                // resolved by the subquery-output scope when building the
                // Result over the aggregate, NOT by mapping the alias.
                if let Some(fr) = subquery_single_base(rte) {
                    out.push(fr);
                }
                continue;
            };
            let alias = if !(*rte).eref.is_null() && !(*(*rte).eref).aliasname.is_null() {
                Some(
                    std::ffi::CStr::from_ptr((*(*rte).eref).aliasname)
                        .to_string_lossy()
                        .to_lowercase(),
                )
            } else {
                None
            };
            out.push(FlatRel { rte: inner, perminfo, alias });
        }
    }
    // Relations from IN/EXISTS/scalar sub-queries (SubLinks) — decorrelation
    // turns these into semi/anti joins that reference the sub-query relation.
    let mut sublinks: Vec<*mut pg_sys::Query> = Vec::new();
    if !(*query).jointree.is_null() {
        collect_sublink_queries((*(*query).jointree).quals, &mut sublinks);
    }
    if !(*query).targetList.is_null() {
        let tl = (*query).targetList;
        let te = (*tl).elements;
        for i in 0..(*tl).length {
            let entry = (*te.add(i as usize)).ptr_value as *mut pg_sys::TargetEntry;
            if !entry.is_null() {
                collect_sublink_queries((*entry).expr.cast(), &mut sublinks);
            }
        }
    }
    for sq in sublinks {
        if sq.is_null() || (*sq).rtable.is_null() {
            continue;
        }
        let srt = (*sq).rtable;
        let sre = (*srt).elements;
        for i in 0..(*srt).length {
            let rte = (*sre.add(i as usize)).ptr_value as *mut pg_sys::RangeTblEntry;
            if rte.is_null() || (*rte).rtekind != pg_sys::RTEKind::RTE_RELATION {
                continue;
            }
            let perminfo = if (*rte).perminfoindex > 0 && !(*sq).rteperminfos.is_null() {
                pg_sys::list_nth((*sq).rteperminfos, ((*rte).perminfoindex - 1) as i32)
                    as *mut pg_sys::RTEPermissionInfo
            } else {
                std::ptr::null_mut()
            };
            // Capture the sub-query relation's alias (e.g. `multi m`) so
            // alias-qualified correlation columns (`m.a`) resolve.
            let alias = if !(*rte).eref.is_null() && !(*(*rte).eref).aliasname.is_null() {
                Some(
                    std::ffi::CStr::from_ptr((*(*rte).eref).aliasname)
                        .to_string_lossy()
                        .to_lowercase(),
                )
            } else {
                None
            };
            out.push(FlatRel { rte, perminfo, alias });
        }
    }
    // Safety: if any pulled-up relation name collides with a relation already
    // in the main range table, name-based resolution would alias two distinct
    // scans to one rtindex (e.g. a self-referencing sub-query). Bail to the
    // native planner in that case. Exception: for a set operation, arms that
    // scan the same base table are independent (separate Append subplans), so
    // a duplicate relid is benign — dedupe it to a single shared rtindex
    // rather than bailing.
    let is_setop = !(*query).setOperations.is_null();
    let mut main_names = std::collections::HashSet::new();
    let mut main_aliases = std::collections::HashSet::new();
    let mrt = (*query).rtable;
    let mre = (*mrt).elements;
    for i in 0..(*mrt).length {
        let rte = (*mre.add(i as usize)).ptr_value as *mut pg_sys::RangeTblEntry;
        if !rte.is_null() && (*rte).rtekind == pg_sys::RTEKind::RTE_RELATION {
            main_names.insert((*rte).relid);
            if !(*rte).eref.is_null() && !(*(*rte).eref).aliasname.is_null() {
                main_aliases.insert(
                    std::ffi::CStr::from_ptr((*(*rte).eref).aliasname)
                        .to_string_lossy()
                        .to_lowercase(),
                );
            }
        }
    }
    let mut seen = main_names.clone();
    let mut deduped = Vec::with_capacity(out.len());
    for fr in out {
        if !seen.insert((*fr.rte).relid) {
            if is_setop {
                continue;
            }
            // Same relid as an existing scan. Safe only when a distinct alias
            // disambiguates it (e.g. a self-correlated `orders o2` against the
            // outer `orders o`): build_table_map maps it by alias to its own
            // appended rtindex.
            let disambiguated = fr
                .alias
                .as_ref()
                .is_some_and(|a| !main_aliases.contains(a));
            if !disambiguated {
                return Vec::new();
            }
        }
        deduped.push(fr);
    }
    deduped
}

/// Collect the sub-query of each SubLink reachable in a scalar expression
/// `node` (handles the common Bool/Op/ScalarArrayOp nesting; unknown node
/// types are ignored, so their relations simply aren't pulled up).
unsafe fn collect_sublink_queries(node: *mut pg_sys::Node, out: &mut Vec<*mut pg_sys::Query>) {
    if node.is_null() {
        return;
    }
    match (*node).type_ {
        pg_sys::NodeTag::T_SubLink => {
            let sl = node.cast::<pg_sys::SubLink>();
            if !(*sl).subselect.is_null() {
                out.push((*sl).subselect.cast());
            }
            collect_sublink_queries((*sl).testexpr, out);
        }
        pg_sys::NodeTag::T_BoolExpr => {
            collect_list_sublinks((*node.cast::<pg_sys::BoolExpr>()).args, out);
        }
        pg_sys::NodeTag::T_OpExpr => {
            collect_list_sublinks((*node.cast::<pg_sys::OpExpr>()).args, out);
        }
        pg_sys::NodeTag::T_ScalarArrayOpExpr => {
            collect_list_sublinks((*node.cast::<pg_sys::ScalarArrayOpExpr>()).args, out);
        }
        _ => {}
    }
}

unsafe fn collect_list_sublinks(list: *mut pg_sys::List, out: &mut Vec<*mut pg_sys::Query>) {
    if list.is_null() {
        return;
    }
    let elems = (*list).elements;
    for i in 0..(*list).length {
        collect_sublink_queries((*elems.add(i as usize)).ptr_value.cast(), out);
    }
}

/// Resolve a table name to its PostgreSQL relation OID using the search path.
///
/// Returns `InvalidOid` if the table is not found.
///
/// # Safety
///
/// Must be called from within a PostgreSQL backend process.
pub unsafe fn resolve_table_oid(table_name: &str) -> pg_sys::Oid {
    let Ok(c_name) = CString::new(table_name) else {
        return pg_sys::InvalidOid;
    };
    let rv = pg_sys::makeRangeVar(
        std::ptr::null_mut(), // use search_path
        c_name.as_ptr().cast_mut(),
        -1,
    );
    if rv.is_null() {
        return pg_sys::InvalidOid;
    }
    // RangeVarGetRelid is a C macro; call the Extended variant directly.
    // Flags: RVR_MISSING_OK = 1 (don't error if relation not found).
    pg_sys::RangeVarGetRelidExtended(
        rv,
        pg_sys::NoLock as i32,
        1, // RVR_MISSING_OK
        None,
        std::ptr::null_mut(),
    )
}


#[cfg(test)]
mod leaf_alias_tests {
    use super::leaf_alias;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Expr};

    fn scan(table: &str, alias: Option<&str>) -> RelExpr {
        RelExpr::Scan {
            table: table.into(),
            alias: alias.map(String::from),
        }
    }

    #[test]
    fn leaf_alias_picks_alias_over_table() {
        assert_eq!(leaf_alias(&scan("orders", Some("o"))), Some("o".into()));
    }

    #[test]
    fn leaf_alias_falls_back_to_table_when_no_alias() {
        assert_eq!(leaf_alias(&scan("orders", None)), Some("orders".into()));
    }

    #[test]
    fn leaf_alias_descends_through_filter() {
        let inner = scan("t", Some("a"));
        let pred = Expr::Const(ra_core::expr::Const::Bool(true));
        let wrapped = RelExpr::Filter {
            predicate: pred,
            input: Box::new(inner),
        };
        assert_eq!(leaf_alias(&wrapped), Some("a".into()));
    }

    #[test]
    fn leaf_alias_returns_none_for_join() {
        let join = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("a", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("b", "id"))),
            },
            left: Box::new(scan("a", None)),
            right: Box::new(scan("b", None)),
        };
        assert_eq!(leaf_alias(&join), None);
    }
}

#[cfg(test)]
mod helpers_tests {
    use super::{
        collect_aliases, collect_eq_columns_for_table, collect_equi_pairs,
        leaf_eq_column, predicate_references_ctid,
    };
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn scan(table: &str) -> RelExpr {
        RelExpr::Scan {
            table: table.into(),
            alias: None,
        }
    }

    fn scan_aliased(table: &str, alias: &str) -> RelExpr {
        RelExpr::Scan {
            table: table.into(),
            alias: Some(alias.into()),
        }
    }

    fn col_eq(table: &str, column: &str, value: i64) -> Expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified(table, column))),
            right: Box::new(Expr::Const(Const::Int(value))),
        }
    }

    fn col_eq_col(lt: &str, lc: &str, rt: &str, rc: &str) -> Expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified(lt, lc))),
            right: Box::new(Expr::Column(ColumnRef::qualified(rt, rc))),
        }
    }

    #[test]
    fn ctid_equality_recognized() {
        let p = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("ctid"))),
            right: Box::new(Expr::Const(Const::String("(0,1)".into()))),
        };
        assert!(predicate_references_ctid(&p));
    }

    #[test]
    fn non_ctid_predicate_rejected() {
        let p = col_eq("t", "id", 5);
        assert!(!predicate_references_ctid(&p));
    }

    #[test]
    fn eq_columns_for_table_skips_ctid_and_collects_eq() {
        let pred = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(col_eq("t", "id", 1)),
            right: Box::new(Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("ctid"))),
                right: Box::new(Expr::Const(Const::String("(0,1)".into()))),
            }),
        };
        let cols = collect_eq_columns_for_table(&pred, "t");
        assert_eq!(cols, vec!["id".to_string()]);
    }

    #[test]
    fn collect_aliases_descends_through_joins() {
        let q = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: col_eq_col("a", "x", "b", "x"),
            left: Box::new(scan_aliased("orders", "a")),
            right: Box::new(scan_aliased("customers", "b")),
        };
        let aliases = collect_aliases(&q);
        assert!(aliases.contains(&"a".to_string()));
        assert!(aliases.contains(&"b".to_string()));
    }

    #[test]
    fn collect_equi_pairs_single_clause() {
        let cond = col_eq_col("a", "id", "b", "id");
        let mut out = Vec::new();
        let ok = collect_equi_pairs(
            &cond,
            &scan_aliased("orders", "a"),
            &scan_aliased("customers", "b"),
            &mut out,
        );
        assert!(ok);
        assert_eq!(out, vec![("id".into(), "id".into())]);
    }

    #[test]
    fn collect_equi_pairs_swapped_order_canonicalizes() {
        // a.id = b.id with the columns intentionally swapped:
        // condition is `b.id = a.id`. Should canonicalize to
        // (left_col, right_col).
        let cond = col_eq_col("b", "id", "a", "id");
        let mut out = Vec::new();
        let ok = collect_equi_pairs(
            &cond,
            &scan_aliased("orders", "a"),
            &scan_aliased("customers", "b"),
            &mut out,
        );
        assert!(ok);
        assert_eq!(out, vec![("id".into(), "id".into())]);
    }

    #[test]
    fn collect_equi_pairs_and_of_two_clauses() {
        let cond = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(col_eq_col("a", "x", "b", "x")),
            right: Box::new(col_eq_col("a", "y", "b", "y")),
        };
        let mut out = Vec::new();
        let ok = collect_equi_pairs(
            &cond,
            &scan_aliased("orders", "a"),
            &scan_aliased("customers", "b"),
            &mut out,
        );
        assert!(ok);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], ("x".into(), "x".into()));
        assert_eq!(out[1], ("y".into(), "y".into()));
    }

    #[test]
    fn collect_equi_pairs_rejects_non_equi_clause() {
        // Includes a non-equi clause: a.id > b.id
        let cond = Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::qualified("a", "id"))),
            right: Box::new(Expr::Column(ColumnRef::qualified("b", "id"))),
        };
        let mut out = Vec::new();
        let ok = collect_equi_pairs(
            &cond,
            &scan_aliased("orders", "a"),
            &scan_aliased("customers", "b"),
            &mut out,
        );
        assert!(!ok);
    }

    #[test]
    fn collect_equi_pairs_rejects_const_operand() {
        // a.id = 7 is not an equi-join clause.
        let cond = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::qualified("a", "id"))),
            right: Box::new(Expr::Const(Const::Int(7))),
        };
        let mut out = Vec::new();
        let ok = collect_equi_pairs(
            &cond,
            &scan_aliased("orders", "a"),
            &scan_aliased("customers", "b"),
            &mut out,
        );
        assert!(!ok);
    }

    #[test]
    fn ctid_in_and_predicate_recognized() {
        let p = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("ctid"))),
                right: Box::new(Expr::Const(Const::String("(0,1)".into()))),
            }),
            right: Box::new(col_eq("t", "id", 5)),
        };
        assert!(predicate_references_ctid(&p));
        // And the eq-column collector skips ctid:
        let cols = collect_eq_columns_for_table(&p, "t");
        assert_eq!(cols, vec!["id".to_string()]);
        // Avoid unused-import lint when scan() helper isn't exercised.
        let _ = scan("t");
    }
}

#[cfg(test)]
mod bitmap_helper_tests {
    use super::leaf_eq_column;
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    fn col_eq_const(col: &str, v: i64) -> Expr {
        Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new(col))),
            right: Box::new(Expr::Const(Const::Int(v))),
        }
    }

    #[test]
    fn leaf_eq_column_extracts_lhs_column() {
        assert_eq!(leaf_eq_column(&col_eq_const("id", 5)), Some("id"));
    }

    #[test]
    fn leaf_eq_column_extracts_rhs_column() {
        let p = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Const(Const::Int(5))),
            right: Box::new(Expr::Column(ColumnRef::new("id"))),
        };
        assert_eq!(leaf_eq_column(&p), Some("id"));
    }

    #[test]
    fn leaf_eq_column_rejects_two_columns() {
        let p = Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("a"))),
            right: Box::new(Expr::Column(ColumnRef::new("b"))),
        };
        assert_eq!(leaf_eq_column(&p), None);
    }

    #[test]
    fn leaf_eq_column_rejects_non_eq_op() {
        let p = Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Const(Const::Int(5))),
        };
        assert_eq!(leaf_eq_column(&p), None);
    }

    #[test]
    fn leaf_eq_column_rejects_compound_predicate() {
        // AND-of-equalities is not a leaf — caller should
        // walk the structure first.
        let p = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(col_eq_const("a", 1)),
            right: Box::new(col_eq_const("b", 2)),
        };
        assert_eq!(leaf_eq_column(&p), None);
    }
}
