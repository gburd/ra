# Rule: Push Predicate Through Shuffle Exchange

**Category:** distributed/filter-pushdown-distributed
**File:** `rules/distributed/filter-pushdown-distributed/predicate-through-shuffle.rra`

## Metadata

- **ID:** `predicate-through-shuffle`
- **Version:** "1.0.0"
- **Databases:** spark, presto, cockroachdb
- **Tags:** distributed, filter, predicate, shuffle, pushdown
- **Authors:** "RA Contributors"


# Push Predicate Through Shuffle Exchange

## Description

When a predicate filters on the shuffle key, it can be pushed below the
exchange to reduce the data volume before hash partitioning. This is
safe because the predicate does not change the partition assignment.

## Relational Algebra

```algebra
Filter[p](Exchange[hash(k)](R))
  -> Exchange[hash(k)](Filter[p](R))
  where columns(p) subset of columns(R)
```

## Test Cases

```sql
-- Test 1: Filter on shuffle key
SELECT *
FROM (
  SELECT * FROM orders
  DISTRIBUTE BY customer_id
) sub
WHERE customer_id > 1000;
-- Expected: Push filter below shuffle
```

```sql
-- Test 2: Filter on non-shuffle column
SELECT *
FROM (
  SELECT * FROM orders
  DISTRIBUTE BY customer_id
) sub
WHERE amount > 100;
-- Expected: Still push below shuffle (reduces volume)
```

```sql
-- Test 3: Filter references post-shuffle computation
SELECT *
FROM (
  SELECT *, ROW_NUMBER() OVER (PARTITION BY cid) as rn
  FROM orders
  DISTRIBUTE BY cid
) sub
WHERE rn = 1;
-- Expected: Cannot push (rn computed after shuffle)
```

## References

Spark: PushPredicateThroughJoin
Presto: PushPredicateIntoTableScan
