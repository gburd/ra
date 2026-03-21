# Rule: Fragment Result Caching (Presto)

**Category:** database-specific/presto
**File:** `rules/database-specific/presto/fragment-result-caching.rra`

## Metadata

- **ID:** `presto-fragment-result-caching`
- **Version:** "1.0.0"
- **Databases:** presto
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Fragment Result Caching (Presto)

## Metadata
- **Rule ID**: `presto-fragment-caching`
- **Category**: Database-Specific / Presto/Trino
- **Source**: Presto

## Description

Presto caches intermediate results of query fragments, enabling reuse across similar queries or repeated executions.

## Test Cases

### Test 1: Repeated subquery
```sql
-- Query 1
SELECT * FROM expensive_view WHERE date = '2024-03-01';

-- Query 2 (same subquery)
SELECT * FROM expensive_view WHERE date = '2024-03-01' AND user_id < 1000;

-- Fragment cache:
-- expensive_view result cached from Query 1
-- Query 2 reuses cached result
```

## References
1. **Presto Docs**: "Fragment Result Caching"

## Tags
`database-specific`, `presto`, `caching`, `fragment`, `reuse`
