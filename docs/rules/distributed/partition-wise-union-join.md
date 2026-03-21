# Rule: Partition-Wise Join with Union Inputs

**Category:** distributed/join-distribution
**File:** `rules/distributed/join-distribution/partition-wise-union-join.rra`

## Metadata

- **ID:** `partition-wise-union-join`
- **Version:** "1.0.0"
- **Databases:** greenplum, oracle, cockroachdb
- **Tags:** distributed, join, partition-wise, union, inheritance
- **Authors:** "RA Contributors"


# Partition-Wise Join with Union Inputs

## Description

When one side of a join is a UNION ALL of partitioned tables (e.g.,
date-partitioned tables), push the join into each union branch to
exploit partition-wise execution within each branch.

## Relational Algebra

```algebra
Join[c](Union(R1, R2, R3), S)
  -> Union(Join[c](R1, S), Join[c](R2, S), Join[c](R3, S))
  where each Ri is partition-aligned with S
```

## Test Cases

```sql
-- Test 1: Date-partitioned union joined with dimension
SELECT u.*, d.name
FROM (
  SELECT * FROM orders_2023
  UNION ALL
  SELECT * FROM orders_2024
) u
JOIN customers d ON u.cid = d.id;
-- Expected: Push join into each partition
```

```sql
-- Test 2: Union branches not aligned
SELECT u.*, d.name
FROM (
  SELECT * FROM sales_us
  UNION ALL
  SELECT * FROM sales_eu
) u
JOIN products d ON u.pid = d.id;
-- Expected: Push join if products is broadcast/replicated
```

```sql
-- Test 3: Many union branches
SELECT u.*, d.name
FROM (
  SELECT * FROM log_jan UNION ALL
  SELECT * FROM log_feb UNION ALL
  SELECT * FROM log_mar
) u
JOIN config d ON u.cfg_id = d.id;
-- Expected: Broadcast config, join in each branch
```

## References

Oracle: partition-wise join with partitioned tables
PostgreSQL: partition pruning with inheritance
