# Database Optimization Shortcuts

Research on how production database systems optimize expensive operations through caching, pre-calculation, estimation, and clever shortcuts.

**Author**: RA Research Team
**Date**: 2026-03-20
**Status**: Draft

---

## Overview

Modern database systems employ numerous "shortcuts" to avoid expensive operations. These optimizations range from simple constant-folding to complex caching systems that trade accuracy for performance. Understanding these shortcuts is critical for RA to generate competitive plans.

This document catalogs:
1. What optimization shortcuts exist
2. Which databases implement them
3. When they're safe to apply
4. How to model them in RA

---

## 1. COUNT(*) Optimizations

### Problem
Full table scans for `COUNT(*)` are expensive on large tables. Many queries just need an approximate count.

### Database Implementations

#### PostgreSQL
- **Method**: `pg_stat_user_tables.n_live_tup` (live tuple estimate)
- **Accuracy**: Approximate (updated by VACUUM, ANALYZE)
- **Speed**: Instant (metadata lookup)
- **When used**: Implicit in planner cost estimates
- **Example**:
```sql
-- Exact (slow on large tables)
SELECT COUNT(*) FROM users;

-- Approximate (instant)
SELECT n_live_tup FROM pg_stat_user_tables WHERE relname = 'users';
```
- **Safety**: Stale after many INSERTs/DELETEs between ANALYZE runs
- **Staleness bound**: Can be off by ±10% on high-churn tables

#### MongoDB
- **Method**: `collection.estimatedDocumentCount()` (cached metadata)
- **Accuracy**: Exact for WiredTiger storage engine (maintained in metadata)
- **Speed**: O(1), no collection scan
- **When used**: Replaces `db.collection.count()` when filter is empty
- **Example**:
```javascript
// Exact count, cached (fast)
db.users.estimatedDocumentCount()

// Exact count with filter (slow)
db.users.countDocuments({status: 'active'})
```
- **Safety**: Always safe, maintained transactionally

#### MySQL (InnoDB)
- **Method**: `information_schema.TABLES.TABLE_ROWS` (estimated from index statistics)
- **Accuracy**: Rough estimate (±30-50%)
- **Speed**: Instant (metadata)
- **When used**: Never automatically, user must query information_schema
- **Example**:
```sql
-- Exact (table scan)
SELECT COUNT(*) FROM users;

-- Approximate (metadata)
SELECT TABLE_ROWS FROM information_schema.TABLES
WHERE TABLE_NAME = 'users' AND TABLE_SCHEMA = 'mydb';
```
- **Safety**: Very stale, only updated during ANALYZE TABLE
- **Note**: MyISAM stores exact count, InnoDB does not

#### Oracle
- **Method**: `num_rows` in `user_tables` / `dba_tables` (gathered by ANALYZE)
- **Accuracy**: Exact at ANALYZE time
- **Speed**: Instant (data dictionary)
- **When used**: Planner uses for cost estimates
- **Example**:
```sql
-- Exact (full table scan)
SELECT COUNT(*) FROM users;

-- Approximate (statistics)
SELECT num_rows FROM user_tables WHERE table_name = 'USERS';
```
- **Safety**: Stale after DML operations
- **Staleness bound**: Oracle recommends re-analyze when >10% change

#### SQL Server
- **Method**: `sys.dm_db_partition_stats.row_count`
- **Accuracy**: Exact (maintained transactionally)
- **Speed**: O(1) for heap/clustered index
- **When used**: Internal optimizer uses for cost estimation
- **Example**:
```sql
-- Exact (uses metadata if possible)
SELECT COUNT(*) FROM users;

-- Direct metadata access
SELECT SUM(row_count)
FROM sys.dm_db_partition_stats
WHERE object_id = OBJECT_ID('users') AND index_id IN (0,1);
```
- **Safety**: Always accurate

