# Date Range Filters

## Description

Filtering data by date or timestamp ranges. One of the most common query patterns in OLTP and OLAP systems for time-series data, logs, and transactional records.

## Use Cases

- Orders within date range
- Log analysis for time period
- Time-series analytics
- Data retention policies
- Incremental ETL (changes since last run)

## Relational Algebra

Range selection on temporal column:

$$
\sigma_{t_{\text{low}} \leq t \leq t_{\text{high}}}(R)
$$

With BETWEEN syntax:

$$
\sigma_{t \text{ BETWEEN } t_{\text{low}} \text{ AND } t_{\text{high}}}(R)
$$

Open-ended ranges:

$$
\sigma_{t \geq t_{\text{low}}}(R) \quad \text{or} \quad \sigma_{t < t_{\text{high}}}(R)
$$

## How Ra Optimizes

### 1. Index Range Scan

**Rule:** `physical/temporal-index-range-scan`

Use B-tree index on temporal column:

$$
\text{Cost} = \log(N) + \text{sel} \times N \times C_{\text{page}}
$$

Where $\text{sel}$ is temporal selectivity:

$$
\text{sel} = \frac{t_{\text{high}} - t_{\text{low}}}{t_{\text{max}} - t_{\text{min}}}
$$

### 2. Partition Pruning

**Rule:** `distributed/temporal-partition-pruning`

For partitioned tables (by date):

$$
\sigma_{t \in [t_1, t_2]}(R_{\text{jan}} \cup R_{\text{feb}} \cup \cdots) \rightarrow \sigma_{t \in [t_1, t_2]}(R_{\text{jan}} \cup R_{\text{feb}})
$$

Only scan partitions overlapping the range.

### 3. Constant Folding

**Rule:** `logical/temporal-constant-folding`

Evaluate temporal expressions at optimization time:

```sql
WHERE created_at > NOW() - INTERVAL '7 days'
```

Becomes:

```sql
WHERE created_at > '2024-03-14 10:30:00'
```

Enables accurate cardinality estimation.

### 4. Temporal Index-Only Scan

**Rule:** `physical/temporal-covering-index`

For queries selecting only temporal column + aggregates:

$$
\gamma_{\emptyset; \text{COUNT}(*)}(\sigma_{t \in [t_1, t_2]}(R)) \rightarrow \text{IndexOnlyScan}(\text{date\_idx})
$$

No heap access needed.

## Statistics API

```rust
use ra_optimizer::{Statistics, ColumnStatistics, TemporalHistogram};

optimizer.add_table_stats("events", Statistics {
    row_count: 100_000_000,
    block_count: 1_000_000,
});

optimizer.add_column_stats("events", "timestamp", ColumnStatistics {
    distinct_count: 86_400_000,  // ~1000 days × 86400 seconds/day
    null_fraction: 0.0,
    min_value: Some("2022-01-01 00:00:00"),
    max_value: Some("2024-12-31 23:59:59"),
    histogram: Some(TemporalHistogram {
        buckets: vec![
            TimeRange { start: "2022-01-01", end: "2022-06-30", frequency: 0.10 },
            TimeRange { start: "2022-07-01", end: "2022-12-31", frequency: 0.15 },
            TimeRange { start: "2023-01-01", end: "2023-06-30", frequency: 0.20 },
            TimeRange { start: "2023-07-01", end: "2023-12-31", frequency: 0.25 },
            TimeRange { start: "2024-01-01", end: "2024-12-31", frequency: 0.30 },
        ],
    }),
});

// Partition information
optimizer.add_partitions("events", vec![
    Partition { name: "events_2022", range: "2022-01-01 TO 2023-01-01", row_count: 25_000_000 },
    Partition { name: "events_2023", range: "2023-01-01 TO 2024-01-01", row_count: 35_000_000 },
    Partition { name: "events_2024", range: "2024-01-01 TO 2025-01-01", row_count: 40_000_000 },
]);
```

## Examples

### Fixed Date Range

```sql
SELECT COUNT(*), SUM(amount)
FROM orders
WHERE order_date BETWEEN '2024-01-01' AND '2024-01-31';
```

**Relational Algebra:**

