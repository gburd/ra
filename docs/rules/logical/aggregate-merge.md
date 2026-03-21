# Rule: "AggregateMerge"

**Category:** logical/aggregate-pushdown
**File:** `rules/logical/aggregate-pushdown/aggregate-merge.rra`

## Metadata

- **ID:** `aggregate-merge`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# AggregateMerge

## Description

Merges two consecutive aggregates

## Relational Algebra

```algebra
Agg2(Agg1(R)) => Agg_merged(R)
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
