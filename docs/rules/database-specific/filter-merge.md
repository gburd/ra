# Rule: Filter Merge

**Category:** database-specific/calcite
**File:** `rules/database-specific/calcite/filter-merge.rra`

## Metadata

- **ID:** `filter-merge`
- **Version:** "1.0.0"
- **Databases:** calcite
- **Tags:** filter, merge, simplification, logical
- **Authors:** "Apache Calcite Contributors"


# Filter Merge

## Description

Merges two consecutive filter operations into a single filter with a
conjunctive (AND) condition. This reduces the number of operators in the
query plan and eliminates intermediate materialization.

**When to apply**: Two filters appear consecutively in the relational tree
with no intervening operators that would prevent the merge.

**Why it works**: Multiple sequential filters are logically equivalent to a
single filter with all conditions ANDed together. Merging reduces operator
overhead and simplifies the plan for subsequent optimizations.

## Relational Algebra

```algebra
$\sigma$_p1($\sigma$_p2(R)) -> $\sigma$_(p1 $\land$ p2)(R)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("filter-merge";
    "(filter ?cond1 (filter ?cond2 ?input))" =>
    "(filter (and ?cond1 ?cond2) ?input)"
),
```

## Preconditions

```rust
fn applicable(
    _stats: &Statistics,
    _hw: &HardwareProfile,
) -> bool {
    // Always applicable when pattern matches
    true
}
```

**Restrictions:**
- Both filters must be logical filters (not implementing specific physical algorithms)
- No ordering dependencies between the two filters

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    // Eliminate one filter operator overhead
    // Benefit: avoid re-scanning intermediate results
    let rows = stats.row_count as f64;
    let per_row_overhead = 0.000001; // 1 microsecond per row
    let eliminated_overhead = rows * per_row_overhead;

    // Normalize to percentage of query time
    let query_time_estimate = rows * 0.00001; // 10 microseconds per row
    eliminated_overhead / query_time_estimate
}
```

**Assumptions:**
- Merging eliminates pipeline breaking
- Single filter evaluation is faster than two separate evaluations
- No significant increase in condition evaluation complexity

**Typical benefit**: 5-15% reduction in filter overhead for multi-filter predicates.

## Test Cases

### Positive: Merge two consecutive filters

```sql
-- Query with separate range and equality filters
SELECT * FROM orders
WHERE order_date >= '2023-01-01'
  AND order_date < '2024-01-01'
  AND status = 'completed';

-- Original plan (conceptual)
-- Filter(status = 'completed')
--   Filter(order_date >= '2023-01-01' AND order_date < '2024-01-01')
--     Scan(orders)

-- After filter-merge:
-- Filter(order_date >= '2023-01-01' AND order_date < '2024-01-01' AND status = 'completed')
--   Scan(orders)
```

### Positive: Merge filters from subquery flattening

```sql
-- Filters introduced by view expansion
SELECT * FROM (
  SELECT * FROM customers WHERE country = 'USA'
) WHERE age > 21;

-- After view expansion and filter-merge:
-- Filter(country = 'USA' AND age > 21)
--   Scan(customers)
```

### Negative: Cannot merge across join

```sql
-- Filters on different sides of join
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.total > 1000 AND c.status = 'active';

-- Filters stay separate (different inputs)
-- Join
--   Filter(total > 1000)
--     Scan(orders)
--   Filter(status = 'active')
--     Scan(customers)
```

## References

**Implementation in databases:**
- Apache Calcite: `FilterMergeRule.java`
- Apache Drill: Filter merge optimization
- flink: FilterMergeRule in optimizer

**Academic papers:**
- Graefe & McKenna, "The Volcano Optimizer Generator", IEEE Data Engineering 1993
  - Section on filter pushdown and merge optimization
- Selinger et al., "Access Path Selection in a Relational Database", ACM SIGMOD 1979
  - DOI: 10.1145/582095.582099
  - Predicate merging in System R
