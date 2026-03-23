# PostgreSQL Planner Techniques

Research notes on PostgreSQL-specific optimization techniques that may inform RA development.

**Source**: PostgreSQL source code `src/backend/optimizer/`

---

## Path Generation Strategies

### Parameterized Paths

**Location**: `src/backend/optimizer/path/indxpath.c`

**Description**: PostgreSQL creates multiple "paths" for the same join, parameterized by outer relation values. This enables nestloop joins to use inner indexes efficiently.

**Example**:
```sql
SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id
WHERE o.amount > 1000

-- Path 1: Hash Join
HashJoin
  |--- SeqScan(orders) WHERE amount > 1000
  |--- SeqScan(customers)

-- Path 2: Parameterized Nested Loop
NestLoop
  |--- SeqScan(orders) WHERE amount > 1000
  |--- IndexScan(customers, idx_customer_id)  -- Parameterized by o.customer_id
```

**Benefit**: Inner index scan can be cheaper than hash join when outer is small.

**RA Status**: Check if RA considers parameterized index scans in join costing.

---

## JOIN Ordering Heuristics

### Genetic Query Optimizer (GEQO)

**Location**: `src/backend/optimizer/geqo/`

**Description**: For joins with >12 relations, PostgreSQL switches from dynamic programming to genetic algorithm.

**Algorithm**:
1. Generate random join orders (population)
2. Evaluate cost of each
3. Select best candidates (fitness)
4. Crossover and mutate
5. Iterate until convergence

**Benefit**: Handles large join graphs (20+ tables) that are infeasible for exhaustive search.

**RA Status**: RA has `large_join::LargeJoinStrategy::SimulatedAnnealing` - similar approach.
**Comparison needed**: GEQO vs Simulated Annealing performance.

---

## Aggregate Optimizations

### Incremental Sort for GROUP BY

**Location**: `src/backend/optimizer/path/pathkeys.c`

**Added**: PostgreSQL 13

**Description**: If input is partially sorted, use incremental sort instead of full sort.

**Example**:
```sql
-- Index on (category, date)
SELECT category, date, SUM(amount)
FROM sales
GROUP BY category, date
ORDER BY category, date

-- Plan:
IncrementalSort (sort by date within each category)
  |--- IndexScan(idx_category_date)  -- Already sorted by category
```

**RA Status**: RA has `IncrementalSort` in algebra.rs - verify grouping integration.

---

### HashAggregate with Disk Spilling

**Location**: `src/backend/executor/nodeAgg.c`

**Added**: PostgreSQL 13

**Description**: HashAggregate now spills to disk when exceeding `work_mem`, instead of failing or switching to Sort+GroupAgg mid-execution.

**Benefit**: Predictable performance for high-cardinality GROUP BY.

**RA Status**: Cost model should account for spilling likelihood.

---

## Index Strategies

### Index-Only Scans with Visibility Map

**Location**: `src/backend/access/heap/visibilitymap.c`

**Description**: PostgreSQL tracks which heap pages have all tuples visible in a bitmap. Index-only scans consult this map to avoid heap fetches.

**Precondition for Index-Only Scan**:
- Index covers all query columns
- Visibility map indicates page is all-visible (no heap check needed)

**RA Status**: Verify if RA's IndexOnlyScan accounts for visibility checks.

---

### Partial Indexes

**Description**: Index only rows matching a WHERE condition.

**Example**:
```sql
CREATE INDEX idx_active_users ON users(email) WHERE active = true;

-- Query: SELECT * FROM users WHERE active = true AND email = 'foo@example.com'
-- Can use idx_active_users (partial index)
```

**Benefit**: Smaller index, faster scans.

**RA Status**: Needs facts::Index to store predicate. Check if supported.

---

## Subquery Optimizations

### Pull-Up Simple UNION ALL

**Location**: `src/backend/optimizer/prep/prepunion.c`

**Description**: Flatten UNION ALL subqueries into parent query.

**Example**:
```sql
-- Before:
SELECT * FROM (
  SELECT * FROM t1
  UNION ALL
  SELECT * FROM t2
) sub WHERE x > 10

-- After pull-up:
SELECT * FROM t1 WHERE x > 10
UNION ALL
SELECT * FROM t2 WHERE x > 10
```

**Benefit**: Pushes filter into both branches.

**RA Status**: Check `rules/logical/filter-pushdown-union.rra`.

---

### ANY/ALL Subquery Transformation

**Location**: `src/backend/optimizer/util/clauses.c`

**Description**: Transform `ANY`/`ALL` subqueries into `EXISTS`/`NOT EXISTS` or joins.

**Example**:
```sql
-- Before:
SELECT * FROM products WHERE price > ALL (SELECT price FROM competitors)

-- Transform to:
SELECT * FROM products
WHERE NOT EXISTS (SELECT 1 FROM competitors WHERE competitors.price >= products.price)
```

**RA Status**: Check `rules/logical/subquery-any-all-*.rra`.

---

## Parallel Query Execution

### Parallel-Aware Hash Join

**Location**: `src/backend/executor/nodeHashjoin.c`

**Description**: Workers cooperatively build shared hash table, then each worker probes with its partition of outer relation.

**Phases**:
1. **Barrier 1**: All workers build hash table collaboratively
2. **Barrier 2**: All workers wait for hash table completion
3. **Probe**: Each worker probes with its outer partition

**RA Status**: Verify if ParallelHashJoin models shared hash table build.

---

### Parallel Bitmap Heap Scan

**Location**: `src/backend/executor/nodeBitmapHeapscan.c`

