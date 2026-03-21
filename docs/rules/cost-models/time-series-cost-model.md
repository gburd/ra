# Rule: Time-Series Specific Cost Model

**Category:** cost-models
**File:** `rules/cost-models/time-series-cost-model.rra`

## Metadata

- **ID:** `time-series-cost-model`
- **Version:** "1.0.0"
- **Databases:** timescaledb, influxdb, clickhouse, questdb, duckdb
- **Tags:** cost, time-series, temporal, window, downsampling, partition
- **Authors:** "RA Contributors"


# Time-Series Specific Cost Model

## Metadata
- **Rule ID**: `time-series-cost-model`
- **Category**: Cost Models
- **Complexity**: O(1) per cost estimation with pre-computed temporal stats
- **Introduced**: TimescaleDB (hypertable cost model), ClickHouse (MergeTree)
- **Prerequisites**: Time-partitioned data, temporal statistics
- **Alternatives**: generic cardinality-estimation, histogram-based-estimation

## Description

Time-series cost models exploit the unique characteristics of temporal data:
monotonically increasing timestamps, append-mostly workloads, natural time
partitioning, and predictable access patterns (recent data is accessed far
more than old data). Generic cost models miss these properties, leading to
suboptimal plans for time-series queries.

Key properties that time-series cost models leverage:
1. **Temporal locality**: Recent partitions are hot (in memory), old
   partitions are cold (on disk). Access cost varies by time range.
2. **Monotonic insertion**: Data arrives in timestamp order, so time
   ranges map directly to contiguous storage regions. Range scans
   on timestamp are always sequential I/O.
3. **Predictable cardinality**: Row rate (rows/second) is often stable,
   making cardinality estimation for time ranges trivial.
4. **Downsampling patterns**: Aggregation over time windows is the
   dominant query pattern, with known cost characteristics.

**When to use:**
- Queries with timestamp-based WHERE clauses
- Time-window aggregations (GROUP BY time_bucket)
- Latest-value queries (ORDER BY ts DESC LIMIT 1 per entity)
- Time-range joins between time-series tables
- Continuous aggregation / materialized view refresh

**Advantages:**
- Near-perfect cardinality estimation for time ranges
- Accurate I/O cost based on partition temperature
- Correct cost for common time-series patterns (downsampling, latest-N)
- Enables partition pruning cost benefits

**Disadvantages:**
- Assumes data arrives in timestamp order (breaks for backfill)
- Row rate estimation may be wrong for bursty workloads
- Time-based partitioning must be configured correctly
- Does not model out-of-order data insertion well

## Formal Model

```
Time-series table T with:
  - row_rate: r rows/second (observed average)
  - partition_interval: P seconds per chunk/partition
  - total_time_span: [t_min, t_max]
  - rows_per_partition: r * P

For time range query WHERE ts BETWEEN t1 AND t2:
  estimated_rows = r * (t2 - t1)
  partitions_touched = ceil((t2 - t1) / P)

  hot_partitions = partitions within retention_hot window
  cold_partitions = partitions beyond retention_hot window

  io_cost = hot_partitions * memory_scan_cost
          + cold_partitions * disk_scan_cost
```

## Implementation (egg rewrite rules)

```lisp
;; Time-range selectivity using row rate
(rewrite (selectivity (between ?ts_col ?t1 ?t2) ?table)
  (time-range-selectivity ?table ?t1 ?t2)
  :if (is-timestamp-column ?table ?ts_col)
  :if (has-temporal-stats ?table))

;; Partition pruning for time range
(rewrite (scan ?table (between ?ts_col ?t1 ?t2))
  (partition-scan ?table
    (time-range-partitions ?table ?t1 ?t2))
  :if (is-time-partitioned ?table ?ts_col))

;; Latest-value optimization
(rewrite (limit 1 (sort-desc ?ts_col (scan ?table)))
  (latest-value-scan ?table ?ts_col)
  :if (is-timestamp-column ?table ?ts_col)
  :if (is-time-partitioned ?table ?ts_col))

;; Time-bucket aggregation cost
(rewrite (aggregate ?aggs
           (time-bucket ?interval ?ts_col)
           (scan ?table ?pred))
  (time-bucket-aggregate ?table ?interval ?ts_col
    ?aggs ?pred
    :estimated-groups (/ (time-range ?pred) ?interval))
  :if (is-timestamp-column ?table ?ts_col))

;; Continuous aggregation: incremental cost
(rewrite (refresh-materialized-view ?mv ?since)
  (incremental-aggregate ?mv
    :new-rows (rows-since ?mv ?since)
    :existing-groups (mv-group-count ?mv))
  :if (is-time-series-mv ?mv))
```

