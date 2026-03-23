# Rule: "Filter Window Transpose"

**Category:** logical/window-pushdown
**File:** `rules/logical/window-pushdown/filter-window-transpose.rra`

## Metadata

- **ID:** `filter-window-transpose`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# Filter Window Transpose

## Description

Pushes filters through window functions

## Relational Algebra

```algebra
$\sigma$(Window(R)) => Window($\sigma$(R))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
