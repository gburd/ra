# Rule: Free Join (Worst-Case Optimal Join)

**Category:** experimental/wcoj
**File:** `rules/experimental/wcoj/free-join.rra`

## Metadata

- **ID:** `free-join`
- **Version:** "1.0.0"
- **Databases:** duckdb, postgresql
- **Tags:** wcoj, free-join, triangle-query, cyclic-join
- **Authors:** "Ngo et al. 2012", "RA Contributors"


# Free Join (Worst-Case Optimal Join)

## Description

Replaces a sequence of binary joins with a worst-case optimal join (WCOJ)
algorithm that processes all relations simultaneously using an intersect-based
approach. Free Join is particularly effective for cyclic queries (e.g., triangle
queries) where binary join plans have exponential worst-case complexity.

**When to apply**: Query contains cyclic join patterns or multiple joins where
traditional binary join ordering has high intermediate result sizes.

**Why it works**: Binary join plans can produce intermediate results exponentially
larger than the final output. WCOJ algorithms like Free Join enumerate output
tuples directly by computing attribute intersections in worst-case optimal time
O(N^ρ*) where ρ* is the fractional edge cover number.

## Relational Algebra

```algebra
(R join[R.a=S.a] S) join[S.b=T.b AND R.c=T.c] T
  -> free_join[{R.a=S.a, S.b=T.b, R.c=T.c}](R, S, T)
  where is_cyclic({R.a=S.a, S.b=T.b, R.c=T.c})
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("free-join";
    "(join ?pred1
       (join ?pred2 ?r1 ?r2)
       ?r3)" =>
    "(free_join (and ?pred1 ?pred2) ?r1 ?r2 ?r3)"
    if is_cyclic_query()
    if all_equijoins("?pred1", "?pred2")
),
```

## Preconditions

```rust
fn applicable(
    relations: &[RelExpr],
    predicates: &[JoinPredicate],
) -> bool {
    // Check if query graph is cyclic
    let join_graph = build_join_graph(relations, predicates);
    if !join_graph.is_cyclic() {
        return false;
    }

    // All predicates must be equi-joins
    if !predicates.iter().all(|p| p.is_equijoin()) {
        return false;
    }

    // Must have at least 3 relations
    relations.len() >= 3
}

fn build_join_graph(
    relations: &[RelExpr],
    predicates: &[JoinPredicate],
) -> JoinGraph {
    let mut graph = JoinGraph::new(relations.len());
    for pred in predicates {
        let (r1, r2) = pred.relation_pair();
        graph.add_edge(r1, r2);
    }
    graph
}
```

**Restrictions:**
- All join predicates must be equi-joins
- Query must form a cyclic join graph
- Relations should have indexes on join attributes
- Not beneficial for simple linear join patterns

## Cost Model

```rust
fn estimated_benefit(
    relations: &[Statistics],
    predicates: &[JoinPredicate],
) -> f64 {
    // Compute AGM bound (worst-case optimal complexity)
    let agm_bound = compute_agm_bound(relations, predicates);

    // Estimate best binary join plan cost
    let binary_plan_cost = estimate_binary_join_cost(
        relations,
        predicates,
    );

    // Free join cost: AGM bound + intersection overhead
    let intersection_overhead = relations.len() as f64 * 1.2;
    let free_join_cost = agm_bound * intersection_overhead;

    if binary_plan_cost > free_join_cost {
        (binary_plan_cost - free_join_cost) / binary_plan_cost
    } else {
        0.0
    }
}

fn compute_agm_bound(
    relations: &[Statistics],
    predicates: &[JoinPredicate],
) -> f64 {
    // AGM bound: N^ρ* where ρ* is fractional edge cover
    let n = relations
        .iter()
        .map(|r| r.row_count)
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or(1.0);

    let rho_star = compute_fractional_edge_cover(predicates);
    n.powf(rho_star)
}
```

**Assumptions:**
- Input relations have indexes on join attributes
- Intersection operations use hash-based set intersection
- ρ* (fractional edge cover) is computed via linear programming
- Galloping intersection when one attribute is significantly smaller

**Typical benefit**: 5x-100x for triangle queries and cyclic patterns with
high intermediate result sizes.

## Test Cases

### Positive: Triangle query (3-cycle)

```sql
-- Find triangles: users who like the same items
SELECT u1.id, u2.id, i.id
FROM likes l1
JOIN likes l2 ON l1.item_id = l2.item_id
JOIN likes l3 ON l1.user_id = l3.user_id AND l2.user_id = l3.user_id
WHERE l1.user_id < l2.user_id;

-- Expected: Free join over (l1, l2, l3)
-- Binary plan intermediate: |likes|^2, Free join: O(|likes|^1.5)
```

### Positive: Diamond query (4-cycle)

```sql
-- Find common connections in social network
SELECT a.id, b.id, c.id, d.id
FROM friends f1
JOIN friends f2 ON f1.user2 = f2.user1
JOIN friends f3 ON f2.user2 = f3.user1
JOIN friends f4 ON f3.user2 = f4.user1 AND f4.user2 = f1.user1;

-- Expected: Free join over diamond pattern
```

### Negative: Linear join chain (acyclic)

```sql
-- Simple linear join pattern
SELECT *
FROM orders o
JOIN lineitem l ON o.orderkey = l.orderkey
JOIN customer c ON o.custkey = c.custkey;

-- Expected: Traditional binary join plan
-- Free join not beneficial for acyclic queries
```

## References

**Academic papers:**
- Ngo et al., "Worst-Case Optimal Join Algorithms", PODS 2012
- Veldhuizen, "Triejoin: A Simple, Worst-Case Optimal Join Algorithm", ICDT 2014
- Khamis et al., "FAQ: Questions Asked Frequently", PODS 2016

**Implementation in databases:**
- DuckDB: `src/execution/operator/join/physical_iejoin.cpp`
- LogicBlox: LeapFrog TrieJoin implementation
- EmptyHeaded: Graph pattern matching engine

**Key insights:**
- AGM bound provides tight worst-case complexity: O(N^ρ*)
- ρ* ≤ (number of relations - 1) / 2 for cyclic queries
- Triangle queries: ρ* = 1.5, so O(N^1.5) vs O(N^2) for binary joins
