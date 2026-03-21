# Rule: Learned Cost Models

**Category:** execution-models/experimental
**File:** `rules/execution-models/experimental/learned-cost-models.rra`

## Metadata

- **ID:** `learned-cost-models`
- **Version:** "1.0.0"
- **Databases:** postgresql, spark, trino, noisepage
- **Tags:** execution, experimental, research, machine-learning, cost-model, optimization, bao
- **Authors:** Ryan Marcus, Anshuman Dutt, Parimarjan Negi


# Learned Cost Models

## Description

Learned cost models replace hand-tuned analytical cost formulas with machine
learning models trained on actual query execution times. Traditional cost models
use formulas like `cost = seq_page_cost * pages + cpu_tuple_cost * rows` with
manually calibrated constants that are often wrong for specific hardware,
workloads, and data distributions. Learned models observe real execution costs
and train to predict them, automatically adapting to the actual system behavior.

**When to apply**: Any optimizer that selects between alternative query plans
using cost estimates. The largest gains come when traditional cost models are
miscalibrated (e.g., cloud environments with variable I/O latency, novel
storage engines, or mixed workloads).

**Why it works**: Traditional cost models have two fundamental problems:
1. **Miscalibrated constants**: The ratio of sequential I/O to random I/O cost
   varies 10x across hardware. Memory access patterns depend on cache behavior
   that simple models ignore. Thread contention, NUMA effects, and OS
   scheduling introduce variance.
2. **Missing interactions**: Cost of a hash join depends not just on input sizes
   but on data distribution, hash table fit in cache, concurrent queries, and
   pipeline scheduling. Analytical models cannot capture these interactions.

Learned models observe the actual execution environment and implicitly capture
hardware characteristics, data distribution effects, and runtime conditions.

**Key approaches:**
- **Bao (Bandit optimizer)**: Uses tree convolutional neural networks on plan
  trees. Learns to steer the optimizer toward better plan structures
  (hint sets) rather than predicting exact costs.
- **Neo**: End-to-end learned optimizer that replaces both cardinality estimation
  and cost modeling. Uses plan embeddings and DQN reinforcement learning.
- **Operator-level models**: Train separate models per operator type
  (hash join, merge join, seq scan). More interpretable and composable.
- **Query-level models**: Predict total query execution time from plan features.
  Simpler but less composable for plan comparison.

## Relational Algebra

```algebra
-- Traditional cost model:
Cost(HashJoin(R, S)) =
  seq_scan_cost(R) + build_hash_cost(|R|) +
  seq_scan_cost(S) + probe_cost(|S|) +
  output_cost(|R join S|)

-- Learned cost model:
Cost(HashJoin(R, S)) = model.predict(
  features = [
    |R|, |S|, |R join S|_est,   -- cardinalities
    width(R), width(S),          -- tuple widths
    build_cols, probe_cols,      -- key info
    concurrency, cache_size,     -- runtime context
    plan_tree_embedding,         -- structural features
  ]
)

-- Bao approach (plan-level steering):
-- Instead of predicting cost, predict relative quality
for hint_set in [default, hash_only, merge_only, ...]:
  plan = optimizer.plan_with_hints(query, hint_set)
  quality[hint_set] = bao_model.predict(plan)
best_plan = plans[argmax(quality)]
```

## Implementation

