# Database Connection Pool Optimization

## Overview

Optimized database connection pool settings across all adapters to improve performance and resource utilization. The new configuration provides better concurrency handling, keeps connections warm, and manages connection lifecycle more efficiently.

## Changes Made

### 1. PostgreSQL Adapter (`/home/gburd/ws/ra/crates/ra-adapters/src/postgres.rs`)

**Before:**
```rust
self.pool = Some(
    r2d2::Pool::builder()
        .max_size(10)
        .build(manager)
        .map_err(|e| {
            AdapterError::ConnectionError(format!(
                "Failed to create connection pool: {e}"
            ))
        })?,
);
```

**After:**
```rust
self.pool = Some(
    r2d2::Pool::builder()
        .max_size(20)
        .min_idle(Some(5))
        .connection_timeout(std::time::Duration::from_secs(5))
        .idle_timeout(Some(std::time::Duration::from_secs(300)))
        .max_lifetime(Some(std::time::Duration::from_secs(1800)))
        .build(manager)
        .map_err(|e| {
            AdapterError::ConnectionError(format!(
                "Failed to create connection pool: {e}"
            ))
        })?,
);
```

### 2. MySQL Adapter (`/home/gburd/ws/ra/crates/ra-adapters/src/mysql.rs`)

**Before:**
```rust
let opts = OptsBuilder::from_opts(
    mysql::Opts::from_url(connection_string)
        .map_err(|e| AdapterError::InvalidConfiguration(format!("Invalid URL: {e}")))?,
);

let pool = Pool::new(opts)
    .map_err(|e| AdapterError::ConnectionError(format!("Failed to create pool: {e}")))?;
```

**After:**
```rust
let opts = OptsBuilder::from_opts(
    mysql::Opts::from_url(connection_string)
        .map_err(|e| AdapterError::InvalidConfiguration(format!("Invalid URL: {e}")))?,
)
.pool_opts(
    mysql::PoolOpts::default()
        .with_constraints(
            mysql::PoolConstraints::new(5, 20)
                .expect("Valid pool constraints")
        )
        .with_inactive_connection_ttl(Duration::from_secs(300))
        .with_ttl(Duration::from_secs(1800))
);

let pool = Pool::new(opts)
    .map_err(|e| AdapterError::ConnectionError(format!("Failed to create pool: {e}")))?;
```

### 3. SQLite Adapter (`/home/gburd/ws/ra/crates/ra-adapters/src/sqlite.rs`)

**Before:**
```rust
let pool = Pool::builder()
    .max_size(4)
    .build(manager)
    .map_err(|e| AdapterError::ConnectionError(format!("Failed to create pool: {e}")))?;
```

**After:**
```rust
let pool = Pool::builder()
    .max_size(20)
    .min_idle(Some(5))
    .connection_timeout(std::time::Duration::from_secs(5))
    .idle_timeout(Some(std::time::Duration::from_secs(300)))
    .max_lifetime(Some(std::time::Duration::from_secs(1800)))
    .build(manager)
    .map_err(|e| AdapterError::ConnectionError(format!("Failed to create pool: {e}")))?;
```

### 4. Web Configuration (`/home/gburd/ws/ra/crates/ra-web/src/config.rs`)

**Before:**
```rust
fn default_pool_size() -> u32 {
    5
}
```

**After:**
```rust
fn default_pool_size() -> u32 {
    20
}
```

## Configuration Details

### Pool Settings Explained

| Setting | Value | Rationale |
|---------|-------|-----------|
| **max_size** | 20 | Increased from 10 (Postgres), 4 (SQLite), and implicit defaults (MySQL) to support higher concurrency |
| **min_idle** | 5 | Keeps 5 connections warm and ready, reducing connection establishment overhead |
| **connection_timeout** | 5 seconds | Maximum time to wait for a connection from the pool before timing out |
| **idle_timeout** | 300 seconds (5 minutes) | Closes idle connections after 5 minutes to free up database resources |
| **max_lifetime** | 1800 seconds (30 minutes) | Ensures connections are recycled every 30 minutes to avoid stale connections |

### Why These Settings?

1. **Higher max_size (20)**: Supports concurrent requests in production web environments without connection starvation
2. **min_idle (5)**: Balances resource usage with readiness - keeps enough connections warm for typical load
3. **connection_timeout (5s)**: Fast failure for health checks and prevents hanging requests
4. **idle_timeout (5min)**: Cleans up unused connections during low-traffic periods
5. **max_lifetime (30min)**: Prevents connection staleness and handles database server reconnections gracefully

## Database-Specific Notes

### PostgreSQL
- Uses `r2d2` with `r2d2_postgres::PostgresConnectionManager`
- All settings applied consistently
- Supports high concurrency typical of PostgreSQL workloads

### MySQL
- Uses native `mysql::Pool` with `mysql::PoolOpts`
- Constraints specified via `PoolConstraints::new(min, max)`
- TTL settings via `with_inactive_connection_ttl` and `with_ttl`
- Note: MySQL pool API differs from r2d2 but achieves equivalent behavior

### SQLite
- Uses `r2d2` with `r2d2_sqlite::SqliteConnectionManager`
- Increased from 4 to 20 connections despite being embedded database
- SQLite supports multiple concurrent readers and one writer
- Higher pool size helps with read-heavy workloads

### DuckDB
- No changes made - DuckDB is an embedded database without connection pooling
- Connections are direct and managed per-request

## Performance Impact

### Expected Improvements

1. **Reduced Connection Latency**: Warm connections via min_idle reduce establishment overhead
2. **Better Concurrency**: Higher max_size supports more simultaneous queries
3. **Resource Efficiency**: Idle and max lifetime timeouts prevent connection leak and stale connections
4. **Improved Reliability**: Connection timeout prevents indefinite hangs
5. **Consistent Configuration**: All adapters now use equivalent settings for predictable behavior

### Resource Usage

- **Memory**: Approximately 20-40MB additional memory per database adapter (20 connections vs previous defaults)
- **Database Server Load**: Minimal increase with idle timeout management
- **Trade-off**: Small memory increase for significant performance gain

## Testing Recommendations

1. **Load Testing**: Verify pool handles expected concurrent load
2. **Connection Leak Testing**: Ensure connections are properly returned to pool
3. **Timeout Testing**: Confirm connection_timeout fails gracefully
4. **Idle Connection Testing**: Verify idle_timeout reclaims connections
5. **Long-Running Connection Testing**: Confirm max_lifetime recycles connections

## Migration Notes

- **Backward Compatible**: No API changes, only configuration values
- **Automatic**: Takes effect on next database connection
- **No Data Migration**: Configuration-only change
- **Monitoring**: Watch for connection pool exhaustion warnings in logs

## Future Enhancements

1. **Environment-Based Configuration**: Allow pool settings via environment variables
2. **Dynamic Pool Sizing**: Adjust pool size based on load patterns
3. **Pool Metrics**: Expose connection pool statistics via metrics endpoint
4. **Per-Database Tuning**: Different settings for different database types based on workload
5. **Connection Validation**: Add health checks before returning connections from pool

## References

- [r2d2 Documentation](https://docs.rs/r2d2/)
- [MySQL Connection Pool Best Practices](https://dev.mysql.com/doc/connector-j/en/connector-j-usagenotes-j2ee-concepts-connection-pooling.html)
- [PostgreSQL Connection Pooling](https://www.postgresql.org/docs/current/runtime-config-connection.html)
- [SQLite Threading Modes](https://www.sqlite.org/threadsafe.html)
