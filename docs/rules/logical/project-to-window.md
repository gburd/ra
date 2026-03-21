# Rule: "ProjectToWindow"

**Category:** logical/window-pushdown
**File:** `rules/logical/window-pushdown/project-to-window.rra`

## Metadata

- **ID:** `project-to-window`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# ProjectToWindow

## Description

Converts projection to window function

## Relational Algebra

```algebra
π(R) => Window(R)
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
