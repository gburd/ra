# Rule: Simplify Aggregate Function Expressions

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/aggregate-function-simplification.rra`

## Metadata

- **ID:** `aggregate-function-simplification`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** logical, function, aggregate, simplification, rewrite
- **Authors:** "RA Contributors"


# Simplify Aggregate Function Expressions

## Description

Rewrites aggregate function expressions to simpler or more efficient
equivalents. SUM(1) becomes COUNT(*), COUNT(non_nullable_col) becomes
COUNT(*), MIN/MAX on a unique indexed column can use index-first/last
scan, and AVG(x) can be decomposed to SUM(x)/COUNT(x) for parallel
execution.

**When to apply**: An aggregate function expression can be rewritten
to a cheaper or more parallelizable equivalent.

## Implementation

```rust
// SUM(1) is equivalent to COUNT(*)
rw!("sum-one-to-count-star";
    "(aggregate ?group (sum (literal 1)) ?input)" =>
    "(aggregate ?group (count-star) ?input)"
),

// COUNT(non-nullable) is equivalent to COUNT(*)
rw!("count-non-nullable-to-count-star";
    "(aggregate ?group (count ?col) ?input)" =>
    "(aggregate ?group (count-star) ?input)"
    if is_not_nullable("?col")
),

// COUNT(DISTINCT x) on unique column is COUNT(*)
rw!("count-distinct-unique-to-count-star";
    "(aggregate ?group (count-distinct ?col) ?input)" =>
    "(aggregate ?group (count-star) ?input)"
    if is_unique("?col")
),

// MIN/MAX on empty group with index: use index scan
rw!("min-to-index-first";
    "(aggregate empty-group (min ?col) (scan ?t))" =>
    "(limit 1 (index-scan ?t ?col asc))"
    if has_ordered_index("?col")
),
rw!("max-to-index-last";
    "(aggregate empty-group (max ?col) (scan ?t))" =>
    "(limit 1 (index-scan ?t ?col desc))"
    if has_ordered_index("?col")
),

// AVG decomposition for parallel aggregation
rw!("avg-decompose";
    "(aggregate ?group (avg ?col) ?input)" =>
    "(project (/ ?sum ?cnt)
       (aggregate ?group [(sum ?col) as ?sum
                          (count ?col) as ?cnt] ?input))"
),
```

## Test Cases

```sql
-- Positive: SUM(1) to COUNT(*)
SELECT deptno, SUM(1) FROM emp GROUP BY deptno;
-- Rewritten to: SELECT deptno, COUNT(*) FROM emp GROUP BY deptno

-- Positive: COUNT on NOT NULL column
SELECT COUNT(id) FROM orders;  -- id is NOT NULL
-- Rewritten to: SELECT COUNT(*) FROM orders

-- Positive: MIN with B-tree index
SELECT MIN(salary) FROM emp;  -- salary has B-tree index
-- Rewritten to: index scan ascending, LIMIT 1

-- Positive: MAX with B-tree index
SELECT MAX(created_at) FROM events;  -- created_at indexed
-- Rewritten to: index scan descending, LIMIT 1

-- Positive: COUNT(DISTINCT id) on primary key
SELECT COUNT(DISTINCT id) FROM orders;
-- Rewritten to: COUNT(*) since id is unique

-- Negative: SUM with non-constant argument
SELECT SUM(salary) FROM emp;
-- No simplification: salary is a real column

-- Negative: MIN without index
SELECT MIN(commission) FROM emp;  -- no index on commission
-- Keep as aggregate: no index optimization available
```

## References

- Calcite: AggregateReduceFunctionsRule
- PostgreSQL: MIN/MAX index optimization
- functions.toml: aggregate function properties
