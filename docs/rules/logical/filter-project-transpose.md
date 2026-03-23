# Rule: "FilterProjectTranspose"

**Category:** logical/predicate-pushdown
**File:** `rules/logical/predicate-pushdown/filter-project-transpose.rra`

## Metadata

- **ID:** `filter-project-transpose`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter ?pred (project ?cols ?input))"
    description: "Filter above a projection"
  - type: "predicate"
    condition: "references_subset(?pred, columns(?input))"
    description: "Filter columns must exist in input before projection"
```


# FilterProjectTranspose

## Description

Transposes filter and projection

## Relational Algebra

```algebra
$\sigma$($\pi$(R)) => $\pi$($\sigma$(R))
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
