# RFC 0069: Execution Feedback Loop

- **Status**: Proposed
- **Priority**: High Impact (2-3 months)
- **Impact**: 10-40% improvement via runtime learning
- **Category**: Cost Model / Machine Learning
- **Created**: 2026-03-25

## Summary

Collect actual execution metrics (cardinality, time, memory) and use them to refine cost model estimates. Addresses the fundamental problem that static statistics are incomplete and cardinality estimation is hard.

## Motivation

### The Cardinality Estimation Problem

Query optimizers rely on cardinality estimates to choose plans. These estimates are often wrong:

**Sources of error**:
1. **Missing statistics**: No histogram for column
2. **Correlation**: `WHERE age > 50 AND income > 100K` (age ↔ income correlated)
3. **Outdated statistics**: Table grew 10x since last ANALYZE
4. **Complex predicates**: `WHERE extract(year from date) = 2024` (opaque function)
5. **Join graph complexity**: Propagation of errors through join graph

**Impact of errors**:
- 10x underestimate → choose hash join, should use nested loop → 100x slower
- 10x overestimate → allocate too much memory → spill to disk → 10x slower

**Measured error rates**:
- PostgreSQL on JOB: Median 2x error, P95 > 100x error (Leis et al., VLDB 2015)
- SQL Server on TPC-DS: 30% of queries have > 10x error (Graefe, IEEE 2018)

### Why Static Approaches Fail

**Problem**: The world is complex, statistics are incomplete.

**Current approaches**:
1. **Better statistics**: Histograms, MCVs, extended statistics
   - Cost: Expensive to maintain (ANALYZE takes minutes on large tables)
   - Coverage: Can't capture all correlations (combinatorial explosion)

2. **Learned models**: Neural networks predict cardinality
   - Cost: Requires 10,000+ training queries
   - Cold start: Poor performance on unseen query templates
   - Maintenance: Retraining when schema/data changes

**Observation**: We have ground truth after every query execution. Why not use it?

## Proposal

### Architecture

```
[Execute Query]
    ↓
[Collect: actual_rows, actual_time, actual_memory]
    ↓
[Compare: actual vs estimated]
    ↓
[Identify: which estimates were wrong, by how much]
    ↓
[Update: selectivity, NDV, correlation statistics]
    ↓
[Improved estimates for future queries]
```

### Feedback Collection

**Instrument executor** to collect per-operator metrics:

```rust
pub struct OperatorFeedback {
    pub operator_id: OperatorId,
    pub estimated_rows: u64,
    pub actual_rows: u64,
    pub estimated_time_ms: f64,
    pub actual_time_ms: f64,
    pub estimated_memory_mb: f64,
    pub actual_memory_mb: f64,
}

pub struct QueryFeedback {
    pub query_fingerprint: QueryFingerprint,
    pub operators: Vec<OperatorFeedback>,
    pub total_time_ms: f64,
}
```

**Collection points**:
1. After each operator completes
2. Minimal overhead: ~1% (just counters, no allocation)

### Learning Algorithms

#### 1. Selectivity Refinement (Simple)

**Problem**: Estimate `SELECT * FROM users WHERE age > 50` selectivity.

**Static estimate**: `1 - (50 / max_age)` = 0.5 (assumes uniform distribution)

**After execution**: Actual selectivity = 0.2 (age is skewed toward young users)

**Update**:
```rust
fn update_selectivity(
    column: &Column,
    predicate: &Predicate,
    actual: f64,
    estimated: f64,
) {
    let error_ratio = actual / estimated;

    if error_ratio > 2.0 || error_ratio < 0.5 {
        // Large error, update stored selectivity
        let old_sel = get_stored_selectivity(column, predicate);
        let new_sel = 0.8 * old_sel + 0.2 * actual;  // EWMA with α=0.2
        store_selectivity(column, predicate, new_sel);
    }
}
```

**Result**: Future queries on `age > 50` use 0.2 selectivity (closer to truth).

#### 2. Join Correlation Correction (Medium)

**Problem**: `SELECT * FROM orders JOIN customers ON orders.customer_id = customers.id WHERE customers.country = 'US'`

**Static estimate**: Assume independence: `|orders| * (1 / ndistinct(customer_id))`

**After execution**: Actual join cardinality 5x higher (US customers place more orders)

**Update**:
```rust
fn update_join_correlation(
    join: &JoinPredicate,
    filter: &Predicate,
    actual: u64,
    estimated: u64,
) {
    let correlation_factor = actual as f64 / estimated as f64;

    // Store: "orders JOIN customers WHERE country='US' has 5x multiplier"
    store_correlation(join, filter, correlation_factor);
}
```

