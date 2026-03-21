# Rule: Window Aggregation Optimization (Flink)

**Category:** database-specific/flink
**File:** `rules/database-specific/flink/window-aggregation-optimization.rra`

## Metadata

- **ID:** `flink-window-aggregation-optimization`
- **Version:** "1.0.0"
- **Databases:** flink
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Window Aggregation Optimization (Flink)

## Metadata
- **Rule ID**: `flink-window-aggregation`
- **Category**: Database-Specific / Flink
- **Source**: flink

## Description

Flink optimizes windowed aggregations using incremental aggregation (update state per event) rather than buffering all events and aggregating at window close.

## Test Cases

### Test 1: Tumbling window with incremental aggregation
```sql
SELECT
  TUMBLE_START(event_time, INTERVAL '1' MINUTE) as window_start,
  user_id,
  COUNT(*) as event_count,
  SUM(amount) as total_amount
FROM events
GROUP BY TUMBLE(event_time, INTERVAL '1' MINUTE), user_id;

-- Incremental aggregation:
-- Each event updates COUNT and SUM in state
-- At window close, emit accumulated results
-- Memory: O(distinct user_ids) not O(all events in window)
```

## References
1. **Flink Docs**: "Window Aggregations"

## Tags
`database-specific`, `flink`, `window`, `aggregation`, `streaming`, `incremental`
