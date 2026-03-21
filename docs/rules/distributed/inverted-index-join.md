# Rule: Inverted Index Lookup Join

**Category:** distributed/distributed-joins
**File:** `rules/distributed/distributed-joins/inverted-index-join.rra`

## Metadata

- **ID:** `inverted-index-join`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** distributed, join, inverted-index, spatial, json, full-text
- **Authors:** "RA Contributors"


# Inverted Index Lookup Join

## Description

Generates an InvertedJoin operator that uses an inverted index (GIN,
spatial, or trigram) to accelerate a join. For each row from the left
input, the inverted index is probed to find matching rows. This is
particularly useful for JSON containment, spatial intersection, and
full-text search predicates.

**When to apply**: The right side of a join has an inverted index, and
the join condition matches the inverted index's supported operations
(containment, intersection, similarity).

**Why it works**: Without an inverted index, these joins require a full
scan of the right table for each left row (nested loop). The inverted
index narrows the search space to only matching entries, turning O(n*m)
into O(n*log(m)).

## Relational Algebra

```algebra
Join[R.json_col @> L.search_val](L, Scan(R))
  -> InvertedJoin[R.json_col @> L.search_val](L, R.inv_idx)
  where R has inverted index on json_col
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("generate-inverted-join";
    "(join ?type ?left (scan ?right_private) ?on ?private)" =>
    "(inverted_join ?type ?left ?right_private ?on ?private)"
    if is_canonical_scan("?right_private")
    if has_inverted_indexes("?right_private")
    if on_matches_inverted_index("?on", "?right_private")
),

rw!("generate-inverted-join-with-filter";
    "(join ?type ?left
        (filter ?pred (scan ?right_private))
        ?on ?private)" =>
    "(inverted_join ?type ?left ?right_private
        (concat_filters ?on ?pred) ?private)"
    if is_canonical_scan("?right_private")
    if has_inverted_indexes("?right_private")
),
```

## Preconditions

```rust
fn applicable(
    join_type: JoinType,
    right_scan: &ScanPrivate,
    on: &FiltersExpr,
) -> bool {
    // Right side must have an inverted index
    right_scan.table().has_inverted_indexes()
    // Must be a supported join type
    && matches!(join_type,
        Inner | Left | Semi | Anti)
    // Join condition must use inverted-indexable operations
    && on.has_inverted_indexable_predicate(right_scan)
}
```

**Restrictions:**
- Only inner, left, semi, and anti joins are supported
- The inverted index must cover the predicate type (JSON @>, spatial
  ST_Intersects, trigram %, etc.)
- Non-covering inverted indexes require a subsequent index join to
  fetch remaining columns
- For paired joins (spatial), an additional lookup join fetches
  the primary key and validates the predicate

## Cost Model

```rust
fn inverted_join_cost(
    left_rows: f64,
    avg_inverted_matches: f64,
    inverted_index_lookup_cost: f64,
    primary_lookup_cost: f64,
    false_positive_rate: f64,
) -> f64 {
    let inverted_probes = left_rows * inverted_index_lookup_cost;
    let matched_rows = left_rows * avg_inverted_matches;
    let primary_lookups = matched_rows
        * (1.0 + false_positive_rate) * primary_lookup_cost;
    inverted_probes + primary_lookups
}
```

## Test Cases

```sql
-- Positive: JSON containment join with GIN index
-- documents has GIN index on metadata column
SELECT d.id, d.title
FROM queries q
JOIN documents d ON d.metadata @> q.search_filter;

-- Plan: InvertedJoin(d.metadata @> q.search_filter)
--   Scan(queries)
--   InvertedIndexScan(documents, idx_metadata_gin)
```

```sql
-- Positive: spatial join with inverted index
SELECT p.name, b.building_name
FROM points p
JOIN buildings b ON ST_Intersects(p.geom, b.geom);

-- Plan: InvertedJoin(ST_Intersects)
--   Scan(points)
--   InvertedIndexScan(buildings, idx_geom_gist)
```

```sql
-- Negative: no inverted index on right side
SELECT * FROM t1 JOIN t2 ON t2.data @> t1.filter;
-- Without inverted index, falls back to nested loop
```

## References

CockroachDB: pkg/sql/opt/xform/rules/join.opt:280 - GenerateInvertedJoins (commit 51e808c)
CockroachDB: pkg/sql/opt/invertedidx/ - inverted index expression matching
CockroachDB: pkg/sql/opt/xform/rules/join.opt:294 - GenerateInvertedJoinsFromSelect
