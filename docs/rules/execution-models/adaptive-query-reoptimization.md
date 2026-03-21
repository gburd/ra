# Rule: Adaptive Query Re-optimization

**Category:** execution-models
**File:** `rules/execution-models/adaptive/adaptive-query-reoptimization.rra`

## Metadata

- **ID:** `adaptive-query-reoptimization`
- **Version:** 1.0.0
- **Databases:** mssql, Oracle, DB2, CockroachDB
- **Tags:** execution, adaptive, reoptimization, runtime, cardinality
- **SQL Standard:** Adaptive query processing (AQP)
- **Authors:** Navin Kabra, David DeWitt


# Adaptive Query Re-optimization

## Description

Adaptive query re-optimization monitors runtime statistics at materialization points (hash table builds, sort buffers, exchange operators) and triggers plan re-optimization when actual cardinalities deviate significantly from optimizer estimates. Rather than committing to a single plan chosen at compile time, the executor places checkpoints at pipeline breakers where intermediate result sizes become known. When observed cardinality differs from the estimate beyond a configurable threshold, the remaining (unexecuted) portion of the plan is re-optimized using the now-known true cardinality.

**Key characteristics:**
- **Checkpoint placement**: At pipeline breakers where cardinality becomes known (hash build, sort, exchange)
- **Deviation threshold**: Re-optimize when actual/estimated ratio exceeds threshold (e.g., 3x or 0.3x)
- **Partial plan reuse**: Already-materialized results are kept; only downstream plan changes
- **Bounded re-optimization**: Limit re-optimization count to prevent oscillation
- **Cost amortization**: Re-optimization cost must be less than expected savings

**Trade-offs:**
- Re-optimization overhead (tens of milliseconds per cycle)
- Cannot undo work already done (upstream operators committed)
- Risk of plan oscillation if thresholds are too sensitive
- Works best for multi-join queries where early misestimates cascade

## Relational Algebra

```
AdaptiveExecute(plan, checkpoints) -> Result

fn execute_adaptive(plan, checkpoints):
  for segment in plan.segments_between(checkpoints):
    result = execute_segment(segment)
    actual_card = result.row_count()
    estimated_card = segment.output_estimate()

    deviation = actual_card / estimated_card
    if deviation > UPPER_THRESHOLD or deviation < LOWER_THRESHOLD:
      remaining = plan.remaining_after(segment)
      new_remaining = reoptimize(remaining, actual_card)
      plan = plan.replace_remaining(segment, new_remaining)

  return final_result(plan)
```

## Implementation

```rust
use std::sync::Arc;

/// Deviation thresholds for triggering re-optimization
pub struct ReoptThresholds {
    pub upper_ratio: f64,  // e.g., 3.0 (actual 3x larger)
    pub lower_ratio: f64,  // e.g., 0.33 (actual 3x smaller)
    pub max_reopts: usize, // maximum re-optimization cycles
}

impl Default for ReoptThresholds {
    fn default() -> Self {
        Self {
            upper_ratio: 3.0,
            lower_ratio: 0.33,
            max_reopts: 3,
        }
    }
}

/// Checkpoint at a pipeline breaker
pub struct Checkpoint {
    pub operator_id: usize,
    pub estimated_cardinality: f64,
    pub actual_cardinality: Option<f64>,
}

/// Adaptive executor that monitors and re-optimizes
pub struct AdaptiveExecutor {
    plan: QueryPlan,
    checkpoints: Vec<Checkpoint>,
    thresholds: ReoptThresholds,
    reopt_count: usize,
}

impl AdaptiveExecutor {
    pub fn execute(&mut self) -> Result<ResultSet> {
        let segments = self.plan.split_at_checkpoints(
            &self.checkpoints,
        );

        let mut materialized = Vec::new();

        for (i, segment) in segments.iter().enumerate() {
            let result = segment.execute(&materialized)?;

            if let Some(cp) = self.checkpoints.get_mut(i) {
                cp.actual_cardinality = Some(
                    result.row_count() as f64,
                );
                let deviation = result.row_count() as f64
                    / cp.estimated_cardinality;

                if self.should_reoptimize(deviation) {
                    let new_plan = self.reoptimize_remaining(
                        i + 1,
                        &materialized,
                        result.row_count(),
                    )?;
                    self.plan = new_plan;
                    self.reopt_count += 1;
                }
            }

            materialized.push(result);
        }

        Ok(materialized.pop().expect("at least one segment"))
    }

    fn should_reoptimize(&self, deviation: f64) -> bool {
        self.reopt_count < self.thresholds.max_reopts
            && (deviation > self.thresholds.upper_ratio
                || deviation < self.thresholds.lower_ratio)
    }

    fn reoptimize_remaining(
        &self,
        from_segment: usize,
        materialized: &[ResultSet],
        actual_card: usize,
    ) -> Result<QueryPlan> {
        let remaining = self.plan.subplan_from(from_segment);
        let optimizer = Optimizer::new();
        optimizer.optimize_with_known_inputs(
            remaining,
            materialized,
            actual_card,
        )
    }
}

/// Cost of re-optimization decision
pub fn reopt_benefit_estimate(
    estimated_card: f64,
    actual_card: f64,
    remaining_cost_current: f64,
) -> f64 {
    let card_ratio = actual_card / estimated_card;
    // Heuristic: cost scales roughly with cardinality for joins
    let projected_new_cost = remaining_cost_current / card_ratio;
    let reopt_overhead = 0.01; // 10ms in normalized units
    remaining_cost_current - projected_new_cost - reopt_overhead
}
```

