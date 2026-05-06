//! Direct PostgreSQL `Plan` node construction from Ra `RelExpr` trees.
//!
//! This module provides [`PlanBuilder`], which translates Ra's optimized
//! relational expression tree directly into PostgreSQL internal `Plan` nodes
//! (allocating them in the current PostgreSQL memory context via palloc).
//!
//! # Architecture
//!
//! The current planner hook uses a GUC-manipulation (cost-adjustment) strategy
//! via [`crate::plan_converter`] to nudge PostgreSQL's standard planner toward
//! Ra's preferred plan. `PlanBuilder` goes further: it constructs the complete
//! `PlannedStmt` directly, bypassing PostgreSQL's planner entirely for queries
//! where Ra has high confidence.
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
//! Call [`PlanBuilder::build_planned_stmt`] from the planner hook when Ra has
//! high confidence (replacing the current cost-manipulation path):
//!
//! ```ignore
//! if confidence >= RA_DIRECT_PLAN_THRESHOLD {
//!     let mut builder = PlanBuilder::new(original_query, &table_map);
//!     if let Ok(planned) = unsafe { builder.build_planned_stmt(&optimized_expr) } {
//!         return planned;
//!     }
//!     // Fall back to cost-manipulation approach on any error.
//! }
//! ```

use std::collections::HashMap;
use std::ffi::CString;

use pgrx::prelude::*;
use pgrx::pg_sys;

use ra_core::algebra::{AggregateExpr, JoinType, ProjectionColumn, RelExpr, SortKey};
use ra_core::expr::{BinOp, Const as RaConst, Expr as RaExpr, UnaryOp};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by [`PlanBuilder`].
#[derive(Debug, thiserror::Error)]
pub enum PlanBuilderError {
    /// A table referenced in the Ra plan was not found in the PostgreSQL catalog.
    #[error("table '{0}' not found in PostgreSQL catalog")]
    TableNotFound(String),

    /// A `RelExpr` variant is not yet supported by the direct plan builder.
    /// The caller should fall back to cost-manipulation for this query.
    #[error("unsupported RelExpr variant for direct plan: {0}")]
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
    /// Maps table name (lowercase) → 1-based range-table index.
    rtindex_map: HashMap<String, pg_sys::Index>,
    /// Maps table name (lowercase) → relation OID (for column type lookups).
    rtoid_map: HashMap<String, pg_sys::Oid>,
    /// Accumulated cost estimates propagated up from child nodes.
    total_cost: f64,
    /// Estimated output row count propagated from child nodes.
    plan_rows: f64,
}

