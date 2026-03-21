# Starburst Semantic Query Optimization

**Rule ID:** `starburst-semantic-optimization`
**Category:** logical/predicate-pushdown
**Supported Databases:**  postgresql
**Tags:** semantic-optimization, constraints, integrity-rules, classic

## Description


# Starburst Semantic Query Optimization

## Description

Leverages semantic knowledge (integrity constraints, functional dependencies,
CHECK constraints, foreign keys) to derive new predicates, eliminate joins,
or prove queries unsatisfiable. This goes beyond syntactic rewriting by using
domain knowledge encoded in schema constraints.

**When to apply**: Queries involving tables with rich semantic constraints
(foreign keys, check constraints, unique constraints). Semantic optimization
can derive implied predicates, eliminate provably-redundant joins, or short-
circuit impossible queries before execution.

**Why it works**: Traditional optimizers only use syntax-directed rewrites.
Semantic optimization reasons about the data model: if a CHECK constraint says
price > 0, and a query filters price < 0, the optimizer can return empty
result without executing. Foreign keys enable join elimination.

## Relational Algebra

```algebra
Given constraints C and query Q:

1. Predicate derivation:
   If C ⊢ P1 → P2 and Q contains P1, add P2 to Q

2. Join elimination:
   If FK(R.a → S.pk) and Q projects only R columns, remove join with S

3. Unsatisfiability detection:
   If Q contains P and C ⊢ ¬P, return ∅

4. Transitive closure of constraints:
   If R.a = S.b and S.b > 100, derive R.a > 100
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Semantic optimization requires constraint database
// Here are pattern-based examples:

rw!("semantic-join-elimination-fk";
    "(project ?cols
       (join inner ?pred ?left
         (scan ?right ?right-name)))" =>
    "(project ?cols ?left)"
    if foreign-key-not-null("?left", "?right", "?pred")
       && !uses-right-columns("?cols", "?right")
),

rw!("semantic-unsatisfiable-query";
    "(filter ?pred ?input)" =>
    "(empty)"
    if contradicts-check-constraint("?pred", "?input")
),

rw!("semantic-derive-predicate";
    "(filter ?pred1 ?input)" =>
    "(filter (and ?pred1 ?pred2) ?input)"
    if can-derive-from-constraints("?pred1", "?pred2", "?input")
),

rw!("semantic-transitive-constraint";
    "(filter (and (= ?col1 ?col2) (> ?col2 ?const))
       ?input)" =>
    "(filter (and (= ?col1 ?col2) (> ?col2 ?const) (> ?col1 ?const))
       ?input)"
),
```

## Preconditions


> **Note:** Formal preconditions are defined in the YAML frontmatter above.

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    // Must have semantic constraints available
    stats.has_integrity_constraints
        // Constraint checking enabled
        && hw.enforce_constraints
        // Constraints must be trusted (not deferred/disabled)
        && stats.constraints_are_trusted
}
```

**Restrictions:**
- Requires accurate constraint metadata
- Constraints must be enforced (not just declared)
- Cannot use violated or unenforced constraints
- Must handle constraint interaction carefully (multiple constraints may interact)

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let mut benefit = 0.0;

    // Join elimination benefit
    if stats.can_eliminate_join_via_fk {
        let join_cost_savings = stats.eliminated_join_cardinality as f64 * 0.00001;
        let scan_cost = stats.base_table_cardinality as f64 * 0.000001;
        benefit += (join_cost_savings / scan_cost).min(1.0);
    }

    // Unsatisfiable query detection (100% benefit)
    if stats.query_is_unsatisfiable {
        return 1.0;
    }

    // Derived predicate benefit (enables further pushdown)
    if stats.can_derive_predicates {
        let n_derived = stats.n_derived_predicates as f64;
        let avg_selectivity = 0.1; // Assume derived predicates are selective
        benefit += 0.2 * n_derived * (1.0 - avg_selectivity);
    }

    // Transitive predicate benefit
    if stats.can_apply_transitive_constraints {
        benefit += 0.3; // Enable index usage, earlier filtering
    }

    benefit.min(5.0) // Cap at 5x improvement
}
```

**Assumptions:**
- Foreign key constraints are enforced (no orphaned references)
- CHECK constraints are valid on all rows
- Derived predicates are selective enough to benefit
- Constraint checking overhead is negligible

