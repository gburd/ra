# Rule: "Reinforcement Learning for Join Ordering"

**Category:** experimental/ml-guided
**File:** `rules/experimental/ml-guided/rl-join-ordering.rra`

## Metadata

- **ID:** `rl-join-ordering`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb
- **Tags:** ml, reinforcement-learning, join-ordering, dqn, policy-gradient
- **Authors:** "RA Contributors"


# Reinforcement Learning for Join Ordering

## Description

Uses reinforcement learning (RL) to learn join ordering policies that
minimize query execution time. Join ordering is NP-hard for general queries
and exponential for bushy plans. Traditional approaches (dynamic programming,
greedy heuristics) struggle with large join graphs (10+ tables) and
unreliable cardinality estimates.

RL formulates join ordering as a sequential decision process: at each step,
the agent selects two relations to join, observing the current state (set of
available relations with statistics) and receiving a reward based on the
resulting plan quality. The agent learns a policy that generalizes across
query shapes.

Key RL approaches:
1. **DQ (Deep Q-learning)**: Learns Q-values for join actions. Explores
   via epsilon-greedy. Works with left-deep plans.
2. **RTOS (RL Tree Optimization with Steering)**: Policy gradient method
   that produces bushy plans. Can steer toward specific plan shapes.
3. **ReJOIN**: DQN-based join ordering with plan execution feedback.

**When to apply**: Large join queries (8+ tables) where dynamic programming
is too expensive and greedy heuristics produce poor plans. Also useful when
cardinality estimates are unreliable and execution feedback is available.

**Why it works**: RL agents learn from actual execution costs rather than
relying on potentially inaccurate cost models. The learned policy can
capture non-obvious patterns (e.g., certain join orders consistently
perform well regardless of estimated cardinality) and generalize across
similar query shapes.

**Research status**: Research prototypes demonstrate improvement over
greedy heuristics for complex queries. Not yet production-ready due to
training instability, sample efficiency, and cold-start issues.

## Relational Algebra

```algebra
State: S = {available_relations, join_graph, statistics}
Action: a = (rel_i, rel_j) -- join two relations
Reward: r = -actual_execution_time (or -estimated_cost)
Transition: S' = S \ {rel_i, rel_j} + {rel_i join rel_j}

Episode: sequence of n-1 join decisions for n tables
Policy: pi(a|S) -> probability of choosing action a in state S

Optimization:
  max_pi E[sum of rewards] = max_pi E[-total_execution_time]

Left-deep plans: actions are (next_table_to_join)
Bushy plans: actions are (any pair of available relations)
```

## Implementation

