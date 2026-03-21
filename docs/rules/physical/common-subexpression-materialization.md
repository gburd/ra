# Rule: "Common Subexpression Materialization"

**Category:** physical/materialization
**File:** `rules/physical/materialization/common-subexpression-materialization.rra`

## Metadata

- **ID:** `common-subexpression-materialization`
- **Version:** "1.0.0"
- **Databases:** postgresql, mssql, oracle, clickhouse, spark
- **Tags:** materialization, common-subexpression, cse, spool, multi-consumer
- **Authors:** "RA Contributors"


# Common Subexpression Materialization

## Metadata
- **Rule ID**: `common-subexpression-materialization`
- **Category**: Physical / Materialization
- **Complexity**: O(n) materialization + O(k*n) for k consumers
- **Prerequisites**: Same subquery/expression appears multiple times in plan
- **Alternatives**: Re-compute each occurrence independently

## Description

When the same subquery or expression subtree appears at multiple points
in a query plan, materializing the result once and sharing it across
consumers eliminates redundant computation. This is the query plan
equivalent of common subexpression elimination (CSE) in compilers.

The optimizer detects identical plan subtrees (by structural equality
or hash), materializes the result into a temp table or in-memory
buffer, and replaces each occurrence with a scan of the materialized
result.

mssql uses "Spool" operators (Table Spool, Index Spool) to share
intermediate results. ClickHouse's `useMemoryBufferForCommonSubplanResult`
optimization stores common subplan results in memory. PostgreSQL uses
CTEs with MATERIALIZED hint.

**When to apply:**
- CTE referenced more than once
- Self-join with identical subquery on both sides
- Correlated subquery with shared computation
- subquery_cost * num_references > subquery_cost + materialization_overhead

**When to skip:**
- Single reference (inline is cheaper)
- Very small results (re-computation cheaper than temp table overhead)
- Streaming plan where materialization would break pipeline

## Relational Algebra

```
query(subexpr, subexpr)
  -> let T = materialize(subexpr)
     query(scan(T), scan(T))
```

## Implementation (egg rewrite rules)

```lisp
;; Materialize common subexpression used twice
(rewrite (op ?a (same-expr ?e) (same-expr ?e))
  (let-materialize ?t ?e
    (op ?a (scan-temp ?t) (scan-temp ?t)))
  :if (> (cost ?e) (materialization-overhead)))

;; CTE materialization for multi-reference
(rewrite (with ?name ?subquery ?body)
  (let-materialize ?name ?subquery ?body)
  :if (> (reference-count ?name ?body) 1)
  :if (> (cost ?subquery) 1000))

;; In-memory spool for small results
(rewrite (let-materialize ?t ?expr ?body)
  (let-memory-spool ?t ?expr ?body)
  :if (< (estimated-size ?expr) (available-memory-for-spool)))

;; Disk spool for large results
(rewrite (let-materialize ?t ?expr ?body)
  (let-disk-spool ?t ?expr ?body)
  :if (>= (estimated-size ?expr) (available-memory-for-spool)))
```

## Cost Model

```rust
pub fn cost_materialize_shared(
    subquery_cost: Cost,
    result_rows: u64,
    result_width: u64,
    num_consumers: usize,
    hardware: &HardwareModel,
) -> Cost {
    let compute_once = subquery_cost;
    let write_cost = Cost::io(
        result_rows as f64 * result_width as f64 * hardware.seq_write_cost()
    );
    let read_cost = Cost::io(
        num_consumers as f64 * result_rows as f64
        * result_width as f64 * hardware.seq_read_cost()
    );
    compute_once + write_cost + read_cost
}

pub fn cost_recompute(
    subquery_cost: Cost,
    num_consumers: usize,
) -> Cost {
    subquery_cost * num_consumers as f64
}

pub fn should_materialize(
    subquery_cost: Cost,
    result_rows: u64,
    result_width: u64,
    num_consumers: usize,
    hardware: &HardwareModel,
) -> bool {
    cost_materialize_shared(
        subquery_cost, result_rows, result_width, num_consumers, hardware
    ) < cost_recompute(subquery_cost, num_consumers)
}
```

**Decision rule**: Materialize when `cost_once + k*read < k*cost_compute`

## Test Cases

### Positive: CTE referenced twice
```sql
WITH expensive AS (
    SELECT user_id, sum(amount) as total
    FROM transactions
    GROUP BY user_id
)
SELECT * FROM expensive WHERE total > 1000
UNION ALL
SELECT * FROM expensive WHERE total < 10;

-- expensive computed once (aggregation of 100M rows)
-- Materialized result scanned twice (fast sequential reads)
-- Savings: ~50% (one aggregation instead of two)
```

### Positive: Self-join with identical subquery
```sql
SELECT a.user_id, b.user_id
FROM (SELECT user_id, count(*) as cnt FROM events GROUP BY user_id) a
JOIN (SELECT user_id, count(*) as cnt FROM events GROUP BY user_id) b
  ON a.cnt = b.cnt AND a.user_id \!= b.user_id;

-- Identical subquery appears twice; materialize once
```

### Negative: Single reference CTE
```sql
WITH filtered AS (SELECT * FROM orders WHERE status = 'pending')
SELECT count(*) FROM filtered;

-- Single reference: inline is cheaper (no temp table overhead)
```

### Negative: Tiny result
```sql
WITH constants AS (SELECT 1 as a, 2 as b)
SELECT * FROM t1, constants
UNION ALL SELECT * FROM t2, constants;

-- 1 row: re-computation cheaper than materialization overhead
```

## References

- mssql: Table Spool and Index Spool operators
- ClickHouse: `useMemoryBufferForCommonSubplanResult` optimization
- PostgreSQL: CTE MATERIALIZED/NOT MATERIALIZED hints
- Neumann, "Efficiently Compiling Efficient Query Plans for Modern Hardware"
