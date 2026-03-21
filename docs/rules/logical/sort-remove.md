# Rule: "SortRemove"

**Category:** logical/limit-pushdown
**File:** `rules/logical/limit-pushdown/sort-remove.rra`

## Metadata

- **ID:** `sort-remove`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# SortRemove

## Description

Removes redundant sort

## Relational Algebra

```algebra
Sort(already_sorted(R)) => R
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
