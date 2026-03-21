# Rule: Learned Join Ordering (Neo/Bao)

**Category:** experimental/ml-guided
**File:** `rules/experimental/ml-guided/learned-join-ordering.rra`

## Metadata

- **ID:** `learned-join-ordering`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb
- **Tags:** ml, join-ordering, reinforcement-learning, deep-learning, bao
- **Authors:** "Marcus et al. 2019", "Marcus et al. 2021", "RA Contributors"


# Learned Join Ordering (Neo/Bao)

## Description

Replaces traditional dynamic programming (System R) or heuristic join
ordering with a neural network that predicts the optimal join order
directly. Neo uses deep reinforcement learning to learn a value function
over partial join plans. Bao (Bandit optimizer) learns to select among
multiple optimizer hints (including join orders) using a tree-structured
model trained on execution feedback.

**When to apply**: Queries with 5+ joins where traditional cost-based
optimization produces suboptimal plans due to cardinality estimation
errors. Particularly effective for recurring query templates where the
model can learn from execution history.

**Why it works**: Traditional optimizers rely on cardinality estimates
that are often wrong by 10-100x for complex joins. Neural models learn
the relationship between query structure and actual execution cost
directly from observed executions, bypassing the error-prone estimation
pipeline entirely.

## Relational Algebra

```algebra
-- Traditional: DP-based join ordering using cost model
join_order_dp(R, S, T, U, V) using cost_model(cardinality_estimator)

-- Learned: Neural network predicts join order
join_order_learned(R, S, T, U, V) using neo_model(query_encoding)

-- Bao: Select among optimizer hints
bao_select(
  hint_1: /*+ HashJoin(R S) NestLoop(T U) */ join(R,S,T,U,V),
  hint_2: /*+ MergeJoin(R S) HashJoin(T U) */ join(R,S,T,U,V),
  hint_3: default_plan(R,S,T,U,V)
) using bandit_model(plan_tree_encoding)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Neo: Reinforcement learning for join ordering
rw!("neo-join-ordering";
    "(join ?p1 (join ?p2 ?r1 ?r2) ?r3)" =>
    "(neo_join_order
       (tables ?r1 ?r2 ?r3)
       (predicates (merge_preds ?p1 ?p2))
       (model neo_v2))"
    if neo_model_available()
    if join_count_ge_3()
),

// Bao: Bandit-based hint selection
rw!("bao-hint-selection";
    "(join ?p1 (join ?p2 ?r1 ?r2) ?r3)" =>
    "(bao_select
       (candidates
         (with_hints (force_hash_join) (join ?p1 (join ?p2 ?r1 ?r2) ?r3))
         (with_hints (force_merge_join) (join ?p1 (join ?p2 ?r1 ?r2) ?r3))
         (with_hints (force_nl_join) (join ?p1 (join ?p2 ?r1 ?r2) ?r3))
         (default (join ?p1 (join ?p2 ?r1 ?r2) ?r3)))
       (model bao_thompson_sampling))"
    if bao_model_available()
),

// Neo model architecture
struct NeoModel {
    // Query encoding: tree-structured LSTM
    tree_lstm: TreeLSTM,
    // Value network: predicts execution time
    value_network: FeedForward,
    // Experience replay buffer
    replay_buffer: Vec<(QueryEncoding, f64)>,
}

impl NeoModel {
    fn predict_join_order(
        &self,
        tables: &[TableId],
        predicates: &[JoinPredicate],
    ) -> JoinOrder {
        // Beam search over possible join orderings
        let mut beam: Vec<PartialPlan> = tables.iter()
            .map(|t| PartialPlan::single(*t))
            .collect();

        while beam[0].remaining_tables() > 0 {
            let mut candidates = Vec::new();

            for partial in &beam {
                for next_table in partial.remaining_tables() {
                    let extended = partial.extend(
                        next_table, predicates,
                    );
                    let encoding =
                        self.tree_lstm.encode(&extended);
                    let value =
                        self.value_network.predict(&encoding);
                    candidates.push((extended, value));
                }
            }

            // Keep top-k by predicted value
            candidates.sort_by(|a, b|
                a.1.partial_cmp(&b.1).unwrap()
            );
            beam = candidates.into_iter()
                .take(5)
                .map(|(plan, _)| plan)
                .collect();
        }

        beam.into_iter().next().unwrap().to_join_order()
    }

    fn train(
        &mut self,
        query: &QueryEncoding,
        actual_time: f64,
    ) {
        self.replay_buffer.push((query.clone(), actual_time));

        if self.replay_buffer.len() >= 64 {
            // Mini-batch training
            let batch: Vec<_> = self.replay_buffer
                .choose_multiple(&mut rng(), 32)
                .cloned()
                .collect();

            for (enc, target) in &batch {
                let pred = self.value_network.predict(
                    &self.tree_lstm.encode_tree(enc),
                );
                let loss = (pred - target).powi(2);
                self.backprop(loss);
            }
        }
    }
}

// Bao model
struct BaoModel {
    // Tree convolution model for plan encoding
    tree_conv: TreeConvolution,
    // Thompson sampling for arm selection
    arm_stats: Vec<BetaDistribution>,
}

impl BaoModel {
    fn select_hint(
        &self,
        candidate_plans: &[PlanTree],
    ) -> usize {
        // Encode each plan using tree convolution
        let plan_values: Vec<f64> = candidate_plans.iter()
            .map(|plan| {
                let encoding = self.tree_conv.encode(plan);
                self.predict_latency(&encoding)
            })
            .collect();

        // Thompson sampling: sample from posterior
        let samples: Vec<f64> = self.arm_stats.iter()
            .zip(plan_values.iter())
            .map(|(beta, pred)| beta.sample() * pred)
            .collect();

        // Select arm with lowest sampled cost
        samples.iter()
            .enumerate()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0
    }
}
```