$$
\gamma_{\emptyset; \text{COUNT}(*), \text{SUM}(\text{amount})}(\sigma_{\text{order\_date} \in [\text{'2024-01-01'}, \text{'2024-01-31'}]}(\text{orders}))
$$

**Ra Plan:**

```
Aggregate
  Aggregates: COUNT(*), SUM(amount)
  IndexRangeScan [orders.order_date_idx]
    Filter: order_date BETWEEN '2024-01-01' AND '2024-01-31'
```

**Selectivity Calculation:**

$$
\text{sel} = \frac{31 \text{ days}}{1826 \text{ days}} \approx 0.017 \quad (1.7\%)
$$

**Expected Rows:** $100{,}000{,}000 \times 0.017 = 1{,}700{,}000$

### Relative Date Range (Last 7 Days)

```sql
SELECT user_id, COUNT(*) as login_count
FROM user_logins
WHERE login_time > NOW() - INTERVAL '7 days'
GROUP BY user_id;
```

**Ra Plan (after constant folding):**

```
HashAggregate [user_id]
  Aggregates: COUNT(*)
  IndexRangeScan [user_logins.login_time_idx]
    Filter: login_time > '2024-03-14 10:30:00'
```

**Optimization:** `NOW()` evaluated at optimization time, enabling:
- Accurate selectivity estimate
- Index range scan
- Partition pruning (if partitioned by day)

### Date Range with Additional Filters

```sql
SELECT *
FROM events
WHERE event_date BETWEEN '2024-01-01' AND '2024-03-31'
  AND event_type = 'purchase'
  AND amount > 100;
```

**Ra Plan:**

```
IndexRangeScan [events.event_date_idx]
  Filter: event_date BETWEEN '2024-01-01' AND '2024-03-31'
         AND event_type = 'purchase'
         AND amount > 100
```

**Selectivity:**

$$
\text{sel}_{\text{total}} = \text{sel}_{\text{date}} \times \text{sel}_{\text{type}} \times \text{sel}_{\text{amount}}
$$

$$
= 0.25 \times 0.10 \times 0.05 = 0.00125 \quad (0.125\%)
$$

**Optimization:** Date filter uses index, others applied as scan filters.

**Alternative with Composite Index:**

```sql
-- Index: CREATE INDEX events_date_type_idx ON events(event_date, event_type);
```

**Ra Plan:**

```
IndexRangeScan [events.date_type_idx]
  Filter: event_date BETWEEN '2024-01-01' AND '2024-03-31'
         AND event_type = 'purchase'
  HeapFilter: amount > 100
```

**Benefit:** Narrower index scan (filters on both date and type).

### Partitioned Table Query

```sql
-- Table partitioned by month: orders_2024_01, orders_2024_02, orders_2024_03
SELECT SUM(total)
FROM orders
WHERE order_date BETWEEN '2024-02-15' AND '2024-03-15';
```

**Ra Plan:**

```
Aggregate
  Aggregates: SUM(total)
  Append
    SeqScan [orders_2024_02]  -- Partition pruned: only Feb, Mar
      Filter: order_date BETWEEN '2024-02-15' AND '2024-03-15'
    SeqScan [orders_2024_03]
      Filter: order_date BETWEEN '2024-02-15' AND '2024-03-15'
```

**Optimization:** January partition eliminated by pruning.

**Cost Reduction:**

$$
\text{Cost}_{\text{pruned}} = \frac{2}{12} \times \text{Cost}_{\text{full scan}} \approx 17\% \text{ of full cost}
$$

### Timestamp with Time Zone

```sql
SELECT COUNT(*)
FROM events
WHERE created_at AT TIME ZONE 'UTC' BETWEEN '2024-01-01 00:00:00' AND '2024-01-31 23:59:59';
```

**Ra Plan:**

```
Aggregate
  Aggregates: COUNT(*)
  IndexRangeScan [events.created_at_idx]
    Filter: created_at BETWEEN '2024-01-01 00:00:00+00' AND '2024-01-31 23:59:59+00'
```

**Optimization:** If `created_at` stored as UTC, timezone conversion eliminated.

### Open-Ended Range (Recent Data)

```sql
SELECT *
FROM logs
WHERE log_time > '2024-03-20 00:00:00'
ORDER BY log_time DESC
LIMIT 100;
```

