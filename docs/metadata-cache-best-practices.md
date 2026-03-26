# Metadata Cache Best Practices

## Overview

This guide provides best practices for using and monitoring Ra's metadata cache with relcache invalidation tracking. Follow these recommendations to maximize cache efficiency and avoid common pitfalls.

## Configuration Best Practices

### Enable Debug Logging (Development Only)

```sql
-- Development/staging environment
SET ra_planner.log_decisions = on;

-- Verify invalidation events are logged
ALTER TABLE users ADD COLUMN last_login TIMESTAMP;
-- Check PostgreSQL logs for:
-- DEBUG:  Ra: invalidated metadata cache for relation OID 16384
-- DEBUG:  Ra: refreshed metadata cache for relation OID 16384 (10000 rows, 5 columns)

-- Production environment (disable for performance)
SET ra_planner.log_decisions = off;
```

**Why**: Debug logging adds overhead (~0.001ms per event) but provides visibility into cache behavior. Use in development to verify invalidation works, disable in production for maximum performance.

### Monitor Cache Statistics

Set up periodic monitoring:

```sql
-- Create monitoring view
CREATE VIEW ra_cache_health AS
SELECT
    entries,
    invalidated,
    hits,
    misses,
    invalidations,
    ROUND(hit_rate::numeric, 3) AS hit_rate,
    CASE
        WHEN hit_rate > 0.95 THEN 'Excellent'
        WHEN hit_rate > 0.80 THEN 'Good'
        WHEN hit_rate > 0.60 THEN 'Fair'
        WHEN hit_rate > 0.40 THEN 'Poor'
        ELSE 'Critical'
    END AS health_status,
    CASE
        WHEN entries > 950 THEN 'Approaching eviction limit'
        WHEN entries > 800 THEN 'High cache usage'
        WHEN entries > 500 THEN 'Normal cache usage'
        ELSE 'Low cache usage'
    END AS capacity_status
FROM ra.metadata_cache_stats();

-- Query periodically (every 5 minutes)
SELECT * FROM ra_cache_health;
```

### Alert Thresholds

Set up alerts for:

1. **Low hit rate** (< 60%): Cache thrashing or frequent DDL
2. **High capacity** (> 900 entries): Approaching eviction
3. **Zero invalidations**: Callback not working (restart PostgreSQL)
4. **High invalidation rate** (> 100/min): Excessive DDL

```sql
-- Alert query (integrate with monitoring system)
SELECT
    CASE
        WHEN hit_rate < 0.60 THEN 'ALERT: Low cache hit rate: ' || hit_rate
        WHEN entries > 900 THEN 'ALERT: Cache near capacity: ' || entries
        ELSE 'OK'
    END AS alert_status
FROM ra.metadata_cache_stats()
WHERE hit_rate < 0.60 OR entries > 900;
```

## Usage Patterns

### Pattern 1: Stable Schema (OLTP)

**Characteristics**:
- Fixed schema (rare DDL)
- Same tables queried repeatedly
- High query concurrency

**Best Practices**:
```sql
-- No special configuration needed
-- Cache naturally achieves 95%+ hit rate

-- Monitor cache health
SELECT entries, hit_rate FROM ra.metadata_cache_stats();
--  entries | hit_rate
-- ---------+----------
--       25 |    0.987

-- Expected behavior:
-- - Initial queries populate cache (misses)
-- - Subsequent queries hit cache (hits)
-- - DDL events trigger refresh (invalidations)
-- - Hit rate stabilizes at 95-99%
```

**Tuning**:
- No tuning needed
- Consider warming cache at startup (see below)

### Pattern 2: Dynamic Schema (Development)

**Characteristics**:
- Frequent DDL (migrations, experiments)
- Schema changes during testing
- Lower hit rate acceptable

**Best Practices**:
```sql
-- Enable debug logging to verify invalidation
SET ra_planner.log_decisions = on;

-- After schema changes, verify cache invalidated
ALTER TABLE users ADD COLUMN email TEXT;

-- Check invalidation logged
SELECT invalidations FROM ra.metadata_cache_stats();
-- Should increment after each DDL

-- Manual cache clear if needed
SELECT ra.clear_metadata_cache();

-- Expected behavior:
-- - Hit rate 40-60% (frequent invalidations)
-- - Invalidations counter grows rapidly
-- - Acceptable for development workload
```

**Tuning**:
- Clear cache periodically if hit rate drops below 40%
- Use connection pooling to reuse backends (cache persists)

