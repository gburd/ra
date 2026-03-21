# Rule: Retraction Optimization (Flink)

**Category:** database-specific/flink
**File:** `rules/database-specific/flink/retract-optimization.rra`

## Metadata

- **ID:** `flink-retract-optimization`
- **Version:** "1.0.0"
- **Databases:** flink
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Retraction Optimization (Flink)

## Metadata
- **Rule ID**: `flink-retraction-optimization`
- **Category**: Database-Specific / Flink
- **Source**: flink

## Description

Flink minimizes retractions in changelog streams by detecting operations that don't require retractions (append-only, monotonic aggregates).

## Test Cases

### Test 1: Append-only aggregation (no retractions)
```sql
SELECT COUNT(*) FROM events;

-- COUNT is monotonic (always increasing)
-- No retractions generated
-- More efficient state management
```

## Tags
`database-specific`, `flink`, `retraction`, `changelog`, `streaming`
