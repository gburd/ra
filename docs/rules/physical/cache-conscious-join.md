# Rule: "Cache-Conscious Join"

**Category:** physical/join-algorithms
**File:** `rules/physical/join-algorithms/cache-conscious-join.rra`

## Metadata

- **ID:** `cache-conscious-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** academic, optimization
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(join inner (= ?lcol ?rcol) ?left ?right)"
    description: "Equi-join with cache-aware partitioning"
  - type: "predicate"
    condition: "is_equijoin(= ?lcol ?rcol)"
    description: "Must be an equi-join"
  - type: "fact"
    fact_type: "hardware.cache_size"
    comparator: "exists"
    description: "Cache size info needed for partition sizing"
```


# Cache-Conscious Join

## Description

Optimizes join for CPU cache locality

## Implementation

Add implementation details for Cache-Conscious Join

## Tests

Add test cases for this optimization

## References

- Paper: Cache-Conscious Join by Barber et al.
- DOI: 10.1145/1007568