## Implementation Pattern

```rust
pub struct TimeSeriesCostModel {
    row_rate: f64,           // Rows per second (smoothed)
    partition_interval_s: u64,
    hot_window_s: u64,       // How far back data is cached
    compression_ratio: f64,  // For cold partitions
    stats_per_partition: Vec<PartitionStats>,
}

struct PartitionStats {
    time_range: (Timestamp, Timestamp),
    row_count: u64,
    size_bytes: u64,
    is_compressed: bool,
    is_in_memory: bool,
}

impl TimeSeriesCostModel {
    pub fn estimate_time_range_cardinality(
        &self,
        t_start: Timestamp,
        t_end: Timestamp,
    ) -> u64 {
        // Use per-partition stats if available
        let mut total = 0u64;
        for part in &self.stats_per_partition {
            let overlap = time_overlap(
                (t_start, t_end),
                part.time_range,
            );
            if overlap > 0.0 {
                let fraction = overlap
                    / duration(part.time_range);
                total += (part.row_count as f64 * fraction) as u64;
            }
        }

        if total > 0 {
            return total;
        }

        // Fall back to row_rate estimation
        let duration_s = (t_end - t_start).as_secs_f64();
        (self.row_rate * duration_s) as u64
    }

    pub fn estimate_scan_cost(
        &self,
        t_start: Timestamp,
        t_end: Timestamp,
        hardware: &HardwareModel,
    ) -> Cost {
        let mut cost = Cost::zero();

        for part in &self.stats_per_partition {
            let overlap = time_overlap(
                (t_start, t_end),
                part.time_range,
            );
            if overlap <= 0.0 {
                continue;
            }

            let fraction = overlap / duration(part.time_range);
            let bytes = part.size_bytes as f64 * fraction;

            if part.is_in_memory {
                // Hot partition: memory scan
                cost = cost + Cost::cpu(
                    (bytes / 64.0) as u64, // Cache line reads
                );
            } else if part.is_compressed {
                // Cold compressed: decompress + disk read
                let disk = Cost::io(
                    bytes / self.compression_ratio
                        / hardware.page_size()
                        * hardware.sequential_page_read_cost(),
                );
                let decompress = Cost::cpu(
                    (bytes / self.compression_ratio) as u64 * 2,
                );
                cost = cost + disk + decompress;
            } else {
                // Cold uncompressed: disk read
                cost = cost + Cost::io(
                    bytes / hardware.page_size()
                        * hardware.sequential_page_read_cost(),
                );
            }
        }

        cost
    }

    pub fn estimate_downsample_cost(
        &self,
        t_start: Timestamp,
        t_end: Timestamp,
        bucket_interval_s: u64,
        agg_count: usize,
        hardware: &HardwareModel,
    ) -> Cost {
        let input_rows = self.estimate_time_range_cardinality(
            t_start, t_end,
        );
        let output_groups = ((t_end - t_start).as_secs()
            / bucket_interval_s) as u64;

        let scan = self.estimate_scan_cost(t_start, t_end, hardware);
        let aggregate = Cost::cpu(
            input_rows * agg_count as u64 * 5,
        );
        let output = Cost::cpu(output_groups * 10);

        scan + aggregate + output
    }
}
```

## Cost Model

