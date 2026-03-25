# RFC 0076: Adaptive Mid-Query Re-Optimization

- **Status**: Proposed
- **Priority**: Long-term (4-6 months)
- **Impact**: 2-20x improvement on cardinality mispredictions
- **Category**: Adaptive Execution / Runtime Re-Optimization
- **Created**: 2026-03-25

## Summary

Detect cardinality estimation errors during query execution and re-optimize remaining operators. Addresses the catastrophic failure mode when estimates are off by > 10x: committed plan is deeply suboptimal but execution proceeds anyway.

## Motivation

### The Cascading Error Problem

**Example**: 5-way join, first join estimate off by 100x
- Estimated: 1,000 rows after first join → choose hash join for next join
- Actual: 100,000 rows → hash join spills to disk → 100x slower

**Static optimization**: Commit to plan upfront, no recovery

**Adaptive optimization**: Checkpoint after each operator, re-optimize if error > threshold

### Evidence

**SQL Server Adaptive Joins** (Graefe et al., IEEE 2018):
- Monitor build side cardinality during execution
- Switch hash join ↔ nested loop if misprediction
- Result: 2-20x improvement on cardinality errors

**Eddies** (Avnur & Hellerstein, SIGMOD 2000):
- Re-order operators dynamically based on observed selectivity
- Result: 5-100x improvement on extreme mispredictions
- Cost: 10-20% overhead on correct estimates

## Proposal

### Checkpointing

```rust
pub struct CheckpointPlan {
    pub completed_operators: Vec<OperatorId>,
    pub checkpoint_data: Vec<Tuple>,
    pub estimated_remaining: u64,
    pub actual_so_far: u64,
}

impl Executor {
    fn execute_with_checkpoints(&mut self, plan: &PhysicalPlan) -> Result<Vec<Tuple>> {
        let mut result = self.initial_scan()?;

        for operator in plan.operators() {
            let estimated = operator.estimated_rows();
            let actual = result.len() as u64;

            // Check for large error
            if actual > estimated * 10 || actual < estimated / 10 {
                // Cardinality misprediction > 10x
                result = self.reoptimize_and_continue(
                    operator,
                    result,
                    actual,
                )?;
            } else {
                // Continue with original plan
                result = self.execute_operator(operator, result)?;
            }
        }

        Ok(result)
    }
}
```

### Re-Optimization

```rust
fn reoptimize_and_continue(
    &mut self,
    operator: &Operator,
    intermediate_result: Vec<Tuple>,
    actual_cardinality: u64,
) -> Result<Vec<Tuple>> {
    // Build partial query from remaining operators
    let remaining_query = self.extract_remaining_plan(operator);

    // Update cardinality estimate with actual value
    let updated_stats = self.update_statistics_with_actual(actual_cardinality);

    // Re-optimize with corrected statistics
    let new_plan = self.optimizer.optimize_with_stats(
        &remaining_query,
        &updated_stats,
    )?;

    // Execute new plan on intermediate result
    self.execute_plan(&new_plan, intermediate_result)
}
```

### Checkpoint Placement

**Heuristic**: Checkpoint after first join
- Reason: Join cardinality is hardest to estimate
- Cost: Minimal (just record intermediate size)

**Advanced**: Checkpoint after any operator with high uncertainty
```rust
fn should_checkpoint(&self, operator: &Operator) -> bool {
    match operator {
        Operator::Join(_) => true,  // Always checkpoint joins
        Operator::Filter(pred) if self.is_complex_predicate(pred) => true,
        Operator::Aggregate(_) => true,  // Aggregates can have high variance
        _ => false,
    }
}
```

### Cost-Benefit Analysis

**Re-optimization cost**:
- 50-100ms to re-optimize (measured)
- Overhead: 5-10% on queries that don't need it

**Benefit**:
- 2-20x speedup when cardinality is off by > 10x
- 30% of JOB queries have > 10x error (Leis et al., VLDB 2015)

**Decision**: Enable by default, allow disable via config

## Implementation Plan

### Phase 1: Checkpointing (Month 1-2)
1. Add checkpointing infrastructure
2. Track estimated vs actual cardinality per operator
3. Test: detect 10x errors

### Phase 2: Re-Optimization (Month 3-4)
1. Implement partial plan extraction
2. Update statistics with actual cardinalities
3. Re-optimize remaining plan
4. Test: verify new plan is used

### Phase 3: Validation (Month 5-6)
1. Run JOB queries with known mispredictions
2. Measure: speedup on error cases, overhead on correct cases
3. Tune threshold (10x vs 5x vs 20x)

## Expected Impact

**On queries with > 10x cardinality error** (30% of JOB):
- 2-20x speedup (SQL Server results)

**On queries with correct estimates** (70% of JOB):
- 5-10% overhead (checkpointing + detection)

**Overall**: 10-30% improvement on JOB benchmark

## Risks and Mitigations

**Risk 1: Re-optimization is expensive** (50-100ms)
- Mitigation: Only re-optimize if error > 10x (rare)
- Alternative: Adaptive joins (switch algorithm, not full re-optimization)

**Risk 2: Intermediate result is large** (memory overhead)
- Mitigation: Checkpoint only row count, not data
- Advanced: Spill intermediate result to disk if large

**Risk 3: Re-optimized plan is worse** (optimizer makes mistake)
- Mitigation: Compare cost estimates, only switch if new plan is 2x better
- Fallback: Continue with original plan if re-optimization fails

## Prior Art

### SQL Server Adaptive Joins
- Switches hash ↔ nested loop at runtime
- 2-20x improvement on mispredictions

### Eddies
- Continuous re-ordering of operators
- 5-100x improvement on extreme errors
- 10-20% overhead

### LEO Mid-Query Re-Optimization
- Re-optimize after first join
- 10-50% improvement on real workloads

## Related RFCs

- RFC 0069: Execution Feedback Loop (complementary, updates statistics)
- RFC 0070: Memory-Pressure-Aware Joins (complementary, runtime adaptation)
