# Rule: In-Memory Materialization

**Category:** physical/materialization
**File:** `rules/physical/materialization/in-memory-materialization.rra`

## Metadata

- **ID:** `in-memory-materialization`
- **Version:** "1.0.0"
- **Databases:** duckdb, clickhouse, memsql, voltdb, hyper
- **Tags:** materialization, in-memory, buffer, pipeline-breaker
- **Authors:** "RA Contributors"


# In-Memory Materialization

## Metadata
- **Rule ID**: `in-memory-materialization`
- **Category**: Physical / Materialization
- **Complexity**: O(n) for materialization, O(1) amortized per subsequent access
- **Introduced**: Common in in-memory OLAP systems (DuckDB, HyPer, ClickHouse)
- **Prerequisites**: Intermediate result fits in available memory
- **Alternatives**: eager-materialization (disk-backed), lazy-materialization, result-caching

## Description

In-memory materialization stores an intermediate result entirely in RAM,
avoiding disk I/O for temporary storage. Unlike disk-backed eager
materialization, this strategy keeps the result in memory buffers (often
as columnar vectors or row arrays) for fast repeated access.

This is a pipeline breaker: it consumes all input before producing output.
Used when downstream operators need random access or multiple passes over
an intermediate result, and the result fits in memory.

**When to use:**
- Intermediate result fits in query memory budget
- Multiple downstream consumers need the same result
- Downstream operator requires random access (e.g., hash join build)
- Pipeline-breaking operator between two pipeline segments

**Advantages:**
- Zero disk I/O for temporary storage
- Sub-microsecond per-tuple access after materialization
- Can use columnar layout for cache-friendly downstream processing
- No serialization/deserialization overhead

**Disadvantages:**
- Consumes query memory budget
- Pipeline breaker (latency to first output = full input consumption)
- Must spill to disk if memory exceeded (fallback to temp table)
- Memory pressure on concurrent queries

## Relational Algebra

```
Let T = expensive_subquery(R)
... multiple references to T ...

-> InMemoryMaterialize(T):
     buffer = allocate(estimated_size)
     for each tuple in T:
       buffer.append(tuple)
     return MaterializedRef(buffer)

Subsequent accesses: scan buffer directly (no recomputation)
```

## Implementation (egg rewrite rules)

```lisp
;; Materialize in memory when result is small and accessed multiple times
(rewrite (with ?name ?subquery ?body)
  (let ?name (in-memory-materialize ?subquery)
    (substitute ?name ?body))
  :if (< (estimated-size-bytes ?subquery) (available-memory))
  :if (> (reference-count ?name ?body) 1))

;; Prefer in-memory over disk-backed when result fits
(rewrite (materialize-to-disk ?subquery)
  (in-memory-materialize ?subquery)
  :if (< (estimated-size-bytes ?subquery) (available-memory)))

;; Pipeline breaker: materialize between pipeline segments
(rewrite (pipeline-break ?input ?consumer)
  (let ?buf (in-memory-materialize ?input)
    (?consumer ?buf))
  :if (requires-full-input ?consumer)
  :if (< (estimated-size-bytes ?input) (available-memory)))

;; Fall back to disk when memory insufficient
(rewrite (in-memory-materialize ?subquery)
  (materialize-to-disk ?subquery)
  :if (> (estimated-size-bytes ?subquery) (available-memory)))
```

## Implementation Pattern

```rust
pub struct InMemoryMaterialization {
    input: Box<dyn Operator>,
    buffer: Vec<Tuple>,
    materialized: bool,
    scan_pos: usize,
}

impl Operator for InMemoryMaterialization {
    fn next(&mut self) -> Option<Tuple> {
        if !self.materialized {
            // Consume all input into memory buffer
            while let Some(tuple) = self.input.next() {
                self.buffer.push(tuple);
            }
            self.materialized = true;
            self.scan_pos = 0;
        }

        if self.scan_pos < self.buffer.len() {
            let tuple = self.buffer[self.scan_pos].clone();
            self.scan_pos += 1;
            Some(tuple)
        } else {
            None
        }
    }

    fn reset(&mut self) {
        // Re-scan without recomputation
        self.scan_pos = 0;
    }

    fn random_access(&self, index: usize) -> Option<&Tuple> {
        self.buffer.get(index)
    }
}

/// Columnar variant for vectorized engines
pub struct ColumnarMaterialization {
    columns: Vec<ColumnVector>,
    num_rows: usize,
}

impl ColumnarMaterialization {
    fn scan_column(&self, col_idx: usize) -> &ColumnVector {
        &self.columns[col_idx]
    }

    fn memory_usage(&self) -> usize {
        self.columns.iter().map(|c| c.size_bytes()).sum()
    }
}
```

