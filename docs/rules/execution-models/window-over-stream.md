# Rule: Window Function Over Stream Optimization

**Category:** execution-models/streaming
**File:** `rules/execution-models/streaming/window-over-stream.rra`

## Metadata

- **ID:** `window-over-stream`
- **Version:** "1.0.0"
- **Databases:** flink, ksqldb, materialize, spark-streaming
- **Tags:** streaming, window, tumbling, sliding, hopping, event-time
- **Authors:** "Apache Flink Team", "Arasu, Babu, Widom"


# Window Function Over Stream Optimization

## Description

Optimizes window aggregations over streaming data by selecting the
appropriate windowing strategy: tumbling (non-overlapping fixed-size),
sliding (overlapping), session (gap-based), or global windows. The
optimizer determines which window type matches the query pattern and
selects the corresponding incremental evaluation strategy that maintains
partial state and emits results as windows close.

**When to apply**: Aggregate queries over streaming sources with time-based
or count-based grouping semantics.

## Relational Algebra

```algebra
-- Before: batch-style window
gamma[TUMBLE(ts, '5 min'); SUM(amount)](stream)

-- After: incremental tumbling window
IncrementalTumbleWindow(
    stream, interval='5 min',
    agg=SUM(amount),
    trigger=on_watermark,
    state=partial_sum)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("tumbling-window-incremental";
    "(aggregate (tumble ?ts ?interval) ?agg ?stream)" =>
    "(incremental-tumble ?stream ?ts ?interval ?agg
        (trigger watermark))"
    if is_decomposable("?agg")
),

rw!("sliding-to-tumble-panes";
    "(aggregate (slide ?ts ?size ?slide) ?agg ?stream)" =>
    "(merge-panes ?slide
        (incremental-tumble ?stream ?ts ?slide ?agg
            (trigger pane-complete)))"
    if slide_divides_size("?slide", "?size")
),
```

## Preconditions

```rust
fn applicable(query: &StreamQuery) -> bool {
    query.has_time_window()
        && query.aggregate_is_decomposable()
        && query.source_has_watermarks()
}
```

**Restrictions:**
- Requires event-time watermarks for correctness
- Late events may need side-output or retractions
- Session windows need per-key state management

## Cost Model

```rust
fn estimated_benefit(
    events_per_second: f64,
    window_duration_seconds: f64,
) -> f64 {
    let batch_cost = events_per_second * window_duration_seconds;
    let incremental_cost = events_per_second * 1.0; // O(1) per event
    batch_cost - incremental_cost
}
```

**Typical benefit**: 20-70% latency reduction vs micro-batch.

## Test Cases

```sql
-- Positive: tumbling window aggregation
SELECT TUMBLE_START(ts, INTERVAL '5' MINUTE) AS window_start,
       device_id, AVG(temperature)
FROM sensor_stream
GROUP BY TUMBLE(ts, INTERVAL '5' MINUTE), device_id;

-- Positive: sliding window with aligned panes
SELECT HOP_START(ts, INTERVAL '1' MINUTE, INTERVAL '5' MINUTE),
       COUNT(*)
FROM click_stream
GROUP BY HOP(ts, INTERVAL '1' MINUTE, INTERVAL '5' MINUTE);

-- Negative: session window (no pane optimization)
SELECT SESSION_START(ts, INTERVAL '30' MINUTE), user_id, COUNT(*)
FROM events GROUP BY SESSION(ts, INTERVAL '30' MINUTE), user_id;
```

## References

- Carbone, P. et al. "Apache Flink: Stream and Batch Processing in a Single Engine" (IEEE Bulletin 2015)
- Li, J. et al. "Out-of-Order Processing: A New Architecture for High-Performance Stream Systems" (VLDB 2008)
