# Rule: Distributed Distinct Count via HyperLogLog

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/distributed-distinct-count-hll.rra`

## Metadata

- **ID:** `distributed-distinct-count-hll`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, bigquery, redshift
- **Tags:** distributed, aggregation, distinct, hyperloglog, approximate
- **Authors:** "RA Contributors"


# Distributed Distinct Count via HyperLogLog

## Description

Replace exact COUNT(DISTINCT x) with an approximate HyperLogLog (HLL)
sketch when approximate results are acceptable. HLL sketches are
composable: local sketches merge in O(1) per sketch, dramatically
reducing shuffle volume.

**When to apply**: Query tolerates approximate results (configurable
error bound, typically ~2% with p=14), and COUNT(DISTINCT) on
high-cardinality columns.

## Relational Algebra

```algebra
-- Before (exact)
gamma[g, COUNT(DISTINCT x)](R)

-- After (approximate HLL)
gamma[g, hll_merge(partial_hll)](
    Exchange[hash(g)](
        gamma[g, hll_add(x) as partial_hll](R)
    )
)
```

## Implementation

```rust
rw!("count-distinct-to-hll";
    "(aggregate ?group (count_distinct ?col) ?child)" =>
    "(aggregate_final ?group (hll_count_distinct)
        (exchange hash_partition
            (aggregate_partial ?group (hll_add ?col) ?child)
            ?group))"
    if allow_approximate("count_distinct")
),
```

## Test Cases

```sql
-- Positive: high-cardinality approximate count
SELECT region, APPROX_COUNT_DISTINCT(user_id)
FROM events GROUP BY region;
-- HLL with p=14 gives ~2% error

-- Negative: exact count required
SELECT region, COUNT(DISTINCT user_id) FROM events GROUP BY region;
-- Must use exact three-phase when approximate not allowed
```

## References

Flajolet et al., "HyperLogLog: the analysis of a near-optimal cardinality estimation algorithm" (2007)
