# Rule: "JoinToHyperGraph"

**Category:** logical/join-reordering
**File:** `rules/logical/join-reordering/join-to-hyper-graph.rra`

## Metadata

- **ID:** `join-to-hyper-graph`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, calcite
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(multi-join ?inputs ?predicates)"
    description: "Multi-way join to convert to hypergraph"
  - type: "predicate"
    condition: "count(?inputs) >= 3"
    description: "At least 3 relations for hypergraph representation"
```


# JoinToHyperGraph

## Description

Converts join tree to hypergraph

## Relational Algebra

```algebra
Join tree => hypergraph for WCOJ
```

## Implementation

```
Implement rule transformation for calcite optimization
```

## Tests

Add test cases for this rule

## References
