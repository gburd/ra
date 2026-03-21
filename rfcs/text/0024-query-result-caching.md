# RFC 0024: Query Result Caching

- Start Date: 2026-03-21
- Author: RA Contributors
- Status: Draft
- Tracking Issue: TBD

## Summary

Implement a multi-level query result caching system that intelligently caches and reuses query results, subquery results, and intermediate computations to accelerate repeated and similar queries.

## Motivation

Many workloads exhibit temporal locality with repeated queries or queries with overlapping computations:
- Dashboard queries refreshing every few seconds
- Pagination queries with LIMIT/OFFSET
- Drill-down analytics with progressive filtering
- Reports with common subqueries

A smart caching layer can serve these from memory, reducing CPU usage and latency by 10-1000x.

## Guide-level explanation

### Automatic Caching

```sql
-- Enable result caching
SET enable_result_cache = true;
SET result_cache_ttl = '5 minutes';

-- First execution: 500ms
SELECT category, SUM(sales) FROM orders
WHERE date >= '2024-01-01'
GROUP BY category;

-- Second execution: 1ms (from cache)
SELECT category, SUM(sales) FROM orders
WHERE date >= '2024-01-01'
GROUP BY category;
```

### Explicit Cache Control

```sql
-- Force cache refresh
SELECT /*+ NO_CACHE */ * FROM expensive_view;

-- Cache with custom TTL
SELECT /*+ CACHE_TTL('1 hour') */ * FROM stable_dimension;

-- Named cache entries
SELECT /*+ CACHE_NAME('daily_revenue') */ ...;
```

## Reference-level explanation

### Cache Levels

1. **Result Cache**: Complete query results
2. **Subquery Cache**: Common Table Expressions and subqueries
3. **Operator Cache**: Expensive operations (sorts, aggregations)
4. **Semantic Cache**: Logically equivalent queries

### Cache Key Generation

Cache keys derived from:
- Normalized query AST (ignoring whitespace, capitalization)
- Parameter values for prepared statements
- Current schema version
- Session settings affecting results
- Table versions/timestamps

### Invalidation Strategies

1. **TTL-based**: Expire after configured duration
2. **Version-based**: Invalidate when underlying tables change
3. **Size-based**: LRU eviction when cache exceeds memory limit
4. **Dependency-based**: Cascade invalidation through dependency graph

### Semantic Caching

Recognize and reuse results from semantically equivalent queries:

```sql
-- These queries share the same result
SELECT * FROM users WHERE age >= 18 AND age <= 65;
SELECT * FROM users WHERE age BETWEEN 18 AND 65;

-- Subset relationships
SELECT * FROM orders WHERE amount > 100;  -- Can be served from:
SELECT * FROM orders;                     -- This broader cached result
```

### Implementation

```rust
pub struct QueryCache {
    result_cache: LruCache<CacheKey, CachedResult>,
    semantic_index: SemanticIndex,
    invalidation_graph: DependencyGraph,
}

pub struct CachedResult {
    data: Arc<RecordBatch>,
    metadata: QueryMetadata,
    created_at: Instant,
    ttl: Duration,
    hit_count: AtomicUsize,
}
```

## Drawbacks

- Memory overhead for cached results
- Cache invalidation complexity
- May serve stale data if invalidation is imperfect
- Increased planning time for cache lookup
- Cache thrashing on frequently updated tables

## Rationale and alternatives

### Why This Design?

- Transparent to applications
- Significant performance gains for read-heavy workloads
- Proven approach in cloud databases

### Alternative Approaches

1. **Application-level caching**: Redis/Memcached (requires app changes)
2. **Materialized views**: More predictable but less flexible
3. **Column stores**: Better compression but doesn't help with computation

## Prior art

- **Oracle**: Result cache with automatic invalidation
- **MySQL**: Query cache (deprecated due to invalidation issues)
- **Snowflake**: Automatic result caching for 24 hours
- **BigQuery**: Query results cached for 24 hours
- **Pinot**: Broker-level result caching
- **Presto**: Fragment result caching

## Unresolved questions

- How to handle non-deterministic functions?
- Cache sharing across users with different permissions?
- Distributed cache coordination?
- Compression strategy for cached results?

## Future possibilities

- Predictive caching based on query patterns
- Cross-query result sharing and assembly
- Persistent cache across restarts
- Cache warming from query logs
- Cost-based cache admission policies
- Integration with CDN for geo-distributed caching