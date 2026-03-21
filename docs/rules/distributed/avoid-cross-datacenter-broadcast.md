# Rule: Avoid Cross-Datacenter Broadcast

**Category:** distributed/join-distribution
**File:** `rules/distributed/join-distribution/avoid-cross-datacenter-broadcast.rra`

## Metadata

- **ID:** `avoid-cross-datacenter-broadcast`
- **Version:** "1.0.0"
- **Databases:** cockroachdb, spanner, citus
- **Tags:** distributed, join, broadcast, datacenter, cost-optimization
- **Authors:** "RA Contributors"


# Avoid Cross-Datacenter Broadcast

## Description

Broadcasting data cross-datacenter multiplies the cost by the number of
datacenters. Prefer to broadcast only to nodes within the same
datacenter and use a pre-aggregated or cached copy in remote DCs.

## Relational Algebra

```algebra
Broadcast(S) to all_nodes
  -> Broadcast(S) to local_dc_nodes
  + AsyncReplicate(S) to remote_dc_nodes
  where cross_dc_broadcast_cost > local_broadcast_cost * dc_count
```

## Implementation

```rust
rw!("avoid-cross-dc-broadcast";
    "(exchange broadcast ?rel ?all_nodes)" =>
    "(exchange broadcast ?rel ?local_dc_nodes)"
    if cross_dc_broadcast_avoidable("?rel", "?all_nodes")
),
```

## Test Cases

```sql
-- Test 1: Small table, broadcast within DC only
SELECT o.*, c.name
FROM orders o          -- US-East DC
JOIN countries c       -- 200 rows, replicate to US-East only
  ON o.cc = c.code;
-- Expected: Broadcast countries within US-East (not to all DCs)
```

```sql
-- Test 2: Already replicated in all DCs
SELECT o.*, c.name
FROM orders o
JOIN countries c ON o.cc = c.code;
-- countries already replicated everywhere -> local join
```

```sql
-- Test 3: Multi-DC query requires cross-DC broadcast
SELECT *
FROM us_orders o       -- US-East
JOIN eu_products p     -- EU-West, 10K products
  ON o.pid = p.id;
-- Expected: Broadcast eu_products to US-East
```

## References

CockroachDB: REGIONAL BY TABLE placement
Citus: reference table replication