## Cost Model

```rust
pub fn cost_in_memory_materialize(
    input_card: u64,
    tuple_width: usize,
    num_accesses: usize,
    hardware: &HardwareModel,
) -> Cost {
    let total_bytes = input_card * tuple_width as u64;

    // Materialization: consume input into buffer
    let materialize_cost = Cost::cpu(input_card * 5); // Copy per tuple
    let memory_alloc = Cost::memory(total_bytes);

    // Each subsequent access: sequential scan of buffer
    let per_access = Cost::cpu(input_card * 2); // Cache-friendly scan
    let access_cost = per_access * (num_accesses as f64);

    materialize_cost + memory_alloc + access_cost
}

pub fn should_materialize_in_memory(
    estimated_bytes: u64,
    num_accesses: usize,
    recompute_cost: f64,
    available_memory: u64,
) -> bool {
    if estimated_bytes > available_memory {
        return false;
    }

    let materialize_and_scan = estimated_bytes as f64 * 0.01
        + num_accesses as f64 * estimated_bytes as f64 * 0.005;
    let recompute_each_time = recompute_cost * num_accesses as f64;

    materialize_and_scan < recompute_each_time
}
```

## Test Cases

### Test 1: CTE referenced multiple times
```sql
WITH active AS (
    SELECT user_id, COUNT(*) as actions
    FROM activity_log
    WHERE timestamp > NOW() - INTERVAL '7 days'
    GROUP BY user_id
)
SELECT
    (SELECT AVG(actions) FROM active),
    (SELECT MAX(actions) FROM active),
    (SELECT COUNT(*) FROM active WHERE actions > 10);

-- Expected: InMemoryMaterialize the CTE result
-- Result: ~100K rows, ~1.6MB -- fits easily in memory
-- Three subsequent scans read from buffer, not recomputed
```

### Test 2: Hash join build side materialization
```sql
SELECT o.*, c.name
FROM orders o
JOIN customers c ON o.customer_id = c.id;

-- Expected: InMemoryMaterialize customers (build side)
-- 50K customers, ~5MB in memory
-- Hash table built from materialized buffer
```

### Test 3: Subquery in correlated context
```sql
SELECT *
FROM products p
WHERE p.price > (
    SELECT AVG(price) FROM products WHERE category = p.category
);

-- Expected: InMemoryMaterialize category averages
-- Decorrelate + materialize per-category averages
-- Random access by category during probe
```

### Test 4: Negative -- result too large for memory
```sql
WITH all_events AS (
    SELECT * FROM event_log  -- 500M rows, 100GB
)
SELECT * FROM all_events WHERE event_type = 'error';

-- NOT suitable: result far exceeds memory
-- Use disk-backed materialization or pipeline without materializing
```

### Test 5: Negative -- single access
```sql
WITH filtered AS (
    SELECT * FROM orders WHERE status = 'pending'
)
SELECT COUNT(*) FROM filtered;

-- NOT suitable: single access, no reuse benefit
-- Pipeline directly without materializing
```

## Performance Characteristics

| Metric | In-Memory | Disk-Backed | Recompute |
|--------|-----------|-------------|-----------|
| First access latency | Input cost | Input + write | Input cost |
| Subsequent access | ~0 (buffer scan) | Disk read | Full recompute |
| Memory usage | Full result | Minimal | None |
| 3 accesses, 10MB result | ~30ms | ~300ms | ~3x input cost |
| Concurrent query impact | High memory | Low memory | High CPU |

## References

1. **DuckDB**: In-memory intermediate materialization in vectorized engine
   - https://duckdb.org/internals/storage

2. **HyPer**: "Efficiently Compiling Efficient Query Plans for Modern Hardware"
   - Neumann, VLDB 2011 -- pipeline breakers and materialization points

3. **MonetDB/X100**: Columnar in-memory materialization
   - Boncz et al., "MonetDB/X100: Hyper-Pipelining Query Execution"

4. **PostgreSQL**: CTE materialization strategy selection
   - https://www.postgresql.org/docs/current/queries-with.html
