# Rule: "Filter Transpose Aggregate"

**Category:** logical/predicate-pushdown
**File:** `rules/logical/predicate-pushdown/filter-into-aggregate.rra`

## Metadata

- **ID:** `filter-into-aggregate`
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
    condition: "references_only(?pred, columns(?input))"
    description: "Filter predicate must reference pre-aggregation columns"
  - type: "predicate"
    condition: "!references_any(?pred, ?aggs)"
    description: "Filter must not reference aggregate results"
```


# Filter Transpose Aggregate

## Description

Transposes filters through aggregation

## Relational Algebra

```algebra
σ(Agg(R)) => Agg(σ(R))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
