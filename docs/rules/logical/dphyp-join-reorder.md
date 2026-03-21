# Rule: Calcite DphypJoinReorderRule

**Category:** logical/join-reordering
**File:** `rules/logical/join-reordering/dphyp-join-reorder.rra`

## Metadata

- **ID:** `calcite-dphyp-join-reorder`
- **Version:** "1.0.0"
- **Databases:** calcite, postgresql
- **Tags:** logical, calcite, join, reorder, dphyp, hypergraph, optimal
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(hyper-graph ?nodes ?edges)"
    description: "Hyper-graph representation of join inputs"
  - type: "predicate"
    condition: "count(?nodes) >= 3"
    description: "At least 3 relations needed for reordering"
  - type: "fact"
    fact_type: "statistics.cardinality"
    table: "?nodes"
    comparator: "exists"
    description: "Cardinality estimates must be available for cost-based decisions"
```


# Calcite DphypJoinReorderRule

## Description

Re-orders a join tree using the DPhyp (Dynamic Programming over
Hypergraph Partitioning) algorithm. DPhyp finds the optimal join
order for queries with complex join predicates, including non-inner
joins and hyperedge conditions.

**When to apply**: A query has 3+ tables joined together and the
current join order may be suboptimal.

**Why it works**: DPhyp explores the space of valid join orderings
using dynamic programming over connected subgraph complements. It
handles hypergraph structures (where a predicate touches 3+ tables)
that simpler algorithms cannot.

**Calcite class**: `org.apache.calcite.rel.rules.DphypJoinReorderRule`

## Relational Algebra

```algebra
-- Before: arbitrary join order
((A JOIN B) JOIN C) JOIN D

-- After: optimal join order (example)
(A JOIN D) JOIN (B JOIN C)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("calcite-dphyp-join-reorder";
    "(hyper-graph ?nodes ?edges)" =>
    "(dphyp-optimal-plan ?nodes ?edges)"
    if has_multiple_join_inputs("?nodes")
),
```

## Preconditions


> **Note:** Formal preconditions are defined in the YAML frontmatter above.

```rust
fn applicable(hyper_graph: &HyperGraph) -> bool {
    hyper_graph.nodes().len() >= 3
}
```

**Restrictions:**
- Exponential in number of tables (practical up to ~20 tables)
- Requires cardinality estimates for cost-based decisions
- Preserves outer join ordering constraints
- Configurable bloat factor controls search space

## Cost Model

```rust
fn estimated_benefit(
    current_cost: f64,
    optimal_cost: f64,
) -> f64 {
    if current_cost > 0.0 {
        (current_cost - optimal_cost) / current_cost
    } else {
        0.0
    }
}
```

**Typical benefit**: 10-95% for multi-way joins with bad initial ordering.

## Test Cases

```sql
-- Positive: 4-way join reordering
SELECT * FROM orders o
JOIN customers c ON o.cust_id = c.id
JOIN products p ON o.prod_id = p.id
JOIN categories cat ON p.cat_id = cat.id
WHERE c.region = 'US';
-- DPhyp finds optimal order based on cardinalities
```

```sql
-- Positive: star schema join reordering
SELECT * FROM fact f
JOIN dim1 d1 ON f.d1_id = d1.id
JOIN dim2 d2 ON f.d2_id = d2.id
JOIN dim3 d3 ON f.d3_id = d3.id;
-- Dimensions joined first if selective
```

```sql
-- Negative: 2-way join
SELECT * FROM emp e JOIN dept d ON e.deptno = d.deptno;
-- Only 2 tables; no reordering needed
```

## References

Calcite: core/src/main/java/org/apache/calcite/rel/rules/DphypJoinReorderRule.java (commit af6367d)
Paper: "Dynamic Programming Strikes Back" (Moerkotte, Neumann, 2006)
