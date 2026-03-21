# Rule: Prefer Same-Datacenter Join Execution

**Category:** distributed/join-distribution
**File:** `rules/distributed/join-distribution/locality-aware-same-datacenter.rra`

## Metadata

- **ID:** `locality-aware-same-datacenter`
- **Version:** "1.0.0"
- **Databases:** cockroachdb, spanner, tidb, yugabyte
- **Tags:** distributed, join, locality, datacenter, geo-distributed
- **Authors:** "RA Contributors"


# Prefer Same-Datacenter Join Execution

## Description

In multi-datacenter deployments, cross-datacenter network transfers are
1000x more expensive than intra-datacenter. Move the smaller dataset
within the same datacenter rather than shuffling across datacenters.

## Relational Algebra

```algebra
Join[c](R@dc1, S@dc2)
  -> Join[c](R@dc1, Transfer(S, dc1))
  where |S| < |R|
  where cross_dc_cost(S) < cross_dc_cost(R)
```

## Implementation

```rust
rw!("locality-aware-same-datacenter";
    "(join ?type ?cond ?left ?right)" =>
    "(join ?type ?cond ?left (transfer ?right ?left_dc))"
    if different_datacenters("?left", "?right")
    if smaller_transfer_cost("?right", "?left")
),
```

## Test Cases

```sql
-- Test 1: Small table in remote DC, move it locally
SELECT o.*, w.name
FROM orders o           -- 100M rows, US-East DC
JOIN warehouses w       -- 50 rows, EU-West DC
  ON o.warehouse_id = w.id;
-- Expected: Transfer warehouses to US-East (50 rows vs 100M)
```

```sql
-- Test 2: Both tables in same DC, no cross-DC transfer
SELECT o.*, c.name
FROM orders o           -- US-East DC
JOIN customers c        -- US-East DC
  ON o.cid = c.id;
-- Expected: Normal join, no cross-DC penalty
```

```sql
-- Test 3: Large tables in different DCs
SELECT s.*, i.*
FROM sales s            -- 500M rows, US-East
JOIN inventory i        -- 200M rows, US-West
  ON s.pid = i.pid;
-- Expected: Minimize cross-DC bytes transferred
```

## References

Google Spanner: TrueTime and locality-aware execution
CockroachDB: follow-the-workload rebalancing
