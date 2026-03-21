# Rule: Retraction (Changelog) Optimization

**Category:** execution-models/streaming
**File:** `rules/execution-models/streaming/retraction-optimization.rra`

## Metadata

- **ID:** `retraction-optimization`
- **Version:** "1.0.0"
- **Databases:** materialize, flink, ksqldb, risingwave
- **Tags:** streaming, retraction, changelog, differential, incremental
- **Authors:** "McSherry, Murray, Isaacs", "Materialize Inc."


# Retraction (Changelog) Optimization

## Description

Optimizes incremental view maintenance in streaming systems by minimizing
retraction (undo) messages. When an upstream change invalidates a previous
result, the system must retract the old value and emit the new one.
This rule consolidates retractions, eliminates redundant retract-then-insert
pairs, and pushes retraction processing to the narrowest point in the
dataflow graph.

**When to apply**: Streaming queries with stateful operators (joins,
aggregations) that produce retractions on upstream changes.

## Relational Algebra

```algebra
-- Before: separate retract + insert for key update
Retract(key=1, old_val=10)
Insert(key=1, new_val=15)

-- After: consolidated update message
Update(key=1, old_val=10, new_val=15)

-- Before: retract propagates through entire join tree
Retract -> Join -> Aggregate -> Output

-- After: retract resolved at earliest stateful operator
Update -> Join(stateful, handles delta) -> Output
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("consolidate-retract-insert";
    "(seq (retract ?key ?old) (insert ?key ?new))" =>
    "(update ?key ?old ?new)"
),

rw!("push-retract-into-join";
    "(retract-through (join ?left ?right))" =>
    "(delta-join (retract-local ?left) ?right)"
    if join_maintains_state("?left", "?right")
),
```

## Preconditions

```rust
fn applicable(dataflow: &StreamDataflow) -> bool {
    dataflow.has_stateful_operators()
        && dataflow.produces_retractions()
        // Delta processing must be supported
        && dataflow.operators_support_delta()
}
```

**Restrictions:**
- Operators must support delta/differential processing
- Non-monotonic operators (NOT IN, EXCEPT) produce many retractions
- State cleanup requires garbage collection of old entries

## Cost Model

```rust
fn estimated_benefit(
    retractions_per_second: f64,
    consolidation_rate: f64,
) -> f64 {
    let messages_saved = retractions_per_second * consolidation_rate;
    messages_saved * 0.001 // processing cost per message
}
```

**Typical benefit**: 30-90% reduction in retraction messages.

## Test Cases

```sql
-- Positive: aggregate with frequent updates
CREATE MATERIALIZED VIEW sales_by_region AS
SELECT region, SUM(amount) FROM orders GROUP BY region;
-- UPDATE to orders produces retract(old_sum) + insert(new_sum)
-- Consolidated to single update message

-- Positive: join with primary key update
CREATE MATERIALIZED VIEW enriched AS
SELECT o.*, c.name FROM orders o JOIN customers c ON o.cust_id = c.id;
-- Customer name update: delta-join processes only changed rows

-- Negative: no retractions (append-only stream)
SELECT COUNT(*) FROM sensor_readings;
-- Only inserts: no retraction optimization needed
```

## References

- McSherry, F. et al. "Differential Dataflow" (CIDR 2013)
- Materialize: Differential Dataflow engine documentation