## Preconditions

```rust
fn applicable(query: &RelExpr) -> bool {
    // Model must be trained and available
    if !model_registry().has_model("neo") &&
       !model_registry().has_model("bao")
    {
        return false;
    }

    // Enough joins to benefit
    let join_count = count_joins(query);
    if join_count < 3 {
        return false;
    }

    // Query should be similar to training distribution
    let coverage = model_registry().coverage_score(query);
    coverage > 0.5
}
```

**Restrictions:**
- Requires significant training data (1000+ executed queries for Neo, 100+ for Bao)
- Model may not generalize to unseen query patterns
- Inference latency: 5-50ms (acceptable for OLAP, marginal for OLTP)
- Periodic retraining needed as data/workload evolve
- Neo requires GPU for training; Bao is CPU-friendly

## Cost Model

```rust
fn estimated_benefit(
    traditional_plan_cost: f64,
    learned_plan_cost: f64,
    inference_latency_ms: f64,
) -> f64 {
    if traditional_plan_cost > learned_plan_cost + inference_latency_ms {
        (traditional_plan_cost - learned_plan_cost)
            / traditional_plan_cost
    } else {
        0.0
    }
}
```

**Typical benefit**: Bao: 2-5x median improvement on challenging
workloads (where traditional optimizer makes errors). Neo: up to 10x
on complex join queries. Both degrade gracefully to traditional plans
for simple queries.

## Test Cases

### Positive: Complex star-snowflake query

```sql
SELECT d1.name, d2.category, SUM(f.amount)
FROM fact f
JOIN dim1 d1 ON f.k1 = d1.id
JOIN dim2 d2 ON f.k2 = d2.id
JOIN dim3 d3 ON f.k3 = d3.id
JOIN dim4 d4 ON f.k4 = d4.id
JOIN dim5 d5 ON d3.parent = d5.id
WHERE d1.region = 'US' AND d4.year = 2024
GROUP BY d1.name, d2.category;

-- Traditional: may pick wrong dim table to join first
-- Neo: learns that filtering d1 by region first reduces intermediate
-- Bao: selects hint set that forces d1 filter pushdown
```

### Positive: Graph query with varying topology

```sql
SELECT p1.name, p2.name
FROM person p1
JOIN knows k1 ON p1.id = k1.person1
JOIN knows k2 ON k1.person2 = k2.person1
JOIN person p2 ON k2.person2 = p2.id
WHERE p1.city = 'Seattle' AND p2.city = 'Portland';

-- Topology varies: some cities have dense connections
-- Learned model adapts join order based on city connectivity
```

### Negative: Simple 2-way equijoin

```sql
SELECT * FROM users u JOIN orders o ON u.id = o.user_id;
-- Traditional optimizer handles this well, ML overhead not justified
```

## References

**Academic papers:**
- Marcus, Papaemmanouil, "Towards a Hands-Free Query Optimizer through Deep Learning", CIDR 2019 (Neo)
- Marcus et al., "Bao: Making Learned Query Optimization Practical", SIGMOD 2021
- Yang et al., "Balsa: Learning a Query Optimizer Without Expert Demonstrations", SIGMOD 2022

**Implementation:**
- Bao: open-source, integrated with PostgreSQL (https://github.com/learnedsystems/BaoForPostgreSQL)
- Neo: research prototype
- Balsa: training without expert demonstrations

**Key insights:**
- Bao is practical: 100 queries to train, CPU-only inference, safe fallback
- Neo is more powerful but requires more training data and GPU
- Both use plan execution time as training signal (not cardinality)
- Bao's Thompson sampling naturally explores new plan strategies
- Balsa shows learned optimizers can bootstrap without expert plans
