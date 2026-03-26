#![allow(clippy::match_same_arms)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::doc_markdown)]
//! Physical property propagation through relational expression trees.
//!
//! This module derives the output physical properties of a `RelExpr`
//! node from the properties of its inputs. It also computes the
//! enforcer cost needed when a required property is not naturally
//! provided (e.g., inserting a Sort node).
//!
//! Property propagation follows the Volcano / Cascades model:
//! - **Required** properties flow top-down (what a parent needs).
//! - **Provided** properties flow bottom-up (what a child delivers).
//! - **Enforcers** bridge the gap when provided < required.

use crate::algebra::{
    JoinType, NullOrdering, RelExpr, SortDirection, SortKey,
};
use crate::cost::Cost;
use crate::expr::{ColumnRef, Expr};
use crate::properties::{
    Ordering, OrderingColumn, Partitioning, PhysicalProperty,
    PropertySet,
};

/// Derive the output physical properties produced by a `RelExpr`
/// given the properties of its input(s).
///
/// For leaf nodes the properties come from catalog knowledge
/// (e.g., a table scanned via an index may be ordered). For
/// interior nodes the properties are inferred from the operator
/// semantics and the input properties.
#[must_use]
pub fn derive_properties(
    expr: &RelExpr,
    input_props: &[&PropertySet],
) -> PropertySet {
    match expr {
        RelExpr::Scan { .. }
        | RelExpr::IndexScan { .. }
        | RelExpr::IndexOnlyScan { .. }
        | RelExpr::Values { .. }
        | RelExpr::MultiUnnest { .. }
        | RelExpr::Unnest { .. }
        | RelExpr::TableFunction { .. }
        | RelExpr::CTE { .. }
        | RelExpr::RecursiveCTE { .. }
        | RelExpr::Union { .. }
        | RelExpr::Intersect { .. }
        | RelExpr::Except { .. } => PropertySet::new(),

        RelExpr::Filter { .. }
        | RelExpr::Limit { .. }
        | RelExpr::Window { .. } => {
            // These operators preserve input properties.
            input_props
                .first()
                .map_or_else(PropertySet::new, |p| (*p).clone())
        }

        RelExpr::Project { columns, .. } => {
            derive_project_properties(columns, input_props)
        }

        RelExpr::Sort { keys, .. } => {
            derive_sort_properties(keys, input_props)
        }

        RelExpr::Distinct { .. } => {
            // Distinct may or may not preserve ordering depending
            // on implementation (hash vs sort). Conservatively
            // drop ordering but preserve partitioning.
            let mut ps = PropertySet::new();
            if let Some(inp) = input_props.first() {
                if let Some(part) = inp.partitioning() {
                    ps.add(PhysicalProperty::Partitioning(
                        part.clone(),
                    ));
                }
            }
            ps
        }

        RelExpr::Aggregate { group_by, .. } => {
            derive_aggregate_properties(group_by)
        }

        RelExpr::Join {
            join_type, left, ..
        } => derive_join_properties(
            *join_type,
            left,
            input_props,
        ),

        RelExpr::RowPattern { order_by, .. } => {
            derive_row_pattern_properties(order_by)
        }

        RelExpr::IncrementalSort {
            prefix_keys,
            suffix_keys,
            ..
        } => {
            let all_keys: Vec<SortKey> = prefix_keys
                .iter()
                .chain(suffix_keys.iter())
                .cloned()
                .collect();
            derive_sort_properties(&all_keys, input_props)
        }

        // Bitmap scans don't provide ordering guarantees
        RelExpr::BitmapIndexScan { .. }
        | RelExpr::BitmapAnd { .. }
        | RelExpr::BitmapOr { .. }
        | RelExpr::BitmapHeapScan { .. } => PropertySet::new(),
        // Parallel operators don't provide ordering guarantees
        RelExpr::ParallelScan { .. }
        | RelExpr::ParallelHashJoin { .. }
        | RelExpr::ParallelAggregate { .. }
        | RelExpr::Gather { .. }
        | RelExpr::MvScan { .. } => PropertySet::new(),
    }
}

