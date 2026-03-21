# Rule: Calcite SampleToFilterRule

**Category:** logical/semantic-rewriting
**File:** `rules/logical/semantic-rewriting/sample-to-filter.rra`

## Metadata

- **ID:** `calcite-sample-to-filter`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql
- **Tags:** logical, calcite, sample, bernoulli, filter, random
- **Authors:** "RA Contributors"


# Calcite SampleToFilterRule

## Description

Rewrites a Bernoulli TABLESAMPLE into an equivalent filter using
rand(). This allows any database that supports random number
generation to execute sampling queries.

**When to apply**: A Sample operator uses Bernoulli sampling and
needs to be executed on a backend without native TABLESAMPLE support.

**Why it works**: Bernoulli sampling includes each row independently
with probability p. This is equivalent to filtering with rand() < p,
since rand() produces uniform random values in [0, 1).

**Calcite class**: `org.apache.calcite.rel.rules.SampleToFilterRule`

## Relational Algebra

```algebra
-- Before: TABLESAMPLE BERNOULLI(50)
Sample[bernoulli, 0.5](R)

-- After: filter with random
sigma[rand() < 0.5](R)

-- With REPEATABLE seed
Sample[bernoulli, 0.5, seed=10](R)
-- Becomes: sigma[rand(10) < 0.5](R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-sample-to-filter";
    "(sample bernoulli ?rate ?input)" =>
    "(filter (< (rand) ?rate) ?input)"
),

rw!("calcite-sample-to-filter-repeatable";
    "(sample bernoulli ?rate ?seed ?input)" =>
    "(filter (< (rand ?seed) ?rate) ?input)"
),
```

## Preconditions

```rust
fn applicable(sample: &Sample) -> bool {
    sample.is_bernoulli()
}
```

**Restrictions:**
- Only Bernoulli sampling (not system/block sampling)
- rand() is non-deterministic; use REPEATABLE for reproducibility
- Not a performance optimization; compatibility transformation

## Cost Model

```rust
fn estimated_benefit(_: f64) -> f64 {
    0.0 // Compatibility transformation
}
```

**Typical benefit**: 0-10% (enables execution, not optimization).

## Test Cases

```sql
-- Positive: Bernoulli sample
SELECT deptno FROM dept TABLESAMPLE BERNOULLI(50);
-- Becomes: SELECT deptno FROM dept WHERE rand() < 0.5
```

```sql
-- Positive: with REPEATABLE
SELECT deptno FROM dept TABLESAMPLE BERNOULLI(50) REPEATABLE(10);
-- Becomes: SELECT deptno FROM dept WHERE rand(10) < 0.5
```

```sql
-- Negative: system sampling
SELECT deptno FROM dept TABLESAMPLE SYSTEM(50);
-- System sampling uses block-level; cannot convert to row filter
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/SampleToFilterRule.java (commit af6367d)
