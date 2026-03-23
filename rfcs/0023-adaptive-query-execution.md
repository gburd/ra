# RFC 0023: Adaptive Query Execution

- Start Date: 2026-03-21
- Author: RA Contributors
- Status: Draft
- Tracking Issue: TBD

## Summary

Implement adaptive query execution (AQE) that dynamically adjusts query plans during execution based on observed runtime statistics, correcting optimizer mistakes and adapting to data skew without restarting queries.

## Motivation

Static query optimization relies on statistics that may be stale, incomplete, or incorrect. This leads to:
- Bad join orders causing excessive intermediate results
- Wrong join algorithms (hash vs. sort-merge vs. nested loop)
- Incorrect parallelism degrees
- Memory allocation mistakes causing spilling

AQE detects and corrects these issues mid-flight, improving query latency and resource utilization.

## Guide-level explanation

Enable adaptive execution globally or per-query:

```sql
SET enable_adaptive_execution = true;
SET adaptive_execution_threshold = '100ms';  -- Minimum runtime before adaptation

-- Per-query hint
SELECT /*+ ADAPTIVE */
    o.*, c.name
FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.amount > 1000;
```

The executor monitors runtime metrics and can:
1. Switch join algorithms mid-execution
2. Reorder remaining joins based on actual cardinalities
3. Adjust parallelism based on data skew
4. Repartition data to balance load

## Reference-level explanation

### Monitoring Infrastructure

Runtime statistics collected:
- Actual row counts at each operator
- Memory consumption and spill events
- CPU time per operator
- Data skew metrics (partition sizes)

### Adaptation Points

Query execution pauses at **adaptation barriers**:
- After materializing build side of hash joins
- After sorting for sort-merge joins
- After completing subquery execution
- At configurable row count thresholds

### Adaptation Strategies

1. **Join Algorithm Selection**
   - Switch from hash to sort-merge if build side exceeds memory
   - Switch to index nested loop for small outer relations
   - Fall back to BNLJ for complex join conditions

2. **Join Reordering**
   - Re-estimate costs using actual cardinalities
   - Reorder remaining joins using dynamic programming
   - Materialize beneficial intermediate results

3. **Parallelism Adjustment**
   - Increase threads for CPU-bound operators
   - Decrease threads if I/O becomes bottleneck
   - Rebalance partitions to handle skew

4. **Memory Management**
   - Dynamically adjust work_mem allocations
   - Switch to external algorithms before OOM
   - Compress intermediate results if beneficial

### Implementation Architecture

```rust
pub trait AdaptiveExecutor {
    fn execute_with_adaptation(&mut self) -> Result<RecordBatch>;
    fn collect_statistics(&self) -> RuntimeStatistics;
    fn should_adapt(&self) -> bool;
    fn generate_adapted_plan(&self) -> PhysicalPlan;
}
```

## Drawbacks

- Adaptation overhead may exceed benefits for short queries
- Complexity in maintaining execution state across adaptations
- Difficult to debug non-deterministic execution paths
- May cause performance regressions if adaptation is too aggressive
- Increases memory footprint for statistics collection

## Rationale and alternatives

### Why This Design?

- Proven benefits in Spark, Presto, and Oracle
- Incremental implementation possible
- Complements rather than replaces static optimization

### Alternative Approaches

1. **Re-optimization**: Restart query with better plan (higher latency)
2. **Robust optimization**: Generate plans resilient to estimation errors
3. **Machine learning**: Predict and prevent bad plans (requires training)

## Prior art

- **Apache Spark**: Adaptive Query Execution since 3.0
- **Presto/Trino**: Adaptive join reordering and distribution
- **Oracle**: Adaptive plans with statistics feedback
- **SQL Server**: Adaptive joins and memory grants
- **Snowflake**: Adaptive optimization in their engine
- **CockroachDB**: Runtime stats injection

Academic research:
- Eddies: Continuously adaptive query processing
- ROX: Run-time Optimization of XQueries
- Progressive Optimization in Action

## Unresolved questions

- How to persist learnings across query executions?
- Integration with prepared statements?
- Adaptation in presence of UDFs?
- Cost model for adaptation decisions?

## Future possibilities

- Machine learning for adaptation thresholds
- Cross-query learning and plan caching
- Speculative execution of multiple plans
- Integration with auto-tuning configuration
- Federated query adaptation across systems