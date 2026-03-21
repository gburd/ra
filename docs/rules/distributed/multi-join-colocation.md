# Rule: Multi-Way Join Co-location

**Category:** distributed/colocation
**File:** `rules/distributed/colocation/multi-join-colocation.rra`

## Metadata

- **ID:** `multi-join-colocation`
- **Version:** "1.0.0"
- **Databases:** citus, cockroachdb, greenplum
- **Tags:** distributed, colocation, multi-join, star-schema, denormalization
- **Authors:** "RA Contributors"


# Multi-Way Join Co-location

## Description

For multi-way joins (3+ tables), determines the optimal join ordering
and exchange placement to minimize total data movement. If a subset of
tables is co-located, the optimizer groups co-located joins first and
defers expensive shuffles to later stages.

**When to apply**: A query joins three or more tables with a mix of
co-located and non-co-located join conditions.

**Why it works**: By executing co-located joins first (free, no network),
intermediate results are smaller. The subsequent shuffle of the smaller
intermediate result is cheaper than shuffling the original large tables.

## Relational Algebra

```algebra
-- Example: 3-way join with partial co-location
Join[c3](Join[c1](R, S), T)

-- R and S co-located on c1, T is not
-- Optimal order: co-located join first
-> Join[c3](
     Exchange[hash(k3)](Join_local[c1](R, S)),
     Exchange[hash(k3)](T)
   )

-- vs naive: shuffle everything
-> Join[c3](
     Join[c1](Exchange[hash(k1)](R), Exchange[hash(k1)](S)),
     Exchange[hash(k3)](T)
   )
-- The naive version shuffles R and S unnecessarily
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("prefer-colocated-join-order";
    "(join ?type3 ?c3
        (join ?type1 ?c1
            (exchange hash_partition ?r ?rk)
            (exchange hash_partition ?s ?sk))
        ?t)" =>
    "(join ?type3 ?c3
        (exchange hash_partition
            (join ?type1 ?c1 ?r ?s)
            ?c3_keys)
        ?t)"
    if are_colocated("?r", "?rk", "?s", "?sk")
),
```

## Preconditions

```rust
fn applicable(
    tables: &[TableRef],
    join_conditions: &[JoinCond],
) -> bool {
    // At least 3 tables in the join
    tables.len() >= 3
    // At least one pair of tables is co-located
    && tables.iter().any(|a| {
        tables.iter().any(|b| {
            a != b && is_colocated(a, b, join_conditions)
        })
    })
}
```

**Restrictions:**
- Heuristic: co-located joins first. This is not always optimal if
  the co-located join produces a large intermediate result
- Must verify that grouping co-located joins does not violate join
  condition dependencies
- Star-schema patterns (1 fact + N dimensions) typically benefit most

## Cost Model

```rust
fn multi_join_cost(
    join_order: &[JoinStep],
    node_count: u32,
    network_bandwidth: f64,
) -> f64 {
    let mut cost = 0.0;
    for step in join_order {
        if step.requires_shuffle {
            let shuffle_fraction =
                (node_count - 1) as f64 / node_count as f64;
            cost += step.bytes_to_shuffle * shuffle_fraction
                / network_bandwidth;
        }
        cost += step.local_join_cost;
    }
    cost
}
```

**Typical benefit**: For a star-schema with 1 fact table co-located with
2 of 5 dimension tables, co-located-first ordering saves 2 shuffle
operations (40% reduction in shuffles).

## Test Cases

```sql
-- Positive: star schema with co-located dimensions
-- fact_sales co-located with dim_customer (both on customer_id)
-- dim_product and dim_date are reference tables
SELECT SUM(f.amount), d.year, p.category, c.region
FROM fact_sales f
JOIN dim_customer c ON f.customer_id = c.id   -- co-located
JOIN dim_product p ON f.product_id = p.id      -- reference table
JOIN dim_date d ON f.date_key = d.key          -- reference table
GROUP BY d.year, p.category, c.region;

-- Optimal plan: local joins for co-located + reference,
-- only the final aggregation requires a shuffle
-- Plan:
-- FinalAggregate
--   Exchange[hash(year, category, region)]
--     PartialAggregate
--       HashJoin(f.date_key = d.key)       -- reference: local
--         HashJoin(f.product_id = p.id)    -- reference: local
--           HashJoin(f.customer_id = c.id) -- co-located: local
--             Scan(fact_sales)
--             Scan(dim_customer)
--           Scan(dim_product)
--         Scan(dim_date)
```

```sql
-- Negative: no co-location, all shuffles needed
-- All tables on different distribution keys
SELECT * FROM a JOIN b ON a.x = b.y
                JOIN c ON b.z = c.w;
-- Must shuffle for every join; ordering depends on cardinality
```

## References

Citus: src/backend/distributed/planner/multi_join_order.c
CockroachDB: pkg/sql/opt/xform/join_funcs.go
Greenplum: src/backend/optimizer/path/joinpath.c
Ioannidis & Kang, "Left-Deep vs Bushy Trees" (VLDB 1991)
