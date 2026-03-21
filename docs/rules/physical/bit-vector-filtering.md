# Rule: "Bit Vector Filtering"

**Category:** physical/join-algorithms
**File:** `rules/physical/join-algorithms/bit-vector-filtering.rra`

## Metadata

- **ID:** `bit-vector-filtering`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, academic
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(join inner (= ?lcol ?rcol) ?left ?right)"
    description: "Equi-join with bit vector (semi-join) filtering"
  - type: "predicate"
    condition: "is_equijoin(= ?lcol ?rcol)"
    description: "Must be an equi-join"
  - type: "fact"
    fact_type: "statistics.cardinality"
    table: "?left"
    comparator: ">"
    threshold: 10000
    optional: true
    description: "Large enough inputs to benefit from bit vector filtering"
```


# Bit Vector Filtering

## Description

Bloom filter based join optimization

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

- Paper: Bloom Filters for Join
- DOI: 10.1145/1687553.1687559
