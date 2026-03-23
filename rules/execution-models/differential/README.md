# Differential Dataflow Execution Model

Rules specific to differential dataflow / streaming execution as implemented in **Materialize**.

## Overview

Differential dataflow is an incremental computation framework that maintains materialized views efficiently by tracking changes (deltas) rather than recomputing from scratch.

## Core Concepts

### Data Model

Data is represented as `(data, time, multiplicity)` triples:
- **data**: The actual value
- **time**: Logical timestamp
- **multiplicity**: +1 for insertion, -1 for deletion

```
Example:
(order_id=123, amount=1500, time=10, count=+1)  // Insert
(order_id=100, amount=500,  time=15, count=-1)  // Delete
```

### Timely Dataflow

- Computation organized as dataflow graph
- Data flows through operators with timestamps
- Progress tracking ensures correctness
- Enables pipelined parallel execution

### Arrangements

Indexed views maintained incrementally:
```sql
-- Query needs to join orders and customers
-- Optimizer maintains arrangements:
Arrangement A: orders indexed by customer_id
Arrangement B: customers indexed by id
```

Arrangements are expensive (memory), so selection is critical.

## Optimization Rules for Differential

### 1. Arrangement Selection

**Rule:** `arrangement-selection.rra`

Choose which indexes to maintain based on:
- Query access patterns
- Join selectivity
- Update frequency
- Available memory

```rust
// Bad: Arrange large table on high-cardinality column
arrange(orders).by(order_id)  // Million keys

// Good: Arrange on join key
arrange(orders).by(customer_id)  // Thousand keys
```

### 2. Join Order for Incrementality

**Rule:** `join-order-for-incrementality.rra`

Prefer to join small delta against large stable collection:

```
Bad:  delta(large_table) $\bowtie$ stable(small_table)
      -> Large delta, expensive to process

Good: delta(small_table) $\bowtie$ stable(large_table)
      -> Small delta, cheap updates
```

### 3. Temporal Filter Pushdown

**Rule:** `temporal-filter-pushdown.rra`

Push temporal predicates to source to maintain only relevant data:

```sql
-- Push temporal filter to source
SELECT * FROM events
WHERE event_time > NOW() - INTERVAL '1 hour'

-- Maintains only 1 hour sliding window
-- Garbage collects old data automatically
```

### 4. Differential Join

**Rule:** `differential-join.rra`

Special join implementation that processes deltas efficiently:

```
Standard Join:
  Output = Left $\bowtie$ Right  // Full recomputation

Differential Join:
  $\Delta$ Output = ($\Delta$ Left $\bowtie$ Right) $\cup$ (Left $\bowtie$ $\Delta$ Right)
  // Only process changes
```

### 5. Arrangement Sharing

**Rule:** `arrangement-sharing.rra`

Reuse arrangements across multiple queries:

```sql
-- Query 1
SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id

-- Query 2
SELECT * FROM returns r JOIN customers c ON r.customer_id = c.id

-- Optimizer: Share customers arrangement indexed by id
```

### 6. Delta Query Optimization

**Rule:** `delta-query-optimization.rra`

Optimize for common pattern of querying recent changes:

```sql
-- Get new orders in last 5 minutes
SELECT * FROM orders
WHERE created_at > NOW() - INTERVAL '5 minutes'

-- Materialize: Use delta collection directly
-- No need to scan entire table
```

### 7. Incremental Aggregation

**Rule:** `incremental-aggregation.rra`

Maintain aggregates incrementally:

```sql
-- Count orders per customer
SELECT customer_id, COUNT(*)
FROM orders
GROUP BY customer_id

-- Update rule:
$\Delta$ count = +1 for new order
$\Delta$ count = -1 for deleted order
// No full recount needed
```

## When to Use Differential Rules

**Best for:**
- Real-time analytics dashboards
- Streaming ETL pipelines
- Change data capture (CDC)
- Incremental view maintenance
- Temporal queries

**Avoid for:**
- One-shot batch queries (no benefit from incrementality)
- Queries without repeated execution
- Very high update rates (arrangement overhead)

## Materialize-Specific Features

### 1. Sources and Sinks

```sql
-- Create source from Kafka
CREATE SOURCE orders_source
FROM KAFKA BROKER 'localhost:9092'
TOPIC 'orders'
FORMAT AVRO USING CONFLUENT SCHEMA REGISTRY 'http://localhost:8081';

-- Materialize maintains source incrementally
```

### 2. Materialized Views

```sql
-- Create incrementally maintained view
CREATE MATERIALIZED VIEW high_value_orders AS
SELECT customer_id, SUM(amount) as total
FROM orders
WHERE amount > 1000
GROUP BY customer_id;

-- Updates in real-time as orders arrive
```

### 3. Temporal Filters

```sql
-- Sliding window aggregation
CREATE MATERIALIZED VIEW recent_orders AS
SELECT customer_id, COUNT(*) as count
FROM orders
WHERE mz_logical_timestamp() <= (mz_now() + INTERVAL '1 hour')
GROUP BY customer_id;

-- Materialize: Maintains only 1 hour window
-- Auto garbage collects old data
```

## Performance Characteristics

**Memory Usage:**
```
Traditional View: O(result_size)
Differential View: O(result_size + arrangement_size)
```

Arrangements add memory overhead but enable incremental updates.

**Update Latency:**
```
Traditional: O(n) full recomputation
Differential: O($\Delta$) process only changes
```

For small deltas, differential is orders of magnitude faster.

## Example Rules to Implement

1. **arrangement-selection.rra** - Choose optimal arrangements
2. **temporal-filter-pushdown.rra** - Push time-based filters
3. **join-order-for-incrementality.rra** - Optimal join order for deltas
4. **differential-join.rra** - Efficient delta join
5. **arrangement-sharing.rra** - Share indexes across queries
6. **delta-query-optimization.rra** - Optimize for recent changes
7. **incremental-aggregation.rra** - Maintain aggregates incrementally
8. **garbage-collection.rra** - Remove obsolete data
9. **arrangement-compaction.rra** - Compact arrangements periodically
10. **index-selection-for-joins.rra** - Choose join indexes

## References

**Materialize Documentation:**
- https://materialize.com/docs/overview/how-materialize-works/
- https://materialize.com/docs/sql/create-materialized-view/
- https://materialize.com/docs/transform-data/patterns/

**Academic Papers:**
- McSherry, Frank, et al. "Differential Dataflow." CIDR 2013.
- Murray, Derek G., et al. "Naiad: A Timely Dataflow System." SOSP 2013.
- McSherry, Frank, et al. "Scalability! But at what COST?" HotOS 2015.

**Source Code:**
- Materialize: https://github.com/MaterializeInc/materialize
- Differential Dataflow: https://github.com/TimelyDataflow/differential-dataflow
- Timely Dataflow: https://github.com/TimelyDataflow/timely-dataflow
