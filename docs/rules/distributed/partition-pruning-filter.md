# Rule: Partition Pruning via Filter Predicate

**Category:** distributed/filter-pushdown-distributed
**File:** `rules/distributed/filter-pushdown-distributed/partition-pruning-filter.rra`

## Metadata

- **ID:** `partition-pruning-filter`
- **Version:** "1.0.0"
- **Databases:** cockroachdb, citus, spark, hive, presto
- **Tags:** distributed, filter, partition-pruning, elimination, scan
- **Authors:** "RA Contributors"


# Partition Pruning via Filter Predicate

## Description

When a filter predicate constrains the partition key, eliminate partitions
that cannot contain matching rows. This avoids scanning and transferring
data from irrelevant nodes entirely.

## Relational Algebra

```algebra
Filter[p](Scan(R[hash(k), partitions P1..Pn]))
  -> Filter[p](Scan(R[partitions matching p on k]))
  where p constrains k
```

## Test Cases

```sql
-- Test 1: Equality on partition key
SELECT *
FROM orders                  -- hash(region), 4 partitions
WHERE region = 'US-East';
-- Expected: Scan only partition for hash('US-East')
```

```sql
-- Test 2: Range on range-partitioned table
SELECT *
FROM events                  -- range(date)
WHERE date BETWEEN '2024-01-01' AND '2024-03-31';
-- Expected: Scan only Q1 partitions
```

```sql
-- Test 3: Filter on non-partition key
SELECT *
FROM orders                  -- hash(region)
WHERE status = 'pending';
-- Expected: Cannot prune, scan all partitions
```

## References

CockroachDB: partition pruning in opt/xform
Spark: PartitionPruning.scala