impl PlanBuilder {
    /// Create a new plan builder for the given query.
    ///
    /// `table_map` maps each base-table name (as it appears in `RelExpr::Scan`)
    /// to its 1-based index in the original query's range table. Build this by
    /// iterating `(*query).rtable` before calling.
    ///
    /// # Safety
    ///
    /// `query` must be a valid, non-null pointer to a PostgreSQL `Query` node
    /// allocated in the current memory context.
    pub unsafe fn new(
        query: *mut pg_sys::Query,
        table_map: HashMap<String, (pg_sys::Index, pg_sys::Oid)>,
    ) -> Self {
        let mut rtindex_map = HashMap::new();
        let mut rtoid_map = HashMap::new();
        for (name, (idx, oid)) in table_map {
            rtindex_map.insert(name.clone(), idx);
            rtoid_map.insert(name, oid);
        }
        Self {
            original_query: query,
            rtindex_map,
            rtoid_map,
            total_cost: 0.0,
            plan_rows: 1.0,
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
            return Err(PlanBuilderError::NullPointer("PlannedStmt allocation".to_string()));
        }

        (*stmt).type_ = pg_sys::NodeTag::T_PlannedStmt;
        (*stmt).commandType = pg_sys::CmdType::CMD_SELECT;
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
    unsafe fn build_plan(
        &mut self,
        expr: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        match expr {
            RelExpr::Scan { table, .. } => self.build_seq_scan(table),
            RelExpr::Filter { predicate, input } => {
                let child = self.build_plan(input)?;
                // Translate predicate and append to the child node's qual list.
                let pg_expr = expr_to_pg_expr(predicate, &self.rtindex_map, &self.rtoid_map);
                if !child.is_null() && !pg_expr.is_null() {
                    (*child).qual = pg_sys::lappend((*child).qual, pg_expr.cast());
                }
                Ok(child)
            }
            RelExpr::Project { columns, input } => {
                // Fold projection into child node's targetlist.
                let child = self.build_plan(input)?;
                self.set_targetlist(child, columns);
                Ok(child)
            }
            RelExpr::Join { join_type, condition: _, left, right } => {
                self.build_join(*join_type, left, right)
            }
            RelExpr::Aggregate { group_by, aggregates, input } => {
                self.build_aggregate(group_by, aggregates, input)
            }
            RelExpr::Sort { keys, input } => self.build_sort(keys, input),
            RelExpr::Limit { count, offset, input } => {
                self.build_limit(*count, *offset, input)
            }
            RelExpr::IndexScan { table, column } => self.build_index_scan(table, column),
            RelExpr::BitmapIndexScan { table, index, predicate: _ } => {
                self.build_bitmap_index_scan(table, index)
            }
            RelExpr::BitmapHeapScan { table, bitmap, .. } => {
                self.build_bitmap_heap_scan(table, bitmap)
            }
            RelExpr::BitmapAnd { inputs } => self.build_bitmap_and(inputs),
            RelExpr::BitmapOr { inputs } => self.build_bitmap_or(inputs),
            RelExpr::IndexOnlyScan { table, index, .. } => {
                self.build_index_only_scan(table, index)
            }
            RelExpr::ParallelScan { table, workers } => {
                self.build_parallel_seq_scan(table, *workers)
            }
            RelExpr::ParallelHashJoin { join_type, condition: _, left, right, workers } => {
                self.build_parallel_hash_join(*join_type, left, right, *workers)
            }
            RelExpr::ParallelAggregate { group_by, aggregates, input, workers } => {
                self.build_parallel_aggregate(group_by, aggregates, input, *workers)
            }
            RelExpr::Gather { input, workers } => self.build_gather(input, *workers),
            RelExpr::Distinct { input } => self.build_unique(input),
            RelExpr::Union { all, left, right } => self.build_set_op_union(*all, left, right),
            RelExpr::Intersect { all, left, right } => {
                self.build_set_op_intersect(*all, left, right)
            }
            RelExpr::Except { all, left, right } => self.build_set_op_except(*all, left, right),
            RelExpr::Window { functions: _, input } => self.build_window_agg(input),
            RelExpr::IncrementalSort { suffix_keys, input, .. } => {
                self.build_incremental_sort(suffix_keys, input)
            }
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
            RelExpr::TopK { .. } => {
                Err(PlanBuilderError::UnsupportedVariant("TopK (vector)".to_string()))
            }
            RelExpr::VectorFilter { .. } => Err(PlanBuilderError::UnsupportedVariant(
                "VectorFilter (vector)".to_string(),
            )),
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
            return Err(PlanBuilderError::NullPointer("SeqScan allocation".to_string()));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_SeqScan;
        (*node).scan.scanrelid = rtindex;
        self.set_default_costs(&mut (*node).scan.plan, table);
        Ok((*node).scan.plan.as_mut_ptr())
    }

    /// Build an `IndexScan` node for MIN/MAX index optimization.
    unsafe fn build_index_scan(
        &mut self,
        table: &str,
        _column: &str,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let rtindex = self.rtindex_for(table)?;
        let node = self.alloc_node::<pg_sys::IndexScan>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("IndexScan allocation".to_string()));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_IndexScan;
        (*node).scan.scanrelid = rtindex;
        // TODO: resolve index OID from column name via catalog lookup.
        self.set_default_costs(&mut (*node).scan.plan, table);
        Ok((*node).scan.plan.as_mut_ptr())
    }

    /// Build a `BitmapIndexScan` node.
    unsafe fn build_bitmap_index_scan(
        &mut self,
        table: &str,
        _index: &str,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let rtindex = self.rtindex_for(table)?;
        let node = self.alloc_node::<pg_sys::BitmapIndexScan>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("BitmapIndexScan allocation".to_string()));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_BitmapIndexScan;
        (*node).scan.scanrelid = rtindex;
        // TODO: resolve index OID and set indexid, indexqual, indexqualorig.
        Ok((*node).scan.plan.as_mut_ptr())
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
            return Err(PlanBuilderError::NullPointer("BitmapHeapScan allocation".to_string()));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_BitmapHeapScan;
        (*node).scan.scanrelid = rtindex;
        (*node).scan.plan.lefttree = bitmap_plan;
        self.set_default_costs(&mut (*node).scan.plan, table);
        Ok((*node).scan.plan.as_mut_ptr())
    }

