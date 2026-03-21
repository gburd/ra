# Rule: "AggregateUnionTranspose"

**Category:** logical/set-operations
**File:** `rules/logical/set-operations/aggregate-union-transpose.rra`

## Metadata

- **ID:** `aggregate-union-transpose`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# AggregateUnionTranspose

## Description

Pushes aggregation through union

## Relational Algebra

```algebra
Agg(Union(R, S)) => Union(Agg(R), Agg(S))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
