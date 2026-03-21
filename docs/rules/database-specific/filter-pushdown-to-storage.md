# Rule: "ClickHouse Filter Pushdown to Storage Layer"

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/filter-pushdown-to-storage.rra`

## Metadata

- **ID:** `clickhouse-filter-pushdown-to-storage`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** filter, pushdown, storage, skip-index, primary-key
- **Authors:** "RA Contributors"


# ClickHouse Filter Pushdown to Storage Layer

## Metadata
- **Rule ID**: `clickhouse-filter-pushdown-to-storage`
- **Category**: Database-specific / ClickHouse
- **Source**: `src/Processors/QueryPlan/Optimizations/filterPushDown.cpp`
- **Complexity**: O(n) with reduced constant factor
- **Prerequisites**: Filter step above a join, aggregation, or expression step
- **Alternatives**: Post-operator filtering

## Description

ClickHouse pushes filter predicates as deep as possible in the query
plan. The filterPushDown optimization moves FilterStep below JoinStep,
AggregatingStep (for predicates on GROUP BY keys), DistinctStep,
SortingStep, UnionStep, and ArrayJoinStep. When filters reach the
storage layer (ReadFromMergeTree), they can leverage the primary key
index and skip indexes.

The optimizer also pushes filters through expressions by composing
the filter DAG with expression DAGs, enabling index analysis even
when column aliases or computed expressions are involved.

**When to apply:**
- Filter above a JOIN (push to one or both sides)
- Filter above aggregation on GROUP BY keys
- Filter above UNION (push to each branch)
- Filter above DISTINCT or SORT

**Why it works for OLAP:**
- Earlier filtering = less data flowing through pipeline
- Reaching storage layer enables index-based pruning
- Reduces memory for intermediate operators

## Relational Algebra

```
filter[pred](join(A, B))
  -> join(filter[pred_A](A), filter[pred_B](B))
     where pred = pred_A AND pred_B

filter[pred](aggregate[groups, aggs](R))
  -> aggregate[groups, aggs](filter[pred](R))
     when pred references only GROUP BY columns
```

## Implementation (egg rewrite rules)

```lisp
;; Push filter below join
(rewrite (filter ?pred (join ?type ?cond ?left ?right))
  (join ?type ?cond
    (filter (extract-left-preds ?pred) ?left)
    (filter (extract-right-preds ?pred) ?right))
  :if (can-split-filter ?pred ?left ?right))

;; Push filter below aggregation (on group-by keys only)
(rewrite (filter ?pred (aggregate ?groups ?aggs ?input))
  (aggregate ?groups ?aggs (filter ?pred ?input))
  :if (references-only ?pred ?groups))

;; Push filter below UNION ALL
(rewrite (filter ?pred (union-all ?inputs))
  (union-all (map ?inputs (lambda (?i) (filter ?pred ?i)))))

;; Push filter below sort (does not affect order)
(rewrite (filter ?pred (sort ?keys ?input))
  (sort ?keys (filter ?pred ?input)))

;; Push filter below distinct
(rewrite (filter ?pred (distinct ?keys ?input))
  (distinct ?keys (filter ?pred ?input)))
```

## Cost Model

```rust
pub fn cost_filter_pushdown(
    rows_before: u64,
    selectivity: f64,
    operator_cost_per_row: f64,
) -> Cost {
    let rows_after = (rows_before as f64 * selectivity) as u64;
    let filter_cost = Cost::cpu(rows_before * 5);
    let operator_savings = Cost::cpu(
        (rows_before - rows_after) as f64 * operator_cost_per_row
    );
    filter_cost - operator_savings
}
```

**Typical benefit**: 30-90% reduction in intermediate data

## Test Cases

### Positive: Filter pushed below join
```sql
SELECT o.*, c.name FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.date > '2024-01-01';

-- Filter on o.date pushed below join to orders scan
-- Only recent orders participate in join
```

### Positive: Filter pushed below aggregation
```sql
SELECT department, sum(salary) FROM employees
GROUP BY department
HAVING department = 'Engineering';

-- HAVING on group key becomes WHERE before aggregation
-- Only Engineering rows aggregated
```

### Negative: Filter on aggregate result
```sql
SELECT department, sum(salary) as total FROM employees
GROUP BY department
HAVING total > 1000000;

-- total is aggregate result; cannot push below aggregation
```

## References

- ClickHouse: `src/Processors/QueryPlan/Optimizations/filterPushDown.cpp`
- ClickHouse: `src/Processors/QueryPlan/Optimizations/Optimizations.h` (tryPushDownFilter)
