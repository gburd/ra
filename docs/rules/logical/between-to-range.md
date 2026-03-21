# Rule: BETWEEN to Range Predicates

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/between-to-range.rra`

## Metadata

- **ID:** `between-to-range`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** function, between, range, predicate, normalization
- **Authors:** "RA Contributors"


# BETWEEN to Range Predicates

## Description

Normalizes BETWEEN expressions to explicit range comparisons for consistent
optimization. Also detects overlapping or contradictory BETWEEN clauses and
simplifies them.

**When to apply**: BETWEEN predicates that benefit from decomposition or
where multiple range predicates can be merged.

## Relational Algebra

```algebra
col BETWEEN a AND b -> col >= a AND col <= b
NOT BETWEEN a AND b -> col < a OR col > b
col BETWEEN a AND b AND col BETWEEN c AND d
  -> col BETWEEN MAX(a,c) AND MIN(b,d)  -- intersection
col BETWEEN 5 AND 3 -> FALSE            -- empty range
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("between-to-range";
    "(between ?col ?low ?high)" =>
    "(and (>= ?col ?low) (<= ?col ?high))"
),

rw!("between-empty-range";
    "(between ?col ?low ?high)" => "(literal false)"
    if low_greater_than_high("?low", "?high")
),

rw!("between-merge";
    "(and (between ?col ?a ?b) (between ?col ?c ?d))" =>
    "(between ?col (max ?a ?c) (min ?b ?d))"
),
```

## Cost Model

```rust
fn estimated_benefit(merged_ranges: usize) -> f64 {
    merged_ranges as f64 * 0.15
}
```

## Test Cases

### Positive: Simple BETWEEN decomposition

```sql
SELECT * FROM events WHERE date BETWEEN '2024-01-01' AND '2024-12-31';
-- Decompose for optimizer: date >= '2024-01-01' AND date <= '2024-12-31'
```

### Positive: Overlapping ranges

```sql
SELECT * FROM t
WHERE val BETWEEN 1 AND 10 AND val BETWEEN 5 AND 15;
-- Merge to: val BETWEEN 5 AND 10
```

### Positive: Empty range detection

```sql
SELECT * FROM t WHERE val BETWEEN 10 AND 5;
-- Empty range: no rows can match -> FALSE
```

### Negative: Non-overlapping ranges (contradiction)

```sql
SELECT * FROM t
WHERE val BETWEEN 1 AND 5 AND val BETWEEN 10 AND 15;
-- MAX(1,10) = 10, MIN(5,15) = 5 -> BETWEEN 10 AND 5 -> FALSE
-- Correctly detects contradiction
```

## References

**Implementation:**
- PostgreSQL: BETWEEN normalization in parser
- MySQL: Range optimization merges overlapping BETWEEN predicates
