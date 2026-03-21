# Rule: "Filter Set Op Transpose"

**Category:** logical/set-operations
**File:** `rules/logical/set-operations/filter-set-op-transpose.rra`

## Metadata

- **ID:** `filter-set-op-transpose`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# Filter Set Op Transpose

## Description

Pushes filters through set operations

## Relational Algebra

```algebra
σ(Union(R, S)) => Union(σ(R), σ(S))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
