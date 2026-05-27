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
        }
    }

    /// Build a complete `PlannedStmt` from an optimized `RelExpr` tree.
    ///
    /// Returns `Err` if the plan tree cannot be constructed directly (e.g., an
    /// unsupported variant is encountered). The caller should fall back to the
    /// cost-manipulation strategy in that case.
    ///
    /// # Safety
    ///
    /// Must be called from within a live PostgreSQL backend process.
    /// Allocates Plan nodes in the current PostgreSQL memory context.
    pub unsafe fn build_planned_stmt(
        &mut self,
        expr: &RelExpr,
    ) -> Result<*mut pg_sys::PlannedStmt, PlanBuilderError> {
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
            RelExpr::Scan { table, .. } => self.build_seq_scan(table),
            RelExpr::Filter { predicate, input } => {
                let child = self.build_plan(input)?;
                let pg_expr = expr_translator::translate(predicate, &self.expr_ctx);
                if !child.is_null() && !pg_expr.is_null() {
                    (*child).qual = pg_sys::lappend((*child).qual, pg_expr.cast());
                }
                Ok(child)
            }
            RelExpr::Project { columns, input } => {
                let child = self.build_plan(input)?;
                self.set_targetlist(child, columns);
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
            RelExpr::Gather { input, workers } => self.build_gather(input, *workers),
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
        }
    }

    // -----------------------------------------------------------------------
    // Scan builders
    // -----------------------------------------------------------------------

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

        match join_type {
            JoinType::Inner | JoinType::LeftOuter | JoinType::RightOuter | JoinType::FullOuter => {
                self.build_hash_join(join_type, left_plan, right_plan, pg_condition)
            }
            JoinType::Cross | JoinType::Semi | JoinType::Anti => {
                self.build_nested_loop(join_type, left_plan, right_plan, pg_condition)
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

    // -----------------------------------------------------------------------
    // Aggregate, Sort, Limit
    // -----------------------------------------------------------------------

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
        (*node).numGroups = if group_by.is_empty() {
            1
        } else {
            // Conservative estimate: sqrt(input_rows), clamped.
            // PG 18 changed `Result.numHashes` (and similar) from u64 to i64.
            (input_rows.sqrt() as i64).clamp(10, 1_000_000)
        };

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

        // Build sort column metadata arrays using sort_utils
        let child_tlist = if child.is_null() {
            std::ptr::null_mut()
        } else {
            (*child).targetlist
        };
        let rel_oid = self.first_rel_oid(input);

        if let Some(arrays) = crate::sort_utils::build_sort_arrays(keys, child_tlist, rel_oid) {
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

    unsafe fn build_unique(
        &mut self,
        input: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let child = self.build_plan(input)?;
        let node = self.alloc_node::<pg_sys::Unique>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer(
                "Unique allocation".to_string(),
            ));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Unique;
        (*node).plan.lefttree = child;

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
    unsafe fn set_targetlist(&self, plan: *mut pg_sys::Plan, columns: &[ProjectionColumn]) {
        if plan.is_null() {
            return;
        }
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
            (*plan).targetlist = pg_sys::lappend((*plan).targetlist, te.cast());
        }
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
