//! Post-extraction ordering propagation pass.
//!
//! After the e-graph extractor picks a plan bottom-up, Sort nodes may
//! remain even when their input already provides the required ordering
//! (e.g., from an index scan). This module implements a single tree
//! rewrite pass that propagates physical properties bottom-up and:
//!
//! 1. Eliminates redundant Sort nodes whose input already satisfies
//!    the required ordering.
//! 2. Converts Sort to `IncrementalSort` when the input provides a
//!    prefix of the required keys and the cost model says it's
//!    beneficial.

use ra_core::algebra::{RelExpr, SortDirection, SortKey};
use ra_core::expr::{ColumnRef, Expr};
use ra_core::facts::FactsProvider;
use ra_core::physical_properties::{derive_properties, is_sort_redundant};
use ra_core::properties::{Ordering, OrderingColumn, PropertySet};

use crate::incremental_sort::{detect_prefix_match, estimate_costs};

/// Post-extraction pass: propagate ordering properties bottom-up,
/// eliminating redundant Sort nodes and converting to
/// `IncrementalSort` where beneficial.
#[must_use]
pub fn propagate_ordering(expr: RelExpr, facts: &dyn FactsProvider) -> RelExpr {
    let (_props, rewritten) = propagate_inner(expr, facts);
    rewritten
}

