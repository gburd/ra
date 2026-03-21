# Rule: Pure Function Common Subexpression Elimination

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/pure-function-cse.rra`

## Metadata

- **ID:** `pure-function-cse`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, mssql, oracle, duckdb, sqlite
- **Tags:** function, pure, cse, subexpression, optimization
- **Authors:** "RA Contributors"


# Pure Function Common Subexpression Elimination

## Description

Extends common subexpression elimination to pure functions across query
blocks. Unlike simple deterministic deduplication (within a single SELECT),
this rule identifies repeated pure function calls across correlated
subqueries, CTEs, and view definitions, factoring them into a single
computation.

**When to apply**: A pure function (no side effects, no external state
dependency) with identical arguments appears in multiple query blocks
that share the same input relation.

**Why it works**: Pure functions depend only on their arguments. If the
arguments are identical and sourced from the same relation, the result
is identical regardless of where in the plan tree the call appears.

## Relational Algebra

```algebra
-- CTE factoring:
WITH t AS (SELECT f(a) AS fa, * FROM R)
SELECT fa FROM t WHERE fa > 0
  -- instead of --
SELECT f(a) FROM R WHERE f(a) > 0
```

## Implementation

```rust
rw!("pure-fn-cse-across-blocks";
    "(project (apply-fn ?fn ?args) ?rest
       (filter (pred (apply-fn ?fn ?args) ?op ?val) ?child))" =>
    "(project ?alias ?rest
       (filter (pred ?alias ?op ?val)
         (project (apply-fn ?fn ?args AS ?alias) ?pass ?child)))"
    if is_pure("?fn")
),
```

## Test Cases

### Positive: Function in SELECT and WHERE

```sql
-- Before
SELECT ST_Area(geom) FROM parcels WHERE ST_Area(geom) > 1000;
-- After: compute ST_Area once, reuse in both positions
```

### Positive: CTE factoring

```sql
WITH computed AS (
  SELECT id, expensive_fn(data) AS result FROM raw
)
SELECT result FROM computed WHERE result > threshold;
```

### Negative: Impure function

```sql
SELECT NEXTVAL('seq'), NEXTVAL('seq') FROM t;
-- Each call must advance the sequence independently
```

## References

- Pirahesh, Hasan, Mohan, "Starburst Query Optimizer", VLDB 1992
