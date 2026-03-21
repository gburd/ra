# Rule: Stream-Table Join Optimization

**Category:** execution-models/streaming
**File:** `rules/execution-models/streaming/stream-table-join.rra`

## Metadata

- **ID:** `stream-table-join`
- **Version:** "1.0.0"
- **Databases:** flink, ksqldb, spark-streaming, materialize
- **Tags:** streaming, join, lookup, temporal, enrichment
- **Authors:** "Apache Flink Team"


# Stream-Table Join Optimization

## Description

Optimizes joins between a streaming source and a static or slowly-changing
dimension table. Instead of maintaining a full join state for both sides,
the optimizer recognizes the asymmetry: the table side can be cached
locally and probed for each stream event. For temporal tables (tables
that change over time), point-in-time lookups ensure correctness.

**When to apply**: Join between a high-throughput stream and a relatively
static lookup table (dimension enrichment pattern).

## Relational Algebra

```algebra
-- Before: symmetric stream-stream join
stream JOIN table ON stream.key = table.id

-- After: lookup join with local cache
LookupJoin(
    stream,
    CachedTable(table, ttl=5min),
    key=stream.key)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("stream-table-lookup-join";
    "(join ?key ?stream ?table)" =>
    "(lookup-join ?key ?stream (cache ?table (ttl 300)))"
    if is_stream("?stream")
    if is_bounded_or_slow_changing("?table")
),

rw!("temporal-lookup-join";
    "(join ?key ?stream ?temporal_table)" =>
    "(temporal-lookup-join ?key ?stream ?temporal_table
        (as-of stream.event_time))"
    if is_temporal_table("?temporal_table")
),
```

## Preconditions

```rust
fn applicable(join: &StreamJoin) -> bool {
    let (stream_side, table_side) = join.classify_sides();
    // One side must be a stream, other must be bounded/slow
    stream_side.is_unbounded()
        && (table_side.is_bounded()
            || table_side.change_rate() < 0.01) // <1% change/sec
}
```

**Restrictions:**
- Table must fit in memory for local caching
- Temporal joins require versioned table with valid-time semantics
- Cache invalidation strategy needed for mutable tables

## Cost Model

```rust
fn estimated_benefit(
    events_per_second: f64,
    table_size: f64,
    join_state_overhead: f64,
) -> f64 {
    let symmetric_cost = events_per_second * join_state_overhead;
    let lookup_cost = events_per_second * 0.001; // cached lookup
    symmetric_cost - lookup_cost
}
```

**Typical benefit**: 20-70% state reduction and latency improvement.

## Test Cases

```sql
-- Positive: stream enriched with static dimension
SELECT s.*, d.category_name
FROM sales_stream s
  JOIN product_categories d ON s.category_id = d.id;
-- Lookup join with cached product_categories

-- Positive: temporal join for exchange rates
SELECT o.amount * r.rate AS usd_amount
FROM orders_stream o
  JOIN currency_rates FOR SYSTEM_TIME AS OF o.order_time r
  ON o.currency = r.currency;

-- Negative: both sides are high-throughput streams
SELECT * FROM clicks_stream c JOIN views_stream v
  ON c.user_id = v.user_id;
```

## References

- Apache Flink: Lookup Joins documentation
- KSQL: Table-Table and Stream-Table joins
