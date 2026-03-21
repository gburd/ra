# Rule: Prefer Same-Rack Data Movement

**Category:** distributed/join-distribution
**File:** `rules/distributed/join-distribution/locality-aware-same-rack.rra`

## Metadata

- **ID:** `locality-aware-same-rack`
- **Version:** "1.0.0"
- **Databases:** cockroachdb, tidb, yugabyte
- **Tags:** distributed, join, locality, rack, network-topology
- **Authors:** "RA Contributors"


# Prefer Same-Rack Data Movement

## Description

When shuffling data for a join, prefer to send data to nodes within the
same rack. Intra-rack bandwidth is 10-100x higher and latency is 10-100x
lower than cross-rack transfers.

## Relational Algebra

```algebra
Exchange[hash(k)](R) to targets
  -> Exchange[hash(k)](R) to same_rack_targets
  where same_rack_targets covers all partitions
  where intra_rack_cost < cross_rack_cost
```

## Implementation

```rust
rw!("locality-aware-same-rack";
    "(exchange hash_partition ?rel ?keys ?targets)" =>
    "(exchange hash_partition ?rel ?keys ?rack_local_targets)"
    if has_same_rack_alternative("?rel", "?targets")
),
```

## Test Cases

```sql
-- Test 1: Prefer intra-rack shuffle
SELECT o.*, c.name
FROM orders o     -- nodes [0,1] rack A
JOIN customers c  -- nodes [2,3] rack B
  ON o.cid = c.id;
-- Expected: Move customers to rack A (cheaper) rather than
-- shuffling orders to rack B
```

```sql
-- Test 2: Single rack, no locality decision needed
SELECT a.*, b.*
FROM table_a a    -- nodes [0,1,2,3] all rack A
JOIN table_b b    -- nodes [0,1,2,3] all rack A
  ON a.id = b.a_id;
-- Expected: Standard shuffle within rack
```

```sql
-- Test 3: Multi-rack cluster, minimize cross-rack traffic
SELECT *
FROM sales s     -- rack A [0,1], rack B [2,3]
JOIN products p  -- rack A [0,1], rack C [4,5]
  ON s.pid = p.id;
-- Expected: Prefer rack A nodes for join execution
```

## References

CockroachDB: locality-aware routing
HDFS: rack-aware block placement
