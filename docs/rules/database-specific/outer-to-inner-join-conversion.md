# Rule: "ClickHouse Outer JOIN to Inner JOIN Conversion"

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/outer-to-inner-join-conversion.rra`

## Metadata

- **ID:** `clickhouse-outer-to-inner-join-conversion`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** join, outer, inner, conversion, optimization
- **Authors:** "RA Contributors"


# ClickHouse Outer JOIN to Inner JOIN Conversion

## Metadata
- **Rule ID**: `clickhouse-outer-to-inner-join-conversion`
- **Category**: Database-specific / ClickHouse
- **Source**: `src/Processors/QueryPlan/Optimizations/convertOuterJoinToInnerJoin.cpp`
- **Complexity**: O(1) plan transformation
- **Prerequisites**: Filter above OUTER JOIN that eliminates NULL rows
- **Alternatives**: Execute as OUTER JOIN with post-filter

## Description

When a filter above a LEFT/RIGHT/FULL OUTER JOIN rejects NULL-extended
rows (the rows produced for non-matching input), the OUTER JOIN can be
safely converted to an INNER JOIN. INNER JOINs are faster because they
do not need to produce NULL-padded rows and enable more aggressive
optimizations like hash join with early termination.

ClickHouse also converts ANY JOIN to SEMI or ANTI JOIN when the post-filter
makes the semantics equivalent, enabling specialized join algorithms.

**When to apply:**
- WHERE clause after OUTER JOIN filters out NULLs from the optional side
- IS NOT NULL, equality, or function on outer-side column

**Why it works for OLAP:**
- INNER JOIN has simpler execution path
- Enables hash join optimizations
- Reduces output cardinality

## Relational Algebra

```
filter[B.col IS NOT NULL](A LEFT JOIN B ON cond)
  -> A INNER JOIN B ON cond
```

## Implementation (egg rewrite rules)

```lisp
;; Convert LEFT JOIN to INNER when filter rejects NULLs
(rewrite (filter ?pred (left-join ?cond ?left ?right))
  (filter ?pred (inner-join ?cond ?left ?right))
  :if (rejects-null-right ?pred))

;; Convert RIGHT JOIN to INNER when filter rejects NULLs
(rewrite (filter ?pred (right-join ?cond ?left ?right))
  (filter ?pred (inner-join ?cond ?left ?right))
  :if (rejects-null-left ?pred))

;; Convert FULL JOIN to LEFT when filter rejects right NULLs
(rewrite (filter ?pred (full-join ?cond ?left ?right))
  (filter ?pred (left-join ?cond ?left ?right))
  :if (rejects-null-right ?pred))

;; Convert ANY LEFT to SEMI when post-filter keeps only matches
(rewrite (filter ?pred (any-left-join ?cond ?left ?right))
  (filter ?pred (semi-join ?cond ?left ?right))
  :if (rejects-null-right ?pred)
  :if (only-uses-left-cols ?pred))
```

## Cost Model

```rust
pub fn cost_join_conversion(
    left_rows: u64,
    right_rows: u64,
    join_selectivity: f64,
) -> Cost {
    let inner_output = (left_rows as f64 * join_selectivity) as u64;
    let outer_output = left_rows;
    let savings = Cost::cpu((outer_output - inner_output) * 10);
    Cost::zero() - savings
}
```

**Typical benefit**: 10-50% depending on NULL row fraction

## Test Cases

### Positive: WHERE filters NULLs
```sql
SELECT o.id, c.name
FROM orders o LEFT JOIN customers c ON o.cust_id = c.id
WHERE c.name IS NOT NULL;

-- Converted to INNER JOIN: c.name IS NOT NULL eliminates
-- all non-matching rows that LEFT JOIN would produce
```

### Positive: Equality filter on outer side
```sql
SELECT * FROM a LEFT JOIN b ON a.id = b.a_id
WHERE b.status = 'active';

-- b.status = 'active' is never true for NULL rows
-- Safe to convert to INNER JOIN
```

### Negative: Filter on inner side only
```sql
SELECT * FROM a LEFT JOIN b ON a.id = b.a_id
WHERE a.date > '2024-01-01';

-- Filter on a (inner side) does not eliminate NULL b rows
-- Must remain LEFT JOIN
```

## References

- ClickHouse: `src/Processors/QueryPlan/Optimizations/convertOuterJoinToInnerJoin.cpp`
- ClickHouse: `src/Processors/QueryPlan/Optimizations/convertAnyJoinToSemiOrAntiJoin.cpp`
