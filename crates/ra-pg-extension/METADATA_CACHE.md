# Metadata Cache with Relcache Invalidation Tracking

## Overview

The Ra PostgreSQL extension now includes automatic metadata cache invalidation tracking. When PostgreSQL detects schema changes (ALTER TABLE, CREATE/DROP INDEX, ANALYZE, etc.), Ra automatically refreshes its cached table metadata to ensure optimization decisions use current schema information.

## Architecture

```
PostgreSQL DDL Command
        |
        v
    Relcache Invalidated
        |
        v
CacheRegisterRelcacheCallback()
        |
        v
ra_relcache_callback(relid)  [C FFI]
        |
        v
ra_rust_invalidate_table(relid)  [Rust]
        |
        v
METADATA_CACHE.invalidate(relid)
        |
        v
Mark metadata as stale (is_valid = false)
        |
        v
Next query on that table
        |
        v
Lazy refresh from pg_catalog
        |
        v
Updated metadata used for optimization
```

## Implementation Files

### Core Implementation

1. **`src/metadata_cache.rs`** (New)
   - `MetadataCache` struct with invalidation tracking
   - Global cache protected by Mutex
   - LRU eviction when cache exceeds 1000 entries
   - Cache statistics (hits, misses, invalidations, hit rate)
   - Public API: `get_table_metadata()`, `clear_cache()`, `get_cache_stats()`

2. **`src/stats_bridge.rs`** (Extended)
   - Added `gather_table_stats_by_oid()` for OID-based queries
   - Added `gather_index_stats_by_oid()` for OID-based index stats
   - Supports metadata refresh without schema/table name resolution

3. **`src/lib.rs`** (Extended)
   - Added `metadata_cache` module
   - Registered relcache callback in `_PG_init()`
   - Added `ra_relcache_callback()` C FFI callback
   - Added SQL functions: `clear_metadata_cache()`, `metadata_cache_stats()`

4. **`Cargo.toml`** (Extended)
   - Added `once_cell = "1.20"` dependency for `Lazy` static initialization

### Tests

5. **`tests/test_metadata_cache.sql`** (New)
   - Integration tests for relcache invalidation
   - Tests: ALTER TABLE, CREATE INDEX, DROP INDEX, ANALYZE, partition changes
   - Tests cache clear and repopulation
   - Tests multiple tables cached simultaneously

6. **`src/integration_tests.rs`** (Extended)
   - Added Rust-based integration tests
   - Tests: invalidation on ALTER TABLE, CREATE INDEX, DROP INDEX, ANALYZE
   - Tests manual cache clear and repopulation
   - Tests cache hit rate calculation

## SQL Functions

### `ra.clear_metadata_cache()`

Manually clear all cached metadata. Forces refresh on next query.

```sql
SELECT ra.clear_metadata_cache();
```

### `ra.metadata_cache_stats()`

Get cache statistics for monitoring and observability.

```sql
SELECT * FROM ra.metadata_cache_stats();
```

Returns:
- `entries`: Number of tables currently cached
- `invalidated`: Number of tables pending refresh
- `hits`: Total cache hits since process start
- `misses`: Total cache misses since process start
- `invalidations`: Total invalidations received
- `hit_rate`: Cache hit rate (hits / (hits + misses))

## Usage Examples

### Basic Usage (Automatic)

No manual intervention required. Ra automatically detects schema changes:

```sql
-- Create table
CREATE TABLE users (id INT, name TEXT);
CREATE INDEX idx_users_id ON users(id);
ANALYZE users;

-- Ra caches metadata on first query
SELECT * FROM users WHERE id = 1;

-- Schema change: add column
ALTER TABLE users ADD COLUMN email TEXT;

-- Ra automatically detects invalidation and refreshes metadata
SELECT * FROM users WHERE id = 1;
-- Uses updated metadata (3 columns instead of 2)
```

### Monitoring Cache Performance

```sql
-- Check cache statistics
SELECT * FROM ra.metadata_cache_stats();

--  entries | invalidated | hits | misses | invalidations | hit_rate
-- ---------+-------------+------+--------+---------------+----------
--       15 |           0 |  234 |     18 |            12 |     0.93
```

### Manual Cache Management

```sql
-- Clear cache (useful for testing or troubleshooting)
SELECT ra.clear_metadata_cache();

-- Verify cache is empty
SELECT entries FROM ra.metadata_cache_stats();
-- returns 0

-- Next query repopulates cache
SELECT COUNT(*) FROM users;

-- Cache now has entry
SELECT entries FROM ra.metadata_cache_stats();
-- returns 1
```

### Debug Logging

Enable detailed logging to see invalidation events:

```sql
-- Enable debug logging
SET ra_planner.log_decisions = on;

-- Trigger invalidation
ALTER TABLE users ADD COLUMN created_at TIMESTAMP;

-- Check PostgreSQL logs for Ra messages:
-- DEBUG:  Ra: invalidated metadata cache for relation OID 16384
-- DEBUG:  Ra: refreshed metadata cache for relation OID 16384 (1000 rows, 4 columns)
```

## Performance Characteristics

### Cache Hit (Fast Path)

- **Latency**: ~0.01ms (mutex lock + HashMap lookup)
- **Syscalls**: 0 (no PostgreSQL catalog access)
- **Best for**: Queries on unchanged tables

### Cache Miss (Slow Path)

