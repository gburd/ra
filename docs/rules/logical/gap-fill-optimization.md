# Rule: Time Series Gap Fill Optimization

**Category:** logical/time-series
**File:** `rules/logical/time-series/gap-fill-optimization.rra`

## Metadata

- **ID:** `gap-fill-optimization`
- **Version:** "1.0.0"
- **Databases:** timescaledb, questdb, influxdb, clickhouse
- **Tags:** logical, time-series, gap-fill, interpolation, locf
- **Authors:** "Timescale Inc."


# Time Series Gap Fill Optimization

## Description

Optimizes gap-filling queries that generate missing time buckets and
interpolate values (LOCF - Last Observation Carried Forward, or linear
interpolation). Instead of generating a complete time series and
performing a LEFT JOIN, this optimization fuses the gap-fill operation
with the aggregate scan, producing missing buckets inline during a
single ordered pass.

**When to apply**: Queries using time_bucket_gapfill() or equivalent
generate_series + LEFT JOIN patterns for time series visualization.

## Relational Algebra

```algebra
-- Before: generate_series + LEFT JOIN
pi[bucket, COALESCE(val, prev_val)](
    generate_series('2024-01-01', '2024-01-31', '1 hour') AS g(bucket)
    LEFT JOIN
    gamma[time_bucket('1h', ts); AVG(val)](data) AS d
    ON g.bucket = d.bucket
)

-- After: fused gap-fill scan
GapFillScan(data, '1 hour', '2024-01-01', '2024-01-31',
    agg=AVG(val), fill=LOCF)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("gap-fill-fusion";
    "(left-join (= ?g_col ?d_col)
        (generate-series ?start ?end ?interval)
        (aggregate (time-bucket ?interval ?ts) ?agg ?input))" =>
    "(gap-fill-scan ?input ?interval ?start ?end ?agg locf)"
),
```

## Preconditions

```rust
fn applicable(join: &LeftJoin) -> bool {
    // Left side must be a time series generator
    join.left().is_generate_series()
        // Right side must be a time-bucketed aggregate
        && join.right().is_time_bucket_aggregate()
        // Intervals must match
        && join.left().interval() == join.right().bucket_interval()
}
```

**Restrictions:**
- Interpolation method must be specified (LOCF, linear, or NULL)
- Only works for regularly-spaced time buckets
- Window function approach (LAG) may be needed for complex interpolation

## Cost Model

```rust
fn estimated_benefit(
    expected_buckets: f64,
    actual_data_points: f64,
) -> f64 {
    // Avoid materializing generate_series + hash join
    let join_cost = expected_buckets * 4.0 + actual_data_points * 4.0;
    let fused_cost = actual_data_points * 1.5;
    join_cost - fused_cost
}
```

**Typical benefit**: 10-50% for sparse time series with many gaps.

## Test Cases

```sql
-- Positive: gap-fill with LOCF
SELECT time_bucket_gapfill('1 hour', ts) AS bucket,
       device_id,
       LOCF(AVG(temperature))
FROM sensor_data
WHERE ts >= '2024-01-01' AND ts < '2024-01-02'
GROUP BY bucket, device_id;

-- Negative: no gap-fill function used
SELECT time_bucket('1 hour', ts), AVG(temperature)
FROM sensor_data GROUP BY 1;
```

## References

- TimescaleDB: time_bucket_gapfill() documentation
- InfluxDB: FILL clause in GROUP BY time()