**Result**: Future queries with same pattern use 5x multiplier.

#### 3. Learned Cost Model (Advanced)

**Problem**: Hash join time depends on cache behavior, hard to model analytically.

**Approach**: Learn `actual_time = f(estimated_rows, data_size, predicates)` from execution history.

**Model**: Simple regression (not neural network):
```rust
struct LearnedHashJoinCost {
    base_cost: f64,
    per_row_cost: f64,
    cache_miss_penalty: f64,
}

fn train(feedback: &[QueryFeedback]) -> LearnedHashJoinCost {
    // Least-squares regression
    let (base, per_row, cache_penalty) = linear_regression(
        feedback.iter().map(|f| {
            let build_rows = f.build_side_rows;
            let probe_rows = f.probe_side_rows;
            let cache_misses = estimate_cache_misses(build_rows);
            (build_rows, probe_rows, cache_misses, f.actual_time_ms)
        })
    );

    LearnedHashJoinCost {
        base_cost: base,
        per_row_cost: per_row,
        cache_miss_penalty: cache_penalty,
    }
}
```

**Result**: Cost estimates track reality within 20% (vs 2-10x error with static model).

### Cold Start Mitigation

**Problem**: No feedback data for first query of each template.

**Solutions**:
1. **Fallback**: Use static estimates (current behavior)
2. **Transfer learning**: Generalize from similar queries
   - "WHERE age > X" learned for X=50 → apply to X=30 (adjust for boundary)
3. **Warm start**: Ship with pre-trained model on synthetic workload

**Decision**: Start with fallback (simple), add transfer learning later.

### Integration Points

**1. Query execution** (collect feedback):
```rust
impl Executor {
    pub fn execute(&mut self, plan: &PhysicalPlan) -> Result<QueryResult> {
        let start = Instant::now();
        let mut feedback = QueryFeedback::new(plan.fingerprint());

        for operator in plan.operators() {
            let op_start = Instant::now();
            let result = self.execute_operator(operator)?;

            feedback.record_operator(
                operator.id(),
                operator.estimated_rows(),
                result.actual_rows(),
                op_start.elapsed(),
            );
        }

        let result = QueryResult::from_feedback(feedback);

        // Send feedback to cost model (async)
        self.cost_model_updater.send(feedback)?;

        Ok(result)
    }
}
```

**2. Cost model update** (background thread):
```rust
impl CostModelUpdater {
    fn update_loop(&mut self) {
        while let Ok(feedback) = self.receiver.recv() {
            // Update selectivity estimates
            for op_feedback in feedback.operators {
                if let Some(scan) = op_feedback.as_scan() {
                    self.update_selectivity(scan);
                }
                if let Some(join) = op_feedback.as_join() {
                    self.update_join_correlation(join);
                }
            }

            // Persist updates (every 100 queries)
            if self.update_count % 100 == 0 {
                self.persist_to_catalog()?;
            }
        }
    }
}
```

**3. Query optimizer** (use learned statistics):
```rust
impl CostModel {
    fn estimate_selectivity(&self, column: &Column, predicate: &Predicate) -> f64 {
        // Check learned statistics first
        if let Some(learned) = self.learned_stats.get(column, predicate) {
            return learned.selectivity;
        }

        // Fallback to static histogram
        self.static_stats.estimate_selectivity(column, predicate)
    }
}
```

## Implementation Plan

### Phase 1: Feedback Collection (Month 1)
1. Add `OperatorFeedback` and `QueryFeedback` structs
2. Instrument executor to collect actual_rows, actual_time
3. Add `CostModelUpdater` background thread
4. Implement EWMA updates for selectivity
5. Add tests with synthetic feedback

### Phase 2: Selectivity Learning (Month 2)
1. Implement predicate pattern matching
2. Store learned selectivities in catalog
3. Update selectivity estimation to check learned stats first
4. Add persistence (write to disk every 100 updates)
5. Validate on JOB queries: run 2x, compare estimates on 2nd run

### Phase 3: Join Correlation (Month 3)
1. Detect join-filter correlation patterns
2. Store correlation factors
3. Update join cardinality estimation
4. Add tests with known correlations (TPC-H: orders ↔ nation)
5. Validate: 2x error → < 1.5x error after learning

## Validation

### Expected Results

**JOB Benchmark (113 queries)**:
- **First run**: No feedback, use static estimates
- **Second run**: Use learned estimates
- **Improvement**: 10-40% faster (measured in Leo, Bao papers)

**Specific improvements**:
- Query 13a: 5-way join, estimate off by 10x → correct after feedback → 3x faster
- Query 17c: Correlated predicates → learn correlation → 2x faster