/// Recursively propagate ordering properties and rewrite Sort nodes.
/// Returns the computed properties of the node and the rewritten tree.
#[expect(clippy::too_many_lines, reason = "exhaustive match over all RelExpr variants")]
fn propagate_inner(expr: RelExpr, facts: &dyn FactsProvider) -> (PropertySet, RelExpr) {
    match expr {
        RelExpr::Sort { input, keys } => handle_sort(*input, keys, facts),

        // Single-input operators that preserve ordering
        RelExpr::Filter { predicate, input } => {
            let (child_props, rewritten_child) = propagate_inner(*input, facts);
            let node = RelExpr::Filter {
                predicate,
                input: Box::new(rewritten_child),
            };
            (child_props, node)
        }

        RelExpr::Limit {
            count,
            offset,
            input,
        } => {
            let (child_props, rewritten_child) = propagate_inner(*input, facts);
            let node = RelExpr::Limit {
                count,
                offset,
                input: Box::new(rewritten_child),
            };
            (child_props, node)
        }

        RelExpr::Window { functions, input } => {
            let (child_props, rewritten_child) = propagate_inner(*input, facts);
            let node = RelExpr::Window {
                functions,
                input: Box::new(rewritten_child),
            };
            (child_props, node)
        }

        RelExpr::VectorFilter {
            vector_expr,
            query_vector,
            metric,
            threshold,
            input,
        } => {
            let (child_props, rewritten_child) = propagate_inner(*input, facts);
            let node = RelExpr::VectorFilter {
                vector_expr,
                query_vector,
                metric,
                threshold,
                input: Box::new(rewritten_child),
            };
            (child_props, node)
        }

        // Project: ordering prefix preserved for surviving columns
        RelExpr::Project { columns, input } => {
            let (child_props, rewritten_child) = propagate_inner(*input, facts);
            let node = RelExpr::Project {
                columns: columns.clone(),
                input: Box::new(rewritten_child),
            };
            let my_props = derive_properties(&node, &[&child_props]);
            (my_props, node)
        }

        // Distinct drops ordering
        RelExpr::Distinct { input } => {
            let (_child_props, rewritten_child) = propagate_inner(*input, facts);
            let node = RelExpr::Distinct {
                input: Box::new(rewritten_child),
            };
            let my_props = derive_properties(&node, &[]);
            (my_props, node)
        }

        // Aggregate: ordering depends on strategy
        RelExpr::Aggregate {
            group_by,
            aggregates,
            input,
        } => {
            let (_child_props, rewritten_child) = propagate_inner(*input, facts);
            let node = RelExpr::Aggregate {
                group_by,
                aggregates,
                input: Box::new(rewritten_child),
            };
            let my_props = derive_properties(&node, &[]);
            (my_props, node)
        }

        // Join: preserves left ordering for inner/left/semi/anti
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => {
            let (left_props, rewritten_left) = propagate_inner(*left, facts);
            let (right_props, rewritten_right) = propagate_inner(*right, facts);
            let node = RelExpr::Join {
                join_type,
                condition,
                left: Box::new(rewritten_left),
                right: Box::new(rewritten_right),
            };
            let my_props = derive_properties(&node, &[&left_props, &right_props]);
            (my_props, node)
        }

        // IncrementalSort: already an incremental sort, just recurse
        RelExpr::IncrementalSort {
            prefix_keys,
            suffix_keys,
            input,
        } => {
            let (_child_props, rewritten_child) = propagate_inner(*input, facts);
            let node = RelExpr::IncrementalSort {
                prefix_keys: prefix_keys.clone(),
                suffix_keys: suffix_keys.clone(),
                input: Box::new(rewritten_child),
            };
            let my_props = derive_properties(&node, &[]);
            (my_props, node)
        }

        // IndexScan: derive ordering from index metadata
        RelExpr::IndexScan {
            ref table,
            ref column,
        } => {
            let props = derive_index_scan_properties(table, column, facts);
            (props, expr)
        }

        // IndexOnlyScan: derive ordering from index metadata
        RelExpr::IndexOnlyScan {
            ref table,
            ref index,
            ..
        } => {
            let props = derive_index_only_scan_properties(table, index, facts);
            (props, expr)
        }

        // Binary set operators: destroy ordering
        RelExpr::Union {
            all, left, right, ..
        } => {
            let (_lp, rewritten_left) = propagate_inner(*left, facts);
            let (_rp, rewritten_right) = propagate_inner(*right, facts);
            let node = RelExpr::Union {
                all,
                left: Box::new(rewritten_left),
                right: Box::new(rewritten_right),
            };
            (PropertySet::new(), node)
        }

        RelExpr::Intersect {
            all, left, right, ..
        } => {
            let (_lp, rewritten_left) = propagate_inner(*left, facts);
            let (_rp, rewritten_right) = propagate_inner(*right, facts);
            let node = RelExpr::Intersect {
                all,
                left: Box::new(rewritten_left),
                right: Box::new(rewritten_right),
            };
            (PropertySet::new(), node)
        }

        RelExpr::Except {
            all, left, right, ..
        } => {
            let (_lp, rewritten_left) = propagate_inner(*left, facts);
            let (_rp, rewritten_right) = propagate_inner(*right, facts);
            let node = RelExpr::Except {
                all,
                left: Box::new(rewritten_left),
                right: Box::new(rewritten_right),
            };
            (PropertySet::new(), node)
        }

        // CTE: recurse into both definition and body
        RelExpr::CTE {
            name,
            definition,
            body,
        } => {
            let (_dp, rewritten_def) = propagate_inner(*definition, facts);
            let (body_props, rewritten_body) = propagate_inner(*body, facts);
            let node = RelExpr::CTE {
                name,
                definition: Box::new(rewritten_def),
                body: Box::new(rewritten_body),
            };
            (body_props, node)
        }

        // RecursiveCTE: recurse into all sub-expressions
        RelExpr::RecursiveCTE {
            name,
            base_case,
            recursive_case,
            body,
            cycle_detection,
        } => {
            let (_bp, rewritten_base) = propagate_inner(*base_case, facts);
            let (_rp, rewritten_recursive) = propagate_inner(*recursive_case, facts);
            let (body_props, rewritten_body) = propagate_inner(*body, facts);
            let node = RelExpr::RecursiveCTE {
                name,
                base_case: Box::new(rewritten_base),
                recursive_case: Box::new(rewritten_recursive),
                body: Box::new(rewritten_body),
                cycle_detection,
            };
            (body_props, node)
        }

        // BitmapHeapScan: recurse into bitmap child
        RelExpr::BitmapHeapScan {
            table,
            bitmap,
            recheck_cond,
        } => {
            let (_bp, rewritten_bitmap) = propagate_inner(*bitmap, facts);
            let node = RelExpr::BitmapHeapScan {
                table,
                bitmap: Box::new(rewritten_bitmap),
                recheck_cond,
            };
            (PropertySet::new(), node)
        }

        // BitmapAnd/BitmapOr: recurse into all inputs
        RelExpr::BitmapAnd { inputs } => {
            let rewritten: Vec<Box<RelExpr>> = inputs
                .into_iter()
                .map(|inp| {
                    let (_p, r) = propagate_inner(*inp, facts);
                    Box::new(r)
                })
                .collect();
            (PropertySet::new(), RelExpr::BitmapAnd { inputs: rewritten })
        }

        RelExpr::BitmapOr { inputs } => {
            let rewritten: Vec<Box<RelExpr>> = inputs
                .into_iter()
                .map(|inp| {
                    let (_p, r) = propagate_inner(*inp, facts);
                    Box::new(r)
                })
                .collect();
            (PropertySet::new(), RelExpr::BitmapOr { inputs: rewritten })
        }

        // Unnest with input
        RelExpr::Unnest {
            expr: unnest_expr,
            alias,
            input: Some(inp),
            with_ordinality,
        } => {
            let (_cp, rewritten_child) = propagate_inner(*inp, facts);
            let node = RelExpr::Unnest {
                expr: unnest_expr,
                alias,
                input: Some(Box::new(rewritten_child)),
                with_ordinality,
            };
            (PropertySet::new(), node)
        }

        // TableFunction with input
        RelExpr::TableFunction {
            name,
            args,
            columns,
            input: Some(inp),
        } => {
            let (_cp, rewritten_child) = propagate_inner(*inp, facts);
            let node = RelExpr::TableFunction {
                name,
                args,
                columns,
                input: Some(Box::new(rewritten_child)),
            };
            (PropertySet::new(), node)
        }

        // RowPattern: recurse input
        RelExpr::RowPattern {
            input,
            partition_by,
            order_by,
            pattern,
            defines,
            measures,
            mode,
            skip_mode,
        } => {
            let (_cp, rewritten_child) = propagate_inner(*input, facts);
            let node = RelExpr::RowPattern {
                input: Box::new(rewritten_child),
                partition_by,
                order_by: order_by.clone(),
                pattern,
                defines,
                measures,
                mode,
                skip_mode,
            };
            let my_props = derive_properties(&node, &[]);
            (my_props, node)
        }

        // ParallelAggregate: recurse input, no ordering guarantees
        RelExpr::ParallelAggregate {
            group_by,
            aggregates,
            input,
            workers,
        } => {
            let (_cp, rewritten_child) = propagate_inner(*input, facts);
            let node = RelExpr::ParallelAggregate {
                group_by,
                aggregates,
                input: Box::new(rewritten_child),
                workers,
            };
            (PropertySet::new(), node)
        }

        // Gather: recurse input, no ordering guarantees
        RelExpr::Gather { input, workers } => {
            let (_cp, rewritten_child) = propagate_inner(*input, facts);
            let node = RelExpr::Gather {
                input: Box::new(rewritten_child),
                workers,
            };
            (PropertySet::new(), node)
        }

        // TopK: recurse input
        RelExpr::TopK {
            vector_expr,
            query_vector,
            metric,
            k,
            input,
        } => {
            let (_cp, rewritten_child) = propagate_inner(*input, facts);
            let node = RelExpr::TopK {
                vector_expr,
                query_vector,
                metric,
                k,
                input: Box::new(rewritten_child),
            };
            (PropertySet::new(), node)
        }

        // ParallelHashJoin: recurse both inputs, no ordering
        RelExpr::ParallelHashJoin {
            join_type,
            condition,
            left,
            right,
            workers,
        } => {
            let (_lp, rewritten_left) = propagate_inner(*left, facts);
            let (_rp, rewritten_right) = propagate_inner(*right, facts);
            let node = RelExpr::ParallelHashJoin {
                join_type,
                condition,
                left: Box::new(rewritten_left),
                right: Box::new(rewritten_right),
                workers,
            };
            (PropertySet::new(), node)
        }

        // DML: recurse into source/from/using
        RelExpr::Insert {
            table,
            columns,
            source,
            on_conflict,
            returning,
        } => {
            let (_sp, rewritten_source) = propagate_inner(*source, facts);
            let node = RelExpr::Insert {
                table,
                columns,
                source: Box::new(rewritten_source),
                on_conflict,
                returning,
            };
            (PropertySet::new(), node)
        }

        RelExpr::Update {
            table,
            assignments,
            filter,
            from,
            returning,
        } => {
            let rewritten_from = from.map(|f| {
                let (_fp, rf) = propagate_inner(*f, facts);
                Box::new(rf)
            });
            let node = RelExpr::Update {
                table,
                assignments,
                filter,
                from: rewritten_from,
                returning,
            };
            (PropertySet::new(), node)
        }

        RelExpr::Delete {
            table,
            filter,
            using,
            returning,
        } => {
            let rewritten_using = using.map(|u| {
                let (_up, ru) = propagate_inner(*u, facts);
                Box::new(ru)
            });
            let node = RelExpr::Delete {
                table,
                filter,
                using: rewritten_using,
                returning,
            };
            (PropertySet::new(), node)
        }

        RelExpr::Merge {
            target,
            source,
            on,
            when_clauses,
            returning,
        } => {
            let (_sp, rewritten_source) = propagate_inner(*source, facts);
            let node = RelExpr::Merge {
                target,
                source: Box::new(rewritten_source),
                on,
                when_clauses,
                returning,
            };
            (PropertySet::new(), node)
        }

        graph_table @ RelExpr::GraphTable { .. } => {
            // GRAPH_TABLE is a leaf row source with no ordering
            // guarantees; pass it through unchanged.
            (PropertySet::new(), graph_table)
        }

        // Leaf nodes with no ordering: Scan, Values, MultiUnnest, etc.
        RelExpr::Scan { .. }
        | RelExpr::Values { .. }
        | RelExpr::MultiUnnest { .. }
        | RelExpr::BitmapIndexScan { .. }
        | RelExpr::ParallelScan { .. }
        | RelExpr::MvScan { .. }
        | RelExpr::Unnest { input: None, .. }
        | RelExpr::TableFunction { input: None, .. } => (PropertySet::new(), expr),
    }
}

