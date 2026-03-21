# Rule: Use Function-Based Index for Matching Predicates

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/function-based-index-scan.rra`

## Metadata

- **ID:** `function-based-index-scan`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle, mysql
- **Tags:** logical, function, index, scan, function-based
- **Authors:** "RA Contributors"


# Use Function-Based Index for Matching Predicates

## Description

Converts a table scan with a function predicate to an index scan when
a function-based index matches the predicate pattern. Unlike simple
expression index matching, this rule handles multi-column function
indexes and complex function compositions.

**When to apply**: A filter contains a function call that exactly
matches a function-based index definition, including argument order
and types.

**Why it works**: Function-based indexes pre-compute and store
function results in sorted order. The optimizer can use these for
lookups, range scans, and ordering without re-evaluating the function.

## Implementation

```rust
// Single-column function-based index scan
rw!("func-index-scan-eq";
    "(filter (= (?f ?col) ?val) (scan ?t))" =>
    "(index-lookup ?t ?idx ?val)"
    if has_func_index("?t", "?f", "?col", "?idx")
),

// Multi-column function-based index
rw!("func-index-scan-composite";
    "(filter (and (= (?f1 ?c1) ?v1) (= (?f2 ?c2) ?v2))
       (scan ?t))" =>
    "(index-lookup ?t ?idx [?v1 ?v2])"
    if has_composite_func_index("?t", ["?f1 ?c1", "?f2 ?c2"], "?idx")
),

// Function-based index for LIKE prefix
rw!("func-index-like-prefix";
    "(filter (like (?f ?col) ?pattern) (scan ?t))" =>
    "(index-range-scan ?t ?idx (prefix ?pattern))"
    if has_func_index("?t", "?f", "?col", "?idx")
    if is_prefix_pattern("?pattern")
),

// Function-based index for IS NOT NULL
rw!("func-index-not-null";
    "(filter (is-not-null (?f ?col)) (scan ?t))" =>
    "(index-scan ?t ?idx)"
    if has_func_index("?t", "?f", "?col", "?idx")
),
```

## Cost Model

```
// Index lookup cost vs sequential scan:
//   index_cost = index_height * page_io + selectivity * table_pages
//   seq_cost   = table_pages
//
// Use index when selectivity < 0.15 (typically)
// Function-based index avoids per-row function evaluation:
//   savings = row_count * function_cost_multiplier
```

## Test Cases

```sql
-- Setup: CREATE INDEX idx_year ON events (EXTRACT(YEAR FROM event_date));

-- Positive: equality on function-based index
SELECT * FROM events
WHERE EXTRACT(YEAR FROM event_date) = 2024;
-- Uses idx_year for point lookup

-- Setup: CREATE INDEX idx_name ON users (LOWER(last_name), LOWER(first_name));

-- Positive: composite function-based index
SELECT * FROM users
WHERE LOWER(last_name) = 'smith' AND LOWER(first_name) = 'john';
-- Uses idx_name composite lookup

-- Positive: prefix scan on function-based index
SELECT * FROM users WHERE LOWER(name) LIKE 'ali%';
-- Uses function index for prefix range scan

-- Negative: function doesn't match index
SELECT * FROM events
WHERE EXTRACT(MONTH FROM event_date) = 12;
-- Index is on YEAR, not MONTH

-- Negative: wrong argument column
SELECT * FROM events
WHERE EXTRACT(YEAR FROM created_at) = 2024;
-- Index is on event_date, not created_at
```

## References

- Oracle: Function-Based Indexes whitepaper
- PostgreSQL: Indexes on Expressions
- functions.toml: function signatures for index matching
