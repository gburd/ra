# Rule: "SortJoinTranspose"

**Category:** logical/limit-pushdown
**File:** `rules/logical/limit-pushdown/sort-join-transpose.rra`

## Metadata

- **ID:** `sort-join-transpose`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# SortJoinTranspose

## Description

Transposes sort and join

## Relational Algebra

```algebra
Sort(Join(R, S))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