```rust
pub fn latest_value_cost(
    num_entities: u64,
    partition_interval_s: u64,
    avg_entity_interval_s: f64,
    hardware: &HardwareModel,
) -> Cost {
    // Latest value per entity: scan most recent partition first
    // Best case: all entities have recent data in 1 partition
    // Worst case: entities spread across many partitions

    let partitions_needed = (avg_entity_interval_s
        / partition_interval_s as f64)
        .ceil() as u64;

    // Index seek per entity in each partition
    let seek_cost = Cost::io(
        num_entities as f64
            * partitions_needed as f64
            * hardware.random_page_read_cost(),
    );

    seek_cost
}

pub fn continuous_agg_refresh_cost(
    new_rows: u64,
    existing_groups: u64,
    agg_count: usize,
) -> Cost {
    // Only process new rows, update existing group states
    let process_new = Cost::cpu(new_rows * agg_count as u64 * 5);
    let merge_groups = Cost::cpu(existing_groups * agg_count as u64 * 2);

    process_new + merge_groups
}
```

## Test Cases

### Test 1: Time range cardinality estimation
```sql
CREATE TABLE sensor_data (
    ts TIMESTAMPTZ NOT NULL,
    sensor_id INT,
    value DOUBLE PRECISION
);
-- Row rate: 100K rows/second, partitioned by 1 hour

SELECT COUNT(*) FROM sensor_data
WHERE ts BETWEEN '2025-03-01' AND '2025-03-02';

-- Expected estimate: 100K * 86400 = 8.64 billion rows
-- Generic optimizer: histogram-based, potentially 2-5x off
-- Time-series model: row_rate * duration = near-exact
```

### Test 2: Hot vs cold partition I/O cost
```sql
-- Last 24 hours in memory, older on disk (compressed 5x)

-- Query A: Recent data (hot)
SELECT AVG(value) FROM sensor_data
WHERE ts > NOW() - INTERVAL '1 hour';
-- Cost: memory scan only, ~10ms

-- Query B: Old data (cold, compressed)
SELECT AVG(value) FROM sensor_data
WHERE ts BETWEEN '2024-01-01' AND '2024-02-01';
-- Cost: disk read + decompress, ~5000ms

-- Generic model: treats both equally (wrong by 500x)
```

### Test 3: Downsampling aggregation
```sql
SELECT time_bucket('5 minutes', ts) AS bucket,
       sensor_id,
       AVG(value), MIN(value), MAX(value)
FROM sensor_data
WHERE ts > NOW() - INTERVAL '24 hours'
GROUP BY bucket, sensor_id;

-- Expected: Input 8.64B rows, output = 288 * 1000 = 288K groups
-- Time-series model: knows bucket count = time_range / interval
-- Generic model: must estimate GROUP BY cardinality heuristically
```

### Test 4: Latest-value query
```sql
SELECT DISTINCT ON (sensor_id) sensor_id, ts, value
FROM sensor_data
ORDER BY sensor_id, ts DESC;

-- 1000 sensors, each reporting every second
-- Time-series model: scan last partition, 1 seek per sensor
-- Cost: 1000 index seeks in hot partition = ~5ms
-- Generic model: might plan full table scan + sort
```

### Test 5: Negative -- non-temporal query on time-series table
```sql
SELECT * FROM sensor_data WHERE sensor_id = 42;

-- No timestamp predicate: time-series cost model not applicable
-- Falls back to generic estimation (index scan on sensor_id)
-- Time partitioning doesn't help (all partitions touched)
```

## Performance Characteristics

| Query Pattern | Generic Model Error | Time-Series Model Error |
|---------------|--------------------|-----------------------|
| Time range (1 hour) | 2-5x | < 1.1x |
| Time range (1 year) | 5-20x | < 1.2x |
| Hot partition scan | 10-100x (wrong I/O) | < 1.5x |
| Downsampling | 5-10x (group count) | < 1.3x |
| Latest-value | 100x (plans full scan) | < 2x |

## References

1. **TimescaleDB**: Hypertable query planner and chunk pruning
   - https://docs.timescale.com/timescaledb/latest/overview/

2. **ClickHouse MergeTree**: Time-partitioned storage and query optimization
   - https://clickhouse.com/docs/en/engines/table-engines/mergetree-family

3. **QuestDB**: Time-series specific query execution
   - https://questdb.io/docs/concept/storage-model/

4. **Pelkonen et al.**: "Gorilla: A Fast, Scalable, In-Memory Time Series Database"
   - VLDB 2015, Facebook's time-series storage and query model

5. **Jensen et al.**: "Temporal Database Management"
   - Time-aware query optimization foundations
