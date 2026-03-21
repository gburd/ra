# Rule: Distributed TopN Push to Index Scan

**Category:** distributed/distributed-sort
**File:** `rules/distributed/distributed-sort/distributed-topn-push-to-scan.rra`

## Metadata

- **ID:** `distributed-topn-push-to-scan`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** distributed, topn, limit, index, scan, pushdown
- **Authors:** "RA Contributors"


# Distributed TopN Push to Index Scan

## Description

Pushes a LIMIT into a group-by's input scan by generating index scans
with limit hints. When a GroupBy has a LIMIT above it, and a secondary
index provides partial ordering on the grouping columns, the scan can
be limited to read only enough rows to satisfy the LIMIT, using an
index join for non-covered columns.

**When to apply**: A Limit sits above a GroupBy whose input is a Scan,
and there exists a secondary index that provides ordering on a prefix
of the grouping columns.

**Why it works**: Without this optimization, the GroupBy must read the
entire table before producing output. With a partially-ordered index
scan and a limit hint, the scan reads rows in grouping-column order
and stops early once enough groups have been produced for the LIMIT.

## Relational Algebra

```algebra
Limit[k](GroupBy[g, agg](Scan(T)))
  -> Limit[k](
       GroupBy[g, agg](
         IndexJoin(T,
           IndexScan(T, idx_g, limit_hint=k))))
  where idx_g provides ordering on prefix of g
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("limited-group-by-index-scan";
    "(limit ?k
        (group_by (scan ?table) ?aggs ?private)
        ?ordering)" =>
    "(limit ?k
        (group_by
            (index_join ?table
                (scan ?table ?index (limit_hint ?k)))
            ?aggs ?private)
        ?ordering)"
    if is_canonical_scan("?table")
    if is_canonical_group_by("?private")
    if is_positive_int("?k")
    if has_partially_ordered_index("?table", "?private")
),
```

## Preconditions

```rust
fn applicable(
    scan: &ScanPrivate,
    group_private: &GroupingPrivate,
    limit: i64,
) -> bool {
    scan.is_canonical_scan()
    && group_private.is_canonical()
    && limit > 0
    // An index exists that orders a prefix of grouping columns
    && scan.table().indexes().any(|idx|
        idx.provides_prefix_ordering(&group_private.grouping_cols()))
}
```

**Restrictions:**
- The index must provide ordering on at least a prefix of the
  grouping columns
- If the index is non-covering, an IndexJoin is added (which has
  per-row cost)
- The limit hint is advisory; the scan may produce more rows than
  the hint suggests
- Only works with canonical (unoptimized) scans and group-bys

## Cost Model

```rust
fn limited_group_scan_cost(
    total_rows: f64,
    limit_k: u64,
    avg_group_size: f64,
    index_join_cost: f64,
) -> f64 {
    // Rows needed: approximately k * avg_group_size
    let rows_to_scan = limit_k as f64 * avg_group_size;
    let scan_cost = rows_to_scan.min(total_rows);
    scan_cost * index_join_cost
}
```

## Test Cases

```sql
-- Positive: LIMIT with GROUP BY on indexed prefix
-- t has INDEX(a)
SELECT a, b, COUNT(*) FROM t GROUP BY a, b LIMIT 10;

-- Plan:
-- Limit(10)
--   StreamingGroupBy(a, b)
--     IndexJoin(t)
--       IndexScan(t@idx_a, ordering=+a, limit_hint=10)
-- Scans only enough of idx_a to find 10 distinct (a,b) groups
```

```sql
-- Negative: no useful index for grouping columns
SELECT category, COUNT(*) FROM products GROUP BY category LIMIT 5;
-- No index on category; full scan required
```

## References

CockroachDB: pkg/sql/opt/xform/rules/groupby.opt:401 - GenerateLimitedGroupByScans (commit 51e808c)
CockroachDB: pkg/sql/opt/xform/groupby_funcs.go - index scan with limit hint