```rust
use std::collections::HashMap;

// State representation for RL agent
struct JoinOrderState {
    available_relations: Vec<RelationInfo>,
    join_graph: JoinGraph,
    joined_so_far: Vec<JoinStep>,
}

struct RelationInfo {
    name: String,
    estimated_rows: f64,
    estimated_width: u32,
    available_indexes: Vec<String>,
}

struct JoinStep {
    left: usize,
    right: usize,
    estimated_output_rows: f64,
}

// Deep Q-Network for join ordering
struct DQNJoinOrderer {
    q_network: NeuralNetwork,
    target_network: NeuralNetwork,
    replay_buffer: ReplayBuffer,
    epsilon: f64,
    gamma: f64,
    learning_rate: f64,
}

impl DQNJoinOrderer {
    fn select_action(
        &self,
        state: &JoinOrderState,
    ) -> (usize, usize) {
        let available = &state.available_relations;
        if available.len() <= 1 {
            panic!("No joins possible with <= 1 relation");
        }

        // Epsilon-greedy exploration
        if rand::random::<f64>() < self.epsilon {
            // Random valid action
            let i = rand::random::<usize>() % available.len();
            let j = loop {
                let j = rand::random::<usize>() % available.len();
                if j != i && state.join_graph.connected(i, j) {
                    break j;
                }
            };
            return (i, j);
        }

        // Greedy: pick action with highest Q-value
        let state_features = self.featurize_state(state);
        let mut best_action = (0, 1);
        let mut best_q = f64::NEG_INFINITY;

        for i in 0..available.len() {
            for j in (i + 1)..available.len() {
                if !state.join_graph.connected(i, j) {
                    continue;
                }
                let action_features = self.featurize_action(
                    state, i, j,
                );
                let features = concat(
                    &state_features,
                    &action_features,
                );
                let q_value =
                    self.q_network.predict(&features)[0];

                if q_value > best_q {
                    best_q = q_value;
                    best_action = (i, j);
                }
            }
        }

        best_action
    }

    fn featurize_state(
        &self,
        state: &JoinOrderState,
    ) -> Vec<f64> {
        let mut features = Vec::new();

        // Number of remaining relations
        features.push(state.available_relations.len() as f64);

        // Statistics of available relations
        let total_rows: f64 = state
            .available_relations
            .iter()
            .map(|r| r.estimated_rows)
            .sum();
        features.push(total_rows.ln());

        let min_rows = state
            .available_relations
            .iter()
            .map(|r| r.estimated_rows)
            .fold(f64::INFINITY, f64::min);
        features.push(min_rows.ln());

        let max_rows = state
            .available_relations
            .iter()
            .map(|r| r.estimated_rows)
            .fold(0.0_f64, f64::max);
        features.push(max_rows.ln());

        // Join graph density
        let edges = state.join_graph.edge_count();
        let n = state.available_relations.len();
        let max_edges = n * (n - 1) / 2;
        features.push(edges as f64 / max_edges.max(1) as f64);

        // History: intermediate result sizes
        for step in &state.joined_so_far {
            features.push(step.estimated_output_rows.ln());
        }

        features
    }

    fn featurize_action(
        &self,
        state: &JoinOrderState,
        i: usize,
        j: usize,
    ) -> Vec<f64> {
        let left = &state.available_relations[i];
        let right = &state.available_relations[j];

        vec![
            left.estimated_rows.ln(),
            right.estimated_rows.ln(),
            (left.estimated_rows / right.estimated_rows).ln(),
            left.estimated_width as f64,
            right.estimated_width as f64,
        ]
    }

    fn train_step(&mut self) {
        let batch = self.replay_buffer.sample(64);

        for experience in &batch {
            let current_q = self.q_network.predict(
                &experience.state_action_features,
            )[0];

            let target_q = if experience.is_terminal {
                experience.reward
            } else {
                let next_max_q =
                    self.max_q_value(&experience.next_state);
                experience.reward + self.gamma * next_max_q
            };

            let loss = (current_q - target_q).powi(2);
            self.q_network.backpropagate(
                &experience.state_action_features,
                target_q,
                self.learning_rate,
            );
        }
    }

    fn generate_join_order(
        &self,
        query: &Query,
    ) -> Vec<JoinStep> {
        let mut state = JoinOrderState::from_query(query);
        let mut steps = Vec::new();

        while state.available_relations.len() > 1 {
            let (i, j) = self.select_action(&state);

            let left = state.available_relations[i].clone();
            let right = state.available_relations[j].clone();
            let output_rows = estimate_join_output(
                &left, &right, &state.join_graph,
            );

            steps.push(JoinStep {
                left: i,
                right: j,
                estimated_output_rows: output_rows,
            });

            // Transition to next state
            state.apply_join(i, j, output_rows);
        }

        steps
    }
}

// Policy gradient approach (RTOS-style)
struct PolicyGradientJoinOrderer {
    policy_network: NeuralNetwork,
    baseline: ExponentialMovingAverage,
}

impl PolicyGradientJoinOrderer {
    fn sample_join_order(
        &self,
        query: &Query,
    ) -> (Vec<JoinStep>, f64) {
        let mut state = JoinOrderState::from_query(query);
        let mut steps = Vec::new();
        let mut log_prob_sum = 0.0;

        while state.available_relations.len() > 1 {
            let features = self.featurize(&state);
            let action_probs =
                self.policy_network.forward_softmax(&features);

            // Sample action from probability distribution
            let action_idx = sample_categorical(&action_probs);
            let (i, j) = decode_action(
                action_idx,
                state.available_relations.len(),
            );

            log_prob_sum += action_probs[action_idx].ln();

            let output_rows = estimate_join_output(
                &state.available_relations[i],
                &state.available_relations[j],
                &state.join_graph,
            );

            steps.push(JoinStep {
                left: i,
                right: j,
                estimated_output_rows: output_rows,
            });

            state.apply_join(i, j, output_rows);
        }

        (steps, log_prob_sum)
    }

    fn update(
        &mut self,
        log_prob: f64,
        reward: f64,
    ) {
        let advantage = reward - self.baseline.value();
        self.baseline.update(reward);

        // REINFORCE gradient: grad(log_prob * advantage)
        let gradient = log_prob * advantage;
        self.policy_network.apply_gradient(gradient);
    }
}

struct ReplayBuffer {
    experiences: Vec<Experience>,
    capacity: usize,
}

struct Experience {
    state_action_features: Vec<f64>,
    reward: f64,
    next_state: JoinOrderState,
    is_terminal: bool,
}
```

