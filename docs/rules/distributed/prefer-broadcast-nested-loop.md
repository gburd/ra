# Rule: Prefer Broadcast for Non-Equi Join (Nested Loop)

**Category:** distributed/join-distribution
**File:** `rules/distributed/join-distribution/prefer-broadcast-nested-loop.rra`

## Metadata

- **ID:** `prefer-broadcast-nested-loop`
- **Version:** "1.0.0"
- **Databases:** spark, presto, trino
- **Tags:** distributed, join, broadcast, nested-loop, non-equi
- **Authors:** "RA Contributors"


# Prefer Broadcast for Non-Equi Join (Nested Loop)

## Description

Non-equi joins (range conditions, theta joins) cannot use hash-based
shuffle because there is no partition key to hash on. Broadcast the
smaller side and perform a nested-loop or band join.

## Relational Algebra

```algebra
Join[c](R, S)
  -> NestedLoopJoin[c](R, Broadcast(S))
  where c is not equi-join
  where |S| < |R|
  where |S| < broadcast_threshold
```

## Implementation

```rust
rw!("prefer-broadcast-nested-loop";
    "(join ?type ?cond ?left ?right)" =>
    "(nested_loop_join ?type ?cond ?left (exchange broadcast ?right))"
    if is_non_equi_join("?cond")
    if is_small("?right", BROADCAST_THRESHOLD)
),
```

## Test Cases

```sql
-- Test 1: Range join, broadcast small side
SELECT e.*, r.label
FROM events e              -- 100M rows
JOIN ranges r              -- 1000 rows
  ON e.ts BETWEEN r.start AND r.end;
-- Expected: Broadcast ranges, nested loop join
```

```sql
-- Test 2: Theta join with inequality
SELECT a.*, b.*
FROM sensors a
JOIN alerts b ON a.value > b.threshold;
-- Expected: Broadcast smaller side
```

```sql
-- Test 3: Both sides large, non-equi - expensive but necessary
SELECT *
FROM events e1           -- 50M rows
JOIN events e2           -- 50M rows
  ON e1.ts < e2.ts AND e1.ts + 3600 > e2.ts;
-- Expected: Broadcast if one side can be filtered first
```

## References

Spark: BroadcastNestedLoopJoinExec.scala
Presto: NestedLoopJoinOperator.java
