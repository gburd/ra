# Rule: Parquet Predicate Pushdown (Impala)

**Category:** database-specific/impala
**File:** `rules/database-specific/impala/parquet-predicate-pushdown.rra`

## Metadata

- **ID:** `impala-parquet-predicate-pushdown`
- **Version:** "1.0.0"
- **Databases:** impala
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Parquet Predicate Pushdown (Impala)

## Metadata
- **Rule ID**: `impala-parquet-predicate-pushdown`
- **Category**: Database-Specific / Impala
- **Source**: Apache Impala

## Description

Impala pushes predicates into Parquet readers, leveraging:
1. Column statistics (min/max in row groups)
2. Dictionary encoding (membership testing)
3. Page-level statistics

**Benefit**: Skip entire row groups without reading data.

## Test Cases

### Test 1: Min/max filtering
```sql
SELECT * FROM parquet_table
WHERE year = 2024 AND month = 3;

-- Parquet row groups have min/max for year, month
-- Skip row groups where max(year) < 2024 or min(year) > 2024
-- Reduces scan from 1000 row groups to ~100
```

## References
1. **Impala Docs**: "Parquet Optimization"

## Tags
`database-specific`, `impala`, `parquet`, `predicate-pushdown`, `columnar`
