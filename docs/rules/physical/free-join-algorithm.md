# Rule: "Free Join Algorithm"

**Category:** physical/join-algorithms
**File:** `rules/physical/join-algorithms/free-join-algorithm.rra`

## Metadata

- **ID:** `free-join-algorithm`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, academic
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(multi-join ?inputs ?predicates)"
    description: "Multi-way join for free join algorithm"
  - type: "predicate"
    condition: "count(?inputs) >= 3"
    description: "At least 3 inputs for free join benefit"
  - type: "predicate"
    condition: "has_cyclic_join_graph(?inputs, ?predicates)"
    description: "Cyclic join graphs benefit most from free join"
    optional: true
```


# Free Join Algorithm

## Description

Worst-case optimal join algorithm for cyclic queries

## Relational Algebra

```algebra

```

## Implementation

```
Implement rule transformation for academic optimization
```

## Tests

Add test cases for this rule

## References
- Paper: Free Join by Ngo et al.
- DOI: 10.1145/2463676.2465314
