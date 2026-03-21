# Rule: TiDB Dynamic Programming Join Reordering

**Category:** database-specific/tidb
**File:** `rules/database-specific/tidb/join-reorder-dp.rra`

## Metadata

- **ID:** `tidb-join-reorder-dp`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** join, reordering, dp, cost-based, optimization
- **Authors:** "PingCAP TiDB Team", "RA Contributors"


# TiDB Dynamic Programming Join Reordering

## Description

Uses dynamic programming to find the optimal join order for multi-way joins
by exploring all valid join trees and selecting the one with minimum estimated
cost. Based on the DPccp algorithm (Dynamic Programming with Connected Complement
Pairs), TiDB efficiently searches the space of bushy join trees.

**When to apply**: Queries with 3+ tables that form a connected join graph.
DP is particularly effective for star schemas and queries with multiple join
predicates where greedy algorithms may miss the optimal order.

**Why it works**: The optimal join order can dramatically affect query performance
(orders of magnitude difference). DP guarantees finding the lowest-cost plan by
enumerating all valid join trees using the principle of optimality: the optimal
plan for a set of tables uses optimal subplans for its subsets.

## Relational Algebra

```algebra
Join_n(T1, T2, ..., Tn)
  -> FindOptimalJoinOrder_DP({T1, T2, ..., Tn}, predicates)

DP recurrence (DPccp algorithm):
For each subset S of tables:
  For each connected complement pair (S1, S2) where S = S1 ∪ S2:
    cost(S) = min(cost(S), cost(S1) + cost(S2) + join_cost(S1, S2))

Returns: Optimal join tree with minimum total cost
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("tidb-join-reorder-dp";
    "(join* ?tables)" =>
    "(join_tree_optimal (dp_reorder ?tables))"
    if table_count("?tables") >= 3
    if table_count("?tables") <= 10  // DP feasible for ≤10 tables
    if is_connected_join_graph("?tables")
),

// TiDB DP join reordering (from rule_join_reorder_dp.go)
struct JoinReorderDPSolver {
    cur_join_group: Vec<JRNode>,
    eq_edges: Vec<JoinGroupEqEdge>,
    non_eq_edges: Vec<JoinGroupNonEqEdge>,
    dp_table: HashMap<u32, JRNode>, // Bitmask -> best plan
}

struct JRNode {
    plan: LogicalPlan,
    cum_cost: f64,
}

struct JoinGroupEqEdge {
    node_ids: Vec<usize>,
    edge: EqualityPredicate,
}

impl JoinReorderDPSolver {
    fn solve(&mut self, join_group: Vec<LogicalPlan>) -> Result<LogicalPlan> {
        // Initialize: single-table plans
        for (idx, node) in join_group.iter().enumerate() {
            let cost = self.base_node_cum_cost(node);
            self.dp_table.insert(1 << idx, JRNode {
                plan: node.clone(),
                cum_cost: cost,
            });
        }

        // Build join graph adjacency
        let adjacents = self.build_adjacency(&join_group);

        // DP: enumerate subsets in increasing size
        let n = join_group.len();
        for subset_size in 2..=n {
            self.enumerate_subsets(n, subset_size, &adjacents)?;
        }

        // Return optimal plan for all tables
        let all_tables_mask = (1 << n) - 1;
        Ok(self.dp_table.get(&all_tables_mask).unwrap().plan.clone())
    }

    fn enumerate_subsets(
        &mut self,
        n: usize,
        subset_size: usize,
        adjacents: &[Vec<usize>],
    ) -> Result<()> {
        // Enumerate all subsets of given size
        for subset_mask in subsets_of_size(n, subset_size) {
            let mut best_cost = f64::MAX;
            let mut best_plan = None;

            // Try all connected complement pairs (S1, S2)
            for s1_mask in non_empty_subsets(subset_mask) {
                let s2_mask = subset_mask ^ s1_mask;

                // Check if S1 and S2 are connected
                if !self.is_connected_pair(s1_mask, s2_mask, adjacents) {
                    continue;
                }

                // Get optimal plans for S1 and S2
                let s1_plan = &self.dp_table[&s1_mask];
                let s2_plan = &self.dp_table[&s2_mask];

                // Estimate cost of joining S1 and S2
                let join_cost = self.estimate_join_cost(
                    &s1_plan.plan,
                    &s2_plan.plan,
                    s1_mask,
                    s2_mask,
                );

                let total_cost = s1_plan.cum_cost + s2_plan.cum_cost + join_cost;

                if total_cost < best_cost {
                    best_cost = total_cost;
                    best_plan = Some(self.make_join(
                        s1_plan.plan.clone(),
                        s2_plan.plan.clone(),
                        s1_mask,
                        s2_mask,
                    ));
                }
            }

            if let Some(plan) = best_plan {
                self.dp_table.insert(subset_mask, JRNode {
                    plan,
                    cum_cost: best_cost,
                });
            }
        }

        Ok(())
    }

    fn is_connected_pair(
        &self,
        s1_mask: u32,
        s2_mask: u32,
        adjacents: &[Vec<usize>],
    ) -> bool {
        // Check if there exists an edge between S1 and S2
        for eq_edge in &self.eq_edges {
            let [node1, node2] = &eq_edge.node_ids[..] else {
                continue;
            };
            let mask1 = 1 << node1;
            let mask2 = 1 << node2;

            if (s1_mask & mask1 != 0 && s2_mask & mask2 != 0)
                || (s1_mask & mask2 != 0 && s2_mask & mask1 != 0)
            {
                return true;
            }
        }
        false
    }

    fn estimate_join_cost(
        &self,
        left: &LogicalPlan,
        right: &LogicalPlan,
        left_mask: u32,
        right_mask: u32,
    ) -> f64 {
        // Use cardinality estimates to compute join cost
        let left_card = left.get_row_count();
        let right_card = right.get_row_count();
        let output_card = self.estimate_join_cardinality(left, right, left_mask, right_mask);

        // Hash join cost: build(smaller) + probe(larger)
        let build_card = left_card.min(right_card);
        let probe_card = left_card.max(right_card);

        build_card * 100.0 + probe_card * 50.0 + output_card * 10.0
    }
}

// Helper: enumerate all subsets of given size
fn subsets_of_size(n: usize, k: usize) -> impl Iterator<Item = u32> {
    (0..(1u32 << n)).filter(move |mask| mask.count_ones() == k as u32)
}

fn non_empty_subsets(mask: u32) -> impl Iterator<Item = u32> {
    (1..mask).filter(move |submask| (submask & mask) == *submask)
}
```

