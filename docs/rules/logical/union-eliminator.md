# Rule: "UnionEliminator"

**Category:** logical/set-operations
**File:** `rules/logical/set-operations/union-eliminator.rra`

## Metadata

- **ID:** `union-eliminator`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# UnionEliminator

## Description

Eliminates unnecessary union

## Relational Algebra

```algebra
Union(R, R) => R
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
