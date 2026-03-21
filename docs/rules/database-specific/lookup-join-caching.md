# Rule: Lookup Join Caching (Flink)

**Category:** database-specific/flink
**File:** `rules/database-specific/flink/lookup-join-caching.rra`

## Metadata

- **ID:** `flink-lookup-join-caching`
- **Version:** "1.0.0"
- **Databases:** flink
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Lookup Join Caching (Flink)

## Metadata
- **Rule ID**: `flink-lookup-join-caching`
- **Category**: Database-Specific / Flink
- **Source**: flink

## Description

Flink caches lookup join results (external table lookups) to reduce repeated queries to external systems.

## Test Cases

### Test 1: Cached JDBC lookup
```sql
SELECT e.*, u.name
FROM events e
LEFT JOIN users FOR SYSTEM_TIME AS OF e.event_time AS u
  ON e.user_id = u.user_id;

-- With caching:
-- user_id=123 looked up once, cached for 5 minutes
-- Subsequent events for user_id=123 use cache
-- Reduces JDBC queries by 90%+
```

## Tags
`database-specific`, `flink`, `lookup-join`, `caching`, `external-table`
