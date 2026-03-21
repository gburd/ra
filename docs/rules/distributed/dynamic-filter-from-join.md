# Rule: Dynamic Filter Generation from Join

**Category:** distributed/filter-pushdown-distributed
**File:** `rules/distributed/filter-pushdown-distributed/dynamic-filter-from-join.rra`

## Metadata

- **ID:** `dynamic-filter-from-join`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, databricks
- **Tags:** distributed, filter, dynamic, join, runtime-pruning
- **Authors:** "RA Contributors"


# Dynamic Filter Generation from Join

## Description

Generate a runtime filter from the build side of a hash join and push it
to the probe side's scan. The filter is constructed after the build side
is processed and applied before the probe side is shuffled.

## Relational Algebra

```algebra
HashJoin[c](R, S)
  -> HashJoin[c](
      DynamicFilter(R, collected_keys(S)),
      S
     )
  where |S| << |R|
```

## Test Cases

```sql
-- Test 1: Dynamic filter from small build side
SELECT f.*, d.date_name
FROM fact_sales f          -- 10B rows, probe side
JOIN dim_dates d           -- 365 rows, build side
  ON f.date_id = d.id
WHERE d.year = 2024;
-- Expected: Collect 365 date_ids from build side,
-- create IN-list filter, push to fact_sales scan
```

```sql
-- Test 2: Dynamic filter with range
SELECT o.*, c.name
FROM orders o
JOIN customers c ON o.cid = c.id
WHERE c.id BETWEEN 1000 AND 2000;
-- Expected: Dynamic range filter [1000, 2000] on orders.cid
```

```sql
-- Test 3: Build side too large for dynamic filter
SELECT *
FROM orders o
JOIN line_items l ON o.id = l.oid;
-- Both sides large -> dynamic filter has minimal selectivity
-- Expected: Skip dynamic filter (overhead > benefit)
```

## References

Presto: DynamicFilterSourceOperator
Trino: Dynamic filtering documentation
Spark: DynamicPruningSubquery
