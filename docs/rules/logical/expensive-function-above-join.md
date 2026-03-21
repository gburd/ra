# Rule: Expensive Function Above Join

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/expensive-function-above-join.rra`

## Metadata

- **ID:** `expensive-function-above-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, mssql, oracle, duckdb
- **Tags:** function, expensive, join, pushdown, cost
- **Authors:** "RA Contributors"


# Expensive Function Above Join

## Description

Moves expensive scalar function evaluations from below a join to above it
(or delays them in the pipeline) so the function is evaluated on fewer rows.
If a function is applied before a join that reduces cardinality, it evaluates
on N rows. Moving it above the join evaluates on N*selectivity rows instead.

**When to apply**: An expensive function (cost_multiplier > 5) appears in a
projection or predicate below a join that significantly reduces cardinality.

**Why it works**: Expensive functions (regex, geospatial, JSON parsing) cost
10-100x more per row than simple comparisons. Evaluating them on fewer rows
after cardinality-reducing joins saves CPU proportionally.

## Relational Algebra

```algebra
join[cond](pi[f(a), ...](R), S)
  -> pi[f(a), ...](join[cond](R, S))
  where is_expensive(f) && |join output| < |R|
  -- f must not be used in join condition
```

## Implementation

```rust
rw!("delay-expensive-function";
    "(join ?cond (project (apply-fn ?expensive_fn ?col) ?rest ?child) ?right)" =>
    "(project (apply-fn ?expensive_fn ?col) ?rest (join ?cond ?child ?right))"
    if is_expensive_function("?expensive_fn") &&
       not_in_join_condition("?expensive_fn", "?cond")
),
```

## Cost Model

```rust
fn benefit(
    fn_cost: f64,
    rows_before_join: u64,
    rows_after_join: u64,
) -> f64 {
    let before = rows_before_join as f64 * fn_cost;
    let after = rows_after_join as f64 * fn_cost;
    (before - after) / before
}
```

**Typical benefit**: 30-90% depending on join selectivity and function cost.

## Test Cases

### Positive: Regex below join

```sql
-- Before: regex evaluated on 1M users, then joined
SELECT u.name, regexp_match(u.bio, '.*PhD.*')
FROM users u JOIN departments d ON u.dept_id = d.id
WHERE d.name = 'Engineering';

-- After: join first (filters to ~100 users), then regex
```

### Negative: Function used in join condition

```sql
SELECT * FROM a JOIN b ON ST_DWithin(a.geom, b.geom, 100);

-- Cannot move ST_DWithin above the join since it IS the join condition
```

## References

- Chaudhuri & Shim, "Including Group-By in Query Optimization", VLDB 1994
- PostgreSQL: Expensive function evaluation ordering
