# Rule: Match Function Predicates with Partial Indexes

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/partial-index-predicate-match.rra`

## Metadata

- **ID:** `partial-index-predicate-match`
- **Version:** "1.0.0"
- **Databases:** postgresql, mssql
- **Tags:** logical, function, index, partial, filtered, predicate
- **Authors:** "RA Contributors"


# Match Function Predicates with Partial Indexes

## Description

Detects when a query's WHERE clause logically implies a partial
index's WHERE predicate, enabling use of the smaller, more efficient
partial index. A partial index on `WHERE status = 'active'` can be
used for queries filtering on `status = 'active' AND f(col) = val`.

**When to apply**: A query's filter predicates are a superset of
(or logically imply) a partial index's predicate clause, and the
remaining predicates can use the index's key columns.

**Why it works**: Partial indexes are smaller than full indexes
because they only include rows matching their predicate. When the
query's filter guarantees all matching rows satisfy the index
predicate, the partial index provides faster, more cache-friendly
access.

## Implementation

```rust
// Query predicate implies partial index predicate
rw!("partial-index-func-match";
    "(filter (and ?query_pred (?op (?f ?col) ?val))
       (scan ?t))" =>
    "(filter (?op (?f ?col) ?val)
       (index-scan ?t ?partial_idx))"
    if has_partial_index("?t", "?partial_idx")
    if implies_index_predicate("?query_pred", "?partial_idx")
    if index_covers_func("?partial_idx", "?f", "?col")
),

// Exact match of partial index predicate
rw!("partial-index-exact-match";
    "(filter ?pred (scan ?t))" =>
    "(index-scan ?t ?partial_idx)"
    if has_partial_index("?t", "?partial_idx")
    if matches_index_predicate("?pred", "?partial_idx")
),

// Partial index with function in predicate
rw!("partial-index-func-pred";
    "(filter (and (?f ?col ?fval) (?op ?key ?val))
       (scan ?t))" =>
    "(filter (?op ?key ?val)
       (index-scan ?t ?partial_idx))"
    if has_partial_index_with_func_pred("?t", "?f", "?col", "?fval",
                                        "?partial_idx")
),
```

## Preconditions

- Database must support partial/filtered indexes (PostgreSQL, mssql)
- Query predicate must logically imply the index's WHERE clause
- Implication check must handle: equality, range subsumption, AND conjunctions
- NULL handling: partial index predicate with IS NOT NULL is implied
  by any equality or range predicate on that column

## Test Cases

```sql
-- Setup: CREATE INDEX idx_active ON orders (customer_id)
--        WHERE status = 'active';

-- Positive: query includes index predicate
SELECT * FROM orders
WHERE status = 'active' AND customer_id = 42;
-- Uses idx_active: status='active' matches index predicate

-- Positive: query predicate implies index predicate
SELECT * FROM orders
WHERE status = 'active' AND customer_id > 100
  AND created_at > '2024-01-01';
-- Uses idx_active: status='active' present in conjunction

-- Setup: CREATE INDEX idx_recent ON events (event_type)
--        WHERE EXTRACT(YEAR FROM created_at) = 2024;

-- Positive: function in partial index predicate
SELECT * FROM events
WHERE EXTRACT(YEAR FROM created_at) = 2024
  AND event_type = 'click';
-- Uses idx_recent: function predicate matches

-- Negative: query doesn't imply index predicate
SELECT * FROM orders WHERE customer_id = 42;
-- Cannot use idx_active: no guarantee status='active'

-- Negative: contradicts index predicate
SELECT * FROM orders
WHERE status = 'completed' AND customer_id = 42;
-- Cannot use idx_active: status='completed' != 'active'
```

## References

- PostgreSQL: Partial Indexes documentation
- mssql: Filtered Indexes
- "Partial Indexing in POSTGRES" (Stonebraker, 1989)
