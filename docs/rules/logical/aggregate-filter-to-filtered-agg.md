# Rule: "Aggregate Filter to Filtered Aggregate"

**Category:** logical/aggregate-pushdown
**File:** `rules/logical/aggregate-pushdown/aggregate-filter-to-filtered-agg.rra`

## Metadata

- **ID:** `aggregate-filter-to-filtered-agg`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# Aggregate Filter to Filtered Aggregate

## Description

Uses SQL FILTER clause for aggregates

## Relational Algebra

```algebra
COUNT(*) FILTER (WHERE cond)
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