```rust
/// Learned cost model interface
pub trait LearnedCostModel {
    /// Predict execution cost for a physical plan node
    fn predict_cost(
        &self,
        plan: &PhysicalPlan,
        context: &ExecutionContext,
    ) -> CostPrediction;

    /// Train or update model with observed execution
    fn observe(
        &mut self,
        plan: &PhysicalPlan,
        actual_time_ns: u64,
        context: &ExecutionContext,
    );
}

/// Operator-level learned cost model
pub struct OperatorLevelModel {
    /// Separate model per operator type
    models: HashMap<OperatorType, OperatorModel>,
    /// Feature normalizer
    normalizer: FeatureNormalizer,
    /// Training buffer
    observations: Vec<Observation>,
}

struct OperatorModel {
    /// Gradient-boosted trees (fast inference)
    model: GradientBoostedTrees,
    /// Feature columns for this operator
    features: Vec<FeatureType>,
    /// Number of training samples seen
    num_samples: usize,
}

impl OperatorLevelModel {
    /// Extract features for a plan node
    fn extract_features(
        &self,
        plan: &PhysicalPlan,
        context: &ExecutionContext,
    ) -> Vec<f32> {
        let mut features = Vec::new();

        // Cardinality features
        features.push(
            (plan.estimated_rows as f64).log2() as f32,
        );
        features.push(
            (plan.estimated_output as f64).log2() as f32,
        );

        // Width features
        features.push(plan.tuple_width as f32);
        features.push(plan.num_columns as f32);

        // Operator-specific features
        match &plan.operator {
            PhysicalOp::HashJoin {
                build_size, probe_size, ..
            } => {
                features.push(
                    (*build_size as f64).log2() as f32,
                );
                features.push(
                    (*probe_size as f64).log2() as f32,
                );
                let ht_size = *build_size
                    * plan.tuple_width;
                let fits_l3 = (ht_size
                    < context.l3_cache_size) as u8;
                features.push(fits_l3 as f32);
            }
            PhysicalOp::SeqScan { pages, .. } => {
                features.push(*pages as f32);
                features.push(
                    context.sequential_io_cost as f32,
                );
            }
            PhysicalOp::IndexScan { selectivity, .. } => {
                features.push(*selectivity as f32);
                features.push(
                    context.random_io_cost as f32,
                );
            }
            PhysicalOp::Sort { input_size, .. } => {
                features.push(
                    (*input_size as f64).log2() as f32,
                );
                let fits_memory =
                    (*input_size * plan.tuple_width
                    < context.work_mem) as u8;
                features.push(fits_memory as f32);
            }
            _ => {}
        }

        // Runtime context features
        features.push(
            context.concurrent_queries as f32,
        );
        features.push(
            context.available_memory_mb as f32,
        );
        features.push(
            context.buffer_pool_hit_rate as f32,
        );

        self.normalizer.normalize(&mut features);
        features
    }
}

impl LearnedCostModel for OperatorLevelModel {
    fn predict_cost(
        &self,
        plan: &PhysicalPlan,
        context: &ExecutionContext,
    ) -> CostPrediction {
        let features = self.extract_features(
            plan, context,
        );
        let op_type = plan.operator.op_type();

        let prediction = match self.models.get(&op_type) {
            Some(model) if model.num_samples > 100 => {
                // Sufficient training data: use ML model
                let log_cost =
                    model.model.predict(&features);
                (10.0_f64).powf(log_cost as f64)
            }
            _ => {
                // Insufficient data: fall back to
                // analytical model
                traditional_cost(plan, context)
            }
        };

        CostPrediction {
            estimated_ns: prediction as u64,
            confidence: self.confidence(plan),
            method: if self.models.contains_key(&op_type) {
                PredictionMethod::Learned
            } else {
                PredictionMethod::Analytical
            },
        }
    }

    fn observe(
        &mut self,
        plan: &PhysicalPlan,
        actual_time_ns: u64,
        context: &ExecutionContext,
    ) {
        let features = self.extract_features(
            plan, context,
        );
        let label = (actual_time_ns as f64).log10() as f32;

        let op_type = plan.operator.op_type();
        let model = self.models
            .entry(op_type)
            .or_insert_with(|| OperatorModel::new(op_type));

        model.model.add_sample(&features, label);
        model.num_samples += 1;

        // Retrain periodically
        if model.num_samples % 100 == 0 {
            model.model.retrain();
        }
    }
}

/// Bao-style plan steering with hint sets
pub struct BaoOptimizer {
    /// Tree-CNN model for plan quality prediction
    model: TreeConvolutionalNetwork,
    /// Available hint sets
    hint_sets: Vec<HintSet>,
    /// Thompson sampling arms
    arms: Vec<ArmStatistics>,
}

impl BaoOptimizer {
    /// Select best hint set for a query
    pub fn select_plan(
        &self,
        query: &Query,
        optimizer: &Optimizer,
    ) -> PhysicalPlan {
        let mut best_plan = None;
        let mut best_score = f64::MIN;

        for (i, hints) in
            self.hint_sets.iter().enumerate()
        {
            // Generate plan with this hint set
            let plan = optimizer.plan_with_hints(
                query, hints,
            );

            // Predict quality using tree-CNN
            let plan_tree = encode_plan_tree(&plan);
            let score = self.model.predict(&plan_tree);

            // Thompson sampling for exploration
            let sample = self.arms[i].thompson_sample();
            let adjusted_score = score + sample * 0.1;

            if adjusted_score > best_score {
                best_score = adjusted_score;
                best_plan = Some(plan);
            }
        }

        best_plan.unwrap_or_else(|| {
            optimizer.plan(query)
        })
    }

    /// Update model after query execution
    pub fn observe_execution(
        &mut self,
        plan: &PhysicalPlan,
        hint_idx: usize,
        execution_time_ns: u64,
    ) {
        let plan_tree = encode_plan_tree(plan);
        let label =
            (execution_time_ns as f64).log10() as f32;
        self.model.train_step(&plan_tree, label);
        self.arms[hint_idx].update(execution_time_ns);
    }
}

/// Hint sets for Bao-style steering
pub struct HintSet {
    /// Force or disable specific operators
    force_hash_join: Option<bool>,
    force_merge_join: Option<bool>,
    force_nested_loop: Option<bool>,
    force_index_scan: Option<bool>,
    force_seq_scan: Option<bool>,
    /// Join order hints
    join_order: Option<Vec<TableRef>>,
}
```