- **Latency**: ~0.2ms for 10-column table
- **Syscalls**: 1 + N (pg_class + N × pg_statistic for N columns)
- **Occurs when**:
  - First query on a table
  - Table metadata invalidated (DDL, ANALYZE)
  - Cache eviction (LRU)

### Memory Usage

- **Per-table**: ~1KB (Statistics struct + metadata)
- **Max cache**: 1000 tables = ~1MB
- **Eviction**: LRU evicts 10% oldest entries when limit reached

### Typical Hit Rates

- **OLTP workload** (frequent queries, infrequent DDL): 95-99% hit rate
- **OLAP workload** (many tables, ad-hoc queries): 60-80% hit rate
- **Development** (frequent schema changes): 40-60% hit rate

## Relcache Invalidation Events

PostgreSQL invalidates the relcache for:

1. **Schema Changes**
   - `ALTER TABLE` (add/drop/modify columns)
   - `ALTER TABLE` (constraints, defaults, NOT NULL)
   - `ALTER TYPE` (affects columns using that type)

2. **Index Changes**
   - `CREATE INDEX`
   - `DROP INDEX`
   - `ALTER INDEX` (SET STATISTICS, etc.)
   - `REINDEX`

3. **Statistics Updates**
   - `ANALYZE`
   - Autovacuum ANALYZE
   - `ALTER TABLE SET STATISTICS`

4. **Storage Changes**
   - `VACUUM FULL`
   - `CLUSTER`
   - `TRUNCATE`

5. **Partition Changes**
   - `CREATE TABLE ... PARTITION OF`
   - `ALTER TABLE ATTACH PARTITION`
   - `ALTER TABLE DETACH PARTITION`

6. **System Events**
   - Extension load/unload
   - Cache overflow (PostgreSQL internal)

## Limitations

### Current Limitations

1. **Process-local cache**: Each PostgreSQL backend has its own cache. Connection poolers with multiple backends don't share cache.

2. **No distributed invalidation**: Standby replicas don't receive invalidation callbacks (read-only).

3. **Coarse invalidation**: Entire table metadata is invalidated even for minor changes (adding a CHECK constraint invalidates all statistics).

4. **Memory unbounded (with limit)**: Cache grows to 1000 tables then uses LRU eviction. Large databases (>1000 tables) may see cache thrashing.

### Future Improvements

1. **Shared memory cache**: Use PostgreSQL shared memory to share cache across backends.

2. **Column-level invalidation**: Track which columns changed, refresh only affected statistics.

3. **Proactive refresh**: Background worker periodically refreshes high-traffic tables.

4. **Distributed caching**: Synchronize metadata across connection pooler or replicas.

## Troubleshooting

### Cache not invalidating

**Symptom**: Old metadata used after schema change.

**Check**:
```sql
SELECT invalidations FROM ra.metadata_cache_stats();
```

If invalidations = 0 after DDL, callback registration failed.

**Solution**: Restart PostgreSQL to re-run `_PG_init()`.

### Low hit rate

**Symptom**: `hit_rate < 0.5` in `metadata_cache_stats()`.

**Causes**:
- Frequent schema changes (development environment)
- Large database with >1000 tables (cache thrashing)
- Workload queries many different tables

**Solutions**:
- Increase `MAX_CACHE_ENTRIES` in `metadata_cache.rs`
- Reduce DDL frequency (use migrations)
- Use connection pooling to reuse backends

### High memory usage

**Symptom**: Ra extension using >10MB per backend.

**Check**:
```sql
SELECT entries FROM ra.metadata_cache_stats();
```

If entries > 1000, LRU eviction is active but memory still high.

**Solution**: Clear cache periodically:
```sql
SELECT ra.clear_metadata_cache();
```

Or reduce `MAX_CACHE_ENTRIES` and recompile.

## Testing

### Run Integration Tests

```bash
# SQL-based tests
cargo pgrx test pg17 --features pg_test -- tests/test_metadata_cache.sql

# Rust-based tests
cargo pgrx test pg17 --features pg_test -- test_metadata_cache
```

### Manual Testing

```sql
-- Enable debug logging
SET ra_planner.log_decisions = on;

-- Create test table
CREATE TABLE test_invalidation (id INT, data TEXT);
INSERT INTO test_invalidation SELECT i, 'data-' || i FROM generate_series(1, 100) i;
ANALYZE test_invalidation;

-- Populate cache
SELECT COUNT(*) FROM test_invalidation;

-- Check cache stats
SELECT * FROM ra.metadata_cache_stats();

-- Trigger invalidation
ALTER TABLE test_invalidation ADD COLUMN created_at TIMESTAMP;

-- Verify invalidation logged
-- Check PostgreSQL logs for:
-- DEBUG:  Ra: invalidated metadata cache for relation OID <oid>

-- Next query refreshes
SELECT COUNT(*) FROM test_invalidation;

-- Check cache stats (invalidations should increment)
SELECT * FROM ra.metadata_cache_stats();
```

## References

- RFC 0083: PostgreSQL Extension Metadata Synchronization (`rfcs/0083-relcache-invalidation-tracking.md`)
- PostgreSQL Documentation: [System Catalogs](https://www.postgresql.org/docs/current/catalogs.html)
- PostgreSQL Source: `src/backend/utils/cache/inval.c` (relcache invalidation implementation)
