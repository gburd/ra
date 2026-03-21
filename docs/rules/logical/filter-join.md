# Rule: "FilterJoin"

**Category:** logical/predicate-pushdown
**File:** `rules/logical/predicate-pushdown/filter-join.rra`

## Metadata

- **ID:** `filter-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter ?pred (join inner ?cond ?left ?right))"
    description: "Filter above an inner join"
  - type: "predicate"
    condition: "is_deterministic(?pred)"
    description: "Predicate must be deterministic"
```


# FilterJoin

## Description

Pushes filter through join

## Relational Algebra

```algebra
σ(Join(R, S)) => Join(σ(R), S)
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