#### DuckDB
- **Method**: Cardinality metadata in Parquet/CSV files
- **Accuracy**: Exact for append-only data
- **Speed**: O(1) from file metadata
- **When used**: `COUNT(*)` on columnar formats
- **Example**:
```sql
-- No table scan, reads Parquet metadata
SELECT COUNT(*) FROM 'data.parquet';
```
- **Safety**: Exact for immutable files

### RA Modeling

**Rules to add**:
1. **count-star-to-metadata**: `agg[count(*)](scan[T]) → metadata_lookup[T.row_count]`
   - Precondition: `supports_count_metadata(database)` AND `staleness[T] < threshold`
   - Cost: O(1) vs O(n) for full scan
   - Safety: Check staleness_acceptable(staleness, query_context)

2. **count-predicate-to-index**: `agg[count(*)](filter[P](scan[T])) → index_count[I, P]` when covering index exists
   - Precondition: `exists_covering_index(T, P)` AND `P` is index-sargable
   - Cost: O(log n + k) where k = matched rows

**Facts needed**:
- `supports_count_metadata: bool` (per database)
- `table_staleness: Duration` (time since last ANALYZE)
- `staleness_acceptable: fn(Duration, QueryType) -> bool`

---

## 2. MIN/MAX Index Optimizations

### Problem
`MIN(col)` / `MAX(col)` require full table scan without index. With B-tree index, it's O(log n) to first/last key.

### Database Implementations

#### PostgreSQL
- **Method**: Index scan to first/last key
- **Index type**: B-tree (sorted)
- **Example**:
```sql
-- Without index: Seq Scan on orders (cost=10000..50000 rows=1)
SELECT MAX(order_id) FROM orders;

-- With index on order_id: Index Only Scan using orders_pkey (cost=0.01..0.02 rows=1)
CREATE INDEX ON orders(order_id);
SELECT MAX(order_id) FROM orders;
```
- **Speed**: O(log n) vs O(n)
- **Plan**: `Limit 1` + `Index Scan Backward`

#### MySQL
- **Method**: "Select tables optimized away" (plan annotation)
- **Example**:
```sql
EXPLAIN SELECT MAX(id) FROM users;
-- Extra: Select tables optimized away
```
- **Speed**: O(1) - reads last key from B-tree
- **Applicability**: Only for simple `MAX(indexed_col)` with no WHERE

#### Oracle
- **Method**: `INDEX FULL SCAN (MIN/MAX)` (optimizer annotation)
- **Example**:
```sql
-- Plan shows: INDEX FULL SCAN (MIN/MAX)
SELECT MAX(salary) FROM employees;
```
- **Speed**: O(log n) to rightmost leaf

#### SQL Server
- **Method**: Top 1 with index seek
- **Plan**: Shows as `Index Seek (Top 1)`
- **Speed**: O(log n)

### RA Modeling

**Rules to add**:
1. **min-max-index-rewrite**:
```
agg[min(C)](scan[T]) →
  limit[1](sort[C asc](index_scan[T, C]))
```
- Precondition: `exists_index(T, C)` AND `C` is first column in index
- Cost reduction: O(n) → O(log n)

2. **min-max-index-only**:
```
agg[max(C)](filter[P](scan[T])) →
  limit[1](sort[C desc](index_scan[T, C, P]))
```
- Precondition: `covering_index(T, [C] + referenced_cols(P))`
- Cost: O(log n + k) where k = filtered rows

**Safety**: Safe when:
- Index is B-tree (sorted)
- No NULLs or `WHERE col IS NOT NULL`
- Single aggregate (not `MIN(a), MAX(b)` requiring different orderings)

---

## 3. Materialized Views

### Problem
Complex aggregations (GROUP BY, JOIN + SUM) are expensive to recompute on every query.

### Database Implementations

#### PostgreSQL
- **Method**: Materialized Views (manually refreshed)
- **Syntax**:
```sql
CREATE MATERIALIZED VIEW sales_summary AS
SELECT product_id, SUM(amount) as total_sales
FROM orders
GROUP BY product_id;

-- Query uses materialized view (fast)
SELECT * FROM sales_summary WHERE product_id = 123;
```
- **Refresh**: Manual (`REFRESH MATERIALIZED VIEW`) or via cron
- **Concurrency**: `REFRESH MATERIALIZED VIEW CONCURRENTLY` (builds in background)
- **Staleness**: User-controlled (trade freshness for speed)