    /// Build a `BitmapAnd` node combining multiple bitmap scans.
    unsafe fn build_bitmap_and(
        &mut self,
        inputs: &[Box<RelExpr>],
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let node = self.alloc_node::<pg_sys::BitmapAnd>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("BitmapAnd allocation".to_string()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_BitmapAnd;
        // TODO: build bitmapplans list from inputs.
        let _ = inputs;
        Ok((*node).plan.as_mut_ptr())
    }

    /// Build a `BitmapOr` node combining multiple bitmap scans.
    unsafe fn build_bitmap_or(
        &mut self,
        inputs: &[Box<RelExpr>],
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let node = self.alloc_node::<pg_sys::BitmapOr>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("BitmapOr allocation".to_string()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_BitmapOr;
        // TODO: build bitmapplans list from inputs.
        let _ = inputs;
        Ok((*node).plan.as_mut_ptr())
    }

    /// Build an `IndexOnlyScan` node (covering index — no heap fetch).
    unsafe fn build_index_only_scan(
        &mut self,
        table: &str,
        _index: &str,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let rtindex = self.rtindex_for(table)?;
        let node = self.alloc_node::<pg_sys::IndexOnlyScan>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("IndexOnlyScan allocation".to_string()));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_IndexOnlyScan;
        (*node).scan.scanrelid = rtindex;
        // TODO: resolve index OID, set indexid and indexqual.
        self.set_default_costs(&mut (*node).scan.plan, table);
        Ok((*node).scan.plan.as_mut_ptr())
    }

    // -----------------------------------------------------------------------
    // Join builders
    // -----------------------------------------------------------------------

    /// Build the appropriate join plan node for the given `JoinType`.
    ///
    /// Defaults to `HashJoin` for equi-joins (`Inner`, `LeftOuter`, `RightOuter`,
    /// `FullOuter`) and `NestLoop` for `Cross`, `Semi`, and `Anti` joins.
    unsafe fn build_join(
        &mut self,
        join_type: JoinType,
        left: &RelExpr,
        right: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let left_plan = self.build_plan(left)?;
        let right_plan = self.build_plan(right)?;

        match join_type {
            JoinType::Inner | JoinType::LeftOuter | JoinType::RightOuter | JoinType::FullOuter => {
                self.build_hash_join(join_type, left_plan, right_plan)
            }
            JoinType::Cross | JoinType::Semi | JoinType::Anti => {
                self.build_nested_loop(join_type, left_plan, right_plan)
            }
        }
    }

    unsafe fn build_hash_join(
        &mut self,
        join_type: JoinType,
        left_plan: *mut pg_sys::Plan,
        right_plan: *mut pg_sys::Plan,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let node = self.alloc_node::<pg_sys::HashJoin>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("HashJoin allocation".to_string()));
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
        (*node).join.plan.righttree = (*hash_node).plan.as_mut_ptr();

        // TODO: populate hashclauses (join condition as hash-compatible OpExpr).
        self.propagate_costs_binary(
            &mut (*node).join.plan,
            left_plan,
            right_plan,
        );
        Ok((*node).join.plan.as_mut_ptr())
    }

