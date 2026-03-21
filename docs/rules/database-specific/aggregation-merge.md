# Rule: TiDB Aggregation Merge

**Category:** database-specific/tidb
**File:** `rules/database-specific/tidb/aggregation-merge.rra`

## Metadata

- **ID:** `tidb-aggregation-merge`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** aggregation, merge, optimization
- **Authors:** "PingCAP TiDB Team", "RA Contributors"


# TiDB Aggregation Merge

## Description

Merges consecutive aggregation operations when the outer aggregation's
grouping columns are a subset of the inner aggregation's grouping columns.

## Relational Algebra

```algebra
Agg[outer_groups, outer_aggs](Agg[inner_groups, inner_aggs](R))
  -> Agg[outer_groups, merged_aggs](R)
  where outer_groups ⊆ inner_groups
```

## Implementation

```rust
fn merge_aggregations(outer: &Agg, inner: &Agg) -> Option<Agg> {
    if outer.group_cols.is_subset_of(&inner.group_cols) {
        Some(Agg::new(outer.group_cols, merge_funcs(outer, inner)))
    } else {
        None
    }
}
```

## Cost Model

Eliminates intermediate materialization and reduces memory usage.

## Test Cases

```sql
-- Nested aggregation
SELECT region, SUM(total) FROM
  (SELECT region, customer, SUM(amount) as total FROM sales GROUP BY region, customer)
GROUP BY region;
-- Merged: Single aggregation over sales
```

## References
- Source: `pkg/planner/core/rule_aggregation_push_down.go`
