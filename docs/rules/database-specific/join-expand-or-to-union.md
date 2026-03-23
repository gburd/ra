# Rule: Join Expand OR to Union

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/join-expand-or-to-union.rra`

## Metadata

- **ID:** `join-expand-or-to-union`
- **Version:** "1.0.0"
- **Databases:** calcite, oracle
- **Tags:** join, or-expansion, union, optimization
- **Authors:** "Apache Calcite Contributors"


# Join Expand OR to Union

## Description

Expands joins with OR conditions in the join predicate into a UNION of
multiple joins, each with a simpler AND-only predicate. This enables the
use of efficient join algorithms (hash join, merge join) that struggle
with OR conditions, and allows index usage on individual branches.

**When to apply**: A join has an OR condition in its join predicate. The
OR can be expanded using the distributive law: (A OR B) becomes UNION of
separate joins on A and B.

**Why it works**: Most join algorithms (hash join, merge join) are optimized
for equality or simple inequality conditions. OR conditions force nested
loop joins or complex hash probing. By expanding to UNION, each branch can
use efficient join algorithms, and the union deduplicates results.

## Relational Algebra

```algebra
R $\bowtie$_{p1 $\lor$ p2} S -> (R $\bowtie$_{p1} S) $\cup$ (R $\bowtie$_{p2} S)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("join-expand-or-to-union";
    "(join ?type (or ?pred1 ?pred2) ?left ?right)" =>
    "(union false (list
       (join ?type ?pred1 ?left ?right)
       (join ?type ?pred2 ?left ?right)))"
    if join-type-preserves-union-semantics("?type")
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    // Must have OR in join condition
    stats.join_predicate_has_or
        // Join type must be INNER (OUTER joins complicate semantics)
        && stats.join_type == JoinType::Inner
        // Individual branches should be hash-joinable
        && stats.or_branches_are_equijoin
        // Combined cost should be less than nested loop
        && stats.estimated_union_cost < stats.estimated_nested_loop_cost
}
```

**Restrictions:**
- Primarily applicable to INNER JOIN (OUTER joins require careful handling)
- OR branches should be equality predicates (to benefit from hash join)
- May increase total work if OR branches overlap significantly
- Need UNION (not UNION ALL) to eliminate duplicates from overlapping matches

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let left_rows = stats.left_cardinality as f64;
    let right_rows = stats.right_cardinality as f64;
    let n_or_branches = stats.n_or_branches as f64;

    // Cost of nested loop join with OR condition:
    // - Must evaluate complex OR for every pair
    let nested_loop_cost = left_rows * right_rows * 0.00001; // 10$\mu$s per pair

    // Cost of expanded UNION approach:
    // - Build hash table per branch: n_branches * right_rows * 1.5
    // - Probe from left per branch: n_branches * left_rows * 1.0
    // - Union deduplication: result_rows * 1.5
    let hash_join_cost = (n_or_branches * right_rows * 1.5)
        + (n_or_branches * left_rows * 1.0);
    let union_cost = (left_rows * stats.join_selectivity * n_or_branches * 1.5);
    let total_union_cost = hash_join_cost + union_cost;

    // Benefit calculation
    if nested_loop_cost > total_union_cost {
        (nested_loop_cost - total_union_cost) / nested_loop_cost
    } else {
        0.0
    }
}
```

**Assumptions:**
- OR conditions force nested loop join (O(n*m))
- Hash joins are much faster: O(n+m) per branch
- UNION deduplication cost is acceptable
- OR branches have low overlap (minimal duplicate elimination cost)

**Typical benefit**: 20-70% for large joins with OR conditions.

## Test Cases

### Positive: OR condition in join

```sql
-- Join with OR condition
SELECT *
FROM orders o
JOIN customers c
  ON o.customer_id = c.id OR o.backup_customer_id = c.id;

-- Before:
-- NestedLoopJoin(o.customer_id = c.id OR o.backup_customer_id = c.id)
--   Scan(orders)
--   Scan(customers)

-- After join-expand-or-to-union:
-- Union
--   HashJoin(o.customer_id = c.id)
--     Scan(orders)
--     Scan(customers)
--   HashJoin(o.backup_customer_id = c.id)
--     Scan(orders)
--     Scan(customers)
```

### Positive: Enable index usage

```sql
-- Indexes on both customer_id and email
SELECT *
FROM orders o
JOIN customers c
  ON o.customer_id = c.id OR o.customer_email = c.email;

-- Each branch can use a different index:
-- Branch 1: Index on (customer_id)
-- Branch 2: Index on (email)
```

### Negative: High overlap between branches

```sql
-- Most rows match both conditions
SELECT *
FROM employees e1
JOIN employees e2
  ON e1.manager_id = e2.id OR e1.department = e2.department;

-- If most employees share departments with their managers,
-- UNION will have significant duplicate elimination cost
```

### Positive: Complex OR with multiple branches

```sql
-- Multiple OR conditions
SELECT *
FROM products p
JOIN suppliers s
  ON p.primary_supplier_id = s.id
  OR p.backup_supplier_id = s.id
  OR p.manufacturer_id = s.id;

-- Expands to 3-way UNION of hash joins
```

### Negative: LEFT JOIN semantic preservation

```sql
-- LEFT JOIN with OR requires careful handling
SELECT *
FROM orders o
LEFT JOIN customers c
  ON o.customer_id = c.id OR o.backup_customer_id = c.id;

-- Cannot simply expand to UNION - must preserve NULL-extended rows
-- Requires more complex transformation
```

## References

**Implementation in databases:**
- Apache Calcite: `JoinExpandOrToUnionRule.java`
- Oracle: OR-expansion optimization (documented in tuning guides)
- mssql: OR condition handling in join optimizer

**Academic papers:**
- Graefe & McKenna, "The Volcano Optimizer Generator", IEEE Data Engineering 1993
  - Transformation rules including OR-expansion
- Chaudhuri et al., "Optimizing Queries with Materialized Views", IEEE Data Engineering 1995
  - DOI: 10.1109/69.382296
  - Query rewriting including OR-expansion
- Ono & Lohman, "Measuring the Complexity of Join Enumeration in Query Optimization", VLDB 1990
  - Join condition complexity and optimization strategies
