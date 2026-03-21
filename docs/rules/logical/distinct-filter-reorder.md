# Rule: Distinct Filter Reorder

**Category:** logical/distinct-elimination
**File:** `rules/logical/distinct-elimination/distinct-filter-reorder.rra`

## Metadata

- **ID:** `distinct-filter-reorder`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, oracle
- **Tags:** distinct, filter, reorder, pushdown
- **Authors:** "RA Contributors"


# Distinct Filter Reorder

## Description

Reorders DISTINCT and Filter so that the filter is applied before DISTINCT, reducing the number of rows that must be deduplicated.

**When to apply**: A filter is applied after DISTINCT and the filter predicate references only columns in the DISTINCT output.

**Why it works**: Filtering first reduces rows; deduplicating fewer rows is cheaper.

## Relational Algebra

```algebra
filter[P](distinct(R))
  -> distinct(filter[P](R))
  where columns(P) ⊆ output(R)
```

## Implementation

```rust
rw!("distinct-filter-reorder";
    "(filter ?pred (distinct ?input))" =>
    "(distinct (filter ?pred ?input))"
),
```

## Cost Model

```rust
fn benefit(rows: u64, selectivity: f64) -> f64 {
    let full_distinct = rows as f64 * (rows as f64).log2();
    let filtered_rows = (rows as f64 * selectivity) as u64;
    let reduced_distinct = filtered_rows as f64 * (filtered_rows as f64).log2();
    (full_distinct - reduced_distinct) / full_distinct
}
```

**Typical benefit**: 10-50% for selective filters

## Test Cases

### Positive: Filter after DISTINCT

```sql
SELECT DISTINCT name, dept FROM employees WHERE dept = 'Engineering';

-- Apply filter first, then distinct
```

### Negative: Filter on derived column

```sql
-- (Not applicable if filter references a computed column not in base)
```

## References

- PostgreSQL: Predicate pushdown past DISTINCT
- MySQL: DISTINCT optimization