**Restrictions:**
- Training requires many episodes (1000-10000+ per query template)
- Sample efficiency is low: each episode needs a full plan execution
- Exploration can produce very bad plans during training
- State representation may not capture all relevant information
- Generalization across query shapes is limited
- Bushy plan space is much larger than left-deep

## Cost Model

```rust
fn rl_join_ordering_benefit(
    rl_orders: &[(Query, Vec<JoinStep>, f64)],
    dp_orders: &[(Query, Vec<JoinStep>, f64)],
    greedy_orders: &[(Query, Vec<JoinStep>, f64)],
) -> RLBenefitMetrics {
    let mut rl_vs_dp_ratios = Vec::new();
    let mut rl_vs_greedy_ratios = Vec::new();

    for i in 0..rl_orders.len() {
        let rl_cost = rl_orders[i].2;
        let dp_cost = dp_orders[i].2;
        let greedy_cost = greedy_orders[i].2;

        rl_vs_dp_ratios.push(rl_cost / dp_cost);
        rl_vs_greedy_ratios.push(rl_cost / greedy_cost);
    }

    RLBenefitMetrics {
        median_vs_dp: median(&mut rl_vs_dp_ratios),
        median_vs_greedy: median(&mut rl_vs_greedy_ratios),
    }
}
```

**Typical benefit**: 20-80% improvement over greedy heuristics for 8+
table joins. Within 10% of optimal DP solution for most queries after
sufficient training.

## Test Cases

### Test 1: Star schema join ordering (8 tables)

```sql
SELECT * FROM fact f
JOIN dim1 d1 ON f.d1_id = d1.id
JOIN dim2 d2 ON f.d2_id = d2.id
-- ... (6 more dimension joins)
WHERE d1.category = 'A' AND d3.region = 'US';

-- Greedy (largest first): starts with fact table, bad intermediate sizes
-- DP: optimal but explores 2^8 * 8! / 2 subsets
-- RL: learns star-schema pattern, joins selective dimensions first
-- RL order: d1 (filtered) -> fact -> d3 (filtered) -> d2 -> ...
-- 3x faster than greedy, matches DP quality
```

### Test 2: Chain query (10 tables)

```sql
SELECT * FROM t1 JOIN t2 ON ... JOIN t3 ON ... -- 10-table chain
WHERE t5.x > 100;

-- DP: infeasible (10! orderings for left-deep alone)
-- Greedy: may start from wrong end of chain
-- RL: learns to start from t5 (filtered), extend both directions
-- Exploits chain structure
```

### Test 3: Training convergence

```sql
-- JOB query 12a (3 tables):
-- Episode 1-100: RL explores, avg cost 5x optimal
-- Episode 100-500: RL improving, avg cost 2x optimal
-- Episode 500-2000: RL converges, avg cost 1.1x optimal
-- Episode 2000+: stable, occasionally finds plans DP misses
--   (when cardinality estimates are wrong)
```

### Test 4: Generalization failure

```sql
-- Trained on TPC-H star schema queries
-- Tested on TPC-DS snowflake schema
-- RL: 40% worse than greedy (out-of-distribution)
-- Needs retraining or meta-learning for new schemas
```

## References

**RL for join ordering:**
- Marcus & Papaemmanouil, "Towards a Hands-Free Query Optimizer through Deep Learning", CIDR 2019
- Krishnan et al., "Learning to Optimize Join Queries With Deep Reinforcement Learning", arXiv 2018
- Yu et al., "Reinforcement Learning with Tree-LSTM for Join Order Selection", ICDE 2020

**DQ and ReJOIN:**
- Marcus et al., "Neo: A Learned Query Optimizer", VLDB 2019
- Marcus & Papaemmanouil, "Plan-Structured Deep Neural Network Models for Query Performance Prediction", VLDB 2019

**Policy gradient approaches:**
- Trummer et al., "SkinnerDB: Regret-Bounded Query Processing", SIGMOD 2019
  (adaptive execution with RL-style exploration)

**Comparative evaluations:**
- Leis et al., "How Good Are Query Optimizers, Really?", VLDB 2015
- Marcus et al., "Bao: Making Learned Query Optimization Practical", SIGMOD 2021
