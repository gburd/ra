# Rule: "ProjectAggregateMerge"

**Category:** logical/projection-pushdown
**File:** `rules/logical/projection-pushdown/project-aggregate-merge.rra`

## Metadata

- **ID:** `project-aggregate-merge`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# ProjectAggregateMerge

## Description

Merges projection into aggregate

## Relational Algebra

```algebra
$\pi$(Agg(R)) => Agg with projection
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
