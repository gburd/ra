# Rule: "FilterAggregateTranspose"

**Category:** logical/predicate-pushdown
**File:** `rules/logical/predicate-pushdown/filter-aggregate-transpose.rra`

## Metadata

- **ID:** `filter-aggregate-transpose`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter ?pred (aggregate ?input ?groups ?aggs))"
    description: "Filter above an aggregation"
  - type: "predicate"
    condition: "references_only(?pred, ?groups)"
    description: "Filter predicate must reference only grouping columns"
  - type: "predicate"
    condition: "is_deterministic(?pred)"
    description: "Predicate must be deterministic"
```


# FilterAggregateTranspose

## Description

Transposes filter and aggregate

## Relational Algebra

```algebra
$\sigma$(Agg(R)) => Agg($\sigma$(R))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