/// Derive properties for a Project node.
///
/// Ordering is preserved only for columns that survive the
/// projection without being recomputed.
fn derive_project_properties(
    columns: &[crate::algebra::ProjectionColumn],
    input_props: &[&PropertySet],
) -> PropertySet {
    let mut ps = PropertySet::new();
    let Some(inp) = input_props.first() else {
        return ps;
    };

    // Collect the set of columns that pass through unchanged.
    let mut surviving_cols: Vec<ColumnRef> = Vec::new();
    for pc in columns {
        if let Expr::Column(col) = &pc.expr {
            surviving_cols.push(col.clone());
        }
    }

    // Preserve the prefix of the input ordering whose columns
    // survive the projection.
    if let Some(ordering) = inp.ordering() {
        let prefix: Vec<OrderingColumn> = ordering
            .columns
            .iter()
            .take_while(|oc| surviving_cols.contains(&oc.column))
            .cloned()
            .collect();
        if !prefix.is_empty() {
            ps.add(PhysicalProperty::Ordering(Ordering::new(
                prefix,
            )));
        }
    }

    // Partitioning is preserved if all partition keys survive.
    if let Some(part) = inp.partitioning() {
        let keys_survive = match part {
            Partitioning::Hash(keys)
            | Partitioning::Range(keys) => keys
                .iter()
                .all(|k| surviving_cols.contains(k)),
            Partitioning::Single
            | Partitioning::Broadcast
            | Partitioning::RoundRobin => true,
        };
        if keys_survive {
            ps.add(PhysicalProperty::Partitioning(part.clone()));
        }
    }

    ps
}

/// Derive properties for a Sort node.
fn derive_sort_properties(
    keys: &[SortKey],
    input_props: &[&PropertySet],
) -> PropertySet {
    let mut ps = PropertySet::new();

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

    if !ordering_cols.is_empty() {
        ps.add(PhysicalProperty::Ordering(Ordering::new(
            ordering_cols,
        )));
    }

    // Partitioning is preserved through sort.
    if let Some(inp) = input_props.first() {
        if let Some(part) = inp.partitioning() {
            ps.add(PhysicalProperty::Partitioning(part.clone()));
        }
    }

    ps
}

/// Derive properties for an Aggregate node.
///
/// If the GROUP BY keys are simple columns, the output is
/// implicitly grouped (and potentially sorted, depending on
/// the aggregation strategy chosen later).
fn derive_aggregate_properties(
    group_by: &[Expr],
) -> PropertySet {
    let mut ps = PropertySet::new();

    // Hash aggregate: no guaranteed ordering.
    // Sort aggregate: output sorted on group keys.
    // Since we don't know the strategy yet, conservatively
    // report no ordering. The physical planner can upgrade
    // this when it chooses a sort-based aggregate.

    // If all group-by keys are simple columns, record a
    // hash-partitioning on those keys (the output is grouped).
    let group_cols: Vec<ColumnRef> = group_by
        .iter()
        .filter_map(|e| {
            if let Expr::Column(col) = e {
                Some(col.clone())
            } else {
                None
            }
        })
        .collect();

    if group_cols.len() == group_by.len() && !group_cols.is_empty()
    {
        ps.add(PhysicalProperty::Partitioning(Partitioning::Hash(
            group_cols,
        )));
    }

    ps
}

/// Derive properties for a Join node.
///
/// For inner and left-outer joins the left input's ordering can
/// be preserved (merge join or nested-loop preserves outer
/// ordering). For other join types ordering is lost.
fn derive_join_properties(
    join_type: JoinType,
    _left: &RelExpr,
    input_props: &[&PropertySet],
) -> PropertySet {
    let mut ps = PropertySet::new();

    match join_type {
        JoinType::Inner
        | JoinType::LeftOuter
        | JoinType::Semi
        | JoinType::Anti => {
            // Preserve left input's ordering (nested-loop or
            // merge join over outer side).
            if let Some(left_props) = input_props.first() {
                if let Some(ordering) = left_props.ordering() {
                    ps.add(PhysicalProperty::Ordering(
                        ordering.clone(),
                    ));
                }
            }
        }
        JoinType::RightOuter
        | JoinType::FullOuter
        | JoinType::Cross => {}
    }

    ps
}

