# Rule: "System R Left-Deep Join Tree Enumeration"

**Category:** logical/join-reordering
**File:** `rules/logical/join-reordering/system-r-left-deep-enumeration.rra`

## Metadata

- **ID:** `system-r-left-deep-enumeration`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, cockroachdb, mssql, oracle
- **Tags:** left-deep, join-ordering, dynamic-programming, pipeline, system-r, classic
- **Authors:** "Selinger, Astrahan, Chamberlin, Lorie, Price - IBM Research"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(multi-join ?inputs ?predicates)"
    description: "Multi-way join for left-deep enumeration"
  - type: "predicate"
    condition: "count(?inputs) >= 3"
    description: "At least 3 relations for reordering benefit"
  - type: "predicate"
    condition: "all_inner_joins(?inputs)"
    description: "All joins must be inner joins"
  - type: "fact"
    fact_type: "hardware.memory"
    comparator: "<"
    threshold: 1073741824
    optional: true
    description: "Left-deep preferred when memory is limited (<1GB)"
```


# System R Left-Deep Join Tree Enumeration

## Description

The original System R optimizer restricts the join ordering search to left-deep
trees: trees where the right input of every join is a base table (not a join
result). This restriction reduces the search space from all possible binary
trees to only left-deep trees, while still allowing the key optimization of
pipelining: in a left-deep tree with nested-loop joins, intermediate results
are never materialized -- each tuple from the outer pipeline flows directly
into the next join.

Left-deep trees have the form: `(...((R1 join R2) join R3) join R4) join R5`
In contrast, bushy trees allow joins of intermediate results:
`(R1 join R2) join (R3 join R4)`

The restriction to left-deep trees reduces the number of join orderings from
the Catalan number C(n) to n! / 2 (still exponential, but significantly
smaller). Combined with dynamic programming, this becomes O(n * 2^n).

**When to apply**: Multi-way join queries where the optimizer uses the System R
DP algorithm. Left-deep restriction is appropriate when pipelining is important
and memory for intermediate materialization is limited.

**Why it works**: Left-deep trees enable pipelining: each join operator receives
tuples from the previous join one at a time and probes the inner (base) table.
No intermediate results need to be materialized to disk. This is critical when
memory is limited. The O(n * 2^n) search with left-deep restriction is also
more practical than the full search for bushy trees.

## Relational Algebra

```algebra
Left-deep tree for 4 relations:

     join4
    /    \
  join3   R4
  /   \
join2  R3
/   \
R1   R2

vs. Bushy tree:

    join3
   /     \
 join1   join2
 /  \    /  \
R1  R2  R3  R4

System R enumerates left-deep orderings:
  For n=4: Consider all 4! = 24 left-deep orderings
  With DP: Only evaluate O(n * 2^n) = 4 * 16 = 64 subproblems
  (vs. bushy: 64 subsets * 6 partitions each = 384 subproblems)

DP recurrence for left-deep:
  best[{Ri}] = scan_cost(Ri)  for each single relation
  best[S] = min over Ri in S of:
    best[S \ {Ri}] + join_cost(S \ {Ri}, Ri)
  (Right input is always a single relation Ri)
```

## Implementation

```rust
use egg::{rewrite as rw, *};
use std::collections::HashMap;

struct LeftDeepEnumerator {
    memo: HashMap<BitSet, (f64, Plan)>,
}

impl LeftDeepEnumerator {
    fn enumerate(
        &mut self,
        relations: &[Relation],
        join_graph: &JoinGraph,
    ) -> Plan {
        let n = relations.len();

        // Base case: single relations
        for (i, rel) in relations.iter().enumerate() {
            let set = BitSet::singleton(i);
            let cost = self.scan_cost(rel);
            self.memo.insert(set, (cost, Plan::Scan(rel.clone())));
        }

        // DP: build from size 2 to n
        for size in 2..=n {
            for subset in all_subsets_of_size(n, size) {
                self.find_best_left_deep(subset, relations, join_graph);
            }
        }

        let full = BitSet::full(n);
        self.memo.get(&full).unwrap().1.clone()
    }

    fn find_best_left_deep(
        &mut self,
        subset: BitSet,
        relations: &[Relation],
        join_graph: &JoinGraph,
    ) {
        let mut best_cost = f64::INFINITY;
        let mut best_plan = None;

        // LEFT-DEEP RESTRICTION: only consider removing one
        // relation from the right side
        for i in subset.iter() {
            let right = BitSet::singleton(i);
            let left = subset.difference(right);

            // Must have a join predicate between left and right
            if !join_graph.has_edge_between(left, right, relations) {
                continue;
            }

            let (left_cost, left_plan) =
                self.memo.get(&left).unwrap();
            let right_rel = &relations[i];

            // Cost of joining left result with right base table
            let join_cost = self.join_cost(
                left_plan, right_rel, join_graph,
            );
            let total = left_cost + join_cost;

            if total < best_cost {
                best_cost = total;
                best_plan = Some(Plan::Join {
                    left: Box::new(left_plan.clone()),
                    right: Box::new(Plan::Scan(right_rel.clone())),
                    predicate: join_graph.predicate_between(
                        left, right, relations,
                    ),
                });
            }
        }

        if let Some(plan) = best_plan {
            self.memo.insert(subset, (best_cost, plan));
        }
    }

