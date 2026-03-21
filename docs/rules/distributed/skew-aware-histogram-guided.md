# Rule: Histogram-Guided Skew-Aware Distribution

**Category:** distributed/join-distribution
**File:** `rules/distributed/join-distribution/skew-aware-histogram-guided.rra`

## Metadata

- **ID:** `skew-aware-histogram-guided`
- **Version:** "1.0.0"
- **Databases:** cockroachdb, oracle, db2
- **Tags:** distributed, join, skew, histogram, statistics
- **Authors:** "RA Contributors"


# Histogram-Guided Skew-Aware Distribution

## Description

Use column histograms from statistics to identify skewed keys before
query execution. Pre-compute the optimal partition assignment that
balances load across nodes.

## Relational Algebra

```algebra
Exchange[hash(k)](R)
  -> Exchange[histogram_partition(k, histogram)](R)
  where histogram shows skew (max_freq / avg_freq > threshold)
```

## Test Cases

```sql
-- Test 1: Histogram shows top-heavy distribution
SELECT o.*, s.name
FROM orders o              -- histogram: status='pending' 40%,
                           --   'complete' 35%, 'cancelled' 25%
JOIN statuses s ON o.status = s.code;
-- Expected: Custom partition: pending split across 2 nodes,
-- complete on 2 nodes, cancelled on 1 node
```

```sql
-- Test 2: Uniform histogram, standard partitioning
SELECT o.*, c.name
FROM orders o              -- histogram: all customer_ids ~equal
JOIN customers c ON o.cid = c.id;
-- Expected: Standard hash partition (no skew adjustment)
```

```sql
-- Test 3: No histogram available, fall back to hash
SELECT a.*, b.*
FROM table_a a             -- no statistics
JOIN table_b b ON a.key = b.key;
-- Expected: Standard hash partition (cannot detect skew)
```

## References

CockroachDB: table statistics and histograms
Oracle: histogram-based partition pruning
IBM DB2: RUNSTATS with distribution statistics