/// Derive properties for a `RowPattern` (`MATCH_RECOGNIZE`) node.
fn derive_row_pattern_properties(
    order_by: &[SortKey],
) -> PropertySet {
    let mut ps = PropertySet::new();

    let ordering_cols: Vec<OrderingColumn> = order_by
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

    if !ordering_cols.is_empty() {
        ps.add(PhysicalProperty::Ordering(Ordering::new(
            ordering_cols,
        )));
    }

    ps
}

/// Safely convert a non-negative `f64` to `u64`, clamping at zero
/// and saturating at `u64::MAX`.
fn f64_to_u64_saturating(value: f64) -> u64 {
    if value <= 0.0 {
        0
    } else if value >= 1.844_674_407_370_955e19 {
        // u64::MAX rounded to nearest f64 representable value
        u64::MAX
    } else {
        #[allow(clippy::cast_possible_truncation)]
        #[allow(clippy::cast_sign_loss)]
        { value as u64 }
    }
}

/// Compute the cost of enforcing a required ordering that is not
/// provided by the input.
///
/// This models the cost of inserting a Sort operator. The cost
/// is `O(n log n)` in CPU for a full sort, or `O(n log m)` when
/// the input is already sorted on a prefix (incremental sort).
#[must_use]
pub fn enforcer_cost(
    required: &Ordering,
    provided: &PropertySet,
    row_count: f64,
    avg_row_bytes: u64,
) -> Cost {
    if row_count <= 0.0 {
        return Cost::ZERO;
    }

    let provided_ordering = provided.ordering();

    let prefix_len = provided_ordering.map_or(0, |po| {
        required.common_prefix(po).columns.len()
    });

    if prefix_len >= required.columns.len() {
        return Cost::ZERO;
    }

    if prefix_len > 0 {
        // Incremental sort: sort within each prefix group.
        let assumed_group_size = (row_count / 100.0).max(2.0);
        let log_group = assumed_group_size.log2().max(1.0);
        let cpu = row_count * log_group * 0.5;
        let mem = f64_to_u64_saturating(assumed_group_size)
            .saturating_mul(avg_row_bytes);
        Cost::new(cpu, 0.0, 0.0, mem)
    } else {
        // Full sort.
        let log_n = row_count.log2().max(1.0);
        let cpu = row_count * log_n;
        let mem = f64_to_u64_saturating(row_count)
            .saturating_mul(avg_row_bytes);
        Cost::new(cpu, 0.0, 0.0, mem)
    }
}

/// Compute the cost of enforcing a required partitioning.
///
/// Models the network cost of a shuffle (hash repartition) or
/// broadcast.
#[must_use]
pub fn repartition_cost(
    required: &Partitioning,
    provided: &PropertySet,
    row_count: f64,
    avg_row_bytes: u64,
) -> Cost {
    if let Some(current) = provided.partitioning() {
        if current == required {
            return Cost::ZERO;
        }
    }

    let total_bytes = f64_to_u64_saturating(row_count)
        .saturating_mul(avg_row_bytes);

    match required {
        Partitioning::Broadcast => {
            #[allow(clippy::cast_precision_loss)]
            let network = total_bytes as f64 * 10.0;
            Cost::new(0.0, 0.0, network, 0)
        }
        Partitioning::Hash(_) | Partitioning::Range(_) => {
            #[allow(clippy::cast_precision_loss)]
            let network = total_bytes as f64;
            Cost::new(0.0, 0.0, network, 0)
        }
        Partitioning::Single | Partitioning::RoundRobin => {
            Cost::ZERO
        }
    }
}

/// Check whether a `RelExpr::Sort` is redundant given input
/// properties.
///
/// A sort is redundant when the input already provides an
/// ordering that satisfies the sort keys.
#[must_use]
pub fn is_sort_redundant(
    sort_keys: &[SortKey],
    input_props: &PropertySet,
) -> bool {
    let required_cols: Vec<OrderingColumn> = sort_keys
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

    if required_cols.len() != sort_keys.len() {
        return false;
    }

    let required = Ordering::new(required_cols);
    input_props.satisfies_ordering(&required)
}

