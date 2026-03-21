# Rule: "AggregateExpandDistinctAggregates"

**Category:** logical/aggregate-pushdown
**File:** `rules/logical/aggregate-pushdown/aggregate-expand-distinct-aggregates.rra`

## Metadata

- **ID:** `aggregate-expand-distinct-aggregates`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# AggregateExpandDistinctAggregates

## Description

Expands distinct aggregates into union

## Relational Algebra

```algebra
COUNT(DISTINCT col) => optimized
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
