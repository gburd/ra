# Rule: Volcano Iterator Model - Tuple-at-a-Time Materialization

**Category:** execution-models
**File:** `rules/execution-models/volcano/volcano-tuple-materialization.rra`

## Metadata

- **ID:** `volcano-tuple-materialization`
- **Version:** 1.0.0
- **Databases:** postgresql, mysql, oracle, sqlite, mssql
- **Tags:** execution, iterator, volcano, materialization, tuple, memory
- **SQL Standard:** Volcano model
- **Authors:** Goetz Graefe


# Volcano Iterator Model - Tuple-at-a-Time Materialization

## Description

In the Volcano model, materialization occurs when an operator must
buffer its entire input before producing any output. This creates a
synchronization boundary in the execution pipeline. Understanding
when materialization is required, what it costs, and how to minimize
its impact is critical for query performance.

**When to apply:** Every query plan must account for materialization
points. The optimizer should minimize the volume of data materialized
by pushing filters and projections below materializing operators.

**Why it matters:** Materialization converts a streaming O(1)-memory
pipeline into an O(N)-memory operation. It introduces latency (no
output until all input is consumed), memory pressure (buffered tuples
may spill to disk), and synchronization points (parallel pipelines
must wait at barriers).

**Materialization categories:**
- **Full materialization**: Sort, Hash Aggregate, Hash Join (build).
  Must consume all input before producing first output.
- **Partial materialization**: Window functions buffer within
  partitions. Memory bounded by partition size, not total input.
- **Spool/cache materialization**: CTEs, correlated subquery results
  cached to avoid recomputation.
- **No materialization**: Filter, Project, Nested Loop Join (probe),
  Limit, Union All. Fully pipelined.

## Relational Algebra

```
Materialization taxonomy:

Pipelined operators (no materialization):
  Filter(R, p)       — evaluates p per tuple, passes or skips
  Project(R, cols)    — transforms tuple, passes immediately
  NLJ_probe(R, S)    — for each outer tuple, scans inner
  Limit(R, n)         — passes first n tuples, then stops
  UnionAll(R, S)      — concatenates streams

Full materializers (buffer all input):
  Sort(R, key)        — must see all tuples to determine order
  HashAgg(R, g, agg)  — must see all groups before output
  HashJoin_build(S)   — must build complete hash table
  Distinct(R)         — must see all tuples (hash or sort)

Partial materializers (bounded buffering):
  WindowFunc(R, partition, frame)
    — buffers one partition at a time
    — memory = max(partition_size) not sum(all)

  MergeJoin(R, S)
    — buffers duplicate keys only
    — memory = max(duplicate_run) not N

Spool materializers (cache for reuse):
  CTE_spool(query)
    — materializes once, reads many times
    — avoids recomputation

Pipeline structure:
  A query plan is split into pipeline segments at
  materialization boundaries.

  Example: SELECT * FROM R JOIN S ON ... ORDER BY ...

  Pipeline 1: Scan(R) → Filter → [HashJoin build]  ← materializes
  Pipeline 2: Scan(S) → [HashJoin probe] → Project  ← pipelined
  Pipeline 3: [Sort input] → [Sort output]           ← materializes
              ↓
  Pipeline 4: Sort output → Limit → Result           ← pipelined
```

## Implementation