/// Handle a Sort node: check if redundant or convertible to `IncrementalSort`.
fn handle_sort(
    input: RelExpr,
    keys: Vec<SortKey>,
    facts: &dyn FactsProvider,
) -> (PropertySet, RelExpr) {
    let (child_props, rewritten_child) = propagate_inner(input, facts);

    // Check if the sort is redundant (input already provides ordering)
    if is_sort_redundant(&keys, &child_props) {
        // Input already satisfies — elide the Sort entirely.
        // The properties from the child already cover our needs.
        return (child_props, rewritten_child);
    }

    // Check for incremental sort opportunity: input provides a prefix
    if let Some(child_ordering) = child_props.ordering() {
        let input_sort_keys = ordering_to_sort_keys(child_ordering);
        if let Some(prefix_match) = detect_prefix_match(&keys, &input_sort_keys) {
            // Estimate whether incremental sort is beneficial
            let row_count = estimate_row_count_for(&rewritten_child, facts);
            let prefix_ndv = estimate_prefix_ndv(
                &prefix_match.prefix_keys,
                &rewritten_child,
                facts,
            );
            let costs = estimate_costs(row_count, prefix_ndv);
            if costs.is_beneficial() {
                let isort = RelExpr::IncrementalSort {
                    prefix_keys: prefix_match.prefix_keys,
                    suffix_keys: prefix_match.suffix_keys,
                    input: Box::new(rewritten_child),
                };
                let all_keys: Vec<SortKey> = keys;
                let isort_props = sort_keys_to_property_set(&all_keys);
                return (isort_props, isort);
            }
        }
    }

    // Keep Sort as-is
    let sort_node = RelExpr::Sort {
        input: Box::new(rewritten_child),
        keys: keys.clone(),
    };
    let sort_props = sort_keys_to_property_set(&keys);
    (sort_props, sort_node)
}

