# Rule: "Pipeline Breaker Analysis and Minimization"

**Category:** physical/materialization
**File:** `rules/physical/materialization/pipeline-breaker-analysis.rra`

## Metadata

- **ID:** `pipeline-breaker-analysis`
- **Version:** "1.0.0"
- **Databases:** postgresql, clickhouse, duckdb, hyper, umbra
- **Tags:** materialization, pipeline, breaker, blocking, streaming
- **Authors:** "RA Contributors"


# Pipeline Breaker Analysis and Minimization

## Metadata
- **Rule ID**: `pipeline-breaker-analysis`
- **Category**: Physical / Materialization
- **Complexity**: Varies (eliminates O(n) materialization per breaker)
- **Prerequisites**: Query plan with multiple operators
- **Alternatives**: Accept pipeline breakers as-is

## Description

A pipeline breaker is an operator that must consume all input before
producing any output, forcing full materialization of intermediate
results. Common pipeline breakers include: sort (must see all rows
to determine order), hash aggregation (must build full hash table),
hash join build side (must build full hash table before probing).

Minimizing pipeline breakers reduces peak memory usage and enables
streaming execution. The optimizer can transform the plan to reduce
the number of breakers:

1. Replace sort with streaming ordered read (if data already sorted)
2. Replace hash aggregation with ordered aggregation (if sorted)
3. Replace hash join with merge join (if both sides sorted)
4. Reorder operators to reduce materialization points
5. Use pipelining-friendly algorithms (e.g., grace hash join)

The goal is to maximize the length of pipeline segments (chains of
streaming operators) and minimize the number of materialization
boundaries.

**When to apply:**
- Memory-constrained environments
- Very large intermediate results
- Queries with multiple blocking operators in sequence

## Relational Algebra

```
Pipeline breakers:
  sort[k](R)           -- must see all R before output
  hash-aggregate[g](R) -- must build full hash table
  hash-join-build(R)   -- must build full hash table

Streaming operators:
  filter[p](R)   -- 1-in, 1-out
  project[c](R)  -- 1-in, 1-out
  limit[n](R)    -- 1-in, up to n out
  merge-join(L, R) -- streaming if both sorted
```

## Implementation (egg rewrite rules)

```lisp
;; Replace hash-agg with streaming-agg when input sorted
(rewrite (hash-aggregate ?groups ?aggs ?input)
  (ordered-aggregate ?groups ?aggs ?input)
  :if (sorted-by ?input ?groups))

;; Replace sort with read-in-order when table supports it
(rewrite (sort ?keys (scan ?table))
  (merge-sorted (read-in-order ?table ?keys))
  :if (is-prefix-of ?keys (sort-key ?table)))

;; Replace hash-join with merge-join when both sides sorted
(rewrite (hash-join ?cond ?left ?right)
  (merge-join ?cond ?left ?right)
  :if (sorted-by ?left (join-key-left ?cond))
  :if (sorted-by ?right (join-key-right ?cond)))

;; Combine sort + hash-agg into sort + stream-agg (one breaker)
(rewrite (hash-aggregate ?groups ?aggs (sort ?keys ?input))
  (ordered-aggregate ?groups ?aggs (sort ?keys ?input))
  :if (is-prefix-of ?groups ?keys))

;; Pipeline length analysis: count breakers
(rewrite (sort ?k1 (hash-aggregate ?g (sort ?k2 ?input)))
  (sort ?k1 (ordered-aggregate ?g (sort ?k2 ?input)))
  :if (is-prefix-of ?g ?k2))
```

## Cost Model

```rust
pub struct PipelineAnalysis {
    pub segments: Vec<PipelineSegment>,
    pub breakers: Vec<PipelineBreaker>,
    pub peak_memory: u64,
}

pub fn analyze_pipeline(plan: &QueryPlan) -> PipelineAnalysis {
    let mut segments = vec\![];
    let mut current_segment = PipelineSegment::new();
    let mut breakers = vec\![];
    let mut peak_memory = 0u64;

    for op in plan.operators() {
        if op.is_pipeline_breaker() {
            segments.push(current_segment);
            current_segment = PipelineSegment::new();
            let mem = op.estimated_memory();
            peak_memory = peak_memory.max(mem);
            breakers.push(PipelineBreaker {
                operator: op.name(),
                memory: mem,
            });
        } else {
            current_segment.add(op);
        }
    }
    segments.push(current_segment);

    PipelineAnalysis { segments, breakers, peak_memory }
}

pub fn cost_pipeline_breaker(rows: u64, row_width: u64) -> Cost {
    Cost::memory(rows * row_width) + Cost::cpu(rows * 2)
}
```

**Typical benefit**: 10-50% memory reduction; faster time-to-first-row

## Test Cases

### Positive: Sort elimination removes breaker
```sql
CREATE TABLE events (...) ENGINE = MergeTree ORDER BY (date);

SELECT date, count(*) FROM events
GROUP BY date ORDER BY date;

-- Without optimization: scan -> hash-agg (breaker) -> sort (breaker)
-- With optimization: read-in-order -> ordered-agg (streaming) -> (no sort)
-- 2 breakers eliminated; fully streaming pipeline
```

### Positive: Merge join instead of hash join
```sql
-- Both tables sorted by join key
SELECT * FROM sorted_a a
JOIN sorted_b b ON a.key = b.key
WHERE a.date > '2024-01-01';

-- Hash join: build hash table (breaker)
-- Merge join: streaming, no materialization
```

### Negative: No sorted input available
```sql
SELECT user_id, count(*) FROM events
GROUP BY user_id ORDER BY count(*) DESC;

-- user_id not sorted; hash aggregation required
-- ORDER BY count(*) requires sort after aggregation
-- Both breakers unavoidable
```

## References

- Neumann, "Efficiently Compiling Efficient Query Plans for Modern Hardware", VLDB 2011
- Graefe, "Volcano: An Extensible and Parallel Query Evaluation System", 1994
- Leis et al., "Morsel-Driven Parallelism", SIGMOD 2014
- ClickHouse: Pipeline concept in QueryPipeline
