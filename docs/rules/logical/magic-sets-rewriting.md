# Rule: Magic Sets Rewriting

**Category:** logical/subquery-unnesting
**File:** `rules/logical/subquery-unnesting/magic-sets-rewriting.rra`

## Metadata

- **ID:** `magic-sets-rewriting`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle, monetdb
- **Tags:** magic-sets, recursion, datalog, sideways-information-passing, classic
- **Authors:** "Bancilhon, Maier, Sagiv, Ullman"


# Magic Sets Rewriting

## Description

Transforms recursive or stratified queries by "pushing" bindings from the query
down into the recursive rules, effectively computing only the relevant portion
of the recursive relation. This is the database equivalent of tabling/memoization
in logic programming, propagating constants and filters to prune the search space.

**When to apply**: Recursive queries (WITH RECURSIVE), transitive closure
computations, or queries where early binding information can dramatically
reduce the search space. Particularly effective for graph traversal where
starting points are known.

**Why it works**: Naive recursive evaluation computes the entire recursive
relation, then filters. Magic sets rewrites the query to only compute tuples
that are "reachable" from the query's constants, using sideways information
passing to propagate bindings backward through the rules.

## Relational Algebra

```algebra
Given query: Q(x) ← R(x, y), T*(y, z), z = 'target'
And recursive rule: T*(x, y) ← T(x, y)
                    T*(x, z) ← T(x, y), T*(y, z)

Magic sets rewrite:
magic_T(z) ← z = 'target'
magic_T(x) ← magic_T(y), T(x, y)
T_magic(x, y) ← magic_T(x), T(x, y)
T_magic(x, z) ← magic_T(x), T_magic(x, y), T_magic(y, z)

Q(x) ← R(x, y), T_magic(y, z), z = 'target'
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Magic sets transformation is complex - here's a simplified version
// Full implementation requires adorned predicate generation

rw!("magic-sets-base";
    // For each recursive predicate P with selection σ_c
    "(recursive ?name
       (rules ?base-case ?recursive-case)
       (query (filter ?const (apply ?name ?args))))" =>
    "(let
       ((magic-pred (adorn ?name ?const))
        (magic-base (derive-magic-base ?const))
        (magic-rec (derive-magic-recursive ?name ?const)))
       (recursive (concat magic- ?name)
         (rules ?magic-base ?magic-rec)
         (query (join
                  (filter ?const (apply magic- ?name ?args))
                  (apply ?name ?args)))))"
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    // Must have recursive query or transitive closure
    stats.has_recursion || stats.has_transitive_closure
        // Must have selective predicates to propagate
        && stats.has_selective_constants_in_query
        // Selectivity should be high (< 10% of full relation)
        && stats.magic_sets_selectivity < 0.1
        // Support for WITH RECURSIVE or equivalent
        && hw.supports_recursive_queries
}
```

**Restrictions:**
- Requires stratifiable recursion (no negation in recursive rules)
- Most effective with selective constants in query
- Original Datalog formulation; SQL WITH RECURSIVE is similar but not identical
- Adorned predicates must be computed correctly for sideways information passing

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    // Naive recursive evaluation computes full transitive closure
    let base_relation_size = stats.base_relation_cardinality as f64;
    let closure_size = stats.estimated_transitive_closure_size as f64;

    // Cost of naive evaluation:
    // - Compute full closure: closure_size * log(closure_size)
    // - Then filter: closure_size
    let naive_cost = closure_size * closure_size.log2() + closure_size;

    // Cost with magic sets:
    // - Only compute relevant portion based on query constants
    let relevant_size = closure_size * stats.magic_sets_selectivity;
    let magic_cost = relevant_size * relevant_size.log2();

    // Benefit calculation
    if naive_cost > magic_cost {
        (naive_cost - magic_cost) / naive_cost
    } else {
        0.0
    }
}
```

**Assumptions:**
- Transitive closure size can be much larger than base relation (O(n²) in worst case)
- Magic sets limits computation to "relevant" tuples (those reachable from constants)
- Sideways information passing overhead is negligible compared to savings
- Most effective when selectivity < 10%

**Typical benefit**: 5x-100x for graph queries with known starting points.

## Test Cases

### Positive: Reachability from specific node

```sql
-- Find all nodes reachable from node 42 in graph
WITH RECURSIVE reachable(node) AS (
  SELECT 42
  UNION
  SELECT dst
  FROM edges e, reachable r
  WHERE e.src = r.node
)
SELECT * FROM reachable;

