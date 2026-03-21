# Rule: "AggregateProjectMerge"

**Category:** logical/aggregate-pushdown
**File:** `rules/logical/aggregate-pushdown/aggregate-project-merge.rra`

## Metadata

- **ID:** `aggregate-project-merge`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"


# AggregateProjectMerge

## Description

Merges projection into aggregate

## Relational Algebra

```algebra
π(Agg(π(R))) => Agg(π(R))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
