# Rule: Array UNNEST Position Optimization

**Category:** logical/multi-model
**File:** `rules/logical/multi-model/array-unnest-pushdown.rra`

## Metadata

- **ID:** `array-unnest-pushdown`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, bigquery, presto, clickhouse
- **Tags:** logical, multi-model, array, unnest, lateral, pushdown
- **Authors:** "PostgreSQL Team", "DuckDB Team"


# Array UNNEST Position Optimization

## Description

Optimizes the position of UNNEST (array flattening) operations in the
query plan. UNNEST can multiply row count significantly; placing it
after filters and before aggregations minimizes the total rows processed.
This rule also detects when UNNEST + aggregation can be replaced by
array functions (e.g., `array_length` instead of `UNNEST + COUNT`).

**When to apply**: Queries using UNNEST, LATERAL, or array expansion
where the position of expansion affects performance.

## Relational Algebra

```algebra
-- Before: UNNEST early, filter late
sigma[pred](UNNEST(R.arr))

-- After: filter early, UNNEST late
UNNEST(sigma[pred](R).arr)

-- Before: UNNEST + COUNT pattern
gamma[id; COUNT(*)](UNNEST(R.arr))

-- After: direct array function
pi[id, array_length(R.arr)](R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("filter-before-unnest";
    "(filter ?pred (unnest ?arr ?input))" =>
    "(unnest ?arr (filter ?pred ?input))"
    if pred_independent_of_unnest("?pred", "?arr")
),

rw!("unnest-count-to-array-length";
    "(aggregate ?groups (count) (unnest ?arr ?input))" =>
    "(project ?groups (array-length ?arr) ?input)"
),
```

## Preconditions

```rust
fn applicable(query: &Query) -> bool {
    query.has_unnest()
        && (query.has_filter_above_unnest()
            || query.has_agg_over_unnest_pattern())
}
```

**Restrictions:**
- Filter pushdown: predicate must not reference unnested columns
- Array function replacement: only for simple aggregates (COUNT, SUM)
- NULL handling: UNNEST of NULL array produces zero rows

## Cost Model

```rust
fn estimated_benefit(
    input_rows: f64,
    avg_array_length: f64,
    filter_selectivity: f64,
) -> f64 {
    let before = input_rows * avg_array_length; // unnest all, then filter
    let after = input_rows * filter_selectivity * avg_array_length;
    before - after
}
```

**Typical benefit**: 10-50% when filters are selective.

## Test Cases

```sql
-- Positive: filter before unnest
SELECT u.id, t.tag
FROM users u, UNNEST(u.tags) AS t_result(tag)
WHERE u.active = true;
-- Push active filter before UNNEST

-- Positive: unnest+count to array_length
SELECT id, COUNT(*) FROM users, UNNEST(tags) GROUP BY id;
-- Replace with: SELECT id, array_length(tags, 1) FROM users

-- Negative: filter on unnested column
SELECT u.id, t.tag FROM users u, UNNEST(u.tags) AS t_result(tag)
WHERE t.tag LIKE 'tech%';
-- Cannot push: filter depends on unnested value
```

## References

- PostgreSQL: Set Returning Functions optimization
- DuckDB: List/Array function optimization
