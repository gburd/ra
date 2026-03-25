# RFC 0065: Time-Series Query Optimization

- Start Date: 2026-03-25
- Author: Ra Research Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Ra should optimize time-series queries on TimescaleDB hypertables by
understanding chunk-based storage, compression characteristics,
continuous aggregate opportunities, and time-bucketed aggregation
patterns. This RFC extends RFC 0061's TimescaleDB section with detailed
chunk cost modeling, skip-scan optimization for time-ordered data,
merge-append optimization for multi-chunk queries, and feedback-driven
continuous aggregate recommendation.

## Motivation

Time-series workloads have distinct query patterns that standard
PostgreSQL optimization handles suboptimally:

**1. Chunk scanning overhead.** A hypertable with 1000 chunks generates
a plan with 1000 append children. PostgreSQL's planner evaluates all
children even when chunk exclusion eliminates most of them. For a
7-day query on a year of data, 995 chunks are excluded but still add
planning overhead.

**2. Compression cost mismatch.** Compressed chunks use columnar storage
that is faster for aggregation but slower for random access. The
standard cost model does not distinguish compressed from uncompressed
chunks, leading to incorrect scan strategy selection.

**3. Missing continuous aggregate detection.** When a query matches an
existing continuous aggregate, PostgreSQL does not automatically rewrite
the query to use the pre-computed view. Users must manually select the
continuous aggregate.

**4. Last-point query optimization.** The common pattern
`SELECT DISTINCT ON (device_id) * FROM data ORDER BY device_id, time DESC`
("get the latest reading per device") results in a full table scan when
it could use a skip-scan optimization.

**Expected impact:**

| Pattern | Current | Optimized | Gain |
|---------|---------|-----------|------|
| Time-range scan (7d / 1y) | Full append plan | Chunk-pruned scan | 50x planning, 2-5x execution |
| Aggregation on compressed | Hash aggregate + decompress | Columnar aggregate | 3-10x |
| Continuous aggregate match | Full re-computation | Pre-computed read | 100-1000x |
| Last-point query | Sequential scan | Skip-scan + index | 10-100x |

## Guide-level explanation

### Hypertable detection

Ra detects hypertables by querying TimescaleDB's catalog:

```sql
SELECT h.table_name, h.schema_name,
       d.column_name AS time_column,
       d.interval_length AS chunk_interval,
       (SELECT count(*) FROM _timescaledb_catalog.chunk c
        WHERE c.hypertable_id = h.id) AS chunk_count
FROM _timescaledb_catalog.hypertable h
JOIN _timescaledb_catalog.dimension d
  ON h.id = d.hypertable_id
WHERE d.num_slices IS NULL; -- time dimension
```

### Chunk-aware cost estimation

For a query with a time predicate on a hypertable:

```sql
SELECT time_bucket('1 hour', time) AS bucket,
       avg(temperature)
FROM sensor_data
WHERE time BETWEEN '2026-03-18' AND '2026-03-24'
GROUP BY bucket;
```

Ra estimates:
1. Total chunks: 365 (1 per day for a year)
2. Matching chunks: 7 (one per day in the range)
3. Compressed chunks: 6 (all except the latest)
4. Cost: 6 * compressed_chunk_cost + 1 * uncompressed_chunk_cost

### Continuous aggregate matching

When Ra detects a query that matches a continuous aggregate:

```sql
-- Existing continuous aggregate:
CREATE MATERIALIZED VIEW hourly_temps
  WITH (timescaledb.continuous) AS
  SELECT time_bucket('1 hour', time) AS bucket,
         device_id,
         avg(temperature) AS avg_temp
  FROM sensor_data
  GROUP BY bucket, device_id;

-- User query:
SELECT time_bucket('1 hour', time) AS bucket,
       device_id,
       avg(temperature) AS avg_temp
FROM sensor_data
WHERE time > now() - interval '30 days'
GROUP BY 1, 2;
```

Ra recommends rewriting to use the continuous aggregate:

```sql
SELECT bucket, device_id, avg_temp
FROM hourly_temps
WHERE bucket > now() - interval '30 days';
```

## Reference-level explanation

### Chunk cost model

```rust
struct ChunkCostFactors {
    /// Number of rows in this chunk
    rows: u64,
    /// Whether the chunk is compressed
    compressed: bool,
    /// Compression ratio (compressed_size / uncompressed_size)
    compression_ratio: f64,
    /// Whether the chunk is in the recent (hot) range
    is_hot: bool,
}

fn estimate_chunk_scan_cost(
    chunk: &ChunkCostFactors,
    aggregation: bool,
) -> f64 {
    if chunk.compressed {
        let io_cost =
            chunk.rows as f64 * SEQ_PAGE_COST / chunk.compression_ratio;
        let cpu_cost = if aggregation {
            // Compressed columnar scan: aggregate directly on compressed data
            chunk.rows as f64 * CPU_OPERATOR_COST * 0.3
        } else {
            // Must decompress for row access
            chunk.rows as f64 * CPU_OPERATOR_COST * 3.5
        };
        io_cost + cpu_cost
    } else {
        let io_cost = chunk.rows as f64 * SEQ_PAGE_COST;
        let cpu_cost = chunk.rows as f64 * CPU_OPERATOR_COST;
        io_cost + cpu_cost
    }
}
```

