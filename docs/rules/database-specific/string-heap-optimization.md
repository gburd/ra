# Rule: MonetDB String Heap Optimization

**Category:** database-specific/monetdb
**File:** `rules/database-specific/monetdb/string-heap-optimization.rra`

## Metadata

- **ID:** `monetdb-string-heap-optimization`
- **Version:** "1.0.0"
- **Databases:** monetdb
- **Tags:** database-specific, monetdb, string, heap, variable-length, storage
- **Authors:** "RA Contributors"


# MonetDB String Heap Optimization

## Description

MonetDB stores variable-length strings in a separate string heap.
The BAT column contains fixed-width offsets into this heap.  String
operations that only need to compare or hash strings can operate on
offsets when the heap guarantees uniqueness (deduplicated heap), or
use prefix comparisons to short-circuit full string comparisons.

**When to apply**: String equality, GROUP BY, or JOIN operations
where the string heap has been deduplicated.

**Why it works**: Comparing 4/8-byte offsets is faster than comparing
variable-length strings.  A deduplicated heap guarantees that equal
strings have equal offsets, enabling integer-based comparison.

**Database version**: MonetDB 11+

## Relational Algebra

```algebra
-- Before: string equality comparison
sigma[name = 'Alice'](scan(users.name))

-- After: offset comparison (deduplicated heap)
offset = heap_lookup('Alice')
sigma[name_offset = offset](scan(users.name_offsets))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("monetdb-string-offset-compare";
    "(filter (= ?str_col ?str_literal) (scan ?table))" =>
    "(filter (= (offset ?str_col) (heap-lookup ?str_literal))
        (scan ?table))"
    if is_database("monetdb")
    if is_deduplicated_heap("?str_col")
),
```

## Preconditions

```rust
fn applicable(column: &Column) -> bool {
    column.is_string_type()
    && column.heap().is_deduplicated()
}
```

**Restrictions:**
- Heap deduplication is not always maintained (depends on update
  patterns)
- LIKE, substring, and regex operations still require full string
  access
- Large heaps may not fit in memory

## Cost Model

```rust
fn estimated_benefit(
    rows: f64,
    avg_string_len: f64,
) -> f64 {
    let string_cost = rows * avg_string_len * 0.001;
    let offset_cost = rows * 8.0 * 0.001;
    string_cost - offset_cost
}
```

**Typical benefit**: 2-5x for string equality and GROUP BY on
deduplicated heaps.

## Test Cases

```sql
-- Positive: string equality on deduplicated heap
SELECT * FROM users WHERE country = 'Germany';
-- Compares 8-byte offsets, not variable-length strings
```

```sql
-- Negative: LIKE requires full string
SELECT * FROM users WHERE name LIKE '%Smith%';
-- Must access actual string data for pattern matching
```

## References

MonetDB: String storage and heap management documentation
Source: gdk/gdk_strimps.c (string optimization)
