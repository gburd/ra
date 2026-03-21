# Rule: "Filter Into Join"

**Category:** logical/predicate-pushdown
**File:** `rules/logical/predicate-pushdown/filter-into-join.rra`

## Metadata

- **ID:** `filter-into-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter ?pred (join ?type ?cond ?left ?right))"
    description: "Filter above a join"
  - type: "predicate"
    condition: "is_deterministic(?pred)"
    description: "Predicate must be deterministic"
  - type: "predicate"
    condition: "can_push_into_join(?pred, ?type, ?left, ?right)"
    description: "Predicate must be safe to merge into join condition"
```


# Filter Into Join

## Description

Pushes predicates into join operands

## Relational Algebra

```algebra
σ(Join) => Join(σ(R), σ(S))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