#### Oracle
- **Method**: Materialized Views with automatic refresh
- **Refresh modes**:
  - `ON COMMIT`: Synchronized (transactional)
  - `ON DEMAND`: Manual
  - `FAST`: Incremental (uses materialized view logs)
  - `COMPLETE`: Full rebuild
- **Query rewrite**: Optimizer automatically uses MV if applicable
- **Example**:
```sql
CREATE MATERIALIZED VIEW sales_summary
REFRESH FAST ON COMMIT
AS SELECT product_id, SUM(amount) FROM orders GROUP BY product_id;

-- Optimizer rewrites to use sales_summary
SELECT SUM(amount) FROM orders WHERE product_id = 123;
```

#### SQL Server (Indexed Views)
- **Method**: Indexed Views (automatically maintained)
- **Syntax**:
```sql
CREATE VIEW sales_summary WITH SCHEMABINDING AS
SELECT product_id, SUM(amount) as total_sales, COUNT_BIG(*) as cnt
FROM dbo.orders
GROUP BY product_id;

CREATE UNIQUE CLUSTERED INDEX idx ON sales_summary(product_id);
```
- **Maintenance**: Automatic on INSERT/UPDATE/DELETE (transactional overhead)
- **Query rewrite**: Optimizer uses view automatically
- **Restrictions**: Must use `SCHEMABINDING`, `COUNT_BIG(*)`, no `TOP`, `OUTER JOIN`, etc.

#### DuckDB
- **Method**: Not yet implemented (as of 2024), but on roadmap
- **Workaround**: Create cached tables via `CREATE TABLE AS SELECT`

### RA Modeling

**Rules to add**:
1. **materialized-view-substitution**:
```
agg[F](join[C](scan[T1], scan[T2])) →
  scan[MV] WHERE equivalent(MV.definition, original_query)
```
- Precondition: `exists_materialized_view(MV)` AND `staleness_acceptable(MV)`
- Cost: O(1) scan vs O(n + m) join + agg

2. **partial-materialized-view**:
```
agg[F](filter[P2](scan[T])) →
  agg[F](filter[P2](scan[MV]))
  WHERE MV.definition = agg[G](filter[P1](scan[T]))
  AND P1 is superset of P2
```
- Example: MV has `GROUP BY product_id, region`, query wants `GROUP BY product_id` → aggregate MV

**Facts needed**:
- `materialized_views: Vec<MVMetadata>` with definition, staleness
- `supports_mv_rewrite: bool` (per database)
- `staleness_policy: fn(QueryType) -> MaxStaleness`

**Safety**:
- Check semantic equivalence
- Ensure MV is fresh enough (staleness < threshold)
- For incremental MV (Oracle FAST), check MV log exists

---

## 4. Approximate Query Processing (AQP)

### Problem
Exact aggregates on huge datasets take too long. Users often accept approximate results for speed.

### Database Implementations

#### PostgreSQL (HyperLogLog extension)
- **Method**: `pg_hll` extension for cardinality estimation
- **Accuracy**: ±2% with 99% confidence (configurable)
- **Speed**: O(1) lookup vs O(n) distinct scan
- **Example**:
```sql
-- Exact (slow)
SELECT COUNT(DISTINCT user_id) FROM page_views;

-- Approximate (fast)
SELECT hll_cardinality(hll_agg(user_id)) FROM page_views;
```
- **Storage**: 1-2KB HLL sketch vs full distinct set

#### Redshift (Approximate COUNT DISTINCT)
- **Method**: `APPROXIMATE COUNT(DISTINCT col)`
- **Accuracy**: ±2-3%
- **Speed**: 10-100x faster than exact
- **Example**:
```sql
SELECT APPROXIMATE COUNT(DISTINCT user_id) FROM events;
```

