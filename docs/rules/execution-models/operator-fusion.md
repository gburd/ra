# Rule: Streaming Operator Fusion

**Category:** execution-models/streaming
**File:** `rules/execution-models/streaming/operator-fusion.rra`

## Metadata

- **ID:** `stream-operator-fusion`
- **Version:** "1.0.0"
- **Databases:** flink, spark-streaming, dataflow, risingwave
- **Tags:** streaming, operator-fusion, pipeline, compilation, performance
- **Authors:** "Hirzel, Soulé, Schneider, Gedik, Grimm"


# Streaming Operator Fusion

## Description

Fuses adjacent streaming operators (filter-project, filter-filter,
project-project) into a single operator to reduce per-event overhead.
In streaming systems, each operator boundary incurs serialization,
deserialization, and scheduling costs. Fusing operators into a single
processing step eliminates these boundaries while preserving semantics.

**When to apply**: Adjacent stateless operators in a streaming dataflow
that can be combined without changing semantics.

## Relational Algebra

```algebra
-- Before: separate filter and project operators
pi[a, b](sigma[a > 10](stream))

-- After: fused filter-project operator
FusedFilterProject(stream, pred: a > 10, cols: [a, b])
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("fuse-filter-project";
    "(project ?cols (filter ?pred ?stream))" =>
    "(fused-filter-project ?pred ?cols ?stream)"
),

rw!("fuse-filter-filter";
    "(filter ?p1 (filter ?p2 ?stream))" =>
    "(filter (and ?p1 ?p2) ?stream)"
),

rw!("fuse-project-project";
    "(project ?cols1 (project ?cols2 ?stream))" =>
    "(project (compose-projections ?cols1 ?cols2) ?stream)"
),
```

## Preconditions

```rust
fn applicable(op1: &StreamOp, op2: &StreamOp) -> bool {
    // Both must be stateless
    op1.is_stateless() && op2.is_stateless()
        // Must be adjacent in the dataflow graph
        && op1.output() == op2.input()
        // Fusion must not break parallelism boundaries
        && op1.parallelism() == op2.parallelism()
}
```

**Restrictions:**
- Cannot fuse stateful operators (joins, aggregations)
- Parallelism must be compatible between fused operators
- Checkpointing boundaries may prevent fusion

## Cost Model

```rust
fn estimated_benefit(
    events_per_second: f64,
    operators_fused: usize,
    serialization_cost: f64,
) -> f64 {
    let boundaries_eliminated = (operators_fused - 1) as f64;
    events_per_second * boundaries_eliminated * serialization_cost
}
```

**Typical benefit**: 10-40% throughput improvement.

## Test Cases

```sql
-- Positive: adjacent filter and project
SELECT user_id, event_type FROM events_stream
WHERE event_type = 'click' AND ts > CURRENT_TIMESTAMP - INTERVAL '1' HOUR;
-- Fuses filter(event_type='click' AND ts>...) with project(user_id, event_type)

-- Positive: cascaded filters
SELECT * FROM events_stream
WHERE region = 'US' AND event_type = 'purchase' AND amount > 100;
-- Fuses three filter conditions into one

-- Negative: stateful operator between filters
SELECT * FROM (
    SELECT *, ROW_NUMBER() OVER (ORDER BY ts) AS rn
    FROM events_stream WHERE region = 'US'
) WHERE rn <= 100;
-- Cannot fuse across the window function
```

## References

- Hirzel, M. et al. "A Catalog of Stream Processing Optimizations" (ACM Computing Surveys 2014)
- Flink: Operator Chaining documentation
