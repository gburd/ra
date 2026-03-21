# Rule: Index Merge Intersection

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/index-merge-intersection.rra`

## Metadata

- **ID:** `index-merge-intersection`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle
- **Tags:** index, merge, intersection, bitmap-and, multi-predicate
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter (and ?pred1 ?pred2) (scan ?table))"
    description: "Multi-predicate filter for index merge intersection"
  - type: "predicate"
    condition: "has_index(?table, columns(?pred1)) && has_index(?table, columns(?pred2))"
    description: "Multiple indexes available for merge intersection"
```


# Index Merge Intersection

## Description

Combines results from two or more single-column indexes using an
intersection (AND) to satisfy a conjunctive predicate without a composite
index. Each index scan produces a sorted list of row pointers; the
intersection yields only rows matching all predicates.

**When to apply**: The query has AND predicates on multiple columns, each
column has its own index, but no composite index covers the combination.
The combined selectivity is low enough to justify the merge overhead.

**Why it works**: Each index independently filters rows. Intersecting
the sorted row-pointer streams requires only a merge pass (no random I/O
to the heap until the final fetch). This can be cheaper than a full table
scan when individual selectivities are moderate but the product is low.

## Relational Algebra

```algebra
sigma[P1 AND P2](R)
  -> heap_fetch(intersect(
       index_scan[I1](P1),
       index_scan[I2](P2)
     ))
  where has_index(I1, P1_cols) && has_index(I2, P2_cols)
    && selectivity(P1) * selectivity(P2) < THRESHOLD
```

## Implementation

```rust
rw!("index-merge-intersect";
    "(filter (and ?p1 ?p2) (scan ?table))" =>
    "(heap-fetch (intersect
        (index-scan ?idx1 ?p1)
        (index-scan ?idx2 ?p2)))"
    if has_index_for("?table", "?p1") &&
       has_index_for("?table", "?p2") &&
       combined_selectivity("?p1", "?p2") < 0.05
),
```

## Cost Model

```rust
fn cost(
    idx1_pages: u64, idx1_rows: u64,
    idx2_pages: u64, idx2_rows: u64,
    result_rows: u64,
) -> f64 {
    let scan1 = idx1_pages as f64 * IO_COST;
    let scan2 = idx2_pages as f64 * IO_COST;
    let merge_cpu = (idx1_rows + idx2_rows) as f64 * CPU_COMPARE_COST;
    let heap_fetch = result_rows as f64 * RANDOM_IO_COST;
    scan1 + scan2 + merge_cpu + heap_fetch
}

fn benefit_vs_seq_scan(total_pages: u64, merge_cost: f64) -> f64 {
    let seq = total_pages as f64 * IO_COST;
    (seq - merge_cost) / seq
}
```

**Typical benefit**: 30-85% when individual selectivities are 5-20% and
the product is under 5%.

## Test Cases

### Positive: Two selective predicates

```sql
-- Index on age, separate index on city
SELECT * FROM users WHERE age = 25 AND city = 'Boston';

-- Each index filters ~5%; intersection yields ~0.25%
```

### Positive: Three-way intersection

```sql
-- Indexes on status, region, category
SELECT * FROM orders
WHERE status = 'pending' AND region = 'US' AND category = 'electronics';

-- Each ~10%; product ~0.1%
```

### Negative: Low selectivity

```sql
-- Index on gender (2 values), index on active (boolean)
SELECT * FROM users WHERE gender = 'M' AND active = true;

-- 50% * 50% = 25%; seq scan is cheaper than index merge
```

## References

- MySQL: Index merge optimization
- PostgreSQL: BitmapAnd node
- Oracle: INDEX_COMBINE hint
