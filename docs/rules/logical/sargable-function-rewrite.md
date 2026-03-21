# Rule: SARGable Function Rewrite

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/sargable-function-rewrite.rra`

## Metadata

- **ID:** `sargable-function-rewrite`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb
- **Tags:** function, sargable, index, predicate, rewrite
- **Authors:** "RA Contributors"


# SARGable Function Rewrite

## Description

Rewrites non-SARGable predicates (those with functions applied to indexed
columns) into SARGable (Search ARGument able) form that can use indexes.
When a function wraps an indexed column, the index cannot be used. Inverting
the function and applying it to the constant side restores index eligibility.

**When to apply**: Predicates of the form `f(column) = constant` where f
has a known inverse, allowing rewrite to `column = f_inverse(constant)`.

**Why it works**: B-tree indexes store raw column values. `f(column) = K`
requires evaluating f per row (full scan). `column = f_inverse(K)` is a
direct index lookup.

## Relational Algebra

```algebra
filter[YEAR(date_col) = 2024](R)
  -> filter[date_col >= '2024-01-01' AND date_col < '2025-01-01'](R)

filter[LOWER(name_col) = 'smith'](R)
  -> filter[name_col ILIKE 'smith'](R)
  -- or use case-insensitive collation index

filter[col + 10 = 50](R)
  -> filter[col = 40](R)

filter[ABS(col) < 5](R)
  -> filter[col > -5 AND col < 5](R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// YEAR extraction to range
rw!("year-extraction-to-range";
    "(= (extract year ?col) (literal ?year))" =>
    "(and (>= ?col (date-start ?year))
          (< ?col (date-start (+ ?year 1))))"
),

// Arithmetic inversion
rw!("add-to-column-sargable";
    "(= (+ ?col (literal ?n)) (literal ?k))" =>
    "(= ?col (literal (- ?k ?n)))"
),

rw!("multiply-to-column-sargable";
    "(= (* ?col (literal ?n)) (literal ?k))" =>
    "(= ?col (literal (/ ?k ?n)))"
    if is_nonzero("?n")
),

// ABS to range
rw!("abs-less-than-to-range";
    "(< (abs ?col) (literal ?k))" =>
    "(and (> ?col (literal (- 0 ?k))) (< ?col (literal ?k)))"
    if is_positive("?k")
),

// LOWER/UPPER to case-insensitive
rw!("lower-equals-to-ilike";
    "(= (lower ?col) (literal ?val))" =>
    "(ilike ?col (literal ?val))"
),
```

**Restrictions:**
- Only works for invertible or range-decomposable functions
- Multi-valued inverses (ABS) produce OR/range predicates
- Non-invertible functions (hash, modulo) cannot be rewritten
- Collation must be compatible for string transformations

## Cost Model

```rust
fn estimated_benefit(table_size: u64, has_index: bool) -> f64 {
    if has_index {
        // Full scan -> index lookup
        let scan_cost = table_size as f64;
        let index_cost = (table_size as f64).log2() * 4.0;
        (scan_cost - index_cost) / scan_cost
    } else {
        0.0 // No index: rewrite has no benefit
    }
}
```

**Typical benefit**: 50-95% when index is available

## Test Cases

### Positive: YEAR extraction on date column

```sql
SELECT * FROM orders WHERE YEAR(order_date) = 2024;
-- Non-SARGable: function on column prevents index use
-- Rewrite to: order_date >= '2024-01-01' AND order_date < '2025-01-01'
-- Now uses date index range scan
```

### Positive: Arithmetic on column

```sql
SELECT * FROM products WHERE price * 1.1 > 100;
-- Rewrite to: price > 100 / 1.1 -> price > 90.909...
-- Index on price can now be used
```

### Positive: LOWER for case-insensitive match

```sql
SELECT * FROM users WHERE LOWER(email) = 'user@example.com';
-- Rewrite to use ILIKE or citext comparison
-- Or: use expression index on LOWER(email)
```

### Negative: Hash function (non-invertible)

```sql
SELECT * FROM users WHERE MD5(password) = 'abc123...';
-- MD5 is not invertible
-- Cannot rewrite to direct column comparison
```

### Negative: Modulo (multi-valued inverse)

```sql
SELECT * FROM t WHERE col % 7 = 0;
-- Infinite solutions: 0, 7, 14, 21, ...
-- Cannot convert to finite range scan
```

## References

**Academic papers:**
- Selinger et al., "Access Path Selection in a Relational Database Management System", SIGMOD 1979

**Implementation:**
- PostgreSQL: Expression index support as alternative
- MySQL: "Cannot use index" warning for non-SARGable predicates
- mssql: SARGability analysis in query optimizer
- Oracle: Function-based index as fallback