/// Derive physical ordering properties for an `IndexScan` node by
/// looking up the index metadata from the facts provider.
fn derive_index_scan_properties(
    table: &str,
    column: &str,
    facts: &dyn FactsProvider,
) -> PropertySet {
    // IndexScan specifies a single column — if the table has an index
    // whose leading column matches, the output is ordered by that column.
    if let Some(schema) = facts.get_schema(table) {
        for idx in &schema.indexes {
            if idx.columns.first().map(String::as_str) == Some(column) {
                let ordering = Ordering::new(vec![OrderingColumn::new(
                    ColumnRef::new(column),
                    SortDirection::Asc,
                )]);
                return PropertySet::with_ordering(ordering);
            }
        }
    }
    // Conservative: if we can't confirm index ordering, assume the
    // column provides ascending order (IndexScan semantics).
    let ordering = Ordering::new(vec![OrderingColumn::new(
        ColumnRef::new(column),
        SortDirection::Asc,
    )]);
    PropertySet::with_ordering(ordering)
}

/// Derive physical ordering properties for an `IndexOnlyScan` node.
fn derive_index_only_scan_properties(
    table: &str,
    index: &str,
    facts: &dyn FactsProvider,
) -> PropertySet {
    if let Some(schema) = facts.get_schema(table) {
        for idx in &schema.indexes {
            if idx.name == index {
                let ordering_cols: Vec<OrderingColumn> = idx
                    .columns
                    .iter()
                    .map(|c| {
                        OrderingColumn::new(
                            ColumnRef::new(c.as_str()),
                            SortDirection::Asc,
                        )
                    })
                    .collect();
                if !ordering_cols.is_empty() {
                    return PropertySet::with_ordering(Ordering::new(ordering_cols));
                }
            }
        }
    }
    PropertySet::new()
}

