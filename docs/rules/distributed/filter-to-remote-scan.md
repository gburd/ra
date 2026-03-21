# Rule: Push Filter to Remote Scan Node

**Category:** distributed/filter-pushdown-distributed
**File:** `rules/distributed/filter-pushdown-distributed/filter-to-remote-scan.rra`

## Metadata

- **ID:** `filter-to-remote-scan`
- **Version:** "1.0.0"
- **Databases:** cockroachdb, tidb, presto, trino
- **Tags:** distributed, filter, pushdown, remote-scan, coprocessor
- **Authors:** "RA Contributors"


# Push Filter to Remote Scan Node

## Description

Push a filter predicate down to the storage node hosting the data. The
filter executes at the scan level, reducing both I/O and network transfer
from the storage tier.

## Relational Algebra

```algebra
Filter[p](RemoteScan(R@node_i))
  -> RemoteScan(Filter[p](R)@node_i)
  where p is evaluable on node_i
```

## Test Cases

```sql
-- Test 1: Simple predicate pushed to storage node
SELECT *
FROM orders         -- stored on node 3
WHERE amount > 1000;
-- Expected: Filter at node 3 storage level
```

```sql
-- Test 2: Expression with function
SELECT *
FROM events
WHERE EXTRACT(YEAR FROM ts) = 2024;
-- Expected: Push if storage node supports EXTRACT
```

```sql
-- Test 3: UDF filter cannot push
SELECT *
FROM orders
WHERE my_custom_udf(data) = true;
-- Expected: Cannot push (UDF not available on storage node)
```

## References

TiDB: coprocessor pushdown
CockroachDB: distributed SQL table reader
