# Rule: Chunk Pruning (TimescaleDB)

**Category:** database-specific/timescaledb
**File:** `rules/database-specific/timescaledb/chunk-pruning.rra`

## Metadata

- **ID:** `timescaledb-chunk-pruning`
- **Version:** "1.0.0"
- **Databases:** timescaledb
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Chunk Pruning (TimescaleDB)

## Metadata
- **Rule ID**: `timescaledb-chunk-pruning`
- **Category**: Database-Specific / TimescaleDB
- **Source**: TimescaleDB

## Description

TimescaleDB automatically prunes (skips) chunks based on time constraints, similar to partition pruning but optimized for time-series workloads.

## Test Cases

### Test 1: Time-based pruning
```sql
CREATE TABLE metrics (
    time TIMESTAMPTZ NOT NULL,
    device_id INT,
    value DOUBLE PRECISION
);

SELECT create_hypertable('metrics', 'time', chunk_time_interval => INTERVAL '1 day');

-- Query with time constraint
SELECT * FROM metrics
WHERE time >= '2024-03-01' AND time < '2024-03-08';

-- Chunk pruning:
-- Hypertable has 365 chunks (1 per day)
-- Query only accesses 7 chunks (2024-03-01 to 2024-03-07)
-- 358 chunks skipped (98% reduction)
```

## References
1. **TimescaleDB Docs**: "Hypertables and Chunks"

## Tags
`database-specific`, `timescaledb`, `chunk-pruning`, `time-series`, `partitioning`
