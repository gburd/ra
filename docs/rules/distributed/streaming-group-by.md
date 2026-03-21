# Rule: Streaming Group By with Interesting Orderings

**Category:** distributed/partial-aggregation
**File:** `rules/distributed/partial-aggregation/streaming-group-by.rra`

## Metadata

- **ID:** `streaming-group-by`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** distributed, aggregation, streaming, ordering, group-by
- **Authors:** "RA Contributors"


# Streaming Group By with Interesting Orderings

## Description

Creates streaming variants of GroupBy, DistinctOn, and related
operators that require specific orderings on the grouping columns.
When the input provides an ordering on the grouping columns (from an
index or sort), the aggregation can execute in streaming fashion,
processing one group at a time without materializing the full input.

**When to apply**: A GroupBy or DistinctOn operator has grouping
columns, and the input has interesting orderings that cover some or
all of those columns.

**Why it works**: Hash-based aggregation requires materializing a
hash table of all groups, using O(groups) memory. Streaming
aggregation processes groups in order, needing only O(1) memory per
group and producing output incrementally. This enables pipeline
execution and early termination with LIMIT.

## Relational Algebra

```algebra
gamma[g, agg](R)
  -> StreamingGroupBy[g, agg](Sort[g](R))
  -- or if R is already ordered by g:
  -> StreamingGroupBy[g, agg](R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("generate-streaming-group-by";
    "(group_by ?input ?aggs ?private)" =>
    "(streaming_group_by ?input ?aggs ?private
        (required_ordering ?private))"
    if is_canonical_group_by("?private")
    if has_interesting_ordering("?input", "?private")
),
```

## Preconditions

```rust
fn applicable(
    input: &RelExpr,
    group_private: &GroupingPrivate,
) -> bool {
    // Must be a canonical group-by (not already streaming)
    group_private.is_canonical()
    // Input must have interesting orderings on grouping columns
    && input.interesting_orderings()
        .iter()
        .any(|ord| ord.covers(&group_private.grouping_cols()))
}
```

**Restrictions:**
- Streaming requires a total ordering on all grouping columns
- If the input only provides a partial ordering, a streaming group-by
  on a prefix is possible (streaming + hash on remainder)
- Adding a sort to enable streaming may not be worthwhile if the
  input has no useful ordering at all
- In distributed settings, each node can stream locally, but a
  final merge step is still needed

## Cost Model

```rust
fn streaming_vs_hash_cost(
    input_rows: f64,
    distinct_groups: f64,
    input_ordered: bool,
    sort_cost_per_row: f64,
    hash_cost_per_row: f64,
) -> f64 {
    let hash_cost = input_rows * hash_cost_per_row
        + distinct_groups * HASH_TABLE_OVERHEAD;

    let streaming_cost = if input_ordered {
        input_rows * STREAMING_COMPARE_COST
    } else {
        input_rows * input_rows.log2() * sort_cost_per_row
            + input_rows * STREAMING_COMPARE_COST
    };

    hash_cost - streaming_cost
}
```

**Typical benefit**: For a GROUP BY on an indexed column with 1M rows
and 10K groups, streaming aggregation avoids building a 10K-entry hash
table and enables incremental output.

## Test Cases

```sql
-- Positive: grouping column has index ordering
-- orders has INDEX(region)
SELECT region, SUM(amount) FROM orders GROUP BY region;

-- Plan: StreamingGroupBy(region, SUM(amount))
--   IndexScan(orders, idx_region)  -- ordered by region
```

```sql
-- Positive: DISTINCT with index ordering
SELECT DISTINCT customer_id FROM orders;

-- If orders has INDEX(customer_id):
-- StreamingDistinctOn(customer_id)
--   IndexScan(orders, idx_customer)
```

```sql
-- Positive: split scan into union for streaming
-- table with CHECK (region IN ('ASIA', 'EUROPE'))
-- INDEX (region, data)
SELECT DISTINCT data FROM tab;

-- SplitGroupByScanIntoUnionScans:
-- StreamingDistinctOn(data)
--   UnionAll
--     IndexScan(tab, idx, region='ASIA')   -- ordered by data
--     IndexScan(tab, idx, region='EUROPE') -- ordered by data
```

```sql
-- Negative: no useful ordering on grouping columns
SELECT category, COUNT(*) FROM products GROUP BY category;
-- No index on category; hash aggregation is cheaper
```

## References

CockroachDB: pkg/sql/opt/xform/rules/groupby.opt:155 - GenerateStreamingGroupBy (commit 51e808c)
CockroachDB: pkg/sql/opt/xform/rules/groupby.opt:193 - SplitGroupByScanIntoUnionScans
CockroachDB: pkg/sql/opt/ordering/ - DeriveInterestingOrderings
