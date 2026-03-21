# RFC 0036: Multi-Query Optimization

## Status
PROPOSED

## Summary
Implement multi-query optimization to identify and share common subexpressions across multiple queries executing concurrently, reducing redundant computation and I/O.

## Motivation
Modern workloads often execute multiple related queries concurrently (dashboards, reports, batch jobs). These queries frequently scan the same tables, compute similar aggregates, or join the same relations. Multi-query optimization can achieve 2-10x speedup by sharing work across queries.

## Design

### Architecture

```rust
pub struct MultiQueryOptimizer {
    query_batch: Vec<LogicalPlan>,
    common_subexpressions: HashMap<PlanHash, SharedPlan>,
    materialization_points: Vec<MaterializationPoint>,
}

pub struct SharedPlan {
    plan: LogicalPlan,
    consumers: Vec<QueryId>,
    estimated_benefit: Cost,
}
```

### Optimization Phases

1. **Common Subexpression Detection**
   - Hash-based identification of identical subplans
   - Semantic equivalence checking for similar plans
   - Cost-benefit analysis for sharing

2. **Materialization Point Selection**
   - Identify where to materialize shared results
   - Balance memory usage vs recomputation
   - Consider result size and reuse frequency

3. **Query Rewriting**
   - Replace common subexpressions with shared scans
   - Insert synchronization points for concurrent execution
   - Add result routing to appropriate consumers

### Sharing Strategies

- **Scan Sharing**: Multiple queries read same table simultaneously
- **Join Sharing**: Reuse join results across queries
- **Aggregate Sharing**: Share partial aggregates
- **Pipeline Sharing**: Merge compatible pipelines

## Implementation Plan

1. Implement subexpression hashing and comparison
2. Create shared scan operators
3. Add materialization point selection algorithm
4. Implement query batch coordinator
5. Add synchronization primitives
6. Create adaptive sharing policies

## Alternatives Considered

- **View Materialization**: Static, doesn't adapt to workload
- **Result Caching**: Reactive, misses sharing opportunities
- **Query Merging**: Too complex, changes query semantics

## Success Criteria

- 2x+ speedup for dashboard workloads (5-10 related queries)
- < 100ms overhead for optimization phase
- Memory usage bounded by configurable limit
- Graceful degradation when sharing not beneficial