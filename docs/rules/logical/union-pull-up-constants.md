# Rule: "Union Pull Up Constants"

**Category:** logical/set-operations
**File:** `rules/logical/set-operations/union-pull-up-constants.rra`

## Metadata

- **ID:** `union-pull-up-constants`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# Union Pull Up Constants

## Description

Pulls up constant expressions in union

## Relational Algebra

```algebra
Union(π_const(R), π_const(S))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
