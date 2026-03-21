# Rule: Comparison Normalization

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/comparison-normalization.rra`

## Metadata

- **ID:** `comparison-normalization`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** function, comparison, normalization, canonical, predicate
- **Authors:** "RA Contributors"


# Comparison Normalization

## Description

Normalizes comparison predicates to a canonical form (column on left,
constant on right) to simplify pattern matching for subsequent optimization
rules and enable consistent index lookup.

**When to apply**: Comparisons where the column is on the right side, or
where equivalent forms exist that are easier to optimize.

## Relational Algebra

```algebra
constant = col       -> col = constant
constant < col       -> col > constant
constant <= col      -> col >= constant
constant > col       -> col < constant
NOT (col < x)        -> col >= x
NOT (col = x)        -> col != x
col != col           -> FALSE (when NOT NULL)
col = col            -> TRUE (when NOT NULL)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("comparison-flip-equals";
    "(= (literal ?val) ?col)" => "(= ?col (literal ?val))"
    if is_column("?col")
),

rw!("comparison-flip-less-than";
    "(< (literal ?val) ?col)" => "(> ?col (literal ?val))"
    if is_column("?col")
),

rw!("not-less-to-gte";
    "(not (< ?col ?val))" => "(>= ?col ?val)"
),

rw!("not-equals-normalization";
    "(not (= ?col ?val))" => "(!= ?col ?val)"
),

rw!("col-equals-self";
    "(= ?col ?col)" => "(is-not-null ?col)"
    // col = col is TRUE when NOT NULL, NULL when NULL
),
```

## Cost Model

```rust
fn estimated_benefit() -> f64 {
    0.1 // Indirect: enables downstream optimizations
}
```

## Test Cases

### Positive: Constant on left

```sql
SELECT * FROM t WHERE 42 = id;
-- Normalize to: WHERE id = 42
```

### Positive: NOT negation of comparison

```sql
SELECT * FROM t WHERE NOT (price < 100);
-- Normalize to: WHERE price >= 100
```

### Positive: Self-comparison

```sql
SELECT * FROM t WHERE col = col;
-- Normalize to: WHERE col IS NOT NULL
```

### Negative: Two columns (no constant)

```sql
SELECT * FROM t WHERE a.col = b.col;
-- Both sides are columns, no normalization to do
```

## References

**Implementation:**
- PostgreSQL: Clause normalization in `canonicalize_qual()`
- All databases perform comparison normalization as early optimization