**Added**: PostgreSQL 10

**Description**: Multiple workers scan heap pages identified by bitmap. Pages distributed dynamically (work-stealing).

**Benefit**: Parallelizes index-guided scans.

**RA Status**: Check if BitmapHeapScan can be parallel.

---

## Planner Tunables

### GUC Parameters Affecting Plans

**Important for cost calibration**:

| Parameter | Default | Effect |
|-----------|---------|--------|
| `random_page_cost` | 4.0 | Cost of random I/O (1.1 for SSD) |
| `seq_page_cost` | 1.0 | Cost of sequential I/O |
| `cpu_tuple_cost` | 0.01 | Cost to process one row |
| `cpu_index_tuple_cost` | 0.005 | Cost to process one index entry |
| `cpu_operator_cost` | 0.0025 | Cost of operator evaluation |
| `effective_cache_size` | 4GB | Estimate of OS cache |
| `enable_seqscan` | on | Allow sequential scans |
| `enable_indexscan` | on | Allow index scans |
| `enable_bitmapscan` | on | Allow bitmap scans |
| `enable_hashjoin` | on | Allow hash joins |
| `enable_mergejoin` | on | Allow merge joins |
| `enable_nestloop` | on | Allow nested loop joins |
| `enable_parallel_hash` | on | Allow parallel hash joins |
| `max_parallel_workers_per_gather` | 2 | Max parallel workers |

**RA Cost Mapper Status**: Verify if all these are mapped to RA cost units.

---

## Statistics Collection

### Extended Statistics (Multivariate)

**Location**: `src/backend/statistics/extended_stats.c`

**Added**: PostgreSQL 10

**Description**: Track correlation between columns to improve cardinality estimates.

**Types**:
- **n_distinct**: COUNT(DISTINCT a, b)
- **dependencies**: Functional dependency a -> b
- **mcv**: Most common value combinations

**Example**:
```sql
CREATE STATISTICS city_zip_stats (dependencies) ON city, zipcode FROM addresses;
-- PostgreSQL learns: city -> zipcode (functional dependency)

-- Query: WHERE city = 'NYC' AND zipcode = '10001'
-- Cardinality: Uses dependency instead of independence assumption
```

**RA Status**: Does RA's Statistics support multi-column dependencies?

---

### Histogram Types

PostgreSQL uses:
- **Equi-depth histograms**: Equal number of rows per bucket
- **Most Common Values (MCVs)**: Separate storage for frequent values
- **NULL fraction**: Explicit tracking

**RA Status**: Verified - RA supports EquiDepthHistogram, MCVs, null_fraction.

---

## Window Function Optimization

### Window Function Pushdown

**Description**: Push window functions below joins when possible.

**Example**:
```sql
-- Before:
SELECT *, ROW_NUMBER() OVER (PARTITION BY category ORDER BY price)
FROM products JOIN inventory ON products.id = inventory.product_id

-- After: Compute window function first, then join
WITH ranked AS (
  SELECT *, ROW_NUMBER() OVER (PARTITION BY category ORDER BY price) AS rn
  FROM products
)
SELECT * FROM ranked JOIN inventory ON ranked.id = inventory.product_id
```

**Benefit**: Smaller intermediate result for join.

**RA Status**: Check `rules/logical/window-pushdown-*.rra`.

---

## Common Table Expression (CTE) Optimization

### CTE Inlining vs Materialization

**Location**: `src/backend/optimizer/util/clauses.c`

**Added**: PostgreSQL 12 (inlining), earlier versions always materialized

**Description**:
- **Inline**: Substitute CTE definition into query (allows further optimization)
- **Materialize**: Compute CTE once, store results (optimization fence)

**Heuristic**: Inline if:
- CTE is referenced once
- CTE is small
- User doesn't specify `MATERIALIZED`

**Example**:
```sql
WITH cheap_products AS (
  SELECT * FROM products WHERE price < 100
)
SELECT * FROM cheap_products WHERE category = 'Electronics'

-- Inline:
SELECT * FROM products WHERE price < 100 AND category = 'Electronics'
-- (Allows index use on both price and category)
```

**RA Status**: Does RA inline CTEs or always materialize?

---

## Join Removal

### Left Join Elimination

**Description**: Remove left join if no columns from right relation are used and join is guaranteed not to duplicate rows.

**Example**:
```sql
-- Before:
SELECT o.order_id, o.amount
FROM orders o
LEFT JOIN customers c ON o.customer_id = c.id

-- After: (customers not used, join can be removed)
SELECT o.order_id, o.amount FROM orders o
```

**Preconditions**:
- No columns from right relation in SELECT, WHERE, etc.
- Join key on right side is UNIQUE (or PK)

**RA Status**: Check `rules/logical/join-elimination-*.rra`.

---

## Next Steps

1. **Map PostgreSQL cost parameters** to RA's cost model
2. **Verify** which techniques are already in RA
3. **Implement missing high-impact rules**:
   - Magic sets for recursive queries
   - GroupJoin (eager aggregation)
   - Skip scan for composite indexes
4. **Benchmark** against PostgreSQL on TPC-H

---

## References

- PostgreSQL Documentation: [Query Planning](https://www.postgresql.org/docs/current/planner-optimizer.html)
- Source Code: [src/backend/optimizer/README](https://github.com/postgres/postgres/blob/master/src/backend/optimizer/README)
- PGCon Talks: Search "query planner" on YouTube
- Robert Haas Blog: [Thoughts on PostgreSQL Development](https://rhaas.blogspot.com/)