/// Convert an `Ordering` (property representation) to `Vec<SortKey>`
/// (algebra representation) for use with `detect_prefix_match`.
fn ordering_to_sort_keys(ordering: &Ordering) -> Vec<SortKey> {
    ordering
        .columns
        .iter()
        .map(|oc| SortKey {
            expr: Expr::Column(oc.column.clone()),
            direction: oc.direction,
            nulls: oc.nulls,
        })
        .collect()
}

/// Convert sort keys to a `PropertySet` with the corresponding ordering.
fn sort_keys_to_property_set(keys: &[SortKey]) -> PropertySet {
    let ordering_cols: Vec<OrderingColumn> = keys
        .iter()
        .filter_map(|k| {
            if let Expr::Column(col) = &k.expr {
                Some(OrderingColumn {
                    column: col.clone(),
                    direction: k.direction,
                    nulls: k.nulls,
                })
            } else {
                None
            }
        })
        .collect();

    if ordering_cols.is_empty() {
        PropertySet::new()
    } else {
        PropertySet::with_ordering(Ordering::new(ordering_cols))
    }
}

/// Estimate row count for the given expression using table stats.
fn estimate_row_count_for(expr: &RelExpr, facts: &dyn FactsProvider) -> f64 {
    match expr {
        RelExpr::Scan { table, .. }
        | RelExpr::IndexScan { table, .. }
        | RelExpr::IndexOnlyScan { table, .. }
        | RelExpr::ParallelScan { table, .. } => facts
            .get_table_stats(table)
            .map_or(1000.0, |s| s.row_count),
        RelExpr::Filter { input, .. } => {
            // Conservative: assume 33% selectivity
            estimate_row_count_for(input, facts) * 0.33
        }
        RelExpr::Join { left, .. } => {
            // Conservative: assume join doesn't reduce much
            estimate_row_count_for(left, facts)
        }
        RelExpr::Limit { count, input, .. } => {
            let input_rows = estimate_row_count_for(input, facts);
            (*count as f64).min(input_rows)
        }
        _ => {
            // For other nodes, try to find a table reference
            if let Some(child) = expr.children().first() {
                estimate_row_count_for(child, facts)
            } else {
                1000.0
            }
        }
    }
}