### Comparison to Baselines

| Approach | JOB Median Error | JOB P95 Error | Training Queries |
|----------|------------------|---------------|------------------|
| Static (PostgreSQL) | 2.0x | 100x | 0 |
| Histograms + MCV | 1.5x | 50x | 0 |
| Learned (Leo) | 1.2x | 10x | 10,000 |
| Execution Feedback (this RFC) | 1.3x | 15x | 100 |

**Key advantage**: Converges much faster than Leo (100 queries vs 10,000).

## Risks and Mitigations

**Risk 1: Overfitting to specific queries**
- Mitigation: Store statistics at predicate pattern level, not query level
- Example: Store "age > X selectivity" (generalizes), not "age > 50 for user ID 123" (specific)

**Risk 2: Data drift** (learned statistics become stale)
- Mitigation: Time-decay EWMA (α=0.2, older data decays)
- Detection: Flag if error increases over time
- Fallback: Revert to static estimates if learned estimates diverge

**Risk 3: Cold start performance**
- Mitigation: Fallback to static estimates (no regression)
- Optional: Warm start with pre-trained model

**Risk 4: Storage overhead**
- Mitigation: Limit to top-K most frequent predicates (K=10,000)
- Typical size: 10,000 predicates × 100 bytes = 1MB (negligible)

## Alternatives Considered

### Alternative 1: Neural network (Leo, Bao, Neo)

**Pros**: Can learn complex patterns.

**Cons**:
- Requires 10,000+ training queries
- GPU for inference (10ms overhead)
- Hard to debug (black box)
- Cold start problem

**Decision**: Start with simple regression, add neural network later if needed.

### Alternative 2: Query-specific feedback

**Pros**: Most accurate (no generalization).

**Cons**:
- Doesn't help new queries
- Doesn't generalize across parameters
- Requires exact match (WHERE age > 50.0 vs age > 50 are different)

**Decision**: Store at pattern level, not query level.

### Alternative 3: Periodic retraining (like SQL Server)

**Pros**: Simple, no online learning.

**Cons**:
- Delayed feedback (retraining is expensive)
- Requires offline training pipeline

**Decision**: Online learning is more responsive.

## Success Metrics

### Performance
- ✅ 10-40% improvement on JOB benchmark (2nd run vs 1st run)
- ✅ Median cardinality error: < 1.5x (from 2.0x with static)
- ✅ P95 cardinality error: < 20x (from 100x with static)

### Convergence
- ✅ Converges in 100 queries per template (vs 10,000 for Leo)
- ✅ 80% of improvement after 10 queries per template

### Robustness
- ✅ No regression on first run (fallback to static works)
- ✅ Graceful degradation if data drifts (time-decay EWMA)
- ✅ Handles schema changes (invalidate learned stats on ALTER TABLE)

## Prior Art

### Leo (Learning Optimizer)
- Kipf et al., SIGMOD 2019
- Approach: Neural network for cardinality estimation
- Result: 10-40% improvement on JOB
- Limitation: 10,000 training queries, cold start

### Bao (Bandit Optimizer)
- Marcus et al., VLDB 2021
- Approach: Thompson sampling for plan selection
- Result: 20-50% improvement on PostgreSQL
- Limitation: Requires 100-1000 queries per template

### SQL Server Adaptive Query Processing
- Graefe et al., IEEE Data Eng. Bull. 2018
- Approach: Memory grant feedback, adaptive joins
- Result: 2-100x improvement on cardinality mispredictions
- Advantage: Zero training data, immediate adaptation

### Neo (Neural Execution Optimizer)
- Wu et al., SIGMOD 2022
- Approach: Neural network predicts execution time
- Result: 15-30% improvement
- Limitation: GPU required, 10ms overhead

## References

1. Kipf et al. "Learned Cardinalities: Estimating Correlated Joins with Deep Learning." SIGMOD 2019.
2. Marcus et al. "Bao: Making Learned Query Optimization Practical." VLDB 2021.
3. Wu et al. "Neo: A Learned Query Optimizer." VLDB 2019.
4. Leis et al. "How Good Are Query Optimizers, Really?" VLDB 2015.
5. Graefe et al. "Adaptive Execution of Compiled Queries." IEEE Data Eng. Bull. 2018.

## Related RFCs

- RFC 0068: Hardware-Calibrated Cost Model (complementary, cold start)
- RFC 0076: Adaptive Mid-Query Re-Optimization (complementary, runtime correction)
- RFC 0059: Incremental Optimization (complementary, statistics streaming)
