# Rule: "Adaptive Index Selection with Runtime Feedback"

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/adaptive-index-selection.rra`

## Metadata

- **ID:** `adaptive-index-selection`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle, mssql, cockroachdb
- **Tags:** index, adaptive, runtime, feedback, statistics, reoptimization
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter ?pred (scan ?table))"
    description: "Filter on table scan with adaptive index choice"
  - type: "fact"
    fact_type: "statistics.workload_history"
    table: "?table"
    comparator: "exists"
    description: "Workload history must be available for adaptation"
  - type: "capability"
    database: "current"
    requires: "adaptive_indexing"
    description: "Database supports adaptive index creation"
```


# Adaptive Index Selection with Runtime Feedback

## Metadata
- **Rule ID**: `adaptive-index-selection`
- **Category**: Physical / Index Selection
- **Complexity**: O(1) decision + execution cost
- **Prerequisites**: Runtime statistics collection; multiple applicable indexes
- **Alternatives**: Static index selection based on optimizer estimates

## Description

Static index selection relies on catalog statistics (histograms,
distinct counts) which may be stale or inaccurate. Adaptive index
selection uses runtime feedback to correct poor index choices:

1. **Query condition caching** (ClickHouse): After executing a query,
   ClickHouse caches which conditions were actually effective at
   pruning granules. Future queries with similar conditions reuse
   this knowledge to pick better indexes.

2. **Adaptive cursor sharing** (Oracle): When the same prepared
   statement has different bind values, Oracle detects skewed
   distributions and re-optimizes with different index choices.

3. **Parametric query optimization**: Pre-compute optimal plans for
   different selectivity ranges and switch at runtime.

4. **Mid-execution reoptimization**: If actual row counts diverge
   significantly from estimates, re-plan remaining operators.

**When to apply:**
- Parameterized queries with varying selectivity
- Stale statistics (tables that change frequently)
- Skewed data distributions where one index is good for some
  values but not others
- Correlated predicates that mislead independence assumptions

## Relational Algebra

```
filter[pred(?param)](scan[T])
  -> if runtime_selectivity(?param) < threshold:
       index-scan[I](pred(?param))
     else:
       seq-scan[T] + filter[pred(?param)]
```

## Implementation (egg rewrite rules)

```lisp
;; Adaptive plan with selectivity switch point
(rewrite (filter (= ?col ?param) (scan ?table))
  (adaptive-scan ?table ?col ?param
    (index-scan (best-index ?table ?col) (= ?col ?param))
    (seq-scan ?table))
  :if (has-applicable-index ?table ?col)
  :if (is-parameterized ?param)
  :if (has-skewed-distribution ?col))

;; Cache query condition effectiveness
(rewrite (filter ?pred (scan ?table))
  (condition-cache-lookup ?table ?pred
    (on-miss
      (filter ?pred (scan ?table))))
  :if (is-mergetree-table ?table)
  :if (enable-condition-cache))

;; Reoptimize when estimate error exceeds threshold
(rewrite (join ?type ?cond ?left ?right)
  (adaptive-join ?type ?cond ?left ?right
    (reoptimize-trigger 10.0))
  :if (may-have-estimate-error ?left ?right))
```

## Cost Model

```rust
pub fn cost_adaptive_index(
    selectivity_distribution: &[(f64, f64)], // (selectivity, probability)
    index_cost_fn: impl Fn(f64) -> Cost,
    seq_scan_cost: Cost,
    switching_overhead: Cost,
) -> Cost {
    let mut total = Cost::zero();
    for &(sel, prob) in selectivity_distribution {
        let index_cost = index_cost_fn(sel);
        let chosen = if index_cost < seq_scan_cost {
            index_cost
        } else {
            seq_scan_cost
        };
        total = total + chosen * prob;
    }
    total + switching_overhead * 0.01
}

pub fn should_reoptimize(
    estimated_rows: u64,
    actual_rows: u64,
    threshold: f64,
) -> bool {
    let ratio = actual_rows as f64 / estimated_rows.max(1) as f64;
    ratio > threshold || ratio < 1.0 / threshold
}
```

**Typical benefit**: 20-80% for queries with varying parameters on skewed data

## Test Cases

### Positive: Parameterized query on skewed column
```sql
-- status has 99% 'active', 1% 'suspended'
PREPARE find_users AS SELECT * FROM users WHERE status = $1;

EXECUTE find_users('suspended');  -- 1%: index scan
EXECUTE find_users('active');     -- 99%: seq scan (adaptive switch)
```

### Positive: Runtime condition cache (ClickHouse)
```sql
-- First execution: tries sparse index on date, finds it prunes 95%
-- Second execution with similar date range: reuses this knowledge
-- Skips indexes that previously had 0% pruning effectiveness
SELECT * FROM events WHERE date = '2024-03-15' AND type = 'click';
```

### Positive: Mid-execution reoptimization
```sql
SELECT * FROM orders o
JOIN customers c ON o.cust_id = c.id
WHERE c.country = 'US';

-- Estimate: 10K US customers
-- Actual: 500K US customers
-- Reoptimize: switch from nested loop to hash join mid-query
```

### Negative: Uniform distribution
```sql
SELECT * FROM events WHERE id = ?;
-- id is uniformly distributed: selectivity always ~1/N
-- Static plan is always correct; adaptive overhead wasted
```

## References

- ClickHouse: `updateQueryConditionCache.cpp` - runtime condition effectiveness
- Oracle: Adaptive Cursor Sharing, Adaptive Plans
- mssql: Adaptive Joins, Interleaved Execution
- PostgreSQL: Custom/generic plan switching for prepared statements
- Ioannidis et al., "Parametric Query Optimization", VLDB 1992