### Pattern 3: Large Database (Many Tables)

**Characteristics**:
- Database has >1000 tables
- Queries access varied subset
- LRU eviction active

**Best Practices**:
```sql
-- Monitor cache capacity
SELECT entries, capacity_status FROM ra_cache_health;
--  entries | capacity_status
-- ---------+---------------------------
--      987 | Approaching eviction limit

-- Check eviction events (debug logging)
SET ra_planner.log_decisions = on;
-- Watch for:
-- DEBUG:  Ra: evicted 100 LRU entries from metadata cache

-- If cache thrashing (hit rate < 60%), increase MAX_CACHE_ENTRIES
-- Requires recompilation (see Advanced Tuning section)
```

**Tuning**:
- Increase `MAX_CACHE_ENTRIES` to 2000 or more
- Consider partitioning workload (separate databases)
- Use connection pooling to share cache across queries

### Pattern 4: Analytical Queries (OLAP)

**Characteristics**:
- Ad-hoc queries on many tables
- Different tables each query
- Lower hit rate expected

**Best Practices**:
```sql
-- Accept lower hit rate (60-80%)
-- Cache still beneficial (avoids repeated catalog queries)

SELECT
    entries,
    hits,
    misses,
    ROUND(hit_rate::numeric, 2) AS hit_rate
FROM ra.metadata_cache_stats();
--  entries | hits  | misses | hit_rate
-- ---------+-------+--------+----------
--      543 | 8,234 | 2,167  |     0.79

-- Expected behavior:
-- - Cache size grows to ~500-800 tables
-- - Hit rate stabilizes at 60-80%
-- - Misses occur for rarely-queried tables
```

**Tuning**:
- No tuning needed (cache working as designed)
- Consider warming cache for frequently-queried tables

## Cache Warming

### Manual Warming at Startup

Populate cache for critical tables after PostgreSQL restart:

```sql
-- Create warmup script
CREATE OR REPLACE FUNCTION ra.warmup_cache()
RETURNS void AS $$
BEGIN
    -- Query critical tables to populate cache
    PERFORM COUNT(*) FROM users;
    PERFORM COUNT(*) FROM orders;
    PERFORM COUNT(*) FROM products;
    PERFORM COUNT(*) FROM inventory;

    RAISE NOTICE 'Cache warmed: % entries', (
        SELECT entries FROM ra.metadata_cache_stats()
    );
END;
$$ LANGUAGE plpgsql;

-- Run after database startup or deployment
SELECT ra.warmup_cache();
-- NOTICE:  Cache warmed: 4 entries
```

### Automated Warming

Add to connection pool initialization:

```python
# Python example with psycopg2
import psycopg2

def warmup_connection(conn):
    """Warm up metadata cache for new connection."""
    with conn.cursor() as cur:
        cur.execute("SELECT ra.warmup_cache()")
    conn.commit()

# In connection pool setup
pool = psycopg2.pool.SimpleConnectionPool(
    minconn=10,
    maxconn=100,
    dsn="postgresql://localhost/mydb"
)

# Warm up each connection
for _ in range(10):
    conn = pool.getconn()
    warmup_connection(conn)
    pool.putconn(conn)
```

## Troubleshooting Scenarios

### Scenario 1: Cache Not Invalidating

**Symptoms**:
- Old metadata used after DDL
- `invalidations` counter stays at 0

**Diagnosis**:
```sql
-- Check if callback registered
SELECT invalidations FROM ra.metadata_cache_stats();
-- If 0 after DDL, callback not working

-- Execute DDL and verify
CREATE TABLE test_invalidation (id INT);
ANALYZE test_invalidation;
ALTER TABLE test_invalidation ADD COLUMN data TEXT;

SELECT invalidations FROM ra.metadata_cache_stats();
-- Should be > 0
```

**Solutions**:
1. Restart PostgreSQL to re-run `_PG_init()`
2. Verify Ra extension loaded: `SHOW shared_preload_libraries;`
3. Check PostgreSQL logs for initialization errors
4. Manual workaround: `SELECT ra.clear_metadata_cache();`

### Scenario 2: Low Hit Rate

**Symptoms**:
- Hit rate < 60% despite stable schema
- Many misses in cache stats

