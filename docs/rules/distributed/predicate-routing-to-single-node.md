# Rule: Route Query to Single Node via Predicate

**Category:** distributed/filter-pushdown-distributed
**File:** `rules/distributed/filter-pushdown-distributed/predicate-routing-to-single-node.rra`

## Metadata

- **ID:** `predicate-routing-to-single-node`
- **Version:** "1.0.0"
- **Databases:** cockroachdb, citus, vitess, tidb
- **Tags:** distributed, filter, routing, single-node, partition-key
- **Authors:** "RA Contributors"


# Route Query to Single Node via Predicate

## Description

When a filter predicate fully specifies the partition key, the query
can be routed to a single node, avoiding distributed execution entirely.
This is the fastest path for point queries in distributed databases.

## Relational Algebra

```algebra
Filter[k = v](Scan(R[hash(k)]))
  -> SingleNodeScan(R, partition_for(v))
  where k is the full partition key
  where predicate is equality on k
```

## Test Cases

```sql
-- Test 1: Point query on partition key
SELECT *
FROM orders
WHERE customer_id = 42;
-- hash(customer_id) routes to exactly one node
-- Expected: Execute on single node, no coordination
```

```sql
-- Test 2: Composite partition key, fully specified
SELECT *
FROM orders
WHERE region = 'US' AND customer_id = 42;
-- hash(region, customer_id) routes to one node
-- Expected: Single-node execution
```

```sql
-- Test 3: Partial partition key, cannot route
SELECT *
FROM orders
WHERE region = 'US';
-- Only part of composite key (region, customer_id) specified
-- Expected: Must scan all partitions for region='US'
```

## References

CockroachDB: routing to leaseholder
Citus: single-shard query routing
Vitess: vindex-based routing
