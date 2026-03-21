# Rule: Rewrite Expressions to Match Expression Indexes

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/expression-index-rewrite.rra`

## Metadata

- **ID:** `expression-index-rewrite`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle, mssql
- **Tags:** logical, function, index, expression, rewrite
- **Authors:** "RA Contributors"


# Rewrite Expressions to Match Expression Indexes

## Description

Detects when a query expression matches an expression index definition
and rewrites the query to use the index's pre-computed column. If an
index exists on LOWER(email), the filter `WHERE LOWER(email) = 'x'`
is rewritten to use the index column directly.

**When to apply**: A function expression in a filter or projection
matches an expression index defined on the target table.

**Why it works**: Expression indexes store pre-computed function
results. Using the index column avoids re-evaluating the function
per row and enables index-based access paths.

## Implementation

```rust
// Match filter expression to expression index
rw!("expr-index-filter-match";
    "(filter (= (?f ?col) ?val) (scan ?t))" =>
    "(filter (= ?idx_col ?val) (index-scan ?t ?idx))"
    if has_expression_index("?t", "?f", "?col", "?idx")
    if extract_index_column("?idx", "?idx_col")
),

// Match expression in range predicate
rw!("expr-index-range-match";
    "(filter (between (?f ?col) ?lo ?hi) (scan ?t))" =>
    "(filter (between ?idx_col ?lo ?hi) (index-scan ?t ?idx))"
    if has_expression_index("?t", "?f", "?col", "?idx")
    if extract_index_column("?idx", "?idx_col")
),

// Match expression in ORDER BY
rw!("expr-index-sort-match";
    "(sort (?f ?col) (scan ?t))" =>
    "(index-scan ?t ?idx asc)"
    if has_expression_index("?t", "?f", "?col", "?idx")
),
```

## Preconditions

- Table must have an expression index on the exact function+column
- Expression in query must structurally match the index definition
- For composite expression indexes, all expressions must match

## Test Cases

```sql
-- Setup: CREATE INDEX idx_lower_email ON users (LOWER(email));

-- Positive: equality on indexed expression
SELECT * FROM users WHERE LOWER(email) = 'alice@example.com';
-- Rewritten to use idx_lower_email index scan

-- Positive: range on indexed expression
SELECT * FROM users WHERE LOWER(email) BETWEEN 'a' AND 'b';
-- Rewritten to use idx_lower_email range scan

-- Positive: ORDER BY indexed expression
SELECT * FROM users ORDER BY LOWER(email);
-- Rewritten to use idx_lower_email ordered scan

-- Negative: different function than index
SELECT * FROM users WHERE UPPER(email) = 'ALICE@EXAMPLE.COM';
-- No match: index is on LOWER, query uses UPPER

-- Negative: no expression index exists
SELECT * FROM users WHERE LENGTH(email) > 50;
-- No expression index on LENGTH(email)
```

## References

- PostgreSQL: Indexes on Expressions documentation
- Oracle: Function-Based Indexes
- mssql: Computed Column Indexes
