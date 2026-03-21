# Rule: Function-Based Expression Index Matching

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/function-index-matching-composite.rra`

## Metadata

- **ID:** `function-index-matching-composite`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle, sqlite
- **Tags:** function, expression-index, index-matching, sargable
- **Authors:** "RA Contributors"


# Function-Based Expression Index Matching

## Description

Rewrites a predicate containing a function call to match an expression
index defined on the same function. For example, a predicate
`WHERE LOWER(email) = 'alice@example.com'` can use an index on
`LOWER(email)` if the optimizer recognizes the structural match.

**When to apply**: A WHERE clause applies a deterministic function to a
column, and an expression index exists on that exact function application.

**Why it works**: Expression indexes pre-compute and store the function
result. Matching the predicate to the index avoids per-row function
evaluation and enables an index scan instead of a sequential scan.

## Relational Algebra

```algebra
sigma[f(col) = val](R)
  -> expression_index_scan[I_expr](val)
  where has_expression_index(I_expr, f(col))
```

## Implementation

```rust
rw!("match-expression-index";
    "(filter (= (apply-fn ?fn ?col) ?val) (scan ?table))" =>
    "(expression-index-scan ?idx ?val)"
    if has_expression_index("?table", "?fn", "?col") &&
       is_deterministic("?fn")
),
```

## Cost Model

```rust
fn benefit_vs_seq_scan(total_pages: u64, fn_cost: f64, rows: u64) -> f64 {
    let seq = total_pages as f64 * IO_COST + rows as f64 * fn_cost;
    let idx = INDEX_LEVELS as f64 * IO_COST;
    (seq - idx) / seq
}
```

**Typical benefit**: 50-95% for selective equality predicates on expression indexes.

## Test Cases

### Positive: LOWER expression index

```sql
-- CREATE INDEX idx_lower_email ON users (LOWER(email));
SELECT * FROM users WHERE LOWER(email) = 'alice@example.com';
-- Uses idx_lower_email instead of sequential scan
```

### Positive: Date truncation index

```sql
-- CREATE INDEX idx_month ON events (DATE_TRUNC('month', created_at));
SELECT * FROM events WHERE DATE_TRUNC('month', created_at) = '2025-01-01';
```

### Negative: Different function on same column

```sql
-- Index on LOWER(email), but query uses UPPER(email)
SELECT * FROM users WHERE UPPER(email) = 'ALICE@EXAMPLE.COM';
-- Cannot use the LOWER index
```

## References

- PostgreSQL: Indexes on expressions
- Oracle: Function-based indexes
- SQLite: Indexes on expressions
