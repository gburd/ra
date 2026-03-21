# Rule: Replace Multiple MIN/MAX with Scalar Subqueries

**Category:** database-specific/cockroachdb
**File:** `rules/database-specific/cockroachdb/scalar-min-max-to-subqueries.rra`

## Metadata

- **ID:** `cockroachdb-scalar-min-max-to-subqueries`
- **Version:** 1.0.0
- **Databases:** cockroachdb
- **Tags:** database-specific, cockroachdb, aggregation, min, max, subquery
- **Authors:** "RA Contributors"


# Replace Multiple MIN/MAX with Scalar Subqueries

## Description

Replaces a scalar GroupBy with multiple MIN/MAX aggregations over a simple scan with N separate scalar subqueries. Each subquery can then use the ReplaceScalarMinMaxWithLimit rule to become an indexed LIMIT 1.

**When to apply**: Scalar GroupBy with 2+ MIN/MAX aggregations over a simple scan.

**Why it works**: Multiple MIN/MAX in one aggregation requires full scan. Splitting into subqueries allows each to use an index with LIMIT 1, converting O(n) full scan to O(n_aggs * log n) index seeks.

**Database version**: CockroachDB v19.2+

## Relational Algebra

```algebra
ScalarGroupBy[min(a), max(b)](Scan(T))
  -> (ScalarSubquery[min(a)], ScalarSubquery[max(b)])
  where each subquery can use ReplaceScalarMinMaxWithLimit
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("cockroachdb-scalar-min-max-to-subqueries";
    "(scalar_group_by
        (scan ?private)
        ?aggs
        ?group_private)" =>
    "(make_min_max_scalar_subqueries ?private ?aggs)"
    if is_database("cockroachdb")
    if is_canonical_scan("?private")
    if two_or_more_min_or_max("?aggs")
    if is_canonical_group_by("?group_private")
),
```

## Preconditions

```rust
fn applicable(
    agg_funcs: &[AggFunc],
) -> bool {
    let min_max_count = agg_funcs.iter()
        .filter(|f| matches!(f, AggFunc::Min(_) | AggFunc::Max(_)))
        .count();

    min_max_count >= 2
}
```

**Restrictions:**
- Only applies to CockroachDB
- Input must be a simple scan
- Must have 2+ MIN/MAX aggregations
- Scalar GroupBy (no grouping columns)
- Most beneficial when columns have indexes

## Cost Model

```rust
fn estimated_benefit(
    table_rows: f64,
    num_aggs: usize,
    indexes_available: bool,
) -> f64 {
    // Single aggregation: full scan
    let single_agg_cost = table_rows * 100.0;

    // Multiple subqueries: N index seeks (if indexed)
    let multi_subquery_cost = if indexes_available {
        num_aggs as f64 * table_rows.log2() * 10.0
    } else {
        num_aggs as f64 * table_rows * 100.0
    };

    ((single_agg_cost - multi_subquery_cost) / single_agg_cost).max(0.0)
}
```

**Typical benefit**: 50-90% when columns are indexed

## Test Cases

```sql
SELECT min(created_at), max(updated_at) FROM orders;

-- Transformed to:
-- (SELECT min(created_at) FROM orders),  -- uses created_at index
-- (SELECT max(updated_at) FROM orders)   -- uses updated_at index
```

## References

**Source code:**
- CockroachDB: `pkg/sql/opt/xform/rules/groupby.opt`
  - Rule: `ReplaceScalarMinMaxWithScalarSubqueries` (lines 5-18)
  - Commit: 6e210ba6aa33cea5e27b1a8fae212c27941781f4
