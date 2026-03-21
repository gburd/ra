# Rule: Watermark Pushdown (Flink)

**Category:** database-specific/flink
**File:** `rules/database-specific/flink/watermark-pushdown.rra`

## Metadata

- **ID:** `flink-watermark-pushdown`
- **Version:** "1.0.0"
- **Databases:** flink
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Watermark Pushdown (Flink)

## Metadata
- **Rule ID**: `flink-watermark-pushdown`
- **Category**: Database-Specific / Flink
- **Source**: flink
- **Docs**: https://nightlies.apache.org/flink/flink-docs-stable/docs/dev/datastream/event-time/generating_watermarks/

## Description

Flink pushes watermark generation as close to sources as possible to enable early time-based operations (windows, joins, deduplication).

**Key benefit**: Early watermarks enable earlier event-time processing and state cleanup.

## Relational Algebra

```
Window(Filter(Source_with_watermark(S)))
→ Window(Source_with_watermark(Filter(S)))
```

## Test Cases

### Test 1: Watermark at source
```sql
CREATE TABLE kafka_source (
    event_id BIGINT,
    event_time TIMESTAMP(3),
    WATERMARK FOR event_time AS event_time - INTERVAL '5' SECOND
) WITH ('connector' = 'kafka', ...);

SELECT TUMBLE_START(event_time, INTERVAL '1' MINUTE), COUNT(*)
FROM kafka_source
GROUP BY TUMBLE(event_time, INTERVAL '1' MINUTE);

-- Watermark generated at Kafka source
-- Windows triggered as soon as watermark passes
```

## References

1. **Flink Docs**: "Generating Watermarks"

## Tags
`database-specific`, `flink`, `watermark`, `stream-processing`, `event-time`
