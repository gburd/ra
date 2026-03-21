# Rule: "SortMerge"

**Category:** physical/join-algorithms
**File:** `rules/physical/join-algorithms/sort-merge.rra`

## Metadata

- **ID:** `sort-merge`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(join inner (= ?lcol ?rcol) ?left ?right)"
    description: "Sort-merge join implementation"
  - type: "predicate"
    condition: "is_equijoin(= ?lcol ?rcol)"
    description: "Must be an equi-join (or inequality with sort)"
```


# SortMerge

## Description

Sort-merge join implementation

## Relational Algebra

```algebra
Join_sorted(sort(R), sort(S))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