#### Snowflake (HyperLogLog)
- **Method**: `HLL()` aggregate function
- **Accuracy**: ±1-2%
- **Speed**: O(1) merge of sketches
- **Example**:
```sql
-- Build HLL sketch
CREATE TABLE user_hll AS
SELECT date, HLL(user_id) as hll_sketch
FROM events
GROUP BY date;

-- Estimate distinct users (fast)
SELECT HLL_ESTIMATE(HLL_UNION_AGG(hll_sketch)) FROM user_hll;
```

#### BlinkDB (Stratified Sampling)
- **Method**: Pre-computed samples with error bounds
- **Accuracy**: User-specified error/confidence bounds
- **Speed**: Query samples instead of full data
- **Example**:
```sql
SELECT AVG(price) FROM sales WITH ERROR 5% CONFIDENCE 95%;
```

### RA Modeling

**Rules to add**:
1. **count-distinct-to-hll**:
```
agg[count(distinct C)](scan[T]) →
  agg[hll_estimate](scan[hll_sketch_table[T, C]])
```
- Precondition: `exists_hll_sketch(T, C)` OR `supports_hll(database)` AND `accuracy_acceptable(query)`
- Cost: O(1) vs O(n)
- Error bound: ±2% with 99% confidence

2. **aggregate-to-sample**:
```
agg[avg(C)](filter[P](scan[T])) →
  agg[avg(C)](filter[P](scan[sample[T, ratio]]))
  WITH error_bound(ratio, confidence)
```
- Precondition: `sample_acceptable(query)` AND `has_uniform_sample(T)`
- Cost: O(n * ratio) vs O(n)

**Facts needed**:
- `hll_sketches: HashMap<(Table, Column), HLLMeta>`
- `samples: HashMap<Table, SampleMeta>`
- `accuracy_requirements: fn(Query) -> (ErrorBound, Confidence)`

**Safety**:
- User must accept approximate results (query annotation: `APPROXIMATE` keyword)
- Error bounds must be propagated through query
- Not safe for financial/audit queries

---

## 5. Query Result Caching

### Problem
Identical queries executed repeatedly waste resources.

### Database Implementations

#### MySQL Query Cache (removed in 8.0)
- **Why removed**: Scalability bottlenecks (global mutex), cache invalidation complexity
- **Replacement**: Application-level caching (Redis, Memcached)

#### Redshift Result Caching
- **Method**: Cache query results for 24 hours
- **Invalidation**: Automatic when underlying tables change
- **Example**: Identical query with same parameters returns cached result (0ms execution)
- **Speed**: O(1) vs O(n) scan

#### Snowflake Result Caching
- **Method**: 24-hour cache per user
- **Sharing**: Results not shared across users (privacy)
- **Speed**: Sub-second response for cached queries
- **Cost**: No warehouse compute charges for cache hits

#### Presto/Trino Alluxio Caching
- **Method**: Cache intermediate results in distributed cache (Alluxio)
- **Use case**: Common subqueries across dashboards
- **Speed**: 5-10x faster on cache hit

### RA Modeling

**Rules to add**:
1. **query-result-cache-lookup**:
```
query[Q] →
  if cache.contains(hash(Q)) AND cache[Q].is_fresh()
  then return cache[Q].result
  else execute(Q) AND cache[Q] = result
```
- Precondition: `supports_result_cache(database)` AND `cache_policy_allows(Q)`
- Cost: O(1) vs O(n)

**Facts needed**:
- `result_cache_enabled: bool`
- `cache_ttl: Duration`
- `cache_invalidation_policy: fn(Table) -> InvalidateQueries`

**Safety**:
- Invalidate on table modifications (INSERT/UPDATE/DELETE)
- Check staleness (TTL)
- Not safe for non-deterministic queries (`random()`, `now()`)

---

## 6. Index-Only Scans (Covering Indexes)

### Problem
Index scan followed by heap fetch is expensive (random I/O). Covering indexes eliminate heap access.

### Database Implementations

