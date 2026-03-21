# Rule: "Semi Join Project Transpose"

**Category:** logical/join-elimination
**File:** `rules/logical/join-elimination/semi-join-project-transpose.rra`

## Metadata

- **ID:** `semi-join-project-transpose`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# Semi Join Project Transpose

## Description

Transposes projection with semi-join

## Relational Algebra

```algebra
π(SemiJoin(R, S)) => optimization
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
