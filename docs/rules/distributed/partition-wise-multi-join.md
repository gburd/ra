# Rule: Partition-Wise Multi-Way Join

**Category:** distributed/join-distribution
**File:** `rules/distributed/join-distribution/partition-wise-multi-join.rra`

## Metadata

- **ID:** `partition-wise-multi-join`
- **Version:** "1.0.0"
- **Databases:** cockroachdb, citus, greenplum, oracle
- **Tags:** distributed, join, partition-wise, multi-join, star-schema
- **Authors:** "RA Contributors"


# Partition-Wise Multi-Way Join

## Description

In star-schema queries, when the fact table and multiple dimension tables
are all partitioned on the same key, execute the entire multi-join
locally on each node.

## Relational Algebra

```algebra
Join[c3](Join[c2](Join[c1](F, D1), D2), D3)
  -> PartitionWiseMultiJoin(F_i, D1_i, D2_i, D3_i)
  where all tables partitioned on same key
```

## Test Cases

```sql
-- Test 1: Star schema, all partitioned on store_id
SELECT f.*, d.name, p.category, s.region
FROM fact_sales f        -- hash(store_id)
JOIN dim_dates d         -- hash(store_id)
JOIN dim_products p      -- hash(store_id)
JOIN dim_stores s        -- hash(store_id)
  ON f.date_id = d.id
 AND f.product_id = p.id
 AND f.store_id = s.id;
-- Expected: Local multi-join on each node
```

```sql
-- Test 2: Mixed partition keys
SELECT f.*, d.name, p.category
FROM fact_sales f        -- hash(store_id)
JOIN dim_dates d         -- hash(date_id)
JOIN dim_products p      -- hash(product_id)
  ON f.date_id = d.id AND f.product_id = p.id;
-- Expected: NOT partition-wise, keys differ
```

```sql
-- Test 3: Two of three tables co-located
SELECT f.*, d.name, p.category
FROM fact_sales f        -- hash(product_id)
JOIN dim_products p      -- hash(product_id)
JOIN dim_dates d         -- hash(date_id)
  ON f.product_id = p.id AND f.date_id = d.id;
-- Expected: Partition-wise for f+p, then shuffle for d
```

## References

Oracle: full partition-wise join
Greenplum: motion elimination
