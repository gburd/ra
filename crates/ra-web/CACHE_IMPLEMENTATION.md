# Redis Result Caching Implementation

## Overview

The `/api/explain` endpoint now includes Redis-based caching to improve performance for repeated EXPLAIN queries. Cached results are stored for 1 hour and keyed by a SHA256 hash of the SQL query, engine, and analyze flag.

## Implementation Details

### Cache Module (`src/cache.rs`)

The cache module provides two main functions:

- `get_cached_plan()` - Retrieves a cached plan from Redis
- `cache_plan()` - Stores a plan result in Redis cache

#### Cache Key Generation

Cache keys are generated using SHA256 hashing:

```rust
fn generate_cache_key(sql: &str, engine: &str, analyze: bool) -> String {
    let mut hasher = Sha256::new();
    hasher.update(sql.as_bytes());
    hasher.update(engine.as_bytes());
    hasher.update(if analyze { b"analyze" } else { b"noanalyze" });
    let hash = hasher.finalize();
    format!("explain:{:x}", hash)
}
```

This ensures that:
- Different SQL queries have different cache keys
- Same query on different engines has different cache keys
- Same query with/without ANALYZE has different cache keys

#### Cache TTL

Cached results expire after **1 hour** (3600 seconds). This can be adjusted by modifying the `CACHE_TTL` constant in `src/cache.rs`.

### Explain Endpoint Integration (`src/api/explain.rs`)

The explain endpoint follows this flow:

1. **Check cache** - Before executing the query, check if a cached result exists
2. **Return cached result** - If cache hit, return immediately with `execution_time_ms: 0.0`
3. **Execute query** - If cache miss, execute the EXPLAIN query
4. **Store result** - Cache the result for future requests
5. **Return result** - Return the fresh result with actual execution time

#### Cache Hit Response

When a cached result is returned:
```json
{
  "plan": { ... },
  "engine": "postgresql",
  "execution_time_ms": 0.0
}
```

The `execution_time_ms: 0.0` indicates this was a cache hit.

#### Cache Miss Response

When a fresh result is returned:
```json
{
  "plan": { ... },
  "engine": "postgresql",
  "execution_time_ms": 125.43
}
```

The non-zero `execution_time_ms` indicates this was a cache miss and the query was executed.

## Configuration

### Environment Variables

- `REDIS_URL` - Redis connection URL (default: `redis://127.0.0.1:6379`)

### Redis Connection

The Redis connection is initialized in `src/main.rs` using `ConnectionManager` for automatic reconnection handling:

```rust
async fn init_redis() -> redis::aio::ConnectionManager {
    let redis_url = std::env::var("REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

    let client = redis::Client::open(redis_url)
        .expect("Failed to create Redis client");

    redis::aio::ConnectionManager::new(client)
        .await
        .expect("Failed to connect to Redis")
}
```

The connection manager is passed to endpoints via Rocket's state management.

## Testing

### Manual Testing

Use the provided test script to verify caching behavior:

```bash
cd crates/ra-web
./test_cache.sh
```

The script tests:
1. First request (cache miss)
2. Second identical request (cache hit, `execution_time_ms: 0`)
3. Different request (cache miss)
4. Same query with different analyze flag (cache miss)

### Expected Behavior

Cache hits should:
- Return results instantly (< 10ms total request time)
- Have `execution_time_ms: 0.0` in the response
- Log "Cache hit" message in server logs

Cache misses should:
- Take normal execution time (varies by query complexity)
- Have non-zero `execution_time_ms` in the response
- Log "Cache miss" message in server logs
- Store result for future requests

### Viewing Cache Contents

You can inspect the Redis cache using `redis-cli`:

```bash
# List all cache keys
redis-cli KEYS "explain:*"

# View a specific cached value
redis-cli GET "explain:<hash>"

# Check TTL for a key
redis-cli TTL "explain:<hash>"

# Clear all cache entries
redis-cli KEYS "explain:*" | xargs redis-cli DEL
```

## Performance Impact

### Cache Hit Performance

- **Before caching**: 50-500ms (depending on query complexity and database)
- **After caching**: < 10ms (cache lookup + network overhead)

### Expected Cache Hit Ratio

Cache effectiveness depends on usage patterns:
- **High hit ratio** (70-90%): Dashboards, reports with repeated queries
- **Low hit ratio** (10-30%): Ad-hoc query exploration, unique queries

### Memory Usage

Each cached entry stores:
- Cache key: ~80 bytes (SHA256 hash + prefix)
- Cached value: Varies by plan size, typically 1-50 KB
- TTL metadata: ~8 bytes

Estimated memory usage:
- 1,000 cached queries: ~10-50 MB
- 10,000 cached queries: ~100-500 MB

Redis automatically evicts entries after the 1-hour TTL.

## Error Handling

The caching layer is designed to be non-blocking:

- **Cache read errors** - Silently fall through to query execution
- **Cache write errors** - Log warning but return result normally
- **Redis connection errors** - Application continues without caching

This ensures that Redis failures don't break the explain endpoint.

## Monitoring

### Logs

Cache operations are logged at these levels:

- `INFO` - Cache hits, successful cache writes
- `DEBUG` - Cache misses
- `WARN` - Cache serialization/deserialization errors, write failures
- `ERROR` - Redis connection failures

Example log output:
```
INFO  Cache hit for key=explain:a3f2b1...
DEBUG Cache miss for key=explain:9d8c7b...
INFO  Cached plan with key=explain:9d8c7b..., ttl=3600s
WARN  Failed to cache EXPLAIN result: connection refused
```

### Metrics to Monitor

When running in production, monitor:
1. Cache hit rate (percentage of requests served from cache)
2. Average response time (should be bimodal: fast for hits, slower for misses)
3. Redis memory usage
4. Cache eviction rate

## Dependencies

New dependencies added:
- `sha2 = "0.10"` - SHA256 hashing for cache keys

Existing dependencies used:
- `redis = { version = "0.27", features = ["tokio-comp", "connection-manager"] }` - Redis client
- `serde` / `serde_json` - Serialization for cached values

## Future Improvements

Potential enhancements:
1. **Configurable TTL** - Allow per-query or per-engine TTL configuration
2. **Cache warming** - Pre-populate cache with common queries
3. **Smart invalidation** - Invalidate cache when schema changes
4. **Metrics endpoint** - Expose cache hit rate and statistics
5. **Cache size limits** - Implement LRU eviction for large result sets
6. **Compression** - Compress large plans before caching