    fn join_cost(
        &self,
        left_plan: &Plan,
        right_rel: &Relation,
        join_graph: &JoinGraph,
    ) -> f64 {
        let left_rows = left_plan.estimated_rows();
        let right_rows = right_rel.row_count as f64;

        let selectivity = join_graph.selectivity(
            left_plan.relations(), &[right_rel.id],
        );

        // Nested-loop with index on right (pipelined)
        let nl_index_cost = if right_rel.has_index_on_join_key() {
            left_rows * (3.0 + selectivity * right_rows / 100.0)
        } else {
            f64::INFINITY
        };

        // Hash join (right as build, left as probe)
        let hash_cost = right_rows * 1.5 + left_rows * 1.0;

        // Sort-merge
        let merge_cost = left_rows * left_rows.log2()
            + right_rows * right_rows.log2()
            + left_rows + right_rows;

        nl_index_cost.min(hash_cost).min(merge_cost)
    }
}
```

## Preconditions


> **Note:** Formal preconditions are defined in the YAML frontmatter above.

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    // Multi-way join
    stats.n_relations >= 3
        // Left-deep preferred when memory is limited
        && (hw.available_memory_mb < 1024
            || stats.n_relations > 15)
        // Pipeline-friendly execution model
        && matches!(hw.execution_model, ExecutionModel::Volcano)
}
```

**Restrictions:**
- Misses potentially optimal bushy trees (important for parallel execution)
- Bushy trees can have lower cost when intermediate results are small
- Left-deep trees have deeper pipeline, increasing latency
- For n > 20 relations, even left-deep DP is impractical (use greedy/genetic)

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let n = stats.n_relations as f64;

    // Space savings vs. bushy enumeration
    let bushy_subproblems = 2.0_f64.powf(n) * n;
    let left_deep_subproblems = 2.0_f64.powf(n);

    // Search space reduction
    let space_reduction = bushy_subproblems / left_deep_subproblems;

    // Quality loss: left-deep may miss optimal bushy plans
    // Typically within 10-20% of bushy optimal
    let quality_factor = 0.85;

    // Net benefit: faster optimization * quality
    (space_reduction * quality_factor).log2() / 10.0
}
```

**Typical benefit**: 2x-5x faster optimization time vs. bushy enumeration,
with plans within 10-20% of bushy optimal for most workloads.

## Test Cases

### Positive: Chain join with pipelining

```sql
-- Chain query: A -> B -> C -> D
SELECT * FROM A
JOIN B ON A.id = B.a_id
JOIN C ON B.id = C.b_id
JOIN D ON C.id = D.c_id;

-- Left-deep: (...((A join B) join C) join D)
-- Fully pipelined: each tuple flows through all joins
-- No intermediate materialization needed
```

### Positive: Star join favors left-deep with fact table as base

```sql
-- Star schema: fact table surrounded by dimensions
SELECT *
FROM lineitem l
JOIN orders o ON l.order_id = o.id
JOIN customer c ON o.cust_id = c.id
JOIN nation n ON c.nation_id = n.id
WHERE n.name = 'FRANCE';

-- Left-deep optimal order (dimension tables first):
-- (((nation join customer) join orders) join lineitem)
-- Filter on nation first (most selective), pipeline through
```

### Negative: Bushy tree would be better

```sql
-- Two independent highly-selective filters
SELECT *
FROM A JOIN B ON A.x = B.x
     JOIN C ON B.y = C.y
     JOIN D ON C.z = D.z
WHERE A.filter1 = true  -- 1% selective
  AND D.filter2 = true; -- 1% selective

-- Left-deep: A(1%) -> B(100%) -> C(100%) -> D(1%)
--   Intermediate sizes: A*B large if B is big

-- Bushy: (A join B) join (C join D)
--   Both sides pre-filtered: much smaller intermediates
--   Left-deep misses this opportunity
```

### Positive: Handles large join counts

```sql
-- 15-way join: left-deep DP is practical
SELECT * FROM t1 JOIN t2 ON ... JOIN t3 ON ... ... JOIN t15 ON ...;

-- Left-deep DP: 15 * 2^15 = 491,520 subproblems (milliseconds)
-- Bushy DP: 2^15 * 15 * many partitions (seconds)
-- Left-deep makes 15-way joins optimizable in real time
```

## References

**Original paper:**
- Selinger, P. Griffiths, et al., "Access Path Selection in a Relational Database Management System", ACM SIGMOD 1979
  - DOI: 10.1145/582095.582099
  - Section 5: Join ordering with left-deep tree restriction
  - "For each set of relations, the optimizer retains the cheapest plan"

**Analysis of left-deep vs. bushy:**
- Ono, K., Lohman, G.M., "Measuring the Complexity of Join Enumeration in Query Optimization", VLDB 1990
  - Analysis of left-deep vs. bushy search space

- Vance, B., Maier, D., "Rapid Bushy Join-Order Optimization with Cartesian Products", ACM SIGMOD 1996
  - DOI: 10.1145/233269.233317
  - Arguments for bushy trees in certain workloads

**Extensions:**
- Moerkotte, G., Neumann, T., "Dynamic Programming Strikes Back", ACM SIGMOD 2008
  - DOI: 10.1145/1376616.1376672
  - DPHyp: efficient bushy tree enumeration

**Implementation in databases:**
- PostgreSQL: Uses bushy trees (GEQO for > 12 relations)
- MySQL: Left-deep only (greedy for > ~10 relations)
- Oracle: Both left-deep and bushy depending on hints
- System R descendants (DB2): Left-deep as default
