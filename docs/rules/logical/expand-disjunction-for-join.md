# Rule: "Expand Disjunction For Join"

**Category:** logical/predicate-pushdown
**File:** `rules/logical/predicate-pushdown/expand-disjunction-for-join.rra`

## Metadata

- **ID:** `expand-disjunction-for-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter (or ?p1 ?p2) (join ?type ?cond ?left ?right))"
    description: "Disjunctive filter above a join"
  - type: "predicate"
    condition: "each_disjunct_references_single_side(?p1, ?p2, ?left, ?right)"
    description: "Each disjunct must reference only one join side"
```


# Expand Disjunction For Join

## Description

Expands OR conditions in join predicates

## Relational Algebra

```algebra
(A OR B) in join => optimization
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
