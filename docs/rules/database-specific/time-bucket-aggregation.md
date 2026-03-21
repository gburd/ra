# Rule: Time-Bucket Aggregation (TimescaleDB)

**Category:** database-specific/timescaledb
**File:** `rules/database-specific/timescaledb/time-bucket-aggregation.rra`

## Metadata

- **ID:** `timescaledb-time-bucket-aggregation`
- **Version:** "1.0.0"
- **Databases:** timescaledb
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Time-Bucket Aggregation (TimescaleDB)

## Metadata
- **Rule ID**: `timescaledb-time-bucket-aggregation`
- **Category**: Database-Specific / TimescaleDB
- **Source**: TimescaleDB (PostgreSQL extension)
- **Docs**: https://docs.timescale.com/

## Description

TimescaleDB's `time_bucket()` function optimizes time-series aggregations by leveraging chunk-based storage and parallel aggregation across time partitions.

## Test Cases

### Test 1: Time-series downsampling
```sql
SELECT time_bucket('1 hour', time) AS hour,
       AVG(temperature),
       MAX(humidity)
FROM sensor_data
WHERE time >= NOW() - INTERVAL '7 days'
GROUP BY hour
ORDER BY hour;

-- Optimization:
-- 1. Chunk exclusion: Skip chunks outside 7-day window
-- 2. Parallel aggregation: Each chunk aggregated in parallel
-- 3. Partial aggregates combined
```

## References
1. **TimescaleDB Docs**: "time_bucket() function"

## Tags
`database-specific`, `timescaledb`, `time-series`, `aggregation`, `chunks`