### Time-range selectivity

Ra computes selectivity for time predicates on hypertables:

```
selectivity = query_time_range / total_time_range
effective_chunks = total_chunks * selectivity
effective_rows = total_rows * selectivity
```

For recent-biased workloads (most data is queried from recent time
ranges), Ra adjusts selectivity based on chunk statistics:

```
IF recent_chunks have higher row density
THEN effective_rows = sum(chunk_rows for matching chunks)
     -- more accurate than uniform distribution assumption
```

### Merge-append optimization

When multiple chunks are scanned, TimescaleDB uses a MergeAppend
(or its custom ChunkAppend) to merge time-ordered results. Ra should
prefer merge-append over hash-append when:

1. Output must be time-ordered (ORDER BY time)
2. Each chunk has an index on the time column
3. The number of chunks is reasonable (< 100)

```
IF query has ORDER BY time_column
   AND each chunk has index on time_column
THEN prefer MergeAppend (cost: n_chunks * log(n_chunks) merge overhead)
ELSE prefer Append with post-sort (cost: total_rows * log(total_rows))
```

### Last-point query optimization

The "latest value per device" pattern:

```sql
SELECT DISTINCT ON (device_id) *
FROM sensor_data
ORDER BY device_id, time DESC;
```

Ra optimizes this by:
1. Detecting the DISTINCT ON + ORDER BY DESC pattern
2. Recommending a compound index on (device_id, time DESC)
3. Suggesting a skip-scan strategy (RFC 0038) that reads only the
   first row per device from the index

### Continuous aggregate matching rules

Ra maintains a registry of continuous aggregates and their definitions.
When a query matches a continuous aggregate:

```
Match criteria:
1. Same base table
2. Same time_bucket interval (or multiple thereof)
3. Same GROUP BY columns
4. Same aggregate functions
5. Optional: additional WHERE clause that can be applied to the CA
```

When a match is found, Ra emits an advisory recommending the continuous
aggregate. If Ra has write access to the query (via plan advice), it
can rewrite the query to use the CA directly.

### Compression-aware join planning

When joining a hypertable with a dimension table:

```sql
SELECT d.name, avg(s.temperature)
FROM sensor_data s
JOIN devices d ON s.device_id = d.id
WHERE s.time > now() - interval '7 days'
GROUP BY d.name;
```

Ra should:
1. Push the time predicate to chunk exclusion (only scan 7 chunks)
2. Prefer hash join for the device lookup (dimension table is small)
3. Push aggregation below the join if possible (aggregate per chunk,
   then combine)

### Error handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum TimeSeriesError {
    #[error(
        "TimescaleDB catalog version mismatch: expected schema \
         {expected}, found {found}"
    )]
    CatalogVersionMismatch {
        expected: String,
        found: String,
    },

    #[error(
        "Hypertable {table} has {count} chunks; chunk-level \
         cost estimation may be slow"
    )]
    TooManyChunks { table: String, count: usize },
}
```

## Drawbacks

**TimescaleDB catalog dependency.** Ra reads from
`_timescaledb_catalog.*` tables whose schema varies between TimescaleDB
major versions. Version detection and graceful fallback are required.

**Planning overhead for many chunks.** Hypertables with thousands of
chunks make per-chunk cost estimation expensive. Ra should cap chunk
enumeration and fall back to statistical estimation.

**Continuous aggregate freshness.** Recommending a CA does not guarantee
fresh data. The CA may lag behind the base table by the refresh interval.
Ra should note the freshness tradeoff in advisories.

## Rationale and alternatives

### Why not rely on TimescaleDB's ChunkAppend

TimescaleDB's custom ChunkAppend node handles chunk exclusion at
execution time. Ra's optimization is complementary: Ra improves the
plan that is input to ChunkAppend (join ordering, aggregation strategy,
index selection) rather than replacing chunk exclusion.

### Alternative: partition-aware planning only

RFC 0019 (Partition Pruning) provides general partition-aware planning.
TimescaleDB hypertables are partitions, but they have additional
characteristics (compression, continuous aggregates, time_bucket) that
require specific optimization rules beyond partition pruning.

## Prior art

- **TimescaleDB's query optimizer**: Implements chunk exclusion and
  compression-aware scanning. Ra extends this with cross-table
  optimization.
- **InfluxDB IOx**: Uses columnar storage with automatic time-range
  pruning. Similar chunk exclusion concept.
- **Apache Druid**: Segment-level pruning for time-series queries.
  Ra's chunk cost model is analogous.
- **QuestDB**: JIT-compiled time-series queries. Ra provides planning
  optimizations rather than execution optimizations.

## Unresolved questions

1. Should Ra recommend compression policies based on query patterns
   (e.g., if old data is only queried via aggregates, compress it)?
2. How to handle multi-dimensional hypertables (time + space
   partitioning)?
3. Should Ra integrate with TimescaleDB's continuous aggregate
   refresh scheduling?

## Future possibilities

- **Tiered storage optimization**: When TimescaleDB supports S3 tiering,
  Ra should account for network latency in cost estimation.
- **Downsampling advisory**: Recommend data retention policies and
  downsampled continuous aggregates based on query patterns.
- **Real-time + historical query fusion**: Optimize queries that combine
  real-time data (recent uncompressed chunks) with historical data
  (compressed chunks) using different strategies for each.
