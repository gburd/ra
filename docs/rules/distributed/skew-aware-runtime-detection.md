# Rule: Runtime Skew Detection and Adaptive Split

**Category:** distributed/join-distribution
**File:** `rules/distributed/join-distribution/skew-aware-runtime-detection.rra`

## Metadata

- **ID:** `skew-aware-runtime-detection`
- **Version:** "1.0.0"
- **Databases:** spark, databricks, trino
- **Tags:** distributed, join, skew, adaptive, runtime, AQE
- **Authors:** "RA Contributors"


# Runtime Skew Detection and Adaptive Split

## Description

Detect partition skew at runtime (after shuffle) by monitoring partition
sizes. When a partition exceeds a threshold relative to the median, split
it into sub-partitions and re-distribute.

## Relational Algebra

```algebra
Exchange[hash(k)](R) producing partitions P1..Pn
  -> if size(Pi) > median(P) * skew_factor:
       Split(Pi) into Pi_1..Pi_m
       Broadcast matching S rows to new sub-partitions
```

## Test Cases

```sql
-- Test 1: Runtime detection of skewed partition
SELECT o.*, c.name
FROM orders o
JOIN customers c ON o.cid = c.id;
-- At runtime: partition for cid=100 is 10x median
-- Expected: Split partition, redistribute
```

```sql
-- Test 2: No skew detected at runtime
SELECT a.*, b.*
FROM balanced_a a
JOIN balanced_b b ON a.key = b.key;
-- All partitions within 2x of median
-- Expected: No adaptive action needed
```

```sql
-- Test 3: Multiple skewed partitions
SELECT s.*, p.*
FROM sales s
JOIN products p ON s.pid = p.id;
-- pid=1 (10x), pid=2 (8x), pid=3 (6x) all skewed
-- Expected: Split all three partitions
```

## References

Spark 3.0: Adaptive Query Execution (AQE)
Databricks: Runtime skew join optimization blog post