**Restrictions:**
- Typically limited to 10-12 tables (3^n complexity)
- Join graph must be connected (no Cartesian products)
- Requires accurate cardinality estimates for cost model
- Falls back to greedy for very large queries

## Cost Model

```rust
fn estimated_benefit(
    n_tables: usize,
    greedy_plan_cost: f64,
) -> f64 {
    // DP optimization time: O(3^n)
    let dp_time_ms = 3_f64.powi(n_tables as i32) * 0.01; // ~0.01ms per state

    // DP finds optimal order, greedy may be suboptimal
    // Typical improvement: 2-10x for badly ordered greedy plans
    let optimal_plan_cost = greedy_plan_cost / 3.0; // Assume 3x improvement

    let total_cost_with_dp = optimal_plan_cost + dp_time_ms;

    if greedy_plan_cost > total_cost_with_dp {
        (greedy_plan_cost - total_cost_with_dp) / greedy_plan_cost
    } else {
        0.0
    }
}
```

**Assumptions:**
- DP optimization time acceptable for ≤10 tables (~20ms for 10 tables)
- Cardinality estimates guide correct cost comparisons
- Optimal join order provides 2-10x improvement over greedy worst case
- DPccp efficiently prunes invalid join trees

**Typical benefit**: 30-95% for queries where join order matters significantly,
especially star schemas with selective dimension filters.

## Test Cases

### Positive: Star schema with 5 tables

```sql
-- TPC-H Q3: Orders-LineItem-Customer with date filters
SELECT l.orderkey, SUM(l.extendedprice * (1 - l.discount)) as revenue
FROM customer c
JOIN orders o ON c.custkey = o.custkey
JOIN lineitem l ON l.orderkey = o.orderkey
WHERE c.mktsegment = 'BUILDING'
  AND o.orderdate < '1995-03-15'
  AND l.shipdate > '1995-03-15'
GROUP BY l.orderkey;

-- Greedy might join: customer ⨝ orders (large), then ⨝ lineitem
-- DP finds: Filter lineitem & orders first, then join with customer
-- Benefit: 5-10x reduction in intermediate result sizes
```

### Positive: Cyclic join pattern

```sql
-- Triangle join: A-B, B-C, A-C edges
SELECT * FROM A, B, C
WHERE A.x = B.x AND B.y = C.y AND A.z = C.z;

-- Greedy: Linear chain (A ⨝ B) ⨝ C
-- DP explores: A ⨝ (B ⨝ C) or (A ⨝ C) ⨝ B
-- Finds optimal based on selectivities
```

### Negative: 2-table join

```sql
SELECT * FROM orders o JOIN lineitem l ON o.orderkey = l.orderkey;

-- DP overhead not justified for 2 tables
-- Direct join execution
```

### Negative: 15 tables (too large for DP)

```sql
SELECT * FROM t1, t2, t3, ..., t15
WHERE <complex join predicates>;

-- DP infeasible: 3^15 = 14M states
-- Falls back to greedy join ordering
```

## References

**Source code:**
- File: `pkg/planner/core/rule_join_reorder_dp.go`
- Struct: `joinReorderDPSolver` (line 27-30)
- Function: `solve()` (line 43-103)
- Repository: https://github.com/pingcap/tidb

**Algorithm:**
- Moerkotte & Neumann, "Dynamic Programming Strikes Back", SIGMOD 2008 (DPccp)
- Selinger et al., "Access Path Selection in a RDBMS", SIGMOD 1979 (Original System R DP)

**TiDB Documentation:**
- Join Reorder: https://docs.pingcap.com/tidb/stable/join-reorder
- Optimizer Hints: https://docs.pingcap.com/tidb/stable/optimizer-hints

**Related:**
- Greedy join ordering (fallback for large queries)
- Cardinality estimation accuracy critical for DP
- Connected complement pairs optimization (DPccp)
