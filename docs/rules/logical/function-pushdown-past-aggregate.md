# Rule: Function Pushdown Past Aggregate

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/function-pushdown-past-aggregate.rra`

## Metadata

- **ID:** `function-pushdown-past-aggregate`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, oracle
- **Tags:** function, pushdown, aggregate, group-by, pre-compute
- **Authors:** "RA Contributors"


# Function Pushdown Past Aggregate

## Description

Pushes a deterministic function applied to a GROUP BY key below the
aggregation operator. Pre-computing the function value before grouping
avoids re-evaluating it in the SELECT list and enables the aggregate to
group on the pre-computed value directly.

**When to apply**: A function of a GROUP BY column appears in the
SELECT list, and the function is deterministic. Computing it once per
input row (below the aggregate) and grouping on the result avoids
computing it again per output group.

**Why it works**: When the function is used in both GROUP BY and SELECT,
pushing it below computes it once per input row. The alternative
(computing above) would require re-evaluation or a separate projection.

## Relational Algebra

```algebra
pi[f(a), agg(b)](gamma[f(a)](R))
  -> pi[fa, agg(b)](gamma[fa](pi[f(a) AS fa, b](R)))
  where is_deterministic(f)
```

## Implementation

```rust
rw!("push-fn-below-groupby";
    "(project (apply-fn ?fn ?col) ?aggs
       (aggregate (apply-fn ?fn ?col) ?agg_exprs ?child))" =>
    "(project ?fn_alias ?aggs
       (aggregate ?fn_alias ?agg_exprs
         (project (apply-fn ?fn ?col AS ?fn_alias) ?pass_cols ?child)))"
    if is_deterministic("?fn")
),
```

## Test Cases

### Positive: DATE_TRUNC in GROUP BY and SELECT

```sql
SELECT DATE_TRUNC('month', created_at), COUNT(*)
FROM events
GROUP BY DATE_TRUNC('month', created_at);
-- Push DATE_TRUNC below: compute once per row, group on result
```

### Negative: Non-deterministic function

```sql
SELECT RANDOM(), COUNT(*) FROM t GROUP BY 1;
-- Cannot push: RANDOM() gives different results each evaluation
```

## References

- Yan & Larson, "Eager Aggregation and Lazy Aggregation", VLDB 1995