**Diagnosis**:
```sql
-- Check cache stats
SELECT * FROM ra.metadata_cache_stats();

-- Check if frequent evictions (debug logging)
SET ra_planner.log_decisions = on;
-- Watch for eviction messages

-- Check if cache size near limit
SELECT entries, capacity_status FROM ra_cache_health;
```

**Solutions**:
1. Increase `MAX_CACHE_ENTRIES` (requires recompilation)
2. Reduce number of queried tables (partition workload)
3. Use connection pooling (cache persists per backend)
4. Warm cache at startup for critical tables

### Scenario 3: Stale Metadata After DDL

**Symptoms**:
- Query uses old column count after ALTER TABLE
- Index recommendations for dropped index

**Diagnosis**:
```sql
-- Execute DDL
ALTER TABLE users ADD COLUMN email TEXT;

-- Query table
SELECT COUNT(*) FROM users WHERE id = 1;

-- Check if metadata refreshed
SELECT invalidations FROM ra.metadata_cache_stats();
-- Should increment
```

**Solutions**:
1. Verify invalidation counter incremented
2. If not, restart PostgreSQL (callback registration failed)
3. Manual workaround: `SELECT ra.clear_metadata_cache();`
4. Enable debug logging to verify refresh happened

### Scenario 4: High Memory Usage

**Symptoms**:
- PostgreSQL backend uses >10 MB for Ra extension
- Memory usage grows over time

**Diagnosis**:
```sql
-- Check cache size
SELECT entries FROM ra.metadata_cache_stats();
-- If > 1000, LRU eviction should be active

-- Check for memory leak (cache growing unbounded)
-- Query cache stats periodically and track entries
```

**Solutions**:
1. Reduce `MAX_CACHE_ENTRIES` (requires recompilation)
2. Periodic cache clear: `SELECT ra.clear_metadata_cache();`
3. Restart backend if memory leak suspected
4. Increase eviction frequency (modify `evict_lru()` threshold)

## Advanced Tuning

### Increase Cache Size

Edit `crates/ra-pg-extension/src/metadata_cache.rs`:

```rust
// Before (default)
const MAX_CACHE_ENTRIES: usize = 1000;

// After (for large databases)
const MAX_CACHE_ENTRIES: usize = 5000;
```

Recompile and install:
```bash
cargo pgrx install --release
```

Restart PostgreSQL:
```bash
pg_ctl restart
```

### Adjust Eviction Policy

Edit `metadata_cache.rs`:

```rust
fn evict_lru(&mut self) {
    // Before (evict 10%)
    let target_size = MAX_CACHE_ENTRIES * 9 / 10;

    // After (evict 20% for more aggressive eviction)
    let target_size = MAX_CACHE_ENTRIES * 8 / 10;

    // ... rest of function
}
```

### Use RwLock Instead of Mutex

For read-heavy workloads, replace `Mutex` with `RwLock`:

```rust
// Before
static METADATA_CACHE: Lazy<Mutex<MetadataCache>> = ...

// After (allows concurrent reads)
static METADATA_CACHE: Lazy<RwLock<MetadataCache>> = ...
```

Update methods to use `read()` for reads, `write()` for writes:

```rust
pub fn get_table_metadata(oid: Oid) -> Option<Statistics> {
    // Try read lock first
    if let Ok(cache) = METADATA_CACHE.read() {
        if let Some(stats) = cache.get(oid) {
            return Some(stats);
        }
    }

    // Upgrade to write lock for refresh
    METADATA_CACHE
        .write()
        .ok()?
        .get_or_refresh(oid)
}
```

## Integration with Monitoring Systems

### Prometheus Exporter

Export cache metrics as Prometheus metrics:

```sql
-- Create metrics view
CREATE VIEW ra_cache_metrics_prometheus AS
SELECT
    'ra_cache_entries' AS metric,
    entries::text AS value,
    'gauge' AS type
FROM ra.metadata_cache_stats()
UNION ALL
SELECT
    'ra_cache_hit_rate',
    hit_rate::text,
    'gauge'
FROM ra.metadata_cache_stats()
UNION ALL
SELECT
    'ra_cache_hits_total',
    hits::text,
    'counter'
FROM ra.metadata_cache_stats()
UNION ALL
SELECT
    'ra_cache_misses_total',
    misses::text,
    'counter'
FROM ra.metadata_cache_stats()
UNION ALL
SELECT
    'ra_cache_invalidations_total',
    invalidations::text,
    'counter'
FROM ra.metadata_cache_stats();

-- Query from prometheus postgres_exporter
-- queries:
--   ra_cache_metrics:
--     query: "SELECT * FROM ra_cache_metrics_prometheus"
--     metrics:
--       - metric:
--           usage: "LABEL"
--       - value:
--           usage: "GAUGE"
```