#### PostgreSQL (Index Only Scan)
- **Method**: Return data from index without accessing table heap
- **Requirements**:
  - All `SELECT` columns in index
  - Visibility map clean (no dead tuples)
- **Example**:
```sql
CREATE INDEX idx_user_email ON users(email, name);

-- Index Only Scan using idx_user_email
SELECT email, name FROM users WHERE email LIKE 'john%';
```
- **Speed**: 2-10x faster than index scan + heap fetch
- **Plan annotation**: `Index Only Scan`

#### SQL Server (Covering Index)
- **Method**: `INCLUDE` clause for non-key columns
- **Syntax**:
```sql
CREATE INDEX idx_orders ON orders(customer_id) INCLUDE (order_date, amount);

-- Index Seek (covering)
SELECT order_date, amount FROM orders WHERE customer_id = 123;
```
- **Speed**: No bookmark lookup to clustered index

#### Oracle (Index-Organized Table)
- **Method**: Store entire row in index (B-tree)
- **Syntax**:
```sql
CREATE TABLE users (
  id NUMBER PRIMARY KEY,
  email VARCHAR2(100),
  name VARCHAR2(100)
) ORGANIZATION INDEX;
```
- **Speed**: All queries are index-only (no heap)
- **Downside**: Secondary indexes store primary key (larger)

#### MySQL (InnoDB Covering Index)
- **Method**: Include columns in secondary index
- **Speed**: No primary key lookup
- **Example**:
```sql
CREATE INDEX idx_email_name ON users(email, name);
SELECT email, name FROM users WHERE email = 'foo@bar.com';
-- Extra: Using index
```

### RA Modeling

**Rules to add**:
1. **project-to-covering-index**:
```
project[C1, C2](filter[P](scan[T])) →
  project[C1, C2](index_scan[I])
  WHERE I covers {C1, C2} ∪ cols(P)
```
- Precondition: `exists_covering_index(T, {C1, C2} ∪ cols(P))`
- Cost: Eliminates heap fetch (2-10x faster)

2. **join-to-covering-index**:
```
join[T1.id = T2.fk](scan[T1], scan[T2]) →
  join[T1.id = T2.fk](scan[T1], index_scan[T2, covering_idx])
```
- Precondition: `covering_index(T2, {fk, output_cols})`

**Facts needed**:
- `covering_indexes: Vec<IndexMeta>` with (table, columns)
- `visibility_map_clean: fn(Table) -> bool` (PostgreSQL-specific)

**Safety**:
- For PostgreSQL: Check visibility map (VACUUM required)
- Index must include all referenced columns

---

## 7. Constant Folding & Short-Circuit Evaluation

### Problem
Some queries can be answered without table access at all.

### Database Implementations

#### All Systems: WHERE 1=0 (Empty Result)
- **Optimization**: Skip table access, return empty set
- **Example**:
```sql
SELECT * FROM users WHERE 1=0;
-- Plan: Result (rows=0)
```

#### All Systems: LIMIT 0 (Schema Only)
- **Optimization**: Return schema without data
- **Example**:
```sql
SELECT * FROM users LIMIT 0;
-- Used for schema introspection (pg_dump, ORMs)
```

#### PostgreSQL: EXISTS Short-Circuit
- **Optimization**: Stop after first matching row
- **Example**:
```sql
SELECT EXISTS(SELECT 1 FROM orders WHERE customer_id = 123);
-- Limit 1 (stop at first match)
```

#### SQL Server: Constant Scan
- **Optimization**: Return literal without table access
- **Example**:
```sql
SELECT 42 AS answer;
-- Plan: Constant Scan (no tables)
```

### RA Modeling

**Rules to add**:
1. **filter-false-to-empty**:
```
filter[false](scan[T]) → empty_relation
```

2. **limit-zero-to-schema**:
```
limit[0](scan[T]) → schema[T]
```

3. **exists-to-limit-one**:
```
exists(filter[P](scan[T])) → limit[1](filter[P](scan[T]))
```
- Cost: O(1) vs O(n) (stop at first match)

