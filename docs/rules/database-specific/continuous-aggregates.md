# Rule: Continuous Aggregates (TimescaleDB)

**Category:** database-specific/timescaledb
**File:** `rules/database-specific/timescaledb/continuous-aggregates.rra`

## Metadata

- **ID:** `timescaledb-continuous-aggregates`
- **Version:** "1.0.0"
- **Databases:** timescaledb
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Continuous Aggregates (TimescaleDB)

## Metadata
- **Rule ID**: `timescaledb-continuous-aggregates`
- **Category**: Database-Specific / TimescaleDB
- **Source**: TimescaleDB

## Description

TimescaleDB incrementally maintains materialized aggregates, updating only new data rather than recomputing from scratch.

## Test Cases

### Test 1: Real-time dashboard with continuous aggregate
```sql
CREATE MATERIALIZED VIEW sensor_hourly
WITH (timescaledb.continuous) AS
SELECT time_bucket('1 hour', time) AS hour,
       device_id,
       AVG(temperature) AS avg_temp
FROM sensor_data
GROUP BY hour, device_id;

-- Automatic incremental updates
-- Query reads from materialized view (fast)
-- Only new data recomputed
```

## Tags
`database-specific`, `timescaledb`, `continuous-aggregate`, `materialized-view`, `incremental`