## Cost Model

**Re-optimization overhead:**
- Plan re-optimization: 1-50 ms depending on join count
- Statistics collection at checkpoint: negligible (already materialized)
- Total overhead per re-optimization: ~10 ms amortized

**When beneficial:**
- Multi-join queries (4+ tables): misestimates cascade exponentially
- Skewed data: optimizer statistics miss tail distributions
- Parameter-sensitive queries: different parameter values yield different optimal plans
- Estimated benefit: `remaining_cost_current - remaining_cost_reoptimized - reopt_overhead`

**When to skip:**
- Single-table queries: no join ordering to change
- Very short queries (< 100 ms): overhead exceeds potential savings
- Already-accurate estimates (deviation < 2x): plan likely already near-optimal

## Test Cases

```sql
-- Test 1: Cardinality misestimate triggers re-optimization
-- Skewed data: orders for customer_id=1 has 1M rows, estimate was 100
SELECT o.*, l.*
FROM orders o
JOIN lineitem l ON o.order_id = l.order_id
WHERE o.customer_id = 1;
-- Checkpoint after hash build on orders: actual 1M vs estimated 100
-- Re-optimize: switch from nested-loop to hash join for lineitem

-- Test 2: Cascading join re-optimization
SELECT *
FROM a JOIN b ON a.x = b.x
       JOIN c ON b.y = c.y
       JOIN d ON c.z = d.z
WHERE a.status = 'rare';
-- First checkpoint reveals |a filtered| = 5 (estimated 50K)
-- Re-optimize: push smaller input as build side downstream

-- Test 3: No re-optimization needed (accurate estimates)
SELECT COUNT(*)
FROM lineitem l JOIN orders o ON l.order_id = o.order_id
WHERE l.quantity > 25;
-- Actual cardinality within 2x of estimate
-- No re-optimization triggered, plan executes normally

-- Test 4: Re-optimization budget exhausted
SELECT *
FROM t1 JOIN t2 ON t1.a = t2.a
       JOIN t3 ON t2.b = t3.b
       JOIN t4 ON t3.c = t4.c
       JOIN t5 ON t4.d = t5.d;
-- Each checkpoint deviates: re-optimize 3 times, then stop
-- Prevents oscillation even with persistent misestimates
```

## References

1. **Kabra, Navin and David DeWitt**. "Efficient Mid-Query Re-Optimization of Sub-Optimal Query Execution Plans." SIGMOD 1998.
   - Foundational work on mid-execution re-optimization

2. **Babu, Shivnath et al**. "Adaptive Query Processing in the Looking Glass." CIDR 2005.
   - Survey of adaptive query processing techniques

3. **mssql Documentation**. "Adaptive Query Processing."
   - Production implementation in mssql 2017+

4. **Deshpande, Amol et al**. "Adaptive Query Processing." Foundations and Trends in Databases, 2007.
   - Comprehensive survey of AQP techniques