**Ra Plan:**

```
Limit (100)
  IndexRangeScan [logs.log_time_idx] (Backward)
    Filter: log_time > '2024-03-20 00:00:00'
```

**Optimizations:**
1. Backward index scan (newest first)
2. Limit stops after 100 rows
3. No sort needed (index provides ordering)

**Cost:** $O(\log N + 100)$ vs $O(N)$ for sequential scan.

## Advanced Patterns

### Date Truncation

```sql
SELECT DATE_TRUNC('month', order_date) as month, COUNT(*)
FROM orders
WHERE order_date BETWEEN '2024-01-01' AND '2024-12-31'
GROUP BY DATE_TRUNC('month', order_date);
```

**Ra Plan:**

```
HashAggregate [DATE_TRUNC('month', order_date)]
  Aggregates: COUNT(*)
  IndexRangeScan [orders.order_date_idx]
    Filter: order_date BETWEEN '2024-01-01' AND '2024-12-31'
```

**Optimization:** Consider functional index:

```sql
CREATE INDEX orders_month_idx ON orders(DATE_TRUNC('month', order_date));
```

Then GROUP BY can use index directly.

### Overlapping Ranges

```sql
-- Find active subscriptions on a given date
SELECT *
FROM subscriptions
WHERE '2024-03-15' BETWEEN start_date AND end_date;
```

**Ra Plan:**

```
SeqScan [subscriptions]
  Filter: '2024-03-15' >= start_date AND '2024-03-15' <= end_date
```

**Challenge:** Hard to index efficiently (needs multi-column range).

**Optimization:** Use interval indexing (GiST):

```sql
CREATE INDEX subscriptions_period_idx ON subscriptions USING GIST (daterange(start_date, end_date));
```

**Ra Plan (with GiST):**

```
IndexScan [subscriptions.period_gist_idx]
  Filter: daterange(start_date, end_date) @> '2024-03-15'
```

## Performance Characteristics

| Range Size | Selectivity | Preferred Method | Expected Cost |
|------------|-------------|-----------------|---------------|
| 1 day | 0.001 | Index range scan | $O(\log N + 0.001N)$ |
| 1 month | 0.03 | Index range scan | $O(\log N + 0.03N)$ |
| 1 year | 0.30 | Sequential scan | $O(N)$ |
| All data | 1.00 | Sequential scan | $O(N)$ |

**Threshold:** Ra uses index when $\text{sel} < 0.05$ typically.

## Anti-Patterns

### 1. Function on Indexed Column

❌ **Bad:**
```sql
WHERE DATE(timestamp_column) = '2024-03-15'
```

Function prevents index usage.

✅ **Good:**
```sql
WHERE timestamp_column >= '2024-03-15 00:00:00'
  AND timestamp_column < '2024-03-16 00:00:00'
```

### 2. OR with Non-Temporal Predicates

❌ **Bad:**
```sql
WHERE created_at < '2024-01-01' OR status = 'archived'
```

Forces sequential scan.

✅ **Good:**
```sql
-- Split into UNION
SELECT * FROM table WHERE created_at < '2024-01-01'
UNION ALL
SELECT * FROM table WHERE status = 'archived' AND created_at >= '2024-01-01';
```

### 3. Implicit Type Conversion

❌ **Bad:**
```sql
-- If timestamp column
WHERE timestamp_column = '2024-03-15'  -- Implicit conversion
```

✅ **Good:**
```sql
WHERE timestamp_column >= '2024-03-15 00:00:00'
  AND timestamp_column < '2024-03-16 00:00:00'
```

## See Also

- [Range Scan](../oltp/range-scan.md) - Index range scans
- [Time Series Aggregation](time-series-aggregation.md) - Temporal analytics
- [Temporal Joins](temporal-joins.md) - Time-based joins
- [Partitioned Tables](../../schema-patterns/partitioned-tables.md) - Temporal partitioning
- [B-tree Indexes](../../index-structures/btree.md) - Range index scans

## References

- Snodgrass, *Developing Time-Oriented Database Applications in SQL*, Morgan Kaufmann, 1999
- PostgreSQL: [Date/Time Types](https://www.postgresql.org/docs/current/datatype-datetime.html)
- ISO 8601 Standard for date/time representation
