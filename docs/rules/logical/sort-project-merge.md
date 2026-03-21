# Rule: "Sort Project Merge"

**Category:** logical/projection-pushdown
**File:** `rules/logical/projection-pushdown/sort-project-merge.rra`

## Metadata

- **ID:** `sort-project-merge`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# Sort Project Merge

## Description

Merges sort and project operations

## Relational Algebra

```algebra
Sort(π(R)) => optimization
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
