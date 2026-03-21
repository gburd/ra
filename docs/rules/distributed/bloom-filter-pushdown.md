# Rule: Bloom Filter Pushdown for Semi-Join Reduction

**Category:** distributed/filter-pushdown-distributed
**File:** `rules/distributed/filter-pushdown-distributed/bloom-filter-pushdown.rra`

## Metadata

- **ID:** `bloom-filter-pushdown`
- **Version:** "1.0.0"
- **Databases:** spark, presto, trino, oracle
- **Tags:** distributed, filter, bloom-filter, semi-join, reduction
- **Authors:** "RA Contributors"


# Bloom Filter Pushdown for Semi-Join Reduction

## Description

Build a Bloom filter from the build side of a join and push it to the
probe side before the exchange. This filters out non-matching rows early,
reducing network transfer.

## Relational Algebra

```algebra
Join[c](R, S)
  -> Join[c](BloomFilter(R, S.keys), S)
  where R is probe side
  where bloom_filter_selectivity(S) > min_reduction
```

## Test Cases

```sql
-- Test 1: Bloom filter from small dimension table
SELECT f.*, d.name
FROM fact_table f          -- 1B rows
JOIN dim_dates d           -- 365 rows
  ON f.date_id = d.id
WHERE d.year = 2024;
-- Expected: Build bloom filter from 365 date IDs,
-- push to fact_table scan to skip non-2024 rows
```

```sql
-- Test 2: Bloom filter from filtered join side
SELECT o.*, c.name
FROM orders o
JOIN customers c ON o.cid = c.id
WHERE c.tier = 'premium';
-- Expected: Bloom filter from premium customer IDs
-- pushed to orders scan
```

```sql
-- Test 3: Low selectivity, bloom filter not useful
SELECT *
FROM table_a a
JOIN table_b b ON a.id = b.a_id;
-- Both tables similar size, bloom filter has ~0% reduction
-- Expected: Skip bloom filter (overhead > benefit)
```

## References

Oracle: Bloom filter in parallel query
Spark: RuntimeFilter / DynamicPruning
Presto: DynamicFilterService
