# Rule: Sort Remove Duplicate Keys

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/sort-remove-duplicate-keys.rra`

## Metadata

- **ID:** `sort-remove-duplicate-keys`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql
- **Tags:** sort, simplification, optimization
- **Authors:** "Apache Calcite Contributors"


# Sort Remove Duplicate Keys

## Description

Removes redundant columns from a sort operation's ORDER BY clause. If a sort
key appears multiple times (possibly with different ASC/DESC or NULLS FIRST/LAST
settings), or if a sort key functionally determines subsequent keys, the
redundant keys can be eliminated without changing the sort result.

**When to apply**: A sort operation has duplicate sort keys, or has sort keys
where earlier keys functionally determine later keys (e.g., sorting by
PRIMARY KEY renders other column sorts redundant).

**Why it works**: Sorting is expensive (O(n log n)). Each additional sort key
adds to the comparison cost. Removing redundant keys reduces the number of
comparisons without affecting the final order, improving sort performance.

## Relational Algebra

```algebra
SORT_{k1, k2, k1}(R) -> SORT_{k1, k2}(R)

SORT_{pk, other}(R) where pk is primary key -> SORT_{pk}(R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("sort-remove-duplicate-keys";
    "(sort (list ?keys) ?input)" =>
    "(sort (remove-duplicates ?keys) ?input)"
    if has-duplicate-sort-keys("?keys")
),

rw!("sort-remove-functionally-determined-keys";
    "(sort (list ?key1 ?rest-keys) ?input)" =>
    "(sort (list ?key1) ?input)"
    if key-functionally-determines("?key1", "?rest-keys", "?input")
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> bool {
    // Must have duplicate keys or functional dependency
    stats.has_duplicate_sort_keys
        || stats.sort_key_has_functional_dependency
}
```

**Restrictions:**
- Must preserve sort semantics (remove only truly redundant keys)
- Functional dependency must be sound (from constraints or statistics)
- Must maintain sort stability properties

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let rows = stats.row_count as f64;
    let original_keys = stats.original_sort_keys as f64;
    let reduced_keys = stats.reduced_sort_keys as f64;
    let removed_keys = original_keys - reduced_keys;

    // Sort cost: O(n log n * k) where k is key comparison cost
    // Each key adds ~20ns comparison overhead
    let comparison_cost_per_key = 0.00000002; // 20ns
    let total_comparisons = rows * rows.log2();
    let savings_per_comparison = removed_keys * comparison_cost_per_key;
    let total_savings = total_comparisons * savings_per_comparison;

    // Normalize
    let total_sort_cost = total_comparisons * original_keys * comparison_cost_per_key;
    (total_savings / total_sort_cost).min(0.15)
}
```

**Assumptions:**
- Sort uses comparison-based algorithm (O(n log n))
- Each additional sort key adds ~20ns comparison cost
- Functional dependency detection is reliable

**Typical benefit**: 5-15% for multi-key sorts with redundancy.

## Test Cases

### Positive: Remove duplicate key

```sql
-- Accidentally sort by same column twice
SELECT *
FROM orders
ORDER BY customer_id, order_date, customer_id DESC;

-- Before:
-- Sort[customer_id ASC, order_date ASC, customer_id DESC]
--   Scan(orders)

-- After sort-remove-duplicate-keys:
-- Sort[customer_id ASC, order_date ASC]
--   Scan(orders)
-- Second customer_id is redundant (conflicts with first)
```

### Positive: Primary key makes other keys redundant

```sql
-- Sorting by PK makes all other columns redundant
SELECT *
FROM orders
ORDER BY order_id, customer_id, order_date;

-- Before:
-- Sort[order_id, customer_id, order_date]
--   Scan(orders)

-- After (if order_id is PRIMARY KEY):
-- Sort[order_id]
--   Scan(orders)
-- order_id uniquely determines row, other columns irrelevant
```

### Positive: Unique constraint enables removal

```sql
-- email is UNIQUE
SELECT *
FROM users
ORDER BY email, last_login;

-- After sort-remove-duplicate-keys:
-- Sort[email]
--   Scan(users)
-- email uniquely identifies rows, last_login sort is redundant
```

### Negative: Keys not functionally dependent

```sql
-- Both keys needed for correct ordering
SELECT *
FROM orders
ORDER BY customer_id, order_date;

-- Cannot remove order_date - multiple orders per customer
-- No functional dependency
```

### Positive: Redundant due to earlier sort

```sql
-- Composite key where prefix determines suffix
-- (e.g., sorting by year, month, day when date column exists)
SELECT *
FROM events
ORDER BY date, YEAR(date), MONTH(date);

-- YEAR(date) and MONTH(date) are redundant - date already includes them
```

## References

**Implementation in databases:**
- Apache Calcite: `SortRemoveDuplicateKeysRule.java`
- PostgreSQL: Redundant sort key elimination (pathkeys.c)
- MySQL: Sort key optimization

**Academic papers:**
- Graefe, "Query Evaluation Techniques for Large Databases", ACM Computing Surveys 1993
  - DOI: 10.1145/152610.152611
  - Section 3: Sorting techniques and optimizations
- Simmen et al., "Fundamental Techniques for Order Optimization", ACM SIGMOD 1996
  - DOI: 10.1145/233269.233320
  - Interesting orders and sort key optimization
- Galindo-Legaria & Joshi, "Orthogonal Optimization of Subqueries and Aggregation", ACM SIGMOD 2001
  - DOI: 10.1145/375663.375746
  - Functional dependencies in optimization