---

## 8. Predicate Pushdown to Storage

### Problem
Filtering after reading wastes I/O. Push predicates into storage engine.

### Database Implementations

#### Parquet Predicate Pushdown
- **Method**: Read Parquet file metadata, skip row groups
- **Example**:
```sql
-- DuckDB reads Parquet metadata
SELECT * FROM 'sales.parquet' WHERE year = 2023;
-- Skips row groups where min(year) > 2023 OR max(year) < 2023
```
- **Speed**: 10-100x faster (skip entire row groups)
- **Metadata**: Min/max, null count per column per row group

#### PostgreSQL JSONB Pushdown
- **Method**: Use GIN index for JSONB predicates
- **Example**:
```sql
CREATE INDEX idx_data_gin ON events USING GIN (data);
SELECT * FROM events WHERE data @> '{"user_id": 123}';
-- Bitmap Index Scan on idx_data_gin
```
- **Speed**: 100x faster than sequential scan with `data->>'user_id' = '123'`

#### ClickHouse Primary Key Filtering
- **Method**: Skip data blocks (8192 rows) using sparse primary index
- **Example**:
```sql
SELECT * FROM events WHERE date = '2023-01-01' AND user_id = 123;
-- Skips 99% of blocks using primary key (date, user_id)
```

### RA Modeling

**Rules to add**:
1. **predicate-pushdown-parquet**:
```
filter[P](scan[parquet_file]) →
  scan[parquet_file, pushed_predicate=P]
```
- Precondition: `P` is sargable (>, <, =, IN)
- Cost: Skip row groups (10-100x faster)

2. **jsonb-predicate-to-gin**:
```
filter[data->>'k' = v](scan[T]) →
  index_scan[gin_index, data @> {"k": v}]
```
- Precondition: `exists_gin_index(T, data)`

---

## 9. Bloom Filters for Joins

### Problem
Hash joins waste CPU probing for non-existent keys.

### Database Implementations

#### PostgreSQL (Parallel Hash Join with Bloom Filter)
- **Method**: Build Bloom filter on join key, filter probe side
- **Example**:
```sql
-- Plan shows: Hash Join (Parallel, with Bloom Filter)
SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id;
```
- **Speed**: 2-5x faster on skewed joins (many non-matching keys)

#### Spark (Broadcast Bloom Filter)
- **Method**: Build Bloom filter on small table, broadcast to all nodes
- **Speed**: Skip 90%+ of probe rows in star schema joins

#### Oracle (Bloom Filter on Parallel Query)
- **Method**: Build Bloom filter in parallel, distribute to slaves
- **Plan annotation**: `PX SEND BROADCAST (BLOOM FILTER)`

### RA Modeling

**Rules to add**:
1. **join-bloom-filter**:
```
join[T1.k = T2.fk](scan[T1], scan[T2]) →
  join[T1.k = T2.fk](
    scan[T1],
    filter[bloom_test(fk, bloom[T1.k])](scan[T2])
  )
```
- Precondition: `|T1| << |T2|` (small build side)
- Cost: Skip 90%+ probe rows

---

## 10. Modeling in RA: Proposed Architecture

### Facts Expansion

Add to `FactsProvider` trait:

```rust
trait FactsProvider {
    // Existing
    fn database_name(&self) -> &str;
    fn has_gpu(&self) -> bool;

    // New: Caching & approximation capabilities
    fn supports_count_metadata(&self) -> bool;
    fn supports_materialized_views(&self) -> bool;
    fn supports_hll_sketches(&self) -> bool;
    fn supports_result_cache(&self) -> bool;
    fn supports_bloom_filters(&self) -> bool;

    // Metadata access
    fn get_table_row_count_cached(&self, table: &str) -> Option<(u64, Duration)>;
    fn get_materialized_views(&self) -> Vec<MaterializedViewMeta>;
    fn get_covering_indexes(&self, table: &str) -> Vec<IndexMeta>;
    fn get_hll_sketches(&self, table: &str) -> Vec<HLLMeta>;

    // Staleness policy
    fn staleness_acceptable(&self, staleness: Duration, query_type: QueryType) -> bool;
}
```