### Grafana Dashboard

Example Grafana queries:

```promql
# Cache hit rate
ra_cache_hit_rate

# Cache entries over time
ra_cache_entries

# Cache misses rate (per second)
rate(ra_cache_misses_total[5m])

# Invalidations rate (per minute)
rate(ra_cache_invalidations_total[1m]) * 60
```

Alert rules:

```yaml
groups:
  - name: ra_cache_alerts
    rules:
      - alert: RaCacheLowHitRate
        expr: ra_cache_hit_rate < 0.6
        for: 10m
        annotations:
          summary: "Ra cache hit rate below 60%"

      - alert: RaCacheHighCapacity
        expr: ra_cache_entries > 900
        for: 5m
        annotations:
          summary: "Ra cache approaching eviction limit"
```

## Performance Benchmarking

### Measure Cache Impact

Compare query latency with and without cache:

```sql
-- Clear cache
SELECT ra.clear_metadata_cache();

-- Measure cold cache (first query)
\timing on
SELECT COUNT(*) FROM large_table WHERE id < 1000;
-- Time: 45.234 ms

-- Measure warm cache (second query)
SELECT COUNT(*) FROM large_table WHERE id < 1000;
-- Time: 45.012 ms (cache hit adds ~0.01ms)

-- Check cache stats
SELECT hits, misses FROM ra.metadata_cache_stats();
--  hits | misses
-- ------+--------
--     1 |      1
```

### Benchmark Cache Operations

```sql
-- Benchmark invalidation callback overhead
CREATE OR REPLACE FUNCTION benchmark_invalidation(n INT)
RETURNS TABLE(invalidations_before BIGINT, invalidations_after BIGINT, time_ms NUMERIC) AS $$
DECLARE
    start_time TIMESTAMP;
    end_time TIMESTAMP;
    inv_before BIGINT;
    inv_after BIGINT;
BEGIN
    SELECT metadata_cache_stats.invalidations INTO inv_before
    FROM ra.metadata_cache_stats();

    start_time := clock_timestamp();

    FOR i IN 1..n LOOP
        -- Trigger invalidation
        EXECUTE 'ALTER TABLE users ADD COLUMN temp_' || i || ' INT';
        EXECUTE 'ALTER TABLE users DROP COLUMN temp_' || i;
    END LOOP;

    end_time := clock_timestamp();

    SELECT metadata_cache_stats.invalidations INTO inv_after
    FROM ra.metadata_cache_stats();

    RETURN QUERY SELECT
        inv_before,
        inv_after,
        EXTRACT(milliseconds FROM (end_time - start_time))::NUMERIC;
END;
$$ LANGUAGE plpgsql;

-- Run benchmark
SELECT * FROM benchmark_invalidation(100);
--  invalidations_before | invalidations_after | time_ms
-- ----------------------+---------------------+---------
--                     5 |                 205 |  123.45

-- Avg time per invalidation: 123.45ms / 100 = 1.23ms
-- (mostly DDL overhead, callback is <0.001ms)
```

## Best Practices Summary

### Do

✅ Monitor cache statistics regularly
✅ Enable debug logging in development
✅ Warm cache at startup for critical tables
✅ Use connection pooling to reuse backends
✅ Clear cache if hit rate drops below 40%
✅ Increase `MAX_CACHE_ENTRIES` for large databases
✅ Set up alerts for low hit rate and high capacity

### Don't

❌ Don't disable invalidation tracking (no config to disable)
❌ Don't ignore low hit rate (<60%) in production
❌ Don't manually clear cache in hot path (expensive)
❌ Don't assume cache is shared across backends
❌ Don't rely on cache for authorization checks
❌ Don't use debug logging in production (performance)
❌ Don't panic if hit rate is low during development (expected)

## Conclusion

The metadata cache with relcache invalidation tracking provides automatic, transparent performance improvements for most workloads. Follow these best practices to maximize cache efficiency and avoid common pitfalls. Monitor cache statistics regularly to ensure optimal performance.

For questions or issues, consult:
- RFC 0083: Design rationale and implementation details
- METADATA_CACHE.md: Comprehensive user documentation
- relcache-invalidation-architecture.md: System architecture diagrams
