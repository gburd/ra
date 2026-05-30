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
use ra_core::expr::Expr;
use ra_core::statistics::Statistics;

use crate::expr_translator::{self, ExprContext};

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
        }
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
    fn first_unsupported_op(expr: &RelExpr) -> Option<&'static str> {
        match expr {
            RelExpr::Scan { .. } => None,
            RelExpr::Filter { input, .. } => Self::first_unsupported_op(input),
            // Project over Aggregate is built as a single Agg node
            // (build_grouped_aggregate); check the aggregate's input. The
            // builder itself returns Err (→ fallback) for shapes it cannot
            // handle (HAVING's nested form, expressions over aggregates,
            // DISTINCT/unsupported aggregates).
            RelExpr::Project { input, .. } => match &**input {
                RelExpr::Aggregate { input: agg_in, .. } => Self::first_unsupported_op(agg_in),
                _ => Self::first_unsupported_op(input),
            },
            // Projection-incapable passthrough nodes: they share the child
            // targetlist; ordering/limit semantics handled by the PG executor
            // node. Sort is admitted only when every key is a plain column —
            // expression keys still need ordering-operator resolution, so they
            // defer to PG.
            RelExpr::Sort { keys, input }
                if keys
                    .iter()
                    .all(|k| matches!(k.expr, ra_core::expr::Expr::Column(_))) =>
            {
                Self::first_unsupported_op(input)
            }
            RelExpr::Sort { .. } => Some("Sort"),
            RelExpr::IncrementalSort { .. } => Some("Sort"),
            RelExpr::Limit { input, .. } => Self::first_unsupported_op(input),
            RelExpr::Join { .. } => Some("Join"),
            RelExpr::Aggregate { .. } => Some("Aggregate"),
            // Distinct (SELECT DISTINCT): build_unique sorts its input on all
            // output columns before the Unique, so adjacent-dedup is correct.
            RelExpr::Distinct { input } => Self::first_unsupported_op(input),
            RelExpr::Union { .. } => Some("Union"),
            RelExpr::Intersect { .. } => Some("Intersect"),
            RelExpr::Except { .. } => Some("Except"),
            RelExpr::Window { .. } => Some("Window"),
            RelExpr::Values { .. } => Some("Values"),
            RelExpr::CTE { .. } => Some("CTE"),
            RelExpr::RecursiveCTE { .. } => Some("RecursiveCTE"),
            RelExpr::Unnest { .. } | RelExpr::MultiUnnest { .. } => Some("Unnest"),
            RelExpr::TableFunction { .. } => Some("TableFunction"),
            RelExpr::IndexScan { .. } | RelExpr::IndexOnlyScan { .. } => Some("IndexScan"),
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

        let plan_tree = self.build_plan(expr)?;

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

        // Copy range table and result relations from original query
        if !self.original_query.is_null() {
            (*stmt).rtable = (*self.original_query).rtable;
            // PG16+ moved per-relation permission checking out of
            // RangeTblEntry into a separate RTEPermissionInfo list that
            // the RTE references via `perminfoindex`. Since we copy the
            // rtable verbatim (perminfoindex values intact), we must
            // propagate the matching permInfos list too, or the
            // executor rejects the plan with "invalid perminfoindex".
            #[cfg(not(any(feature = "pg13", feature = "pg14", feature = "pg15")))]
            {
                (*stmt).permInfos = (*self.original_query).rteperminfos;
            }
            // PG18+ run-time pruning tracks which relids the executor may
            // open via `unprunableRelids`; relations not listed there are
            // assumed pruned and raise "trying to open a pruned relation".
            // Ra never emits run-time-pruning plans, so every range-table
            // entry is unprunable (always opened).
            #[cfg(any(feature = "pg18", feature = "pg19"))]
            {
                let n = pg_sys::list_length((*self.original_query).rtable);
                if n > 0 {
                    (*stmt).unprunableRelids =
                        pg_sys::bms_add_range(std::ptr::null_mut(), 1, n);
                }
            }
            (*stmt).resultRelations = std::ptr::null_mut();
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
                self.build_scan_with_advice(table, alias.as_deref())
            }
            RelExpr::Filter { predicate, input } => {
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
                // Project over Aggregate is built as one Agg node whose
                // targetlist carries the group Vars and Aggref nodes.
                if let RelExpr::Aggregate {
                    group_by,
                    input: agg_input,
                    ..
                } = &**input
                {
                    return self.build_grouped_aggregate(columns, group_by, agg_input);
                }
                let child = self.build_plan(input)?;
                self.set_targetlist(child, columns)?;
                Ok(child)
            }
            RelExpr::Join {
                join_type,
                condition,
                left,
                right,
            } => self.build_join(*join_type, condition, left, right),
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
            RelExpr::IndexOnlyScan { table, index, .. } => self.build_index_only_scan(table, index),
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
            RelExpr::Union { all, left, right } => self.build_set_op_union(*all, left, right),
            RelExpr::Intersect { all, left, right } => {
                self.build_set_op_intersect(*all, left, right)
            }
            RelExpr::Except { all, left, right } => self.build_set_op_except(*all, left, right),
            RelExpr::Window { functions, input } => self.build_window_agg(functions, input),
            RelExpr::IncrementalSort {
                prefix_keys,
                suffix_keys,
                input,
            } => self.build_incremental_sort(prefix_keys, suffix_keys, input),
            RelExpr::CTE { body, .. } => {
                // CTE body is the primary output; definition is already materialized.
                self.build_plan(body)
            }
            RelExpr::RecursiveCTE { body, .. } => self.build_plan(body),
            RelExpr::MvScan { view_name, .. } => self.build_seq_scan(view_name),
            RelExpr::Unnest { .. } | RelExpr::TableFunction { .. } => {
                self.build_function_scan(expr)
            }
            RelExpr::Values { .. } => self.build_values_result(),
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
        // 1.0 safety: only SeqScan is verified correct end-to-end (see
        // scripts/replan-equivalence-test.sh). IndexScan / IndexOnlyScan
        // / BitmapHeap / Tid builders are not yet verified and have
        // produced wrong results or backend crashes, so scan-strategy
        // advice is parsed and validated but not physically honored —
        // every Scan becomes a SeqScan. Re-enable a strategy here only
        // once it passes the replan-equivalence property test.
        match strategy {
            None
            | Some(ScanStrategy::Seq)
            | Some(ScanStrategy::Index { .. })
            | Some(ScanStrategy::IndexOnly { .. })
            | Some(ScanStrategy::BitmapHeap)
            | Some(ScanStrategy::Tid)
            | Some(ScanStrategy::DoNotScan) => self.build_seq_scan(table),
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
                self.build_seq_scan(table)
            }
        }
    }

    /// Build a `SeqScan` plan node for the given relation name.
    unsafe fn build_seq_scan(
        &mut self,
        table: &str,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let rtindex = self.rtindex_for(table)?;
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
        let pg_expr = expr_translator::translate(clause, &self.expr_ctx);
        if pg_expr.is_null() {
            return Err("clause translation produced null".to_string());
        }
        let bis = self.alloc_node::<pg_sys::BitmapIndexScan>();
        if bis.is_null() {
            return Err("BitmapIndexScan allocation returned null".to_string());
        }
        (*bis).scan.plan.type_ = pg_sys::NodeTag::T_BitmapIndexScan;
        (*bis).scan.scanrelid = rtindex;
        (*bis).indexid = idx_info.oid;
        (*bis).indexqual = pg_sys::lappend((*bis).indexqual, pg_expr.cast());
        (*bis).indexqualorig = pg_sys::lappend((*bis).indexqualorig, pg_expr.cast());
        (*bis).scan.plan.startup_cost = 0.0;
        (*bis).scan.plan.total_cost = 0.5;
        (*bis).scan.plan.plan_rows = 1.0;
        (*bis).scan.plan.plan_width = 0;
        Ok(&mut (*bis).scan.plan as *mut pg_sys::Plan)
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
    unsafe fn build_index_only_scan(
        &mut self,
        table: &str,
        index: &str,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let rtindex = self.rtindex_for(table)?;
        let rel_oid = self.rel_oid_for(table)?;
        let node = self.alloc_node::<pg_sys::IndexOnlyScan>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "IndexOnlyScan allocation".to_string(),
            ));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_IndexOnlyScan;
        (*node).scan.scanrelid = rtindex;

        // Resolve the named index to its OID
        if let Some(info) = crate::index_resolver::resolve_index_by_name(rel_oid, index) {
            (*node).indexid = info.oid;
            debug!(
                table = table,
                index = index,
                index_oid = info.oid.to_u32(),
                "IndexOnlyScan: resolved index"
            );
        } else {
            debug!(
                table = table,
                index = index,
                "IndexOnlyScan: named index not found"
            );
        }

        // Index-only scans are cheaper than regular index scans (no heap fetch)
        self.set_index_costs(&mut (*node).scan.plan, table, 0.05);
        Ok(&mut (*node).scan.plan as *mut pg_sys::Plan)
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
    unsafe fn build_aggref(
        &self,
        name: &str,
        arg: Option<(*mut pg_sys::Expr, pg_sys::Oid, pg_sys::Oid)>,
        aggno: i32,
    ) -> Result<*mut pg_sys::Expr, PlanBuilderError> {
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        let lower = name.to_lowercase();
        let nargs = i32::from(arg.is_some());
        let mut argtypes = [pg_sys::InvalidOid; 1];
        if let Some((_, ty, _)) = arg {
            argtypes[0] = ty;
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

        let aggtype = pg_sys::get_func_rettype(aggfnoid);
        let node = self.alloc_node::<pg_sys::Aggref>();
        (*node).xpr.type_ = pg_sys::NodeTag::T_Aggref;
        (*node).aggfnoid = aggfnoid;
        (*node).aggtype = aggtype;
        (*node).aggcollid = pg_sys::get_typcollation(aggtype);
        (*node).aggtranstype = aggtranstype;
        (*node).aggstar = arg.is_none();
        (*node).aggkind = b'n' as i8; // AGGKIND_NORMAL
        (*node).aggsplit = pg_sys::AggSplit::AGGSPLIT_SIMPLE;
        (*node).aggno = aggno;
        (*node).aggtransno = aggno;
        (*node).agglevelsup = 0;
        if let Some((arg_expr, ty, coll)) = arg {
            (*node).inputcollid = coll;
            (*node).aggargtypes = pg_sys::lappend_oid(std::ptr::null_mut(), ty);
            let te = pg_sys::makeTargetEntry(arg_expr, 1, std::ptr::null_mut(), false);
            (*node).args = pg_sys::lappend(std::ptr::null_mut(), te.cast());
        }
        Ok(node.cast())
    }

    /// Build an `Agg` plan node for `Project(out_columns)` over
    /// `Aggregate(group_by)`. Output columns must each be a group column or
    /// a supported aggregate (`count`/`sum`/`avg`/`min`/`max`) over a
    /// single-relation input. Any other shape returns `Err` (→ native
    /// planner): expressions over aggregates, `DISTINCT` aggregates,
    /// `HAVING`'s nested form, unsupported functions, multi-relation input.
    unsafe fn build_grouped_aggregate(
        &mut self,
        out_columns: &[ProjectionColumn],
        group_by: &[Expr],
        agg_input: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let unsupported = |m: &str| PlanBuilderError::UnsupportedVariant(m.to_owned());
        let child = self.build_plan(agg_input)?;
        if child.is_null() {
            return Err(unsupported("aggregate input"));
        }
        let mut in_tlist: *mut pg_sys::List = std::ptr::null_mut();
        let mut colmap: Vec<(i16, i32, pg_sys::Oid, pg_sys::Oid)> = Vec::new();

        // Pass 1: register every input column referenced by group_by and by
        // aggregate arguments so the scan exposes them.
        let mut grp_pos: Vec<(i32, pg_sys::Oid, pg_sys::Oid)> = Vec::new();
        for g in group_by {
            match self.add_input_col(g, &mut in_tlist, &mut colmap) {
                Some(t) => grp_pos.push(t),
                None => return Err(unsupported("group key not a column")),
            }
        }
        for pc in out_columns {
            if let Expr::Function { name, args } = &pc.expr {
                if Self::is_supported_agg(name) {
                    if let Some(arg) = Self::agg_column_arg(args) {
                        if self.add_input_col(arg, &mut in_tlist, &mut colmap).is_none() {
                            return Err(unsupported("aggregate arg not a column"));
                        }
                    }
                }
            }
        }
        (*child).targetlist = in_tlist;

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
            let entry: *mut pg_sys::Expr = match &pc.expr {
                Expr::Column(_) => {
                    let (pos, ty, coll) = self
                        .add_input_col(&pc.expr, &mut in_tlist, &mut colmap)
                        .ok_or_else(|| unsupported("output column"))?;
                    self.outer_var(pos, ty, coll)
                }
                Expr::Function { name, args } if Self::is_supported_agg(name) => {
                    let arg = match Self::agg_column_arg(args) {
                        Some(c) => {
                            let (pos, ty, coll) = self
                                .add_input_col(c, &mut in_tlist, &mut colmap)
                                .ok_or_else(|| unsupported("aggregate arg"))?;
                            Some((self.outer_var(pos, ty, coll), ty, coll))
                        }
                        None => None,
                    };
                    let aggref = self.build_aggref(name, arg, aggno)?;
                    aggno += 1;
                    aggref
                }
                _ => return Err(unsupported("aggregate output expression")),
            };
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

        // Grouping metadata.
        if group_by.is_empty() {
            (*node).aggstrategy = pg_sys::AggStrategy::AGG_PLAIN;
            (*node).numGroups = 1;
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
            let rows = (*child).plan_rows.max(1.0);
            (*node).numGroups = (rows.sqrt() as i64).clamp(1, 1_000_000);
        }
        (*node).plan.plan_rows = (*node).numGroups as f64;
        (*node).plan.total_cost = (*child).total_cost + (*child).plan_rows * 0.01;
        (*node).plan.startup_cost = (*node).plan.total_cost;
        Ok(&mut (*node).plan as *mut pg_sys::Plan)
    }

    fn is_supported_agg(name: &str) -> bool {
        matches!(
            name.to_lowercase().as_str(),
            "count" | "sum" | "avg" | "min" | "max"
        )
    }

    /// The single column argument of an aggregate, or `None` for a star
    /// aggregate (`count(*)`) or no-arg form.
    fn agg_column_arg(args: &[Expr]) -> Option<&Expr> {
        match args {
            [arg] => match arg {
                Expr::Column(c) if c.column == "*" => None,
                _ => Some(arg),
            },
            _ => None,
        }
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
    unsafe fn resolve_sort_indices(
        &self,
        keys: &[SortKey],
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
            if !resolved {
                return false;
            }
        }
        true
    }

    unsafe fn build_sort(
        &mut self,
        keys: &[SortKey],
        input: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let child = self.build_plan(input)?;
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
            // Every sort key must map to an output column; otherwise the sort
            // index dangles (ORDER BY a non-selected column needs a resjunk
            // targetlist entry Ra does not build). Defer to native PG.
            if !self.resolve_sort_indices(keys, child_tlist, arrays.col_idx, arrays.num_cols) {
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

        // Build Const nodes for limitCount and limitOffset
        (*node).limitCount = make_int8_const(count as i64).cast();
        if offset > 0 {
            (*node).limitOffset = make_int8_const(offset as i64).cast();
        }

        if !child.is_null() {
            (*node).plan.total_cost = (*child).total_cost;
            (*node).plan.plan_rows = count as f64;
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
        let plan = self.build_seq_scan(table)?;
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
        // Unique only collapses *adjacent* equal rows, so its input must be
        // sorted on the distinct columns. Sort by all output columns first.
        let child = self.build_sort_all_columns(raw_child);
        let node = self.alloc_node::<pg_sys::Unique>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "Unique allocation".to_string(),
            ));
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
            let rel_oid = self.first_rel_oid(input);

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
                    let _ = rel_oid; // used for fallback if needed
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
        Ok(&mut (*node).plan as *mut pg_sys::Plan)
    }

    unsafe fn build_set_op_union(
        &mut self,
        all: bool,
        left: &RelExpr,
        right: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let left_plan = self.build_plan(left)?;
        let right_plan = self.build_plan(right)?;
        let node = self.alloc_node::<pg_sys::Append>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "Append allocation".to_string(),
            ));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Append;
        // Build the appendplans list from the two child plans.
        let mut plans_list = std::ptr::null_mut::<pg_sys::List>();
        if !left_plan.is_null() {
            plans_list = pg_sys::lappend(plans_list, left_plan.cast());
        }
        if !right_plan.is_null() {
            plans_list = pg_sys::lappend(plans_list, right_plan.cast());
        }
        (*node).appendplans = plans_list;

        // Propagate cost estimates from children.
        let left_cost = if left_plan.is_null() {
            0.0
        } else {
            (*left_plan).total_cost
        };
        let right_cost = if right_plan.is_null() {
            0.0
        } else {
            (*right_plan).total_cost
        };
        let left_rows = if left_plan.is_null() {
            0.0
        } else {
            (*left_plan).plan_rows
        };
        let right_rows = if right_plan.is_null() {
            0.0
        } else {
            (*right_plan).plan_rows
        };
        (*node).plan.total_cost = left_cost + right_cost;
        (*node).plan.plan_rows = left_rows + right_rows;

        if !all {
            // UNION DISTINCT: wrap Append in Unique to deduplicate.
            // The Unique node uses all output columns for comparison.
            let append_plan = &mut (*node).plan as *mut pg_sys::Plan;
            let unique = self.alloc_node::<pg_sys::Unique>();
            if unique.is_null() {
                return Ok(append_plan);
            }
            (*unique).plan.type_ = pg_sys::NodeTag::T_Unique;
            (*unique).plan.lefttree = append_plan;
            (*unique).plan.total_cost = (*append_plan).total_cost;
            (*unique).plan.plan_rows = ((*append_plan).plan_rows * 0.75).max(1.0);
            (*unique).plan.plan_width = (*append_plan).plan_width;

            // Set unique column arrays for all output columns
            let ncols = if !left_plan.is_null() && !(*left_plan).targetlist.is_null() {
                (*(*left_plan).targetlist).length
            } else {
                0
            };
            if ncols > 0 {
                let col_idx =
                    pg_sys::palloc(ncols as usize * std::mem::size_of::<pg_sys::AttrNumber>())
                        as *mut pg_sys::AttrNumber;
                let operators = pg_sys::palloc(ncols as usize * std::mem::size_of::<pg_sys::Oid>())
                    as *mut pg_sys::Oid;
                let collations = pg_sys::palloc(ncols as usize * std::mem::size_of::<pg_sys::Oid>())
                    as *mut pg_sys::Oid;

                for i in 0..ncols as usize {
                    *col_idx.add(i) = (i + 1) as pg_sys::AttrNumber;
                    *operators.add(i) = pg_sys::InvalidOid; // will use default eq
                    *collations.add(i) = pg_sys::InvalidOid;
                }
                (*unique).numCols = ncols;
                (*unique).uniqColIdx = col_idx;
                (*unique).uniqOperators = operators;
                (*unique).uniqCollations = collations;
            }

            return Ok(&mut (*unique).plan as *mut pg_sys::Plan);
        }

        Ok(&mut (*node).plan as *mut pg_sys::Plan)
    }

    unsafe fn build_set_op_intersect(
        &mut self,
        all: bool,
        left: &RelExpr,
        right: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        self.build_setop_node(pg_sys::SetOpCmd::SETOPCMD_INTERSECT, all, left, right)
    }

    unsafe fn build_set_op_except(
        &mut self,
        all: bool,
        left: &RelExpr,
        right: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        self.build_setop_node(pg_sys::SetOpCmd::SETOPCMD_EXCEPT, all, left, right)
    }

    unsafe fn build_setop_node(
        &mut self,
        cmd: pg_sys::SetOpCmd::Type,
        all: bool,
        left: &RelExpr,
        right: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let left_plan = self.build_plan(left)?;
        let right_plan = self.build_plan(right)?;
        let node = self.alloc_node::<pg_sys::SetOp>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "SetOp allocation".to_string(),
            ));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_SetOp;
        (*node).cmd = cmd;
        (*node).strategy = if all {
            pg_sys::SetOpStrategy::SETOP_HASHED
        } else {
            pg_sys::SetOpStrategy::SETOP_SORTED
        };
        (*node).plan.lefttree = left_plan;
        (*node).plan.righttree = right_plan;
        Ok(&mut (*node).plan as *mut pg_sys::Plan)
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

    unsafe fn build_values_result(&mut self) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let node = self.alloc_node::<pg_sys::Result>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "Result allocation".to_string(),
            ));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Result;
        (*node).plan.plan_rows = 1.0;
        (*node).plan.total_cost = 0.01;
        Ok(&mut (*node).plan as *mut pg_sys::Plan)
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
        // PostgreSQL cost constants (from GUC defaults)
        const SEQ_PAGE_COST: f64 = 1.0;
        const CPU_TUPLE_COST: f64 = 0.01;

        let table_lower = table.to_lowercase();
        if let Some(stats) = self.stats.get(&table_lower) {
            let reltuples = stats.row_count.max(1.0);
            let relpages = if stats.total_size > 0 {
                (stats.total_size as f64 / 8192.0).max(1.0)
            } else {
                (reltuples / 50.0).max(1.0) // estimate ~50 tuples per page
            };

            plan.startup_cost = 0.0;
            plan.total_cost = relpages * SEQ_PAGE_COST + reltuples * CPU_TUPLE_COST;
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
        const RANDOM_PAGE_COST: f64 = 4.0;
        const CPU_TUPLE_COST: f64 = 0.01;
        const CPU_INDEX_TUPLE_COST: f64 = 0.005;

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
            plan.total_cost = pages_fetched * RANDOM_PAGE_COST
                + selected_tuples * (CPU_INDEX_TUPLE_COST + CPU_TUPLE_COST);
            plan.plan_rows = selected_tuples;
            plan.plan_width = 100;
        } else {
            plan.startup_cost = 0.1;
            plan.total_cost = 50.0;
            plan.plan_rows = 100.0;
            plan.plan_width = 100;
        }
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
    }
    map
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