**Typical benefit**: 30% to 5x for constraint-rich schemas.

## Test Cases

### Positive: Join elimination via foreign key

```sql
-- Schema: orders.customer_id REFERENCES customers(id) NOT NULL
-- Query only needs order data, not customer data
SELECT order_id, total, order_date
FROM orders o
JOIN customers c ON o.customer_id = c.id;

-- Before:
-- Project[order_id, total, order_date]
--   Join(o.customer_id = c.id)
--     Scan(orders)
--     Scan(customers)

-- After semantic optimization:
-- Project[order_id, total, order_date]
--   Scan(orders)
-- Join eliminated: FK ensures every order.customer_id exists in customers
-- and query doesn't use any customer columns
```

### Positive: Unsatisfiable query detection

```sql
-- Schema: products.price > 0 (CHECK constraint)
-- Impossible query
SELECT * FROM products
WHERE price < 0;

-- After semantic optimization:
-- Empty() -- returns no rows without executing
```

### Positive: Derive predicate from equality and constraint

```sql
-- Schema: employees.salary > 0 (CHECK constraint)
-- Query with transitive opportunity
SELECT *
FROM departments d
JOIN employees e ON d.manager_id = e.employee_id
WHERE e.salary > 100000;

-- Semantic optimization derives:
-- d.manager_id exists in employees (FK)
-- → d.manager's salary > 0 (CHECK constraint)
-- → Can use index on employees(salary) if available
```

### Positive: Contradiction detection

```sql
-- Schema: users.age >= 18 AND users.age <= 120 (CHECK)
-- Contradictory query
SELECT * FROM users
WHERE age > 150 OR age < 0;

-- After semantic optimization:
-- Empty() -- constraints make this impossible
```

### Negative: Unenforced constraints

```sql
-- Schema: orders.customer_id REFERENCES customers(id) -- BUT NOT ENFORCED
SELECT order_id, total
FROM orders o
JOIN customers c ON o.customer_id = c.id;

-- Cannot eliminate join: constraint not enforced
-- May have orphaned orders with invalid customer_id
```

### Positive: Partition pruning via constraint

```sql
-- Schema: sales_2023.year = 2023 (CHECK constraint from partitioning)
-- Query with year filter
SELECT *
FROM (
  SELECT * FROM sales_2023
  UNION ALL
  SELECT * FROM sales_2024
) AS sales
WHERE year = 2024;

-- Semantic optimization:
-- sales_2023.year = 2023 (always) contradicts year = 2024
-- → Eliminate sales_2023 scan entirely
```

## References

**Original papers:**
- Pirahesh, H., Hellerstein, J.M., Hasan, W., "Extensible/Rule Based Query Rewrite Optimization in Starburst", ACM SIGMOD 1992
  - DOI: 10.1145/130283.130294
  - THE foundational paper on semantic query optimization
  - Constraint-based rewriting, integrity rules

- King, J.J., "QUIST: A System for Semantic Query Optimization in Relational Databases", VLDB 1981
  - Early work on semantic optimization
  - Predicate derivation from constraints

- Chakravarthy, U.S., Grant, J., Minker, J., "Logic-Based Approach to Semantic Query Optimization", ACM TODS 1990
  - DOI: 10.1145/78922.78924
  - Theoretical foundations using integrity constraints

**Modern implementations and extensions:**
- Galindo-Legaria, C., Joshi, M., "Orthogonal Optimization of Subqueries and Aggregation", ACM SIGMOD 2001
  - DOI: 10.1145/375663.375746
  - Modern semantic optimization in mssql

- Rao, J., et al., "Using EELs, a Practical Approach to Outerjoin and Antijoin Reordering", IEEE Data Engineering 2001
  - Semantic optimization with outer joins

**Implementation in databases:**
- IBM DB2: Full Starburst semantic optimization (production since 1990s)
- PostgreSQL: Partial semantic optimization (constraint exclusion, join removal)
  - `src/backend/optimizer/plan/analyzejoins.c` - join removal
  - `src/backend/optimizer/util/plancat.c` - constraint exclusion
- Oracle: Constraint-based optimization
- mssql: Semantic optimization (join elimination, contradiction detection)
