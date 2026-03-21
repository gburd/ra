# Rule: Neo4j Pattern Comprehension Optimization

**Category:** database-specific/neo4j
**File:** `rules/database-specific/neo4j/pattern-comprehension-optimization.rra`

## Metadata

- **ID:** `neo4j-pattern-comprehension-optimization`
- **Version:** "1.0.0"
- **Databases:** neo4j
- **Tags:** pattern-comprehension, subquery, list, cypher, optimization
- **Authors:** "Neo4j Inc."


# Neo4j Pattern Comprehension Optimization

## Description

Optimizes Cypher pattern comprehensions (inline subqueries that produce lists)
by fusing them with the outer query's expansion plan. Instead of executing the
pattern comprehension as a separate nested subquery for each row, the optimizer
rewrites it into a single traversal with aggregation.

**When to apply**: Cypher queries using pattern comprehensions like
`[(n)-[:REL]->(m) | m.prop]` or `EXISTS {(n)-[:REL]->(m)}` patterns. The planner
rewrites nested pattern matching into flat traversal with rollup.

**Why it works**: A naive implementation executes the inner pattern for each
outer row independently, like a correlated subquery in SQL. By flattening the
pattern into the outer plan with a group-by aggregation, Neo4j avoids repeated
traversal setup and can batch relationship lookups.

## Relational Algebra

```algebra
-- Before: correlated subquery per row
for each n in NodeScan(:Person):
  result[n] = [m.name for m in expand(n, :KNOWS, outgoing)]

-- After: flattened traversal with rollup
expand(NodeScan(:Person), :KNOWS, outgoing)
  |> group by source_node
  |> collect(target.name) as list
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("neo4j-flatten-pattern-comprehension";
    "(pattern-comprehension ?outer
       (expand ?var ?rel-type ?dir)
       (project ?expr))" =>
    "(rollup-aggregate ?outer
       (expand ?var ?rel-type ?dir)
       (collect (project ?expr)))"
    if is_simple_expansion("?rel-type", "?dir")
),

rw!("neo4j-exists-to-semi-apply";
    "(filter (exists (expand ?var ?rel-type ?dir))
       ?source)" =>
    "(semi-apply ?source
       (expand ?var ?rel-type ?dir))"
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> bool {
    stats.has_pattern_comprehension
        && stats.outer_cardinality > 10
        && stats.inner_pattern_is_simple_expansion
}
```

**Restrictions:**
- Complex inner patterns with multiple hops may not flatten
- Pattern comprehensions with WHERE clauses require careful predicate placement
- Aggregation rollup changes memory profile (stores partial lists)
- Cannot flatten when inner pattern references variables from multiple outer scopes

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let outer_rows = stats.outer_cardinality as f64;
    let avg_inner_results = stats.avg_pattern_results as f64;
    let setup_cost_per_subquery = 0.01; // ms

    // Correlated: setup cost per outer row
    let correlated_cost = outer_rows * (setup_cost_per_subquery
        + avg_inner_results * 0.001);

    // Flattened: single traversal + aggregation
    let flattened_cost = outer_rows * avg_inner_results * 0.001
        + outer_rows * 0.0001; // aggregation overhead

    if correlated_cost > flattened_cost {
        (correlated_cost - flattened_cost) / correlated_cost
    } else {
        0.0
    }
}
```

**Typical benefit**: 20% to 5x for pattern comprehensions over many rows.

## Test Cases

### Positive: Simple pattern comprehension flattening

```cypher
// Collect all friend names for each person
MATCH (p:Person)
RETURN p.name, [(p)-[:KNOWS]->(f) | f.name] AS friends

// Optimizer flattens into:
// MATCH (p:Person)-[:KNOWS]->(f)
// RETURN p.name, collect(f.name) AS friends
// Single traversal instead of per-row subquery
```

### Positive: EXISTS to semi-apply

```cypher
// Find people who have at least one friend
MATCH (p:Person)
WHERE EXISTS { (p)-[:KNOWS]->() }
RETURN p

// Rewritten to semi-apply:
// NodeByLabelScan(:Person) |> SemiApply(Expand(:KNOWS))
// Stops at first match per person (short-circuit)
```

### Negative: Complex inner pattern

```cypher
// Multi-hop pattern with filter cannot trivially flatten
MATCH (p:Person)
RETURN p.name,
  [(p)-[:KNOWS*2..3]->(f) WHERE f.age > 30 | f.name] AS distant_friends

// Variable-length path + filter: kept as nested pattern
```

## References

**Implementation:**
- Neo4j source: `org.neo4j.cypher.internal.compiler.planner.logical.PatternExpressionSolver`
- RollUp apply: `org.neo4j.cypher.internal.logical.plans.RollUpApply`
- Semi-apply: `org.neo4j.cypher.internal.logical.plans.SemiApply`

**Documentation:**
- Neo4j Manual: "Pattern Comprehension"
  - https://neo4j.com/docs/cypher-manual/current/values-and-types/lists/

**Papers:**
- Angles, R., et al., "G-CORE: A Core for Future Graph Query Languages", SIGMOD 2018
