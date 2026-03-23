# Rule: Intersect to Semi-Join

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/intersect-to-semi-join.rra`

## Metadata

- **ID:** `intersect-to-semi-join`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql, oracle
- **Tags:** intersect, semi-join, transformation, optimization
- **Authors:** "Apache Calcite Contributors"


# Intersect to Semi-Join

## Description

Converts an INTERSECT operation into a semi-join, which can be more efficiently
executed using hash-based algorithms. Semi-joins avoid materializing the right
side beyond a hash table and don't require distinct elimination in the same way
INTERSECT does.

**When to apply**: An INTERSECT operation appears between two relations. The
semi-join transformation enables hash-based execution and can leverage
specialized semi-join optimizations like bloom filters and early termination.

**Why it works**: INTERSECT finds rows from R that also appear in S. A semi-join
R $\ltimes$ S finds rows from R where a matching row exists in S - semantically
equivalent for INTERSECT. Semi-joins are optimized extensively in modern
databases and avoid the overhead of explicit distinct elimination.

## Relational Algebra

```algebra
INTERSECT(R, S) -> DISTINCT(R $\ltimes$_R=S S)
or with implicit deduplication:
INTERSECT(R, S) -> R $\ltimes$_R=S DISTINCT(S)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("intersect-to-semi-join";
    "(intersect (list ?r1 ?r2))" =>
    "(distinct
       (semi-join (all-columns-equal ?r1 ?r2)
         ?r1
         (distinct ?r2)))"
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    // Semi-join should be supported
    hw.supports_semi_join
        // Both relations should have compatible schemas
        && stats.schemas_compatible
        // Not applicable for INTERSECT ALL (different semantics)
        && !stats.is_intersect_all
        // Hash-based execution beneficial
        && stats.right_cardinality < stats.hash_table_memory_limit
}
```

**Restrictions:**
- Only applicable to INTERSECT DISTINCT (not INTERSECT ALL)
- Relations must have the same schema
- Join condition must be equality on all columns

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let left_rows = stats.left_cardinality as f64;
    let right_rows = stats.right_cardinality as f64;

    // Cost of INTERSECT (naive):
    // - Hash distinct on R: left_rows * 1.5
    // - Hash distinct on S: right_rows * 1.5
    // - Compare hash tables: min(left_distinct, right_distinct) * 1.0
    let distinct_factor = 0.7; // Assume 70% duplicates
    let intersect_cost = (left_rows * 1.5) + (right_rows * 1.5)
        + (left_rows * distinct_factor);

    // Cost of semi-join:
    // - Hash distinct on right: right_rows * 1.5
    // - Build hash table: right_distinct * 1.0
    // - Probe from left with dedup: left_rows * 1.2
    let semi_join_cost = (right_rows * 1.5)
        + (right_rows * distinct_factor)
        + (left_rows * 1.2);

    // Benefit from:
    // 1. Better hash join algorithms (bloom filters, vectorization)
    // 2. Short-circuit on first match (semi-join semantics)
    let optimization_benefit = 0.2;

    if intersect_cost > semi_join_cost {
        ((intersect_cost - semi_join_cost) / intersect_cost) + optimization_benefit
    } else {
        optimization_benefit
    }
}
```

**Assumptions:**
- Semi-join avoids materializing full right side matches
- Hash-based semi-join is well-optimized (bloom filters, SIMD)
- DISTINCT on right side can use early deduplication

**Typical benefit**: 20-60% from better semi-join execution.

## Test Cases

### Positive: Two-way intersect

```sql
-- Find customers who made purchases in both 2023 and 2024
SELECT customer_id
FROM purchases_2023
INTERSECT
SELECT customer_id
FROM purchases_2024;

-- Before:
-- Intersect
--   Scan(purchases_2023)
--   Scan(purchases_2024)

-- After intersect-to-semi-join:
-- Distinct
--   SemiJoin(p1.customer_id = p2.customer_id)
--     Scan(purchases_2023 as p1)
--     Distinct
--       Scan(purchases_2024 as p2)
```

### Positive: Small right side for hash table

```sql
-- Right side (premium_customers) is small, fits in hash table
SELECT user_id, email
FROM all_users
INTERSECT
SELECT user_id, email
FROM premium_customers;

-- Semi-join builds hash table on small premium_customers set
```

### Negative: INTERSECT ALL

```sql
-- Preserve duplicate counts
SELECT order_id
FROM orders_batch_1
INTERSECT ALL
SELECT order_id
FROM orders_batch_2;

-- INTERSECT ALL has different semantics:
-- Result count = min(count_in_R, count_in_S)
-- Cannot use semi-join (which only checks existence)
```

### Positive: Multi-column intersect

```sql
-- Intersect on all columns
SELECT customer_id, product_id, order_date
FROM orders_region_1
INTERSECT
SELECT customer_id, product_id, order_date
FROM orders_region_2;

-- Semi-join on (customer_id, product_id, order_date)
```

## References

**Implementation in databases:**
- Apache Calcite: `IntersectToSemiJoinRule.java`
- PostgreSQL: INTERSECT implementation via semi-join (prepunion.c)
- Oracle: Semi-join optimization for set operations

**Academic papers:**
- Bernstein & Chiu, "Using Semi-Joins to Solve Relational Queries", JACM 1981
  - DOI: 10.1145/322234.322238
  - Foundational work on semi-join optimization
- Graefe, "Query Evaluation Techniques for Large Databases", ACM Computing Surveys 1993
  - DOI: 10.1145/152610.152611
  - Section 7.3: Set operations and semi-joins
- Galindo-Legaria & Joshi, "Orthogonal Optimization of Subqueries and Aggregation", ACM SIGMOD 2001
  - DOI: 10.1145/375663.375746
  - Semi-join transformations
