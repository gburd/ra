# Rule: Union Eliminator

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/union-eliminator.rra`

## Metadata

- **ID:** `union-eliminator`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql
- **Tags:** union, elimination, simplification
- **Authors:** "Apache Calcite Contributors"


# Union Eliminator

## Description

Eliminates UNION operations when one or more branches are provably empty,
or when all branches are identical. This removes unnecessary union overhead
and simplifies the query plan when branches can be statically determined
to be redundant.

**When to apply**: A UNION operation has empty branches (e.g., filter that
eliminates all rows), or all branches reference the same table with identical
filters/projections.

**Why it works**: Union operations have overhead (deduplication for UNION,
concatenation for UNION ALL). If branches are empty or identical, the union
is unnecessary and can be eliminated or simplified.

## Relational Algebra

```algebra
UNION(R, ∅) -> R
UNION(∅, ∅) -> ∅
UNION(R, R) -> R
UNION_ALL(R, ∅) -> R
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("union-eliminator-empty-left";
    "(union ?all (list (empty) ?right))" =>
    "?right"
),

rw!("union-eliminator-empty-right";
    "(union ?all (list ?left (empty)))" =>
    "?left"
),

rw!("union-eliminator-identical";
    "(union false (list ?rel ?rel))" =>
    "?rel"
),

rw!("union-eliminator-all-empty";
    "(union ?all (list (empty) (empty)))" =>
    "(empty)"
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> bool {
    // At least one branch is empty
    stats.has_empty_branch
        // Or all branches are identical
        || stats.all_branches_identical
}
```

**Restrictions:**
- Empty branch detection must be sound (from statistics or constraints)
- Identical branch detection must account for query semantics

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let n_empty_branches = stats.n_empty_branches as f64;
    let n_total_branches = stats.n_branches as f64;

    if n_empty_branches == n_total_branches {
        // All branches empty -> entire union eliminated
        return 1.0;
    }

    if stats.all_branches_identical {
        // Identical branches -> eliminate union and duplicates
        return 0.9;
    }

    // Partial elimination - remove empty branches
    // Benefit: avoid scanning empty branches, simpler union
    let elimination_ratio = n_empty_branches / n_total_branches;
    0.8 * elimination_ratio
}
```

**Assumptions:**
- Empty branches contribute minimal cost but union overhead remains
- Identical branch elimination avoids redundant work entirely
- Complete union elimination is 100% benefit

**Typical benefit**: 80-100% when union can be fully or mostly eliminated.

## Test Cases

### Positive: Remove empty branch

```sql
-- One branch has contradictory filter
SELECT * FROM orders WHERE region = 'US'
UNION
SELECT * FROM orders WHERE region = 'EU' AND 1 = 0;

-- Before:
-- Union
--   Filter(region = 'US')
--     Scan(orders)
--   Filter(region = 'EU' AND 1 = 0)  -- Always empty!
--     Scan(orders)

-- After union-eliminator:
-- Filter(region = 'US')
--   Scan(orders)
```

### Positive: Both branches empty

```sql
-- Contradictory conditions in both branches
SELECT * FROM products WHERE price > 1000 AND price < 100
UNION
SELECT * FROM products WHERE category = 'X' AND category = 'Y';

-- After union-eliminator:
-- Empty()
```

### Positive: Identical branches

```sql
-- Same query in both branches (user error)
SELECT customer_id, name FROM customers WHERE active = true
UNION
SELECT customer_id, name FROM customers WHERE active = true;

-- After union-eliminator:
-- SELECT customer_id, name FROM customers WHERE active = true
-- (Union eliminated, single query remains)
```

### Positive: Empty from partition pruning

```sql
-- Partition pruning makes one branch empty
SELECT * FROM sales_2023 WHERE year = 2024  -- Empty!
UNION ALL
SELECT * FROM sales_2024 WHERE year = 2024;

-- After union-eliminator:
-- SELECT * FROM sales_2024 WHERE year = 2024
```

### Negative: Both branches non-empty and different

```sql
-- Both branches contribute different results
SELECT * FROM orders_2023
UNION
SELECT * FROM orders_2024;

-- Cannot eliminate - both branches needed
```

## References

**Implementation in databases:**
- Apache Calcite: `UnionEliminatorRule.java`
- PostgreSQL: Empty relation elimination (prepunion.c)
- Presto: Union branch pruning

**Academic papers:**
- Graefe & DeWitt, "The EXODUS Optimizer Generator", ACM SIGMOD 1987
  - DOI: 10.1145/38713.38734
  - Dead code elimination in query plans
- Chaudhuri, "An Overview of Query Optimization in Relational Systems", ACM PODS 1998
  - DOI: 10.1145/275487.275492
  - Query simplification techniques
- Pirahesh et al., "Extensible/Rule Based Query Rewrite Optimization in Starburst", ACM SIGMOD 1992
  - DOI: 10.1145/130283.130294
  - Query rewrite rules including dead branch elimination