### Rule Preconditions

Each shortcut rule needs preconditions:

```yaml
# rules/shortcuts/count-star-to-metadata.rra
---
id: count-star-to-metadata
preconditions:
  - type: predicate
    condition: "supports_count_metadata()"
  - type: predicate
    condition: "table_staleness(T) < staleness_threshold"
  - type: feature
    flag: "approximate_aggregates"
---
```

### Cost Model Adjustments

Add shortcut costs:

```rust
impl CostModel {
    fn cost_metadata_lookup(&self) -> f64 {
        1.0 // O(1) constant cost
    }

    fn cost_covering_index_scan(&self, rows: u64) -> f64 {
        self.cost_index_scan(rows) * 0.3 // 70% faster (no heap fetch)
    }

    fn cost_materialized_view_scan(&self, rows: u64) -> f64 {
        self.cost_seq_scan(rows) // Scan MV instead of base tables
    }
}
```

---

## 11. Proposed RFCs

### RFC: Database-Specific Shortcut System

**Problem**: RA needs to generate database-competitive plans by leveraging shortcuts.

**Proposal**:
1. Extend `FactsProvider` with capability checks
2. Add 20+ shortcut rules (COUNT metadata, covering indexes, etc.)
3. Add staleness tracking system
4. Add `ApproximateQuery` annotation for AQP

**Phases**:
1. Phase 1: Metadata shortcuts (COUNT, MIN/MAX)
2. Phase 2: Materialized views
3. Phase 3: Approximate query processing
4. Phase 4: Bloom filters and advanced shortcuts

---

## 12. Summary Table

| Shortcut | Databases | Safety | RA Priority |
|----------|-----------|--------|-------------|
| COUNT(*) metadata | PostgreSQL, MongoDB, MySQL | Staleness-bounded | **High** |
| MIN/MAX index scan | All | Always safe | **High** |
| Covering index | All | Always safe | **High** |
| Materialized views | PostgreSQL, Oracle, SQL Server | Staleness-bounded | **High** |
| EXISTS → LIMIT 1 | All | Always safe | **High** |
| WHERE 1=0 → empty | All | Always safe | **Medium** |
| HyperLogLog COUNT DISTINCT | PostgreSQL (extension), Redshift, Snowflake | Accuracy-bounded | **Medium** |
| Result caching | Redshift, Snowflake | Staleness-bounded | **Medium** |
| Bloom filter joins | PostgreSQL, Oracle, Spark | Always safe | **Low** |
| Parquet predicate pushdown | DuckDB, Spark, Trino | Always safe | **Medium** |

---

## 13. Next Steps

1. **Implement high-priority shortcuts** (COUNT metadata, covering indexes, MIN/MAX)
2. **Add staleness tracking** to statistics system
3. **Extend FactsProvider** trait with capability checks
4. **Write tests** for each shortcut (verify cost reduction)
5. **Add preconditions** to rule files
6. **Create RFC** for shortcut system
7. **Benchmark** against PostgreSQL, MySQL, DuckDB

---

## References

- PostgreSQL: https://www.postgresql.org/docs/current/indexes-index-only-scans.html
- MongoDB: https://docs.mongodb.com/manual/reference/method/db.collection.estimatedDocumentCount/
- Oracle: https://docs.oracle.com/en/database/oracle/oracle-database/19/tgsql/query-optimizer-concepts.html
- SQL Server: https://docs.microsoft.com/en-us/sql/relational-databases/indexes/indexes
- DuckDB: https://duckdb.org/docs/guides/performance/indexing
- HyperLogLog: Flajolet et al. "HyperLogLog: the analysis of a near-optimal cardinality estimation algorithm" (2007)
- BlinkDB: Agarwal et al. "BlinkDB: Queries with Bounded Errors and Bounded Response Times on Very Large Data" (EuroSys 2013)
