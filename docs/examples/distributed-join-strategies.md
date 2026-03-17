# Example: Distributed Join Strategies

This example demonstrates how the distributed optimization rules
choose between broadcast, shuffle, and co-located join strategies.

## Scenario

A star schema query on a 4-node cluster:

```sql
SELECT c.c_name, SUM(o.o_totalprice)
FROM orders o
JOIN customer c ON o.o_custkey = c.c_custkey
WHERE c.c_mktsegment = 'BUILDING'
GROUP BY c.c_name
ORDER BY SUM(o.o_totalprice) DESC
LIMIT 10;
```

**Table statistics:**
- orders: 150M rows, hash-partitioned by o_orderkey across 4 nodes
- customer: 15M rows, hash-partitioned by c_custkey across 4 nodes
- c_mktsegment = 'BUILDING' selectivity: ~20% (3M qualifying rows)

## Strategy 1: Shuffle Join (Baseline)

Both tables repartitioned by join key (custkey):

```
Limit [10]
  └─ MergeSort [SUM(o_totalprice) DESC]
      └─ Exchange [gather]
          └─ TopN [10, SUM DESC]
              └─ HashAggregate [c_name, SUM(o_totalprice)]
                  └─ HashJoin [o_custkey = c_custkey]
                      ├─ Exchange [hash on o_custkey]   -- 150M rows shuffled
                      │   └─ Scan [orders]
                      └─ Exchange [hash on c_custkey]   -- 3M rows shuffled
                          └─ Filter [c_mktsegment = 'BUILDING']
                              └─ Scan [customer]
```

**Network cost:** 150M + 3M = 153M rows shuffled across network.

## Strategy 2: Broadcast Join

The `broadcast-join` rule detects that the filtered customer table
(3M rows) is small relative to orders (150M rows):

```
Limit [10]
  └─ MergeSort [SUM(o_totalprice) DESC]
      └─ Exchange [gather]
          └─ TopN [10, SUM DESC]
              └─ HashAggregate [c_name, SUM(o_totalprice)]
                  └─ HashJoin [o_custkey = c_custkey]
                      ├─ Scan [orders]          -- no shuffle needed
                      └─ Exchange [broadcast]   -- 3M rows * 4 nodes = 12M
                          └─ Filter [c_mktsegment = 'BUILDING']
                              └─ Scan [customer]
```

**Network cost:** 3M * 4 = 12M rows broadcast (12x less than shuffle).

The orders table stays local -- no network transfer for the large side.

## Strategy 3: Broadcast + Filter Pushdown + Distributed TopN

The `push-filter-below-exchange` rule pushes the filter before the
broadcast. The `distributed-topn` rule adds local TopN before gather:

```
Limit [10]
  └─ MergeSort [SUM(o_totalprice) DESC]
      └─ Exchange [gather]                   -- 4 * 10 = 40 rows
          └─ TopN [10, SUM DESC]             -- local top-10 per node
              └─ HashAggregate [c_name, SUM(o_totalprice)]
                  └─ HashJoin [o_custkey = c_custkey]
                      ├─ Scan [orders]
                      └─ Exchange [broadcast]  -- 3M rows * 4 = 12M
                          └─ Filter [c_mktsegment = 'BUILDING']
                              └─ Scan [customer]
```

**Network cost:**
- Broadcast: 12M rows
- Gather: 40 rows (local top-10 from each of 4 nodes)
- **Total: ~12M rows** (vs 153M for shuffle)

## Strategy 4: Semi-Join Reduction

For cases where the broadcast side is too large, the
`semi-join-reduction` rule sends only the join keys first:

```
HashJoin [o_custkey = c_custkey]
  ├─ SemiJoinFilter [o_custkey IN bloom_filter]
  │   └─ Scan [orders]
  └─ Exchange [broadcast]
      └─ Filter [c_mktsegment = 'BUILDING']
          └─ Scan [customer]
```

The semi-join filter uses a Bloom filter of qualifying custkeys
(3M keys, ~4 MB). This pre-filters orders that have no matching
customer, reducing the join input.

## When to Use Each Strategy

| Strategy | Best When | Network Cost |
|----------|-----------|--------------|
| Shuffle | Both sides large, no filtering | O(N + M) |
| Broadcast | One side << other after filtering | O(small * nodes) |
| Co-located | Both partitioned by join key | 0 |
| Semi-join | High selectivity, large broadcast | O(keys + filtered) |
| Lookup | Very small dimension table | O(point lookups) |

## Key Rules Applied

1. `broadcast-join` - Chose broadcast over shuffle (3M << 150M)
2. `push-filter-below-exchange` - Filter before network transfer
3. `distributed-topn` - Local top-N before gather
4. `two-phase-aggregation` - Local aggregate before shuffle
5. `semi-join-reduction` - Bloom filter pre-filtering
