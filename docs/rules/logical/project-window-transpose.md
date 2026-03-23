# Rule: "ProjectWindowTranspose"

**Category:** logical/window-pushdown
**File:** `rules/logical/window-pushdown/project-window-transpose.rra`

## Metadata

- **ID:** `project-window-transpose`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# ProjectWindowTranspose

## Description

Transposes projection and window

## Relational Algebra

```algebra
$\pi$(Window(R)) => optimization
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
