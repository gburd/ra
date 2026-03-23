# Rule: Join to MultiJoin

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/join-to-multi-join.rra`

## Metadata

- **ID:** `join-to-multi-join`
- **Version:** "1.0.0"
- **Databases:** calcite
- **Tags:** join, multi-join, join-reordering, optimization
- **Authors:** "Apache Calcite Contributors"


# Join to MultiJoin

## Description

Converts a tree of inner joins into a single MultiJoin operator that
represents all join inputs and join conditions together. This enables
advanced join reordering algorithms (dynamic programming, greedy heuristics)
to consider all possible join orders simultaneously.

**When to apply**: A query has multiple inner joins that could benefit from
reordering. Converting to MultiJoin creates a flattened representation that
join reordering rules can then optimize.

**Why it works**: Binary join trees impose a fixed join order. By flattening
to MultiJoin, the optimizer can explore alternative join orders using
sophisticated algorithms like DPHyp (dynamic programming with hypergraphs)
or bushy join enumeration, finding lower-cost plans.

## Relational Algebra

```algebra
(R1 $\bowtie$_{p1} R2) $\bowtie$_{p2} R3 ->
  MultiJoin([R1, R2, R3], [p1, p2])
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("join-to-multi-join-binary";
    "(join inner ?pred2
       (join inner ?pred1 ?left ?right1)
       ?right2)" =>
    "(multi-join (list ?left ?right1 ?right2)
                 (list ?pred1 ?pred2))"
    if all-inner-joins("?pred1", "?pred2")
),

// Recursive case: add to existing MultiJoin
rw!("join-to-multi-join-extend";
    "(join inner ?pred
       (multi-join ?inputs ?preds)
       ?right)" =>
    "(multi-join (append ?inputs ?right)
                 (append ?preds ?pred))"
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    // Only applicable to inner joins
    stats.all_inner_joins
        // Must have at least 3 relations to benefit from reordering
        && stats.n_relations >= 3
        // Should use advanced join reordering
        && hw.enable_advanced_join_reordering
}
```

**Restrictions:**
- Only applicable to INNER JOIN (not outer joins)
- All join predicates must be on the relation inputs (no cross products)
- Join conditions should be equality-based for best results

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    // MultiJoin itself has no direct benefit
    // Benefit comes from subsequent join reordering rules

    // The transformation is essentially free (just representation change)
    // But enables expensive join enumeration algorithms

    // If join enumeration is disabled, this is just overhead
    if !stats.enable_join_reordering {
        return -0.05; // Small penalty for conversion overhead
    }

    // Otherwise, neutral transformation that enables optimization
    // Actual benefit depends on subsequent MultiJoinOptimize rules
    0.0
}
```

**Assumptions:**
- MultiJoin is an intermediate representation (not executed directly)
- Subsequent rules (DPHyp, BushyJoinOptimize) will find better join orders
- Transformation overhead is negligible

**Typical benefit**: 0% directly, but enables 10-100x improvements from optimal join ordering.

## Test Cases

### Positive: Convert 3-way join

```sql
-- Three-way join with potential for reordering
SELECT *
FROM orders o
JOIN customers c ON o.customer_id = c.id
JOIN products p ON o.product_id = p.id;

-- Before:
-- Join(o.product_id = p.id)
--   Join(o.customer_id = c.id)
--     Scan(orders)
--     Scan(customers)
--   Scan(products)

-- After join-to-multi-join:
-- MultiJoin([orders, customers, products],
--           [o.customer_id = c.id, o.product_id = p.id])

-- This enables DPHyp or BushyJoin to find optimal order
```

### Positive: Star join schema

```sql
-- Fact table with multiple dimension joins
SELECT *
FROM fact_sales f
JOIN dim_date d ON f.date_id = d.id
JOIN dim_product p ON f.product_id = p.id
JOIN dim_customer c ON f.customer_id = c.id
JOIN dim_store s ON f.store_id = s.id;

-- Converts to MultiJoin with 5 inputs, enabling star join optimization
```

### Negative: Contains outer join

```sql
-- Cannot flatten outer joins into MultiJoin
SELECT *
FROM orders o
LEFT JOIN customers c ON o.customer_id = c.id
JOIN products p ON o.product_id = p.id;

-- Left join must stay separate (preserves NULL semantics)
```

### Negative: Too few relations

```sql
-- Only 2 relations, no reordering benefit
SELECT *
FROM orders o
JOIN customers c ON o.customer_id = c.id;

-- MultiJoin overhead not worth it for 2-way join
```

## References

**Implementation in databases:**
- Apache Calcite: `JoinToMultiJoinRule.java`, `MultiJoin.java`
- Subsequent optimization: `DphypJoinReorderRule.java`, `MultiJoinOptimizeBushyRule.java`

**Academic papers:**
- Moerkotte & Neumann, "Dynamic Programming Strikes Back", ACM SIGMOD 2008
  - DOI: 10.1145/1376616.1376672
  - DPHyp algorithm for optimal join ordering using hypergraphs
- Moerkotte & Neumann, "Analysis of Two Existing and One New Dynamic Programming Algorithm for the Generation of Optimal Bushy Join Trees without Cross Products", VLDB 2006
  - DOI: 10.1007/s00778-005-0165-6
  - Algorithms for bushy join tree enumeration
- Selinger et al., "Access Path Selection in a Relational Database", ACM SIGMOD 1979
  - DOI: 10.1145/582095.582099
  - Original dynamic programming approach for join ordering
