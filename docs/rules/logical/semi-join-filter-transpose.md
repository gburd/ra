# Rule: "Semi Join Filter Transpose"

**Category:** logical/join-elimination
**File:** `rules/logical/join-elimination/semi-join-filter-transpose.rra`

## Metadata

- **ID:** `semi-join-filter-transpose`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# Semi Join Filter Transpose

## Description

Transposes filter with semi-join

## Relational Algebra

```algebra
σ(SemiJoin(R, S)) => SemiJoin(σ(R), S)
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