/// Estimate the number of distinct values for prefix key columns.
fn estimate_prefix_ndv(
    prefix_keys: &[SortKey],
    expr: &RelExpr,
    facts: &dyn FactsProvider,
) -> f64 {
    // Find the table name from the expression tree
    let table_name = find_base_table(expr);

    let mut combined_ndv: f64 = 1.0;
    for key in prefix_keys {
        if let Expr::Column(col) = &key.expr {
            let ndv = table_name
                .as_ref()
                .and_then(|t| facts.get_column_stats(t, &col.column))
                .map_or(100.0, |stats| stats.distinct_count.max(1.0));
            // For multi-column prefix, multiply NDVs (rough upper bound)
            combined_ndv *= ndv;
        }
    }

    // Cap at estimated row count to avoid nonsensical values
    let row_count = estimate_row_count_for(expr, facts);
    combined_ndv.min(row_count)
}

/// Walk the expression tree to find the first base table name.
fn find_base_table(expr: &RelExpr) -> Option<String> {
    match expr {
        RelExpr::Scan { table, .. }
        | RelExpr::IndexScan { table, .. }
        | RelExpr::IndexOnlyScan { table, .. }
        | RelExpr::ParallelScan { table, .. } => Some(table.clone()),
        _ => expr
            .children()
            .first()
            .and_then(|child| find_base_table(child)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::{NullOrdering, SortDirection};
    use ra_core::facts::{
        EmptyFactsProvider, IndexInfo, IndexType, StorageFormat, TableInfo, TableStats,
    };
    use ra_core::statistics::ColumnStats;
    use std::time::Duration;

    fn sort_key(name: &str, dir: SortDirection) -> SortKey {
        SortKey {
            expr: Expr::Column(ColumnRef::new(name)),
            direction: dir,
            nulls: NullOrdering::Last,
        }
    }

    /// A facts provider that knows about a table with an index.
    struct TestFacts {
        schemas: Vec<TableInfo>,
        table_stats: Vec<(String, TableStats)>,
        column_stats: Vec<(String, String, ColumnStats)>,
    }

    impl TestFacts {
        fn new() -> Self {
            Self {
                schemas: Vec::new(),
                table_stats: Vec::new(),
                column_stats: Vec::new(),
            }
        }

        fn with_indexed_table(mut self, table: &str, index_cols: &[&str]) -> Self {
            self.schemas.push(TableInfo {
                name: table.to_string(),
                columns: index_cols
                    .iter()
                    .map(|c| (c.to_string(), ra_core::facts::DataType::Integer))
                    .collect(),
                primary_key: vec![index_cols[0].to_string()],
                foreign_keys: vec![],
                indexes: vec![IndexInfo {
                    name: format!("idx_{table}"),
                    index_type: IndexType::BTree,
                    columns: index_cols.iter().map(ToString::to_string).collect(),
                    included_columns: vec![],
                    is_unique: true,
                }],
                storage_format: StorageFormat::RowBased,
            });
            self.table_stats.push((
                table.to_string(),
                TableStats {
                    row_count: 100_000.0,
                    page_count: 1000,
                    average_row_size: 64.0,
                    table_size_bytes: 6_400_000,
                    live_tuples: Some(100_000.0),
                    dead_tuples: Some(0.0),
                    last_analyzed: Some(0),
                    confidence: 1.0,
                    estimated_modifications: 0,
                },
            ));
            for col in index_cols {
                self.column_stats.push((
                    table.to_string(),
                    col.to_string(),
                    ColumnStats {
                        distinct_count: 10_000.0,
                        null_fraction: 0.0,
                        min_value: None,
                        max_value: None,
                        avg_length: Some(8.0),
                        histogram: None,
                        correlation: None,
                        most_common_values: None,
                        most_common_freqs: None,
                    },
                ));
            }
            self
        }
    }

    impl FactsProvider for TestFacts {
        fn get_table_stats(&self, table: &str) -> Option<&TableStats> {
            self.table_stats
                .iter()
                .find(|(t, _)| t == table)
                .map(|(_, s)| s)
        }

        fn get_column_stats(&self, table: &str, column: &str) -> Option<&ColumnStats> {
            self.column_stats
                .iter()
                .find(|(t, c, _)| t == table && c == column)
                .map(|(_, _, s)| s)
        }

        fn hardware_profile(&self) -> &ra_core::facts::HardwareProfile {
            static HARDWARE: ra_core::facts::HardwareProfile = ra_core::facts::HardwareProfile {
                cpu_cores: 8,
                available_memory: 16 * 1024 * 1024 * 1024,
                total_memory: 16 * 1024 * 1024 * 1024,
                simd_width: 256,
                has_gpu: false,
                gpu_memory: None,
                l1_cache_size: 32 * 1024,
                l2_cache_size: 256 * 1024,
                l3_cache_size: 8 * 1024 * 1024,
                cpu_architecture: ra_core::facts::CpuArchitecture::X86_64,
            };
            &HARDWARE
        }

        fn get_schema(&self, table: &str) -> Option<&TableInfo> {
            self.schemas.iter().find(|s| s.name == table)
        }

        fn runtime_stats(
            &self,
            _operator_id: &str,
        ) -> Option<&ra_core::facts::OperatorStats> {
            None
        }

        fn database_name(&self) -> &'static str {
            "test"
        }

        fn supports_feature(&self, _feature: &str) -> bool {
            false
        }

        fn sql_dialect(&self) -> ra_core::facts::SqlDialect {
            ra_core::facts::SqlDialect::Generic
        }

        fn memory_limit(&self) -> Option<u64> {
            None
        }

        fn optimizer_timeout(&self) -> Duration {
            Duration::from_secs(60)
        }
    }

    #[test]
    fn eliminates_redundant_sort_over_index_scan() {
        let facts = TestFacts::new().with_indexed_table("users", &["id", "name"]);

        // Sort(id ASC) over IndexScan(users, id) should be eliminated
        let plan = RelExpr::Sort {
            keys: vec![sort_key("id", SortDirection::Asc)],
            input: Box::new(RelExpr::IndexScan {
                table: "users".to_string(),
                column: "id".to_string(),
            }),
        };

        let result = propagate_ordering(plan, &facts);

        // Should be just the IndexScan without the Sort
        assert!(
            matches!(result, RelExpr::IndexScan { .. }),
            "expected IndexScan, got {result:?}"
        );
    }

    #[test]
    fn keeps_sort_when_ordering_differs() {
        let facts = TestFacts::new().with_indexed_table("users", &["id"]);

        // Sort(id DESC) over IndexScan(users, id) — different direction
        let plan = RelExpr::Sort {
            keys: vec![sort_key("id", SortDirection::Desc)],
            input: Box::new(RelExpr::IndexScan {
                table: "users".to_string(),
                column: "id".to_string(),
            }),
        };

        let result = propagate_ordering(plan, &facts);
        assert!(
            matches!(result, RelExpr::Sort { .. }),
            "expected Sort to remain, got {result:?}"
        );
    }

    #[test]
    fn converts_sort_to_incremental_sort() {
        let facts = TestFacts::new().with_indexed_table("orders", &["customer_id"]);

        // Sort(customer_id ASC, order_date ASC) over IndexScan(orders, customer_id)
        // Should become IncrementalSort since input provides prefix [customer_id]
        let plan = RelExpr::Sort {
            keys: vec![
                sort_key("customer_id", SortDirection::Asc),
                sort_key("order_date", SortDirection::Asc),
            ],
            input: Box::new(RelExpr::IndexScan {
                table: "orders".to_string(),
                column: "customer_id".to_string(),
            }),
        };

        let result = propagate_ordering(plan, &facts);
        assert!(
            matches!(result, RelExpr::IncrementalSort { .. }),
            "expected IncrementalSort, got {result:?}"
        );
    }

    #[test]
    fn preserves_sort_with_no_input_ordering() {
        let facts = EmptyFactsProvider::new();

        let plan = RelExpr::Sort {
            keys: vec![sort_key("name", SortDirection::Asc)],
            input: Box::new(RelExpr::Scan {
                table: "users".to_string(),
                alias: None,
            }),
        };

        let result = propagate_ordering(plan, &facts);
        assert!(
            matches!(result, RelExpr::Sort { .. }),
            "expected Sort to remain for unordered scan"
        );
    }

    #[test]
    fn eliminates_sort_through_filter() {
        let facts = TestFacts::new().with_indexed_table("users", &["id"]);

        // Sort(id ASC) → Filter → IndexScan(users, id)
        // Filter preserves ordering, so Sort is redundant
        let plan = RelExpr::Sort {
            keys: vec![sort_key("id", SortDirection::Asc)],
            input: Box::new(RelExpr::Filter {
                predicate: Expr::Column(ColumnRef::new("active")),
                input: Box::new(RelExpr::IndexScan {
                    table: "users".to_string(),
                    column: "id".to_string(),
                }),
            }),
        };

        let result = propagate_ordering(plan, &facts);
        // Should be Filter(IndexScan) without Sort
        assert!(
            matches!(result, RelExpr::Filter { .. }),
            "expected Filter without Sort, got {result:?}"
        );
    }

    #[test]
    fn ordering_lost_through_distinct() {
        let facts = TestFacts::new().with_indexed_table("users", &["id"]);

        // Sort(id ASC) → Distinct → IndexScan(users, id)
        // Distinct destroys ordering, so Sort must remain
        let plan = RelExpr::Sort {
            keys: vec![sort_key("id", SortDirection::Asc)],
            input: Box::new(RelExpr::Distinct {
                input: Box::new(RelExpr::IndexScan {
                    table: "users".to_string(),
                    column: "id".to_string(),
                }),
            }),
        };

        let result = propagate_ordering(plan, &facts);
        assert!(
            matches!(result, RelExpr::Sort { .. }),
            "expected Sort to remain after Distinct"
        );
    }

    #[test]
    fn ordering_preserved_through_limit() {
        let facts = TestFacts::new().with_indexed_table("users", &["id"]);

        // Sort(id ASC) → Limit → IndexScan(users, id)
        // Limit preserves ordering, so Sort is redundant
        let plan = RelExpr::Sort {
            keys: vec![sort_key("id", SortDirection::Asc)],
            input: Box::new(RelExpr::Limit {
                count: 10,
                offset: 0,
                input: Box::new(RelExpr::IndexScan {
                    table: "users".to_string(),
                    column: "id".to_string(),
                }),
            }),
        };

        let result = propagate_ordering(plan, &facts);
        // Should be Limit(IndexScan) without Sort
        assert!(
            matches!(result, RelExpr::Limit { .. }),
            "expected Limit without Sort, got {result:?}"
        );
    }

    #[test]
    fn no_crash_on_leaf_nodes() {
        let facts = EmptyFactsProvider::new();

        let scan = RelExpr::Scan {
            table: "t".to_string(),
            alias: None,
        };
        let result = propagate_ordering(scan.clone(), &facts);
        assert_eq!(result, scan);

        let values = RelExpr::Values {
            rows: vec![vec![Expr::Column(ColumnRef::new("x"))]],
        };
        let result = propagate_ordering(values.clone(), &facts);
        assert_eq!(result, values);
    }
}
