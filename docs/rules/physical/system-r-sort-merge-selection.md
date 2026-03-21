# Rule: "System R Sort-Merge Join Selection"

**Category:** physical/access-path-selection
**File:** `rules/physical/access-path-selection/system-r-sort-merge-selection.rra`

## Metadata

- **ID:** `system-r-sort-merge-selection`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, cockroachdb, mssql, oracle
- **Tags:** sort-merge, join-selection, system-r, cost-based, merge-join, classic
- **Authors:** "Selinger, Astrahan, Chamberlin, Lorie, Price - IBM Research"


# System R Sort-Merge Join Selection

## Description

System R's algorithm for selecting sort-merge join as the physical join
method. Sort-merge join was one of only two join methods in the original
System R optimizer (along with nested-loop). It sorts both inputs on the
join key, then merges them in a single pass. The key advantage is that
sort-merge produces sorted output, which can be an "interesting order"
for downstream operations (GROUP BY, ORDER BY, subsequent merge joins).

The System R optimizer evaluates sort-merge join by computing:
- Sort cost for each input (if not already sorted)
- Merge cost (single pass over both sorted inputs)
- Value of the output ordering (interesting orders concept)

Sort-merge is preferred when:
1. One or both inputs are already sorted (index scan)
2. The output ordering is useful for downstream operations
3. Memory is insufficient for hash join's build phase

**When to apply**: Equijoins where at least one input is already sorted,
or where the sorted output order is needed later.

**Why it works**: Sorting each input is O(n log n) and the merge is O(n + m).
When inputs are already sorted (from index scans or prior sorts), the sort
cost is zero, making merge join O(n + m) -- the same as hash join but with
the bonus of producing sorted output.

## Relational Algebra

```algebra
Sort-Merge Join cost:

sort_cost(R) = if R already sorted on join key: 0
               else: 2 * N_pages(R) * ceil(log_B(N_pages(R)))
               (B = available buffer pages)

merge_cost = N_pages(R) + N_pages(S)
             (single pass over both inputs)

total_cost = sort_cost(R) + sort_cost(S) + merge_cost

Output: sorted on join key (interesting order!)

Compare with:
- Hash join: N_pages(build) * 1.5 + N_pages(probe) (no output order)
- NL with index: N_tuples(outer) * (index_depth + matches)

Choose sort-merge when:
  total_cost < hash_join_cost (direct cost comparison)
  OR total_cost + downstream_sort_savings < hash_join_cost + downstream_sort_cost
  (accounting for interesting orders)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

struct SortMergeJoinSelector;

impl SortMergeJoinSelector {
    fn cost_sort_merge(
        &self,
        left: &PlanNode,
        right: &PlanNode,
        join_key: &JoinKey,
        available_memory_pages: usize,
    ) -> SortMergeEstimate {
        // Sort cost for left input
        let left_sort = if left.is_sorted_on(&join_key.left_col) {
            0.0
        } else {
            self.external_sort_cost(
                left.est_pages(),
                available_memory_pages,
            )
        };

        // Sort cost for right input
        let right_sort = if right.is_sorted_on(&join_key.right_col) {
            0.0
        } else {
            self.external_sort_cost(
                right.est_pages(),
                available_memory_pages,
            )
        };

        // Merge cost: single sequential pass
        let merge_cost =
            left.est_pages() as f64 + right.est_pages() as f64;

        // CPU cost for merge (comparison per output tuple)
        let cpu_cost = 0.05 * (
            left.est_tuples() as f64 + right.est_tuples() as f64
        );

        SortMergeEstimate {
            sort_cost_left: left_sort,
            sort_cost_right: right_sort,
            merge_cost,
            cpu_cost,
            total: left_sort + right_sort + merge_cost + cpu_cost,
            output_sorted: true,
            output_order: join_key.left_col.clone(),
        }
    }

    fn external_sort_cost(
        &self,
        input_pages: usize,
        buffer_pages: usize,
    ) -> f64 {
        let n = input_pages as f64;
        let b = buffer_pages as f64;

        if n <= b {
            // In-memory sort: read + write
            2.0 * n
        } else {
            // External sort: multiple merge passes
            let num_runs = (n / b).ceil();
            let num_passes = (num_runs.log2() / b.log2()).ceil() + 1.0;
            2.0 * n * num_passes
        }
    }

    fn should_prefer_sort_merge(
        &self,
        sm_cost: &SortMergeEstimate,
        hash_cost: f64,
        downstream_needs_sort: bool,
        sort_on_join_key: bool,
    ) -> bool {
        if downstream_needs_sort && sort_on_join_key {
            // Sort-merge provides free output ordering
            // Hash join would need additional sort
            let hash_plus_sort = hash_cost
                + sm_cost.merge_cost * 2.0; // Estimate sort cost
            sm_cost.total < hash_plus_sort
        } else {
            sm_cost.total < hash_cost
        }
    }
}

// Egg rewrite for choosing sort-merge join
rw!("impl-join-sort-merge";
    "(join inner (= ?left_key ?right_key) ?left ?right)" =>
    "(merge-join (= ?left_key ?right_key)
       (ensure-sorted ?left_key ?left)
       (ensure-sorted ?right_key ?right))"
    if is_equijoin_key("?left_key", "?right_key")
),

rw!("sort-merge-avoids-sort";
    "(sort ?key
       (merge-join (= ?key ?right_key) ?left ?right))" =>
    "(merge-join (= ?key ?right_key) ?left ?right)"
    // Sort-merge output is already sorted on join key
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    // Must be an equijoin
    stats.is_equijoin
        // Sort-merge beneficial when:
        && (
            // At least one input already sorted
            stats.left_input_sorted_on_join_key
            || stats.right_input_sorted_on_join_key
            // OR downstream needs sorted output
            || stats.downstream_needs_sort_on_join_key
            // OR memory insufficient for hash join
            || stats.smaller_input_pages > hw.hash_join_memory_pages
        )
}
```