**Restrictions:**
- Requires execution feedback (cold start problem)
- Training time: minutes to hours for initial model
- Model staleness: must retrain on data or hardware changes
- Inference overhead: 0.1-5ms per prediction
- Non-compositional: query-level models cannot be reused for subplans
- Debugging: harder to explain why one plan was chosen over another

## Cost Model

```rust
fn learned_model_tradeoff(
    num_training_queries: usize,
    avg_query_time_ms: f64,
    traditional_cost_error: f64,  // e.g., 5x
    learned_cost_error: f64,      // e.g., 1.5x
) -> TradeoffAnalysis {
    // Training cost
    let training_overhead_ms =
        num_training_queries as f64 * 10.0; // feature extraction

    // Per-query inference overhead
    let inference_ms = 1.0;

    // Plan quality improvement
    // Cost error translates to suboptimal plans
    let traditional_plan_overhead =
        traditional_cost_error.powf(0.5);
    let learned_plan_overhead =
        learned_cost_error.powf(0.5);

    let speedup_per_query = traditional_plan_overhead
        / learned_plan_overhead;

    let savings_per_query = avg_query_time_ms
        * (1.0 - 1.0 / speedup_per_query);

    TradeoffAnalysis {
        training_cost_ms: training_overhead_ms,
        inference_overhead_ms: inference_ms,
        savings_per_query_ms: savings_per_query,
        break_even_queries: (training_overhead_ms
            / savings_per_query) as usize,
    }
}
```

**Typical performance:**
- Training: 100-1000 queries to reach stable model
- Inference: 0.5-2ms per plan evaluation
- Cost prediction accuracy: median 1.5x error (vs. 5x for traditional)
- Plan quality improvement: 2-5x on complex workloads
- Bao: 50% median improvement on OLAP workloads

## Test Cases

### Positive: Hash join cache sensitivity

```sql
SELECT * FROM orders o JOIN customers c
ON o.customer_id = c.id;
-- Traditional: cost = build_cost + probe_cost (ignores cache)
-- Reality: customers table (1M rows) fits in L3 cache
--   Build once, probe is cache-local: actual 2x faster
-- Learned model: observes fast execution, predicts correctly
-- Better than traditional by capturing hardware-specific behavior
```

### Positive: Bao steering away from bad plans

```sql
-- Complex TPC-H Q8 with 8 tables
-- Default optimizer: merge join plan, 15 seconds
-- Bao tries hash-only hint set: 3 seconds
-- Bao tries index-only hint set: 25 seconds
-- Bao learns: hash-only is 5x better for this query shape
-- Subsequent similar queries: Bao steers to hash-only
```

### Positive: Adapting to cloud I/O variability

```sql
-- On-premise: random I/O = 5x sequential cost
-- Cloud EBS: random I/O = 20x sequential cost (variable)
-- Traditional model: calibrated for on-premise constants
-- Learned model: observes actual I/O latencies
-- Correctly prefers sequential scans over index scans on EBS
```

### Negative: Cold start on new workload

```sql
-- New application deployed, no training data
-- Learned model must fall back to traditional cost model
-- First 100 queries: no benefit from ML
-- Risk: worse than traditional if model predicts randomly
-- Mitigation: hybrid approach with analytical fallback
```

### Negative: Workload shift

```sql
-- Model trained on OLTP workload (short point queries)
-- New batch job: large analytical queries
-- Learned model extrapolates poorly to unseen plan shapes
-- Traditional model with correct formulas may be better
-- Solution: detect distribution shift, retrain or fall back
```

### Negative: Non-reproducible execution times

```sql
-- Query runs 5x: times = [100ms, 500ms, 120ms, 800ms, 90ms]
-- High variance from concurrent load, GC pauses, I/O
-- Learned model receives noisy labels
-- Cannot converge to accurate predictions
-- Solution: use median of multiple executions, control for concurrency
```

## References

**Academic papers:**
- Marcus, Negi, et al., "Neo: A Learned Query Optimizer", VLDB 2019
- Marcus et al., "Bao: Making Learned Query Optimization Practical", SIGMOD 2021
- Dutt et al., "Selectivity Estimation for Range Predicates using Lightweight Models", VLDB 2019
- Sun, Li, "An End-to-End Learning-based Cost Estimator", VLDB 2019
- Hilprecht, Binnig, "One Model to Rule them All: Towards Zero-Shot Learning for Databases", CIDR 2022
- Negi et al., "Steering Query Optimizers: A Practical Take on Big Data Workloads", SIGMOD 2021

**Implementation:**
- Bao: Open-source learned query optimizer (PostgreSQL)
- NoisePage (CMU): Self-driving database with learned components
- PostgreSQL: pg_hint_plan for manual steering
- Spark AQE: Adaptive query execution with runtime statistics
