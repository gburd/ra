# Rule: Prefer Broadcast for Already-Replicated Tables

**Category:** distributed/join-distribution
**File:** `rules/distributed/join-distribution/prefer-broadcast-replicated-table.rra`

## Metadata

- **ID:** `prefer-broadcast-replicated-table`
- **Version:** "1.0.0"
- **Databases:** cockroachdb, citus, greenplum, tidb
- **Tags:** distributed, join, broadcast, replicated, reference-table
- **Authors:** "RA Contributors"


# Prefer Broadcast for Already-Replicated Tables

## Description

When one side of a join is already replicated on all nodes (e.g., a
reference table or materialized view), there is no data movement needed.
The join can execute locally on each node.

## Relational Algebra

```algebra
Join[c](R, S)
  -> LocalJoin[c](R, S)
  where S.distribution == Replicated
```

## Implementation

```rust
rw!("prefer-broadcast-replicated";
    "(join ?type ?cond ?left ?right)" =>
    "(local_join ?type ?cond ?left ?right)"
    if is_replicated("?right")
),
```

## Test Cases

```sql
-- Test 1: Reference table already replicated
SELECT o.*, cc.name
FROM orders o
JOIN currency_codes cc ON o.currency = cc.code;
-- currency_codes is replicated -> local join, zero cost
```

```sql
-- Test 2: Materialized view replicated
SELECT t.*, mv.summary
FROM transactions t
JOIN mv_daily_summary mv ON t.date = mv.date;
-- mv_daily_summary is replicated -> local join
```

```sql
-- Test 3: Non-replicated table needs actual broadcast
SELECT o.*, p.name
FROM orders o
JOIN products p ON o.product_id = p.id;
-- products is NOT replicated -> must choose broadcast or shuffle
```

## References

CockroachDB: zone configurations for replicated tables
Citus: reference tables documentation
