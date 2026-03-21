# Rule: Push Function Predicates Below Joins

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/function-filter-pushdown.rra`

## Metadata

- **ID:** `function-filter-pushdown`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** logical, function, pushdown, filter, join
- **Authors:** "RA Contributors"


# Push Function Predicates Below Joins

## Description

Pushes filter predicates that contain function calls down past joins
when the function only references columns from one side. A filter
like `WHERE UPPER(a.name) = 'ALICE'` on a join of tables A and B
can be pushed to table A's scan, reducing the join input size.

**When to apply**: A filter predicate containing a function call
references columns from only one side of a join.

**Why it works**: Evaluating the function filter before the join
reduces the number of rows entering the join, which has quadratic
or worse cost characteristics.

## Implementation

```rust
// Push function predicate to left side of join
rw!("push-func-filter-to-left";
    "(filter (?op (?f ?left_col) ?val)
       (join ?jtype ?jcond ?left ?right))" =>
    "(join ?jtype ?jcond
       (filter (?op (?f ?left_col) ?val) ?left)
       ?right)"
    if references_only("?left_col", "?left")
),

// Push function predicate to right side of join
rw!("push-func-filter-to-right";
    "(filter (?op (?f ?right_col) ?val)
       (join ?jtype ?jcond ?left ?right))" =>
    "(join ?jtype ?jcond
       ?left
       (filter (?op (?f ?right_col) ?val) ?right))"
    if references_only("?right_col", "?right")
),

// Push function predicate past aggregate
rw!("push-func-filter-past-agg";
    "(filter (?op (?f ?col) ?val)
       (aggregate ?group ?aggs ?input))" =>
    "(aggregate ?group ?aggs
       (filter (?op (?f ?col) ?val) ?input))"
    if references_only("?col", "?input")
    if not_aggregate_function("?f")
),
```

## Preconditions

- Function arguments must reference columns from only one side
- For outer joins, only push to the preserved side
- Function must be deterministic (volatile functions cannot be pushed)

## Test Cases

```sql
-- Positive: function filter on left table
SELECT * FROM emp e JOIN dept d ON e.deptno = d.deptno
WHERE UPPER(e.name) = 'ALICE';
-- Push UPPER(e.name) = 'ALICE' to emp scan

-- Positive: function filter on right table
SELECT * FROM orders o JOIN products p ON o.pid = p.id
WHERE LENGTH(p.description) > 100;
-- Push LENGTH(p.description) > 100 to products scan

-- Positive: function in HAVING pushed to WHERE
SELECT deptno, COUNT(*) FROM emp
WHERE YEAR(hire_date) = 2023
GROUP BY deptno;
-- Push YEAR(hire_date) filter before aggregate

-- Negative: function references both sides
SELECT * FROM emp e JOIN dept d ON e.deptno = d.deptno
WHERE CONCAT(e.name, d.dname) LIKE '%Engineering%';
-- Cannot push: references columns from both tables

-- Negative: volatile function
SELECT * FROM events e JOIN users u ON e.uid = u.id
WHERE RANDOM() < 0.01;
-- Cannot push: RANDOM() is volatile
```

## References

- Calcite: FilterJoinRule with function predicates
- functions.toml: deterministic property for pushdown safety
- "Predicate Migration" (Levy, Mumick, Sagiv)