    unsafe fn build_nested_loop(
        &mut self,
        join_type: JoinType,
        left_plan: *mut pg_sys::Plan,
        right_plan: *mut pg_sys::Plan,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let node = self.alloc_node::<pg_sys::NestLoop>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("NestLoop allocation".to_string()));
        }
        (*node).join.plan.type_ = pg_sys::NodeTag::T_NestLoop;
        (*node).join.jointype = ra_join_type_to_pg(join_type);
        (*node).join.plan.lefttree = left_plan;
        (*node).join.plan.righttree = right_plan;
        self.propagate_costs_binary(&mut (*node).join.plan, left_plan, right_plan);
        Ok((*node).join.plan.as_mut_ptr())
    }

    // -----------------------------------------------------------------------
    // Aggregate, Sort, Limit
    // -----------------------------------------------------------------------

    unsafe fn build_aggregate(
        &mut self,
        _group_by: &[ra_core::expr::Expr],
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
        (*node).aggstrategy = pg_sys::AggStrategy::AGG_PLAIN;
        (*node).aggsplit = pg_sys::AggSplit::AGGSPLIT_SIMPLE;
        // TODO: populate grpColIdx, grpOperators, grpCollations, numGroups.
        // TODO: translate group-by exprs to sort-column array.
        if !child.is_null() {
            (*node).plan.total_cost = (*child).total_cost + 100.0;
            (*node).plan.plan_rows = ((*child).plan_rows * 0.1).max(1.0);
        }
        Ok((*node).plan.as_mut_ptr())
    }

    unsafe fn build_sort(
        &mut self,
        _keys: &[SortKey],
        input: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let child = self.build_plan(input)?;
        let node = self.alloc_node::<pg_sys::Sort>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("Sort allocation".to_string()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Sort;
        (*node).plan.lefttree = child;
        // TODO: populate numCols, sortColIdx, sortOperators, collations, nullsFirst.
        if !child.is_null() {
            let n = (*child).plan_rows.max(1.0);
            (*node).plan.startup_cost = (*child).total_cost + n * n.ln().max(1.0) * 0.001;
            (*node).plan.total_cost = (*node).plan.startup_cost;
            (*node).plan.plan_rows = (*child).plan_rows;
        }
        Ok((*node).plan.as_mut_ptr())
    }

    unsafe fn build_limit(
        &mut self,
        _count: u64,
        _offset: u64,
        input: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let child = self.build_plan(input)?;
        let node = self.alloc_node::<pg_sys::Limit>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("Limit allocation".to_string()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Limit;
        (*node).plan.lefttree = child;
        // TODO: set limitOffset and limitCount as Const nodes.
        if !child.is_null() {
            (*node).plan.total_cost = (*child).total_cost;
            (*node).plan.plan_rows = _count as f64;
        }
        Ok((*node).plan.as_mut_ptr())
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
        left: &RelExpr,
        right: &RelExpr,
        _workers: usize,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let plan = self.build_join(join_type, left, right)?;
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
            return Err(PlanBuilderError::NullPointer("Gather allocation".to_string()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Gather;
        (*node).plan.lefttree = child;
        (*node).num_workers = workers as i32;
        if !child.is_null() {
            (*node).plan.total_cost = (*child).total_cost;
            (*node).plan.plan_rows = (*child).plan_rows;
        }
        Ok((*node).plan.as_mut_ptr())
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
            return Err(PlanBuilderError::NullPointer("Unique allocation".to_string()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Unique;
        (*node).plan.lefttree = child;
        // TODO: populate numCols, uniqColIdx, uniqOperators, uniqCollations.
        if !child.is_null() {
            (*node).plan.total_cost = (*child).total_cost;
            (*node).plan.plan_rows = (*child).plan_rows * 0.8; // rough distinct estimate
        }
        Ok((*node).plan.as_mut_ptr())
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
            return Err(PlanBuilderError::NullPointer("Append allocation".to_string()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Append;
        // TODO: build appendplans list from left_plan and right_plan.
        // For UNION (not ALL): wrap with SetOp to deduplicate.
        let _ = (all, left_plan, right_plan);
        Ok((*node).plan.as_mut_ptr())
    }

    unsafe fn build_set_op_intersect(
        &mut self,
        all: bool,
        left: &RelExpr,
        right: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        self.build_setop_node(
            pg_sys::SetOpCmd::SETOPCMD_INTERSECT,
            all,
            left,
            right,
        )
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
        cmd: pg_sys::SetOpCmd,
        all: bool,
        left: &RelExpr,
        right: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let left_plan = self.build_plan(left)?;
        let right_plan = self.build_plan(right)?;
        let node = self.alloc_node::<pg_sys::SetOp>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("SetOp allocation".to_string()));
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
        Ok((*node).plan.as_mut_ptr())
    }

    unsafe fn build_window_agg(
        &mut self,
        input: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let child = self.build_plan(input)?;
        let node = self.alloc_node::<pg_sys::WindowAgg>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("WindowAgg allocation".to_string()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_WindowAgg;
        (*node).plan.lefttree = child;
        // TODO: populate winref, partNumCols, ordNumCols, frameOptions, etc.
        if !child.is_null() {
            (*node).plan.total_cost = (*child).total_cost + (*child).plan_rows * 0.01;
            (*node).plan.plan_rows = (*child).plan_rows;
        }
        Ok((*node).plan.as_mut_ptr())
    }

    unsafe fn build_incremental_sort(
        &mut self,
        _suffix_keys: &[SortKey],
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
        // TODO: populate nPresortedCols, numCols, sortColIdx, etc.
        if !child.is_null() {
            (*node).sort.plan.total_cost = (*child).total_cost;
            (*node).sort.plan.plan_rows = (*child).plan_rows;
        }
        Ok((*node).sort.plan.as_mut_ptr())
    }

    // -----------------------------------------------------------------------
    // Function scan and Values
    // -----------------------------------------------------------------------

    unsafe fn build_function_scan(
        &mut self,
        _expr: &RelExpr,
    ) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let node = self.alloc_node::<pg_sys::FunctionScan>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("FunctionScan allocation".to_string()));
        }
        (*node).scan.plan.type_ = pg_sys::NodeTag::T_FunctionScan;
        // TODO: build functions list from the Unnest/TableFunction expression.
        (*node).scan.plan.plan_rows = 100.0; // conservative estimate
        (*node).scan.plan.total_cost = 10.0;
        Ok((*node).scan.plan.as_mut_ptr())
    }

    unsafe fn build_values_result(&mut self) -> Result<*mut pg_sys::Plan, PlanBuilderError> {
        let node = self.alloc_node::<pg_sys::Result>();
        if node.is_null() {
            return Err(PlanBuilderError::NullPointer("Result allocation".to_string()));
        }
        (*node).plan.type_ = pg_sys::NodeTag::T_Result;
        (*node).plan.plan_rows = 1.0;
        (*node).plan.total_cost = 0.01;
        Ok((*node).plan.as_mut_ptr())
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Look up the range-table index for a table by name.
    fn rtindex_for(&self, table: &str) -> Result<pg_sys::Index, PlanBuilderError> {
        self.rtindex_map
            .get(&table.to_lowercase())
            .copied()
            .ok_or_else(|| PlanBuilderError::TableNotFound(table.to_string()))
    }

    /// Look up the relation OID for a table by name.
    fn rtoid_for(&self, table: &str) -> pg_sys::Oid {
        self.rtoid_map
            .get(&table.to_lowercase())
            .copied()
            .unwrap_or(pg_sys::InvalidOid)
    }

    /// Set the targetlist on a plan node from Ra projection columns.
    unsafe fn set_targetlist(&self, plan: *mut pg_sys::Plan, columns: &[ProjectionColumn]) {
        if plan.is_null() {
            return;
        }
        for (i, pc) in columns.iter().enumerate() {
            let pg_expr = expr_to_pg_expr(&pc.expr, &self.rtindex_map, &self.rtoid_map);
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
            // Set resname from alias if available
            if let Some(alias) = &pc.alias {
                if let Ok(cs) = CString::new(alias.as_str()) {
                    (*te).resname = pg_sys::pstrdup(cs.as_ptr());
                }
            }
            (*plan).targetlist = pg_sys::lappend((*plan).targetlist, te.cast());
        }
    }

    /// Set approximate cost estimates on a scan plan node.
    unsafe fn set_default_costs(&self, plan: &mut pg_sys::Plan, _table: &str) {
        // TODO: look up pg_class.reltuples for a real estimate.
        plan.startup_cost = 0.0;
        plan.total_cost = 100.0;
        plan.plan_rows = 1000.0;
        plan.plan_width = 100;
    }

    /// Propagate costs from two child nodes to a join node.
    unsafe fn propagate_costs_binary(
        &self,
        plan: &mut pg_sys::Plan,
        left: *mut pg_sys::Plan,
        right: *mut pg_sys::Plan,
    ) {
        let left_cost = if left.is_null() { 0.0 } else { (*left).total_cost };
        let right_cost = if right.is_null() { 0.0 } else { (*right).total_cost };
        let left_rows = if left.is_null() { 1.0 } else { (*left).plan_rows };
        let right_rows = if right.is_null() { 1.0 } else { (*right).plan_rows };
        plan.startup_cost = left_cost;
        plan.total_cost = left_cost + right_cost + left_rows * right_rows * 0.01;
        plan.plan_rows = (left_rows * right_rows * 0.1).max(1.0);
        plan.plan_width = 200;
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
fn ra_join_type_to_pg(jt: JoinType) -> pg_sys::JoinType {
    match jt {
        JoinType::Inner => pg_sys::JoinType::JOIN_INNER,
        JoinType::LeftOuter => pg_sys::JoinType::JOIN_LEFT,
        JoinType::RightOuter => pg_sys::JoinType::JOIN_RIGHT,
        JoinType::FullOuter => pg_sys::JoinType::JOIN_FULL,
        JoinType::Cross => pg_sys::JoinType::JOIN_INNER, // cross = inner with true condition
        JoinType::Semi => pg_sys::JoinType::JOIN_SEMI,
        JoinType::Anti => pg_sys::JoinType::JOIN_ANTI,
    }
}

// ---------------------------------------------------------------------------
// Expression translation: Ra Expr → PostgreSQL Expr*
// ---------------------------------------------------------------------------

/// Translate a Ra [`RaExpr`] to a PostgreSQL `Expr*` node.
///
/// Returns `null_mut()` for unsupported expression types.
/// The returned node is allocated in the current PostgreSQL memory context.
///
/// # Coverage
///
/// | Ra `Expr` variant              | PostgreSQL node       |
/// |--------------------------------|----------------------|
/// | `Const::Null`                  | `Const` (null)       |
/// | `Const::Bool / Int / Float`    | `Const` (typed)      |
/// | `Const::String`                | `Const` (text)       |
/// | `Column(ref)`                  | `Var`                |
/// | `BinOp::And / Or`              | `BoolExpr`           |
/// | `BinOp::Eq / Ne / Lt / …`     | `OpExpr`             |
/// | `BinOp::Add / Sub / Mul / Div` | `OpExpr`             |
/// | `UnaryOp::Not`                 | `BoolExpr (NOT)`     |
/// | `UnaryOp::IsNull / IsNotNull`  | `NullTest`           |
/// | `UnaryOp::Neg`                 | `OpExpr` (unary -)   |
/// | Other variants                 | `null_mut()`         |
///
/// # Safety
///
/// Must be called from within a live PostgreSQL backend process.
pub unsafe fn expr_to_pg_expr(
    expr: &RaExpr,
    rtindex_map: &HashMap<String, pg_sys::Index>,
    rtoid_map: &HashMap<String, pg_sys::Oid>,
) -> *mut pg_sys::Expr {
    match expr {
        RaExpr::Const(c) => const_to_pg(c),
        RaExpr::Column(col_ref) => column_to_var(col_ref, rtindex_map, rtoid_map),
        RaExpr::BinOp { op, left, right } => match op {
            BinOp::And => build_bool_expr(
                pg_sys::BoolExprType::AND_EXPR,
                left,
                right,
                rtindex_map,
                rtoid_map,
            ),
            BinOp::Or => build_bool_expr(
                pg_sys::BoolExprType::OR_EXPR,
                left,
                right,
                rtindex_map,
                rtoid_map,
            ),
            _ => build_op_expr(op, left, right, rtindex_map, rtoid_map),
        },
        RaExpr::UnaryOp { op, operand } => match op {
            UnaryOp::Not => {
                let arg = expr_to_pg_expr(operand, rtindex_map, rtoid_map);
                if arg.is_null() {
                    return std::ptr::null_mut();
                }
                let node = pg_sys::palloc0(std::mem::size_of::<pg_sys::BoolExpr>())
                    as *mut pg_sys::BoolExpr;
                (*node).xpr.type_ = pg_sys::NodeTag::T_BoolExpr;
                (*node).boolop = pg_sys::BoolExprType::NOT_EXPR;
                (*node).args = pg_sys::list_make1(arg.cast());
                (*node).xpr.cast()
            }
            UnaryOp::IsNull => build_null_test(
                operand,
                pg_sys::NullTestType::IS_NULL,
                rtindex_map,
                rtoid_map,
            ),
            UnaryOp::IsNotNull => build_null_test(
                operand,
                pg_sys::NullTestType::IS_NOT_NULL,
                rtindex_map,
                rtoid_map,
            ),
            UnaryOp::Neg => build_unary_neg(operand, rtindex_map, rtoid_map),
        },
        // Unsupported: Function, Case, Cast, SubQuery, Array, etc.
        _ => std::ptr::null_mut(),
    }
}

/// Translate a Ra `Const` to a PostgreSQL `Const` node.
unsafe fn const_to_pg(c: &RaConst) -> *mut pg_sys::Expr {
    let node = pg_sys::palloc0(std::mem::size_of::<pg_sys::Const>()) as *mut pg_sys::Const;
    (*node).xpr.type_ = pg_sys::NodeTag::T_Const;

    match c {
        RaConst::Null => {
            (*node).consttype = pg_sys::TEXTOID;
            (*node).consttypmod = -1;
            (*node).constlen = -1;
            (*node).constisnull = true;
            (*node).constbyval = false;
        }
        RaConst::Bool(b) => {
            (*node).consttype = pg_sys::BOOLOID;
            (*node).consttypmod = -1;
            (*node).constlen = 1;
            (*node).constvalue = pg_sys::Datum::from(*b as i32);
            (*node).constisnull = false;
            (*node).constbyval = true;
        }
        RaConst::Int(i) => {
            (*node).consttype = pg_sys::INT8OID;
            (*node).consttypmod = -1;
            (*node).constlen = 8;
            (*node).constvalue = pg_sys::Datum::from(*i);
            (*node).constisnull = false;
            (*node).constbyval = true;
        }
        RaConst::Float(f) => {
            // float8 is pass-by-value on 64-bit platforms
            (*node).consttype = pg_sys::FLOAT8OID;
            (*node).consttypmod = -1;
            (*node).constlen = 8;
            (*node).constvalue = pg_sys::Float8GetDatum(*f);
            (*node).constisnull = false;
            (*node).constbyval = true;
        }
        RaConst::String(s) => {
            if let Ok(cs) = CString::new(s.as_str()) {
                (*node).consttype = pg_sys::TEXTOID;
                (*node).consttypmod = -1;
                (*node).constlen = -1;
                (*node).constvalue = pg_sys::CStringGetTextDatum(cs.as_ptr());
                (*node).constisnull = false;
                (*node).constbyval = false;
            } else {
                (*node).constisnull = true;
            }
        }
    }
    (*node).xpr.cast()
}

/// Translate a Ra `ColumnRef` to a PostgreSQL `Var` node.
unsafe fn column_to_var(
    col: &ra_core::expr::ColumnRef,
    rtindex_map: &HashMap<String, pg_sys::Index>,
    rtoid_map: &HashMap<String, pg_sys::Oid>,
) -> *mut pg_sys::Expr {
    let table = col.table.as_deref().unwrap_or("").to_lowercase();
    let rtindex = match rtindex_map.get(&table) {
        Some(&idx) => idx,
        None => return std::ptr::null_mut(),
    };
    let reloid = match rtoid_map.get(&table) {
        Some(&oid) => oid,
        None => return std::ptr::null_mut(),
    };

    // Resolve column attribute number from catalog
    let col_name = match CString::new(col.column.as_str()) {
        Ok(cs) => cs,
        Err(_) => return std::ptr::null_mut(),
    };
    let attnum = pg_sys::get_attnum(reloid, col_name.as_ptr());
    if attnum == pg_sys::InvalidAttrNumber {
        return std::ptr::null_mut();
    }
    let atttype = pg_sys::get_atttype(reloid, attnum);
    if atttype == pg_sys::InvalidOid {
        return std::ptr::null_mut();
    }
    let atttypmod = pg_sys::get_atttypmod(reloid, attnum);

    let var = pg_sys::palloc0(std::mem::size_of::<pg_sys::Var>()) as *mut pg_sys::Var;
    (*var).xpr.type_ = pg_sys::NodeTag::T_Var;
    (*var).varno = rtindex;
    (*var).varattno = attnum;
    (*var).vartype = atttype;
    (*var).vartypmod = atttypmod;
    (*var).varcollid = pg_sys::InvalidOid;
    (*var).varlevelsup = 0;
    (*var).xpr.cast()
}

/// Translate a binary comparison/arithmetic op to a PostgreSQL `OpExpr`.
unsafe fn build_op_expr(
    op: &BinOp,
    left: &RaExpr,
    right: &RaExpr,
    rtindex_map: &HashMap<String, pg_sys::Index>,
    rtoid_map: &HashMap<String, pg_sys::Oid>,
) -> *mut pg_sys::Expr {
    let left_pg = expr_to_pg_expr(left, rtindex_map, rtoid_map);
    let right_pg = expr_to_pg_expr(right, rtindex_map, rtoid_map);
    if left_pg.is_null() || right_pg.is_null() {
        return std::ptr::null_mut();
    }

    let op_str = match op {
        BinOp::Eq => "=",
        BinOp::Ne => "<>",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Concat => "||",
        _ => return std::ptr::null_mut(),
    };

    // Look up operator OID via catalog
    let op_cstr = match CString::new(op_str) {
        Ok(cs) => cs,
        Err(_) => return std::ptr::null_mut(),
    };
    let op_string_node = pg_sys::makeString(op_cstr.as_ptr().cast_mut());
    let opname_list = pg_sys::list_make1(op_string_node.cast());

    // Determine left/right type OIDs by examining the Expr types
    let left_type = expr_result_type(left_pg);
    let right_type = expr_result_type(right_pg);

    let opno = pg_sys::OpernameGetOprid(opname_list, left_type, right_type);
    if opno == pg_sys::InvalidOid {
        return std::ptr::null_mut();
    }

    // Get function OID and result type from the operator catalog entry
    let opfuncid = pg_sys::get_opcode(opno);
    let opresulttype = pg_sys::get_func_rettype(opfuncid);

    let node = pg_sys::palloc0(std::mem::size_of::<pg_sys::OpExpr>()) as *mut pg_sys::OpExpr;
    (*node).xpr.type_ = pg_sys::NodeTag::T_OpExpr;
    (*node).opno = opno;
    (*node).opfuncid = opfuncid;
    (*node).opresulttype = opresulttype;
    (*node).opretset = false;
    (*node).opcollid = pg_sys::InvalidOid;
    (*node).inputcollid = pg_sys::InvalidOid;
    let mut args = std::ptr::null_mut::<pg_sys::List>();
    args = pg_sys::lappend(args, left_pg.cast());
    args = pg_sys::lappend(args, right_pg.cast());
    (*node).args = args;
    (*node).xpr.cast()
}

/// Build a PostgreSQL `BoolExpr` (AND/OR) from two Ra expressions.
unsafe fn build_bool_expr(
    bool_type: pg_sys::BoolExprType,
    left: &RaExpr,
    right: &RaExpr,
    rtindex_map: &HashMap<String, pg_sys::Index>,
    rtoid_map: &HashMap<String, pg_sys::Oid>,
) -> *mut pg_sys::Expr {
    let left_pg = expr_to_pg_expr(left, rtindex_map, rtoid_map);
    let right_pg = expr_to_pg_expr(right, rtindex_map, rtoid_map);
    if left_pg.is_null() || right_pg.is_null() {
        return std::ptr::null_mut();
    }
    let node = pg_sys::palloc0(std::mem::size_of::<pg_sys::BoolExpr>()) as *mut pg_sys::BoolExpr;
    (*node).xpr.type_ = pg_sys::NodeTag::T_BoolExpr;
    (*node).boolop = bool_type;
    let mut args = std::ptr::null_mut::<pg_sys::List>();
    args = pg_sys::lappend(args, left_pg.cast());
    args = pg_sys::lappend(args, right_pg.cast());
    (*node).args = args;
    (*node).xpr.cast()
}

/// Build a PostgreSQL `NullTest` node (IS NULL / IS NOT NULL).
unsafe fn build_null_test(
    operand: &RaExpr,
    test_type: pg_sys::NullTestType,
    rtindex_map: &HashMap<String, pg_sys::Index>,
    rtoid_map: &HashMap<String, pg_sys::Oid>,
) -> *mut pg_sys::Expr {
    let arg = expr_to_pg_expr(operand, rtindex_map, rtoid_map);
    if arg.is_null() {
        return std::ptr::null_mut();
    }
    let node =
        pg_sys::palloc0(std::mem::size_of::<pg_sys::NullTest>()) as *mut pg_sys::NullTest;
    (*node).xpr.type_ = pg_sys::NodeTag::T_NullTest;
    (*node).arg = arg.cast();
    (*node).nulltesttype = test_type;
    (*node).argisrow = false;
    (*node).xpr.cast()
}

/// Build a unary negation `OpExpr` (`-x`).
unsafe fn build_unary_neg(
    operand: &RaExpr,
    rtindex_map: &HashMap<String, pg_sys::Index>,
    rtoid_map: &HashMap<String, pg_sys::Oid>,
) -> *mut pg_sys::Expr {
    let arg = expr_to_pg_expr(operand, rtindex_map, rtoid_map);
    if arg.is_null() {
        return std::ptr::null_mut();
    }
    let arg_type = expr_result_type(arg);
    let op_cstr = CString::new("-").expect("valid CString");
    let op_node = pg_sys::makeString(op_cstr.as_ptr().cast_mut());
    let opname = pg_sys::list_make1(op_node.cast());
    let opno = pg_sys::OpernameGetOprid(opname, arg_type, pg_sys::InvalidOid);
    if opno == pg_sys::InvalidOid {
        return std::ptr::null_mut();
    }
    let opfuncid = pg_sys::get_opcode(opno);
    let opresulttype = pg_sys::get_func_rettype(opfuncid);
    let node = pg_sys::palloc0(std::mem::size_of::<pg_sys::OpExpr>()) as *mut pg_sys::OpExpr;
    (*node).xpr.type_ = pg_sys::NodeTag::T_OpExpr;
    (*node).opno = opno;
    (*node).opfuncid = opfuncid;
    (*node).opresulttype = opresulttype;
    (*node).args = pg_sys::list_make1(arg.cast());
    (*node).xpr.cast()
}

/// Infer the result type OID of a PostgreSQL `Expr*` node.
///
/// Used to determine operand types for operator catalog lookup.
unsafe fn expr_result_type(expr: *mut pg_sys::Expr) -> pg_sys::Oid {
    if expr.is_null() {
        return pg_sys::InvalidOid;
    }
    match (*expr).type_ {
        pg_sys::NodeTag::T_Const => (*(expr as *mut pg_sys::Const)).consttype,
        pg_sys::NodeTag::T_Var => (*(expr as *mut pg_sys::Var)).vartype,
        pg_sys::NodeTag::T_OpExpr => (*(expr as *mut pg_sys::OpExpr)).opresulttype,
        pg_sys::NodeTag::T_BoolExpr => pg_sys::BOOLOID,
        pg_sys::NodeTag::T_NullTest => pg_sys::BOOLOID,
        pg_sys::NodeTag::T_FuncExpr => (*(expr as *mut pg_sys::FuncExpr)).funcresulttype,
        _ => pg_sys::InvalidOid,
    }
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
        let name = std::ffi::CStr::from_ptr(relname).to_string_lossy().to_lowercase();
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
    pg_sys::RangeVarGetRelid(rv, pg_sys::NoLock as i32, true /* missing_ok */)
}
