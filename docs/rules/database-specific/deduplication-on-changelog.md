# Rule: Deduplication on Changelog Stream (Flink)

**Category:** database-specific/flink
**File:** `rules/database-specific/flink/deduplication-on-changelog.rra`

## Metadata

- **ID:** `flink-deduplication-on-changelog`
- **Version:** "1.0.0"
- **Databases:** flink
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Deduplication on Changelog Stream (Flink)

## Metadata
- **Rule ID**: `flink-deduplication-changelog`
- **Category**: Database-Specific / Flink
- **Source**: flink Table API

## Description

Flink optimizes DISTINCT / FIRST_VALUE / LAST_VALUE on changelog streams (with retractions) using state-efficient deduplication operators.

## Test Cases

### Test 1: Deduplication with ROW_NUMBER
```sql
SELECT user_id, event_time, event_data
FROM (
  SELECT *,
         ROW_NUMBER() OVER (PARTITION BY user_id ORDER BY event_time DESC) as rn
  FROM events
) WHERE rn = 1;

-- Optimized to DeduplicateOperator
-- Keeps only latest event per user in state
```

## References
1. **Flink Docs**: "Deduplication"

## Tags
`database-specific`, `flink`, `deduplication`, `changelog`, `streaming`
