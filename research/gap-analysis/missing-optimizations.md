# Missing Optimizations in RA

This document tracks optimization techniques from database literature that may not yet be implemented in RA.

**Current Status**: RA has 1,354 rules across logical, physical, parallel, hardware, and database-specific categories.

## Methodology

1. Review database literature (papers, textbooks, lectures)
2. Check if technique exists in `rules/` directory
3. Document gaps with references
4. Prioritize by impact and complexity

---

## Confirmed Gaps

### 1. Magic Sets for Recursive Queries

**Source**: "Optimization of Queries with Recursive Predicates" (Bancilhon et al., 1986)

**Description**: Transform recursive datalog-style queries to reduce intermediate result sizes by pushing selections into recursion.

**Pattern**:
```sql
-- Before: Computes full transitive closure
WITH RECURSIVE ancestors AS (
  SELECT parent_id, child_id FROM family
  UNION ALL
  SELECT f.parent_id, a.child_id
  FROM family f JOIN ancestors a ON f.child_id = a.parent_id
)
SELECT * FROM ancestors WHERE child_id = 'Bob';

-- After: Magic sets push the filter into recursion
WITH RECURSIVE ancestors_magic AS (
  SELECT parent_id, child_id FROM family WHERE child_id = 'Bob'
  UNION ALL
  SELECT f.parent_id, a.child_id
  FROM family f JOIN ancestors_magic a ON f.child_id = a.parent_id
)
SELECT * FROM ancestors_magic;
```

**Status**: Not found in `rules/logical/` or `rules/rpr/`
**Priority**: High (significant performance improvement for recursive queries)
**RFC**: Needed

---

### 2. Semi-Join Reduction for Distributed Queries

**Source**: "Processing Distributed Queries" (Bernstein & Chiu, 1981)

**Description**: In distributed settings, reduce data transfer by sending only join keys instead of full tuples.

**Pattern**:
```sql
-- Before: Ship all rows from R1
SELECT * FROM R1 JOIN R2 ON R1.id = R2.id
WHERE R2.category = 'A'

-- After: Semi-join reduction
-- 1. Ship {R2.id WHERE category = 'A'} to R1's site
-- 2. Filter R1 locally: R1 WHERE id IN (...)
-- 3. Ship filtered R1 to R2's site
-- 4. Perform final join
```

**Status**: Checked `rules/distributed/` - has bloom filters but not explicit semi-join reduction
**Priority**: Medium (important for federated/distributed queries)
**RFC**: Needed

---

### 3. GroupJoin (Eager Aggregation Before Join)

**Source**: "Eager Aggregation and Lazy Aggregation" (Yan & Larson, 1995)

**Description**: When aggregating after a join, sometimes it's cheaper to aggregate one relation before joining.

**Pattern**:
```sql
-- Before: Join then aggregate
SELECT category, SUM(sales.amount)
FROM sales JOIN products ON sales.product_id = products.id
GROUP BY products.category

-- After: Aggregate sales by product_id first, then join
WITH aggregated_sales AS (
  SELECT product_id, SUM(amount) AS total
  FROM sales
  GROUP BY product_id
)
SELECT category, SUM(total)
FROM aggregated_sales JOIN products ON aggregated_sales.product_id = products.id
GROUP BY products.category
```

**Preconditions**:
- Aggregate function is decomposable (SUM, COUNT, MIN, MAX)
- Grouping keys include join keys

**Status**: Not found in `rules/logical/aggregate-pushdown*.rra`
**Priority**: High (common pattern in OLAP queries)
**RFC**: Needed

---

### 4. Distinct Aggregation Rewrite

**Source**: PostgreSQL planner, "Multiple Distinct Aggregates" optimization

**Description**: Multiple `COUNT(DISTINCT col)` in same query can be rewritten using UNION ALL + aggregation.

**Pattern**:
```sql
-- Before: Expensive (scans table twice)
SELECT COUNT(DISTINCT user_id), COUNT(DISTINCT product_id)
FROM orders

-- After: Single scan with UNION ALL
SELECT
  SUM(CASE WHEN tag = 'user' THEN 1 ELSE 0 END) AS distinct_users,
  SUM(CASE WHEN tag = 'product' THEN 1 ELSE 0 END) AS distinct_products
FROM (
  SELECT 'user' AS tag, user_id AS val FROM orders
  UNION ALL
  SELECT 'product' AS tag, product_id AS val FROM orders
) sub
GROUP BY tag, val
```

**Status**: Not found in `rules/logical/distinct-*.rra`
**Priority**: Medium (performance gain for multi-distinct queries)
**RFC**: Needed

---

### 5. Sideways Information Passing (SIP)

**Source**: "Adaptive Optimization of Very Large Join Queries" (Deshpande et al., 2007)

**Description**: During join execution, pass bloom filters/bitmaps from completed joins to remaining scans.

**Pattern**:
```
-- Query: R1 $\bowtie$ R2 $\bowtie$ R3
-- Execution:
-- 1. Start scan of R1
-- 2. Build bloom filter B1 from R1.join_key
-- 3. While scanning R2, filter using B1 (SIP)
-- 4. Build bloom filter B2 from (R1 $\bowtie$ R2).join_key
-- 5. While scanning R3, filter using B2 (SIP)
```

**Status**: Not found in `rules/physical/` or `rules/parallel/`
**Priority**: High (Adaptive query processing, significant speedup)
**RFC**: Needed - requires runtime adaptivity

---

### 6. Partial Aggregation (Two-Phase Aggregation)

**Source**: Common technique in MPP databases (Vertica, Greenplum)