-- Without magic sets: computes full transitive closure, then selects node 42's descendants
-- With magic sets: only explores paths starting from node 42
-- If graph has 1M nodes but node 42 reaches only 1K nodes: 1000x speedup
```

### Positive: Bill of materials (BOM) explosion

```sql
-- Find all components needed for part 'ENGINE-V8'
WITH RECURSIVE components(part_id, component_id, quantity) AS (
  SELECT part_id, component_id, quantity
  FROM bill_of_materials
  WHERE part_id = 'ENGINE-V8'

  UNION ALL

  SELECT bom.part_id, bom.component_id, c.quantity * bom.quantity
  FROM components c
  JOIN bill_of_materials bom ON c.component_id = bom.part_id
)
SELECT component_id, SUM(quantity) AS total_quantity
FROM components
GROUP BY component_id;

-- Magic sets: only explodes BOM for ENGINE-V8, not entire catalog
```

### Positive: Transitive closure with two bound endpoints

```sql
-- Check if there's a path from 'NYC' to 'LAX'
WITH RECURSIVE paths(src, dst, hops) AS (
  SELECT src, dst, 1
  FROM flights
  WHERE src = 'NYC'

  UNION

  SELECT p.src, f.dst, p.hops + 1
  FROM paths p
  JOIN flights f ON p.dst = f.src
  WHERE f.dst = 'LAX' AND p.hops < 5
)
SELECT MIN(hops) FROM paths WHERE dst = 'LAX';

-- Magic sets: propagate both 'NYC' (forward) and 'LAX' (backward)
-- Only explore paths that could connect the two cities
```

### Negative: Recursive query without selective constants

```sql
-- Compute full transitive closure (no selective constants)
WITH RECURSIVE closure(src, dst) AS (
  SELECT src, dst FROM edges
  UNION
  SELECT c1.src, c2.dst
  FROM closure c1
  JOIN closure c2 ON c1.dst = c2.src
)
SELECT * FROM closure;

-- No magic sets benefit: must compute full closure anyway
```

### Positive: Organization hierarchy

```sql
-- Find all reports under CEO (employee_id = 1)
WITH RECURSIVE org_chart(employee_id, manager_id, level) AS (
  SELECT employee_id, manager_id, 0
  FROM employees
  WHERE employee_id = 1

  UNION ALL

  SELECT e.employee_id, e.manager_id, o.level + 1
  FROM employees e
  JOIN org_chart o ON e.manager_id = o.employee_id
)
SELECT * FROM org_chart;

-- Magic sets: only traverse downward from CEO, not entire org
```

## References

**Original papers:**
- Bancilhon, F., Maier, D., Sagiv, Y., Ullman, J.D., "Magic Sets and Other Strange Ways to Implement Logic Programs", ACM PODS 1986
  - DOI: 10.1145/6012.15399
  - THE foundational paper introducing magic sets transformation
  - Adorned predicates, sideways information passing

- Beeri, C., Ramakrishnan, R., "On the Power of Magic", Journal of Logic Programming 1991
  - DOI: 10.1016/0743-1066(91)90038-Q
  - Theoretical analysis and extensions

**Modern implementations:**
- Mumick, I.S., Finkelstein, S.J., Pirahesh, H., Ramakrishnan, R., "Magic is Relevant", ACM SIGMOD 1990
  - DOI: 10.1145/93597.98738
  - Magic sets for SQL, implementation in Starburst

- Shkapsky, A., Yang, M., Interlandi, M., et al., "Big Data Analytics with Datalog Queries on Spark", ACM SIGMOD 2016
  - DOI: 10.1145/2882903.2915229
  - Modern magic sets in BigDatalog/Spark

**Implementation in databases:**
- PostgreSQL: Recursive CTE optimization (not full magic sets, but similar optimizations)
- Oracle: `CONNECT BY` with `START WITH` clause (similar to magic sets)
- MonetDB: Datalog with magic sets (historical)
- LogicBlox: Full magic sets implementation for Datalog