```rust
/// Identifies materialization points in a query plan and
/// estimates their memory and latency impact.

/// Classification of operator materialization behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterializationKind {
    /// No buffering; tuple-in, tuple-out.
    Pipelined,
    /// Must consume all input before producing output.
    FullMaterialization,
    /// Buffers bounded subsets (partitions, duplicate runs).
    PartialMaterialization,
    /// Caches results for repeated access.
    SpoolMaterialization,
}

/// A pipeline segment: a maximal chain of pipelined operators
/// between two materialization points.
#[derive(Debug)]
pub struct PipelineSegment {
    /// Operators in this segment (root to leaf order).
    pub operators: Vec<String>,
    /// Estimated tuples flowing through this segment.
    pub estimated_rows: f64,
    /// Estimated tuple width in bytes.
    pub tuple_width: usize,
}

/// Analyze a plan to identify materialization points.
pub fn classify_materialization(
    plan: &RelExpr,
) -> MaterializationKind {
    match plan {
        // Pipelined operators
        RelExpr::Scan { .. }
        | RelExpr::Filter { .. }
        | RelExpr::Project { .. }
        | RelExpr::Limit { .. } => MaterializationKind::Pipelined,

        // Full materializers
        RelExpr::Sort { .. } => {
            MaterializationKind::FullMaterialization
        }
        RelExpr::Aggregate { .. } => {
            MaterializationKind::FullMaterialization
        }
        RelExpr::Join {
            join_type: JoinType::Hash,
            ..
        } => {
            // Build side is full materialization
            MaterializationKind::FullMaterialization
        }

        // Partial materializers
        RelExpr::Window { .. } => {
            MaterializationKind::PartialMaterialization
        }

        _ => MaterializationKind::Pipelined,
    }
}

/// Estimate memory required for materialization at a given node.
pub fn estimate_materialization_memory(
    plan: &RelExpr,
    stats: &TableStats,
) -> MemoryEstimate {
    let kind = classify_materialization(plan);
    let row_count = stats.estimated_rows;
    let row_width = stats.avg_row_width;

    match kind {
        MaterializationKind::Pipelined => MemoryEstimate {
            bytes: row_width, // single tuple buffer
            spill_possible: false,
            kind,
        },
        MaterializationKind::FullMaterialization => {
            let bytes = (row_count as usize) * row_width;
            MemoryEstimate {
                bytes,
                spill_possible: bytes > WORK_MEM_LIMIT,
                kind,
            }
        }
        MaterializationKind::PartialMaterialization => {
            // Bounded by largest partition, not total input
            let partition_fraction =
                stats.distinct_values as f64
                    / row_count.max(1.0);
            let avg_partition_size =
                (row_count * partition_fraction) as usize
                    * row_width;
            MemoryEstimate {
                bytes: avg_partition_size,
                spill_possible: avg_partition_size
                    > WORK_MEM_LIMIT,
                kind,
            }
        }
        MaterializationKind::SpoolMaterialization => {
            let bytes = (row_count as usize) * row_width;
            MemoryEstimate {
                bytes,
                spill_possible: bytes > WORK_MEM_LIMIT,
                kind,
            }
        }
    }
}

/// Split a plan into pipeline segments at materialization
/// boundaries.
pub fn identify_pipelines(
    plan: &RelExpr,
) -> Vec<PipelineSegment> {
    let mut segments = Vec::new();
    let mut current_ops = Vec::new();

    fn walk(
        node: &RelExpr,
        current: &mut Vec<String>,
        segments: &mut Vec<PipelineSegment>,
    ) {
        let kind = classify_materialization(node);
        current.push(node.operator_name().to_string());

        if kind != MaterializationKind::Pipelined {
            // End current pipeline, start new one
            if !current.is_empty() {
                segments.push(PipelineSegment {
                    operators: current.drain(..).collect(),
                    estimated_rows: node.estimated_rows(),
                    tuple_width: node.tuple_width(),
                });
            }
        }

        for child in node.children() {
            walk(child, current, segments);
        }
    }

    walk(plan, &mut current_ops, &mut segments);

    if !current_ops.is_empty() {
        segments.push(PipelineSegment {
            operators: current_ops,
            estimated_rows: 0.0,
            tuple_width: 0,
        });
    }

    segments
}

const WORK_MEM_LIMIT: usize = 256 * 1024 * 1024; // 256 MB

pub struct MemoryEstimate {
    pub bytes: usize,
    pub spill_possible: bool,
    pub kind: MaterializationKind,
}
```

## Preconditions