**Description**: For parallel aggregation, do local aggregation on each node before global aggregation.

**Pattern**:
```
-- Before: Global hash aggregation
Aggregate(SUM(amount) GROUP BY category)
  |--- ParallelScan(orders)

-- After: Two-phase aggregation
Aggregate(SUM(partial_sum) GROUP BY category)  -- Global phase
  |--- Gather
      |--- Aggregate(SUM(amount) AS partial_sum GROUP BY category)  -- Local phase
          |--- ParallelScan(orders)
```

**Status**: Checked `rules/parallel/` - has ParallelAggregate but not explicit two-phase
**Priority**: High (standard for parallel OLAP)
**RFC**: May exist but not documented - needs verification

---

### 7. Index Intersection

**Source**: "Index Intersection" (IBM DB2, SQL Server)

**Description**: Use multiple indexes and intersect their bitmaps instead of scanning with one index.

**Pattern**:
```sql
-- Query: WHERE age > 30 AND city = 'NYC'
-- Indexes: idx_age(age), idx_city(city)

-- Instead of choosing one index:
-- Plan 1: Index scan on idx_age, filter city = 'NYC'
-- Plan 2: Index scan on idx_city, filter age > 30

-- Better: Bitmap intersection
BitmapHeapScan
  |--- BitmapAnd
      |--- BitmapIndexScan(idx_age)
      |--- BitmapIndexScan(idx_city)
```

**Status**: Found `BitmapAnd` in algebra.rs, likely implemented
**Priority**: Medium - verify implementation
**RFC**: Not needed

---

### 8. Runtime Filter Pushdown (Bloom Filters)

**Source**: Apache Impala, "Runtime Filter Pushdown"

**Description**: During join execution, build bloom filter from small table and push to scan of large table.

**Pattern**:
```
-- Query: SELECT * FROM large_table JOIN small_table ON large_table.id = small_table.id

-- Execution:
-- 1. Build bloom filter BF from small_table.id
-- 2. Push BF to large_table scan
-- 3. Filter large_table rows using BF before join
-- Result: Fewer rows sent to join operator
```

**Status**: Checked `rules/distributed/bloom-filter-*.rra` - has bloom filters for distributed, need runtime pushdown
**Priority**: High (massive performance gain for star schema joins)
**RFC**: Extend existing bloom filter rules

---

### 9. Skip Scan (Index Skip Scan)

**Source**: Oracle, "Index Skip Scan"

**Description**: Use a composite index even when leading column is not in WHERE clause.

**Pattern**:
```sql
-- Index: idx_category_price(category, price)
-- Query: SELECT * FROM products WHERE price > 100
-- (No filter on category)

-- Traditional: Can't use index (leading column missing)
-- Skip Scan: Iterate through distinct categories, scan each range
-- Equivalent to:
SELECT * FROM products WHERE category = 'A' AND price > 100
UNION ALL
SELECT * FROM products WHERE category = 'B' AND price > 100
...
```

**Preconditions**:
- Leading column has low cardinality
- Trailing column is highly selective

**Status**: Not found in `rules/physical/index-*.rra`
**Priority**: Medium (useful for multi-tenant schemas)
**RFC**: Needed

---

### 10. Decorrelation of Nested Aggregates

**Source**: "Unnesting Arbitrary Queries" (Neumann & Kemper, 2015)

**Description**: Transform correlated subqueries with aggregates into joins.

**Pattern**:
```sql
-- Before: Correlated subquery
SELECT c.name,
       (SELECT AVG(o.amount) FROM orders o WHERE o.customer_id = c.id)
FROM customers c

-- After: Decorrelated with grouping
SELECT c.name, agg.avg_amount
FROM customers c
LEFT JOIN (
  SELECT customer_id, AVG(amount) AS avg_amount
  FROM orders
  GROUP BY customer_id
) agg ON c.id = agg.customer_id
```

**Status**: Checked `rules/logical/subquery-*.rra` - has unnesting rules, need to verify aggregate case
**Priority**: High (common pattern in business queries)
**RFC**: Verification needed

---

## Research Sources to Mine

### CMU 15-445/645 Lectures
- [ ] Lecture 13: Query Optimization
- [ ] Lecture 14: Cost Models
- [ ] Lecture 15: Join Algorithms
- [ ] Lecture 19: Query Compilation
- [ ] Lecture 20: Adaptive Query Processing

### Papers
- [ ] "Eddies: Continuously Adaptive Query Processing" (Avnur & Hellerstein, 2000)
- [ ] "Robust Query Processing through Progressive Optimization" (Markl et al., 2004)
- [ ] "LEO - DB2's LEarning Optimizer" (Stillger et al., 2001)
- [ ] "Building Query Compilers" (Neumann, 2011)
- [ ] "MonetDB/X100: Hyper-Pipelining Query Execution" (Boncz et al., 2005)

### Database System Source Code
- [ ] PostgreSQL: `src/backend/optimizer/path/`
- [ ] PostgreSQL: `src/backend/optimizer/plan/`
- [ ] SQLite: query planner (sqlite3_create_function, VDBE)
- [ ] Apache Calcite: optimizer rules

---

## Next Steps

1. **Verify** if techniques marked "needs verification" are already implemented
2. **Create RFCs** for high-priority gaps
3. **Implement** top 3 missing optimizations
4. **Benchmark** impact on TPC-H queries

---

## Contributing

To add a new gap:
1. Describe the technique with before/after examples
2. Cite source (paper, database system, lecture)
3. Check `rules/` directory: `grep -r "technique_name" rules/`
4. Mark status and priority
5. Link to RFC if created
