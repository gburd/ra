# Rule: Mini-Batch Aggregation (Flink)

**Category:** database-specific/flink
**File:** `rules/database-specific/flink/minibatch-aggregation.rra`

## Metadata

- **ID:** `flink-minibatch-aggregation`
- **Version:** "1.0.0"
- **Databases:** flink
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Mini-Batch Aggregation (Flink)

## Metadata
- **Rule ID**: `flink-minibatch-aggregation`
- **Category**: Database-Specific / Flink
- **Source**: flink Table API

## Description

Flink batches multiple input records before updating aggregation state, reducing state backend overhead and increasing throughput.

**Configuration:**
- `table.exec.mini-batch.enabled = true`
- `table.exec.mini-batch.allow-latency = 5s`
- `table.exec.mini-batch.size = 1000`

## Relational Algebra

```
// Per-record aggregation
FOR EACH record: UPDATE state

// Mini-batch aggregation
BATCH records (size=1000 OR latency=5s)
THEN: UPDATE state once per batch
```

## Implementation Pattern

```java
// Accumulate events in buffer
Buffer<Event> buffer = new Buffer<>(maxSize, maxLatency);

public void processElement(Event event) {
    buffer.add(event);

    if (buffer.shouldFlush()) {
        // Process batch
        Map<Key, List<Event>> batched = buffer.groupByKey();
        for (Map.Entry<Key, List<Event>> entry : batched.entrySet()) {
            AggregateState state = getState(entry.getKey());
            for (Event e : entry.getValue()) {
                state.accumulate(e);
            }
            updateState(entry.getKey(), state);
        }
        buffer.clear();
    }
}
```

## Test Cases

### Test 1: High-frequency updates
```sql
-- Configuration
SET 'table.exec.mini-batch.enabled' = 'true';
SET 'table.exec.mini-batch.allow-latency' = '5s';
SET 'table.exec.mini-batch.size' = '1000';

-- High-cardinality GROUP BY
SELECT user_id, COUNT(*), SUM(amount)
FROM click_stream
GROUP BY user_id;

-- Without mini-batch: State update per event (high overhead)
-- With mini-batch: State update per 1000 events or 5s (much lower overhead)
```

## References

1. **Flink Docs**: "Performance Tuning"
   - https://nightlies.apache.org/flink/flink-docs-stable/docs/dev/table/tuning/

## Tags
`database-specific`, `flink`, `aggregation`, `mini-batch`, `performance`, `streaming`
