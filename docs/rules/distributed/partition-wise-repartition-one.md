# Rule: Repartition One Side for Partition-Wise Join

**Category:** distributed/join-distribution
**File:** `rules/distributed/join-distribution/partition-wise-repartition-one.rra`

## Metadata

- **ID:** `partition-wise-repartition-one`
- **Version:** "1.0.0"
- **Databases:** cockroachdb, citus, spark, greenplum
- **Tags:** distributed, join, repartition, partition-wise, asymmetric
- **Authors:** "RA Contributors"


# Repartition One Side for Partition-Wise Join

## Description

When one side is already partitioned on the join key but the other is
not, repartition only the non-matching side to align with the existing
partitioning. This avoids moving the larger (already-partitioned) side.

## Relational Algebra

```algebra
Join[c](R, S)
  -> PartitionWiseJoin[c](R, Exchange[hash(k)](S))
  where R.hash_key == join_key_left
  where S.hash_key != join_key_right
```

## Test Cases

```sql
-- Test 1: Left partitioned, right not
SELECT o.*, p.name
FROM orders o        -- hash(product_id), 8 partitions
JOIN products p      -- arbitrary distribution
  ON o.product_id = p.id;
-- Expected: Repartition products by id, then partition-wise join
```

```sql
-- Test 2: Right partitioned, left not
SELECT e.*, d.name
FROM events e        -- arbitrary
JOIN departments d   -- hash(dept_id)
  ON e.dept_id = d.dept_id;
-- Expected: Repartition events by dept_id
```

```sql
-- Test 3: Different partition counts
SELECT o.*, c.name
FROM orders o        -- hash(cid), 8 partitions
JOIN customers c     -- hash(id), 16 partitions
  ON o.cid = c.id;
-- Expected: Repartition one side to match the other
```

## References

Spark: EnsureRequirements.scala
CockroachDB: GenerateStreamingGroupBy