**Restrictions:**
- Only for equijoins (exact equality on join key)
- External sort cost depends on available memory (buffer pages)
- Non-equijoin predicates handled as post-merge filters
- Duplicate handling: merge must handle multiple matches correctly
- Not beneficial when both inputs need sorting and no downstream order needed

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> f64 {
    let left_pages = stats.left_pages as f64;
    let right_pages = stats.right_pages as f64;

    // Hash join cost (baseline)
    let smaller = left_pages.min(right_pages);
    let larger = left_pages.max(right_pages);
    let hash_cost = smaller * 1.5 + larger;

    // Sort-merge cost
    let left_sort = if stats.left_sorted {
        0.0
    } else {
        2.0 * left_pages * (left_pages.log2() / (hw.buffer_pages as f64).log2()).ceil()
    };
    let right_sort = if stats.right_sorted {
        0.0
    } else {
        2.0 * right_pages * (right_pages.log2() / (hw.buffer_pages as f64).log2()).ceil()
    };
    let merge_cost = left_pages + right_pages;
    let sm_cost = left_sort + right_sort + merge_cost;

    // Account for interesting order benefit
    let order_savings = if stats.downstream_needs_sort {
        2.0 * (left_pages + right_pages) // Avoided sort cost
    } else {
        0.0
    };

    let effective_sm_cost = sm_cost - order_savings;

    if hash_cost > effective_sm_cost {
        (hash_cost - effective_sm_cost) / hash_cost
    } else {
        0.0
    }
}
```

**Typical benefit**: 2x-10x when inputs are pre-sorted or output order is needed.

## Test Cases

### Positive: Both inputs pre-sorted (index scans)

```sql
-- Clustered index on orders(customer_id)
-- Clustered index on customers(id)
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id;

-- Sort-merge: 0 (sorted) + 0 (sorted) + merge(100K + 10K) = 110K
-- Hash join: build(10K * 1.5) + probe(100K) = 115K
-- Sort-merge slightly cheaper AND provides sorted output
```

### Positive: Sort-merge avoids downstream sort

```sql
SELECT * FROM orders o
JOIN lineitem l ON o.id = l.order_id
ORDER BY o.id;

-- Hash join: build + probe = 500K, then sort output = 200K. Total: 700K
-- Sort-merge: sort(orders) + sort(lineitem) + merge = 600K
--   Output already sorted by o.id -> no additional sort needed!
-- Sort-merge wins due to interesting order savings
```

### Positive: Memory-constrained environment

```sql
-- Available memory: 100 pages
-- Build side: 10,000 pages (won't fit in hash table)
SELECT * FROM big_table1 b1
JOIN big_table2 b2 ON b1.key = b2.key;

-- Hash join: must spill to disk (Grace hash join) = expensive
-- Sort-merge: external sort is designed for disk-based operation
-- Sort-merge preferred when memory is limited
```

### Negative: Small join where hash join dominates

```sql
-- Small tables, no order needed downstream
SELECT * FROM departments d
JOIN locations l ON d.location_id = l.id;

-- departments: 50 rows, locations: 100 rows
-- Hash join: build(50 * 1.5) + probe(100) = 175 comparisons
-- Sort-merge: sort(50) + sort(100) + merge(150) = ~1000 comparisons
-- Hash join much cheaper for small inputs
```

## References

**Original paper:**
- Selinger, P. Griffiths, et al., "Access Path Selection in a Relational Database Management System", ACM SIGMOD 1979
  - DOI: 10.1145/582095.582099
  - Section 5.2: "Sort-merge join" method
  - Cost formula for sort-merge including pre-sorted inputs
  - Interaction with interesting sort orders

**Sort algorithms:**
- Knuth, D.E., "The Art of Computer Programming, Volume 3: Sorting and Searching", 1973
  - External merge sort algorithm used in database systems

**Modern analysis:**
- Graefe, G., "Implementing Sorting in Database Systems", ACM Computing Surveys 2006
  - DOI: 10.1145/1132960.1132964
  - Comprehensive treatment of sorting in database context

**Implementation in databases:**
- PostgreSQL: `src/backend/executor/nodeMergejoin.c` - merge join execution
- PostgreSQL: `src/backend/optimizer/path/joinpath.c` - sort_inner_and_outer()
- DuckDB: Sort-merge join implementation
- All major databases support sort-merge join selection