- Query plan has been logically optimized
- Table statistics available for cardinality estimates
- Work memory limit configured (e.g., PostgreSQL `work_mem`)
- Disk available for spill if memory exceeded

## Cost Model

**Memory cost per materialization point:**
- Full: `input_rows x tuple_width` bytes
- Partial: `max_partition_size x tuple_width` bytes
- Spool: `query_result_size` bytes (amortized over reuses)

**Latency impact:**
- Full materializer: first output delayed by entire input scan
- Time-to-first-tuple = `input_rows x per_tuple_cost`
- For Sort(10M rows): ~2-5 seconds before first output

**Spill-to-disk penalty:**
- When materialized data exceeds work_mem
- External sort: 2x I/O (write + read-back)
- External hash: partition, spill, re-read each partition
- Penalty: 10-100x slower than in-memory

**Optimization strategies:**
- Push filters below materializers to reduce volume
- Push projections to narrow tuple width
- Use streaming alternatives (streaming aggregation for
  pre-sorted input, merge join for sorted inputs)
- Increase work_mem for memory-intensive queries
- Decorrelate subqueries to eliminate repeated rescans

## Test Cases

```sql
-- Test 1: Pipelined query (no materialization)
SELECT name FROM users WHERE active = true;
-- Expected: Scan → Filter → Project, all pipelined
-- Memory: O(1), single tuple in flight
-- Materialization points: 0

-- Test 2: Single materializer (sort)
SELECT * FROM orders ORDER BY created_at;
-- Expected: Scan (pipelined) → Sort (materializes all)
-- Memory: O(N) for sort buffer
-- Pipeline segments: 2 (scan→sort input, sort output→result)

-- Test 3: Cascading materializers
SELECT region, SUM(amount)
FROM orders
GROUP BY region
ORDER BY SUM(amount) DESC;
-- Expected: Scan → Aggregate (materializes) → Sort (materializes)
-- Memory: O(groups) + O(groups) = O(groups) sequentially
-- Pipeline segments: 3

-- Test 4: Filter pushdown reduces materialization
-- Before optimization:
SELECT * FROM (
  SELECT * FROM orders ORDER BY created_at
) t WHERE amount > 1000;
-- Sort materializes ALL rows, then filter discards most

-- After optimization:
SELECT * FROM orders WHERE amount > 1000 ORDER BY created_at;
-- Filter first, sort materializes only matching rows
-- Memory savings: (1 - selectivity) x original

-- Test 5: Partial materialization (window function)
SELECT *, ROW_NUMBER() OVER (PARTITION BY region ORDER BY amount)
FROM orders;
-- Expected: buffers one partition at a time
-- Memory: O(max_partition_size), not O(total_rows)

-- Test 6: Spool materialization (CTE)
WITH active_users AS (
  SELECT * FROM users WHERE active = true
)
SELECT * FROM active_users a1
JOIN active_users a2 ON a1.region = a2.region;
-- Expected: CTE materialized once, read twice
-- Memory: O(active_users), avoids double scan
```

## References

1. **Graefe, Goetz**. "Query Evaluation Techniques for Large
   Databases." ACM Computing Surveys 25(2), 1993.
   - Pipeline breaker analysis and cost models
   - Materialization strategies

2. **Neumann, Thomas**. "Efficiently Compiling Efficient Query Plans
   for Modern Hardware." PVLDB 4(9), 2011.
   - Pipeline segment identification
   - Compiled code eliminates per-tuple materialization overhead

3. **PostgreSQL Source**: `src/backend/executor/nodeMaterial.c`
   - Materialization node implementation
   - Tuple store for intermediate results

4. **Graefe, Goetz**. "Sort-Merge-Join: An Idea Whose Time Has(h)
   Passed?" ICDE 1994.
   - Analysis of materialization in join algorithms

5. **Larson, Per-Ake et al**. "Cardinality Estimation Using Sample
   Views with Quality Assurance." SIGMOD 2007.
   - Materialization impact on query plan selection