/// Determine whether a merge join is applicable given the
/// properties of both inputs and the join keys.
///
/// Merge join requires both inputs to be sorted on the join key
/// columns in the same direction.
#[must_use]
pub fn can_merge_join(
    left_props: &PropertySet,
    right_props: &PropertySet,
    left_keys: &[ColumnRef],
    right_keys: &[ColumnRef],
) -> bool {
    let Some(left_ordering) = left_props.ordering() else {
        return false;
    };
    let Some(right_ordering) = right_props.ordering() else {
        return false;
    };

    if left_keys.len() != right_keys.len() {
        return false;
    }
    if left_keys.is_empty() {
        return false;
    }

    for (i, (lk, rk)) in
        left_keys.iter().zip(right_keys).enumerate()
    {
        let Some(lo) = left_ordering.columns.get(i) else {
            return false;
        };
        let Some(ro) = right_ordering.columns.get(i) else {
            return false;
        };

        if lo.column != *lk || ro.column != *rk {
            return false;
        }
        if lo.direction != ro.direction {
            return false;
        }
    }

    true
}

/// Build required input properties for a merge join on the given
/// key columns.
///
/// Returns `(left_required, right_required)`.
#[must_use]
pub fn merge_join_required_properties(
    left_keys: &[ColumnRef],
    right_keys: &[ColumnRef],
) -> (PropertySet, PropertySet) {
    let direction = SortDirection::Asc;
    let nulls = NullOrdering::Last;

    let left_ordering = Ordering::new(
        left_keys
            .iter()
            .map(|k| OrderingColumn {
                column: k.clone(),
                direction,
                nulls,
            })
            .collect(),
    );

    let right_ordering = Ordering::new(
        right_keys
            .iter()
            .map(|k| OrderingColumn {
                column: k.clone(),
                direction,
                nulls,
            })
            .collect(),
    );

    (
        PropertySet::with_ordering(left_ordering),
        PropertySet::with_ordering(right_ordering),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algebra::{
        NullOrdering, ProjectionColumn, RelExpr, SortDirection,
        SortKey,
    };
    use crate::expr::{ColumnRef, Const, Expr};
    use crate::properties::{
        Ordering, OrderingColumn, Partitioning, PhysicalProperty,
        PropertySet,
    };

    fn col(name: &str) -> ColumnRef {
        ColumnRef::new(name)
    }

    fn ord_col(
        name: &str,
        dir: SortDirection,
    ) -> OrderingColumn {
        OrderingColumn::new(ColumnRef::new(name), dir)
    }

    fn sort_key(name: &str, dir: SortDirection) -> SortKey {
        SortKey {
            expr: Expr::Column(col(name)),
            direction: dir,
            nulls: NullOrdering::Last,
        }
    }

    fn make_input_props_ordered(
        cols: &[(&str, SortDirection)],
    ) -> PropertySet {
        let ordering_cols: Vec<OrderingColumn> = cols
            .iter()
            .map(|(n, d)| ord_col(n, *d))
            .collect();
        PropertySet::with_ordering(Ordering::new(ordering_cols))
    }

    // --- derive_properties ---

    #[test]
    fn scan_has_no_properties() {
        let expr = RelExpr::scan("users");
        let props = derive_properties(&expr, &[]);
        assert!(props.is_empty());
    }

    #[test]
    fn filter_preserves_input_properties() {
        let expr = RelExpr::Filter {
            predicate: Expr::Const(Const::Bool(true)),
            input: Box::new(RelExpr::scan("t")),
        };
        let input = make_input_props_ordered(&[(
            "id",
            SortDirection::Asc,
        )]);
        let props = derive_properties(&expr, &[&input]);
        assert!(props.ordering().is_some());
    }

    #[test]
    fn sort_produces_ordering() {
        let expr = RelExpr::Sort {
            keys: vec![sort_key("name", SortDirection::Desc)],
            input: Box::new(RelExpr::scan("t")),
        };
        let empty = PropertySet::new();
        let props = derive_properties(&expr, &[&empty]);
        let ordering =
            props.ordering().expect("should have ordering");
        assert_eq!(ordering.columns.len(), 1);
        assert_eq!(ordering.columns[0].column.column, "name");
        assert_eq!(
            ordering.columns[0].direction,
            SortDirection::Desc
        );
    }

    #[test]
    fn limit_preserves_ordering() {
        let expr = RelExpr::Limit {
            count: 10,
            offset: 0,
            input: Box::new(RelExpr::scan("t")),
        };
        let input = make_input_props_ordered(&[(
            "id",
            SortDirection::Asc,
        )]);
        let props = derive_properties(&expr, &[&input]);
        assert!(props.ordering().is_some());
    }

    #[test]
    fn distinct_drops_ordering() {
        let expr =
            RelExpr::Distinct {
                input: Box::new(RelExpr::scan("t")),
            };
        let input = make_input_props_ordered(&[(
            "id",
            SortDirection::Asc,
        )]);
        let props = derive_properties(&expr, &[&input]);
        assert!(props.ordering().is_none());
    }

    #[test]
    fn project_preserves_surviving_ordering() {
        let expr = RelExpr::Project {
            columns: vec![
                ProjectionColumn {
                    expr: Expr::Column(col("id")),
                    alias: None,
                },
                ProjectionColumn {
                    expr: Expr::Column(col("name")),
                    alias: None,
                },
            ],
            input: Box::new(RelExpr::scan("t")),
        };
        let input = make_input_props_ordered(&[
            ("id", SortDirection::Asc),
            ("age", SortDirection::Desc),
        ]);
        let props = derive_properties(&expr, &[&input]);
        let ordering =
            props.ordering().expect("should have ordering");
        assert_eq!(ordering.columns.len(), 1);
        assert_eq!(ordering.columns[0].column.column, "id");
    }

    #[test]
    fn project_drops_ordering_when_column_removed() {
        let expr = RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: Expr::Column(col("name")),
                alias: None,
            }],
            input: Box::new(RelExpr::scan("t")),
        };
        let input = make_input_props_ordered(&[(
            "id",
            SortDirection::Asc,
        )]);
        let props = derive_properties(&expr, &[&input]);
        assert!(props.ordering().is_none());
    }

    #[test]
    fn join_inner_preserves_left_ordering() {
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let left = make_input_props_ordered(&[(
            "id",
            SortDirection::Asc,
        )]);
        let right = PropertySet::new();
        let props = derive_properties(&expr, &[&left, &right]);
        assert!(props.ordering().is_some());
    }

    #[test]
    fn join_full_outer_drops_ordering() {
        let expr = RelExpr::Join {
            join_type: JoinType::FullOuter,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let left = make_input_props_ordered(&[(
            "id",
            SortDirection::Asc,
        )]);
        let right = PropertySet::new();
        let props = derive_properties(&expr, &[&left, &right]);
        assert!(props.ordering().is_none());
    }

    #[test]
    fn union_destroys_ordering() {
        let expr = RelExpr::Union {
            all: true,
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        let left = make_input_props_ordered(&[(
            "id",
            SortDirection::Asc,
        )]);
        let right = PropertySet::new();
        let props = derive_properties(&expr, &[&left, &right]);
        assert!(props.ordering().is_none());
    }

    #[test]
    fn aggregate_produces_partitioning() {
        let expr = RelExpr::Aggregate {
            group_by: vec![
                Expr::Column(col("region")),
                Expr::Column(col("year")),
            ],
            aggregates: vec![],
            input: Box::new(RelExpr::scan("sales")),
        };
        let empty = PropertySet::new();
        let props = derive_properties(&expr, &[&empty]);
        assert!(props.partitioning().is_some());
        if let Some(Partitioning::Hash(keys)) =
            props.partitioning()
        {
            assert_eq!(keys.len(), 2);
        } else {
            panic!("expected Hash partitioning");
        }
    }

    #[test]
    fn window_preserves_ordering() {
        let expr = RelExpr::Window {
            functions: vec![],
            input: Box::new(RelExpr::scan("t")),
        };
        let input = make_input_props_ordered(&[(
            "ts",
            SortDirection::Asc,
        )]);
        let props = derive_properties(&expr, &[&input]);
        assert!(props.ordering().is_some());
    }

    // --- enforcer_cost ---

    #[test]
    fn enforcer_cost_zero_when_satisfied() {
        let required = Ordering::new(vec![ord_col(
            "id",
            SortDirection::Asc,
        )]);
        let provided = make_input_props_ordered(&[
            ("id", SortDirection::Asc),
            ("name", SortDirection::Desc),
        ]);
        let cost =
            enforcer_cost(&required, &provided, 1000.0, 64);
        assert!(cost.cpu.abs() < f64::EPSILON);
    }

    #[test]
    fn enforcer_cost_full_sort() {
        let required = Ordering::new(vec![ord_col(
            "name",
            SortDirection::Asc,
        )]);
        let provided = PropertySet::new();
        let cost =
            enforcer_cost(&required, &provided, 1000.0, 64);
        assert!(cost.cpu > 0.0);
        assert!(cost.memory > 0);
    }

    #[test]
    fn enforcer_cost_incremental_sort() {
        let required = Ordering::new(vec![
            ord_col("a", SortDirection::Asc),
            ord_col("b", SortDirection::Asc),
        ]);
        let provided = make_input_props_ordered(&[(
            "a",
            SortDirection::Asc,
        )]);
        let cost_incr =
            enforcer_cost(&required, &provided, 10000.0, 64);

        let cost_full = enforcer_cost(
            &required,
            &PropertySet::new(),
            10000.0,
            64,
        );

        assert!(cost_incr.cpu < cost_full.cpu);
    }

    #[test]
    fn enforcer_cost_zero_rows() {
        let required = Ordering::new(vec![ord_col(
            "id",
            SortDirection::Asc,
        )]);
        let provided = PropertySet::new();
        let cost = enforcer_cost(&required, &provided, 0.0, 64);
        assert!(cost.cpu.abs() < f64::EPSILON);
    }

    // --- repartition_cost ---

    #[test]
    fn repartition_cost_zero_when_satisfied() {
        let required =
            Partitioning::Hash(vec![ColumnRef::new("id")]);
        let provided = PropertySet::with_partitioning(
            Partitioning::Hash(vec![ColumnRef::new("id")]),
        );
        let cost =
            repartition_cost(&required, &provided, 1000.0, 64);
        assert!(cost.network.abs() < f64::EPSILON);
    }

    #[test]
    fn repartition_cost_shuffle() {
        let required =
            Partitioning::Hash(vec![ColumnRef::new("id")]);
        let provided = PropertySet::new();
        let cost =
            repartition_cost(&required, &provided, 1000.0, 64);
        assert!(cost.network > 0.0);
    }

    #[test]
    fn repartition_cost_broadcast() {
        let required = Partitioning::Broadcast;
        let provided = PropertySet::new();
        let cost =
            repartition_cost(&required, &provided, 1000.0, 64);
        assert!(cost.network > 0.0);
    }

    // --- is_sort_redundant ---

    #[test]
    fn sort_redundant_when_already_sorted() {
        let keys = vec![sort_key("id", SortDirection::Asc)];
        let input = make_input_props_ordered(&[
            ("id", SortDirection::Asc),
            ("name", SortDirection::Desc),
        ]);
        assert!(is_sort_redundant(&keys, &input));
    }

    #[test]
    fn sort_not_redundant_different_direction() {
        let keys = vec![sort_key("id", SortDirection::Desc)];
        let input = make_input_props_ordered(&[(
            "id",
            SortDirection::Asc,
        )]);
        assert!(!is_sort_redundant(&keys, &input));
    }

    #[test]
    fn sort_not_redundant_no_ordering() {
        let keys = vec![sort_key("id", SortDirection::Asc)];
        let input = PropertySet::new();
        assert!(!is_sort_redundant(&keys, &input));
    }

    #[test]
    fn sort_not_redundant_different_column() {
        let keys = vec![sort_key("name", SortDirection::Asc)];
        let input = make_input_props_ordered(&[(
            "id",
            SortDirection::Asc,
        )]);
        assert!(!is_sort_redundant(&keys, &input));
    }

    // --- can_merge_join ---

    #[test]
    fn merge_join_possible_when_both_sorted() {
        let left = make_input_props_ordered(&[(
            "id",
            SortDirection::Asc,
        )]);
        let right = make_input_props_ordered(&[(
            "fk",
            SortDirection::Asc,
        )]);
        assert!(can_merge_join(
            &left,
            &right,
            &[col("id")],
            &[col("fk")]
        ));
    }

    #[test]
    fn merge_join_not_possible_different_directions() {
        let left = make_input_props_ordered(&[(
            "id",
            SortDirection::Asc,
        )]);
        let right = make_input_props_ordered(&[(
            "fk",
            SortDirection::Desc,
        )]);
        assert!(!can_merge_join(
            &left,
            &right,
            &[col("id")],
            &[col("fk")]
        ));
    }

    #[test]
    fn merge_join_not_possible_no_right_ordering() {
        let left = make_input_props_ordered(&[(
            "id",
            SortDirection::Asc,
        )]);
        let right = PropertySet::new();
        assert!(!can_merge_join(
            &left,
            &right,
            &[col("id")],
            &[col("fk")]
        ));
    }

    #[test]
    fn merge_join_not_possible_empty_keys() {
        let left = make_input_props_ordered(&[(
            "id",
            SortDirection::Asc,
        )]);
        let right = make_input_props_ordered(&[(
            "fk",
            SortDirection::Asc,
        )]);
        assert!(!can_merge_join(&left, &right, &[], &[]));
    }

    #[test]
    fn merge_join_multi_key() {
        let left = make_input_props_ordered(&[
            ("a", SortDirection::Asc),
            ("b", SortDirection::Asc),
        ]);
        let right = make_input_props_ordered(&[
            ("x", SortDirection::Asc),
            ("y", SortDirection::Asc),
        ]);
        assert!(can_merge_join(
            &left,
            &right,
            &[col("a"), col("b")],
            &[col("x"), col("y")]
        ));
    }

    // --- merge_join_required_properties ---

    #[test]
    fn merge_join_required_props() {
        let (left_req, right_req) =
            merge_join_required_properties(
                &[col("id")],
                &[col("fk")],
            );
        let left_ord =
            left_req.ordering().expect("should have ordering");
        assert_eq!(left_ord.columns.len(), 1);
        assert_eq!(left_ord.columns[0].column.column, "id");

        let right_ord =
            right_req.ordering().expect("should have ordering");
        assert_eq!(right_ord.columns.len(), 1);
        assert_eq!(right_ord.columns[0].column.column, "fk");
    }

    // --- property propagation edge cases ---

    #[test]
    fn sort_preserves_partitioning() {
        let expr = RelExpr::Sort {
            keys: vec![sort_key("name", SortDirection::Asc)],
            input: Box::new(RelExpr::scan("t")),
        };
        let mut input = PropertySet::new();
        input.add(PhysicalProperty::Partitioning(
            Partitioning::Hash(vec![col("region")]),
        ));
        let props = derive_properties(&expr, &[&input]);
        assert!(props.partitioning().is_some());
    }

    #[test]
    fn project_preserves_partitioning() {
        let expr = RelExpr::Project {
            columns: vec![
                ProjectionColumn {
                    expr: Expr::Column(col("id")),
                    alias: None,
                },
                ProjectionColumn {
                    expr: Expr::Column(col("region")),
                    alias: None,
                },
            ],
            input: Box::new(RelExpr::scan("t")),
        };
        let mut input = PropertySet::new();
        input.add(PhysicalProperty::Partitioning(
            Partitioning::Hash(vec![col("region")]),
        ));
        let props = derive_properties(&expr, &[&input]);
        assert!(props.partitioning().is_some());
    }

    #[test]
    fn project_drops_partitioning_when_key_removed() {
        let expr = RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: Expr::Column(col("id")),
                alias: None,
            }],
            input: Box::new(RelExpr::scan("t")),
        };
        let mut input = PropertySet::new();
        input.add(PhysicalProperty::Partitioning(
            Partitioning::Hash(vec![col("region")]),
        ));
        let props = derive_properties(&expr, &[&input]);
        assert!(props.partitioning().is_none());
    }

    // --- f64_to_u64_saturating ---

    #[test]
    fn f64_to_u64_negative_is_zero() {
        assert_eq!(f64_to_u64_saturating(-1.0), 0);
    }

    #[test]
    fn f64_to_u64_overflow_saturates() {
        assert_eq!(f64_to_u64_saturating(f64::MAX), u64::MAX);
    }

    #[test]
    fn f64_to_u64_normal() {
        assert_eq!(f64_to_u64_saturating(42.0), 42);
    }
}
