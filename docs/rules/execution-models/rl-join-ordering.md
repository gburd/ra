# Rule: Reinforcement Learning for Join Ordering

**Category:** execution-models/experimental
**File:** `rules/execution-models/experimental/rl-join-ordering.rra`

## Metadata

- **ID:** `rl-join-ordering`
- **Version:** "1.0.0"
- **Databases:** postgresql, spark, trino, noisepage
- **Tags:** execution, experimental, research, reinforcement-learning, join-ordering, dqn, optimization
- **Authors:** Ryan Marcus, Sanjay Krishnan, Parimarjan Negi


# Reinforcement Learning for Join Ordering

## Description

Reinforcement learning (RL) for join ordering treats query optimization as a
sequential decision problem: the agent (optimizer) observes the current state
of a partially constructed join plan, selects the next table to join, and
receives a reward based on the resulting execution time. Over many queries, the
agent learns a policy that produces near-optimal join orders without requiring
accurate cardinality estimates or cost models.

**When to apply**: Complex multi-table joins (5+ tables) where traditional
dynamic programming (DP) is too expensive (exponential in the number of tables)
or where cardinality estimation errors cause DP to select poor join orders.
RL-based approaches scale better than DP and can learn from execution feedback.

**Why it works**: Traditional join ordering has two failure modes:
1. **Exponential search space**: DP is O(3^n) for n tables, infeasible for
   n > 15-20. Greedy heuristics miss good plans.
2. **Cardinality estimation errors**: Even optimal DP produces bad plans when
   input cardinality estimates are wrong (common for multi-table joins).

RL addresses both: the agent learns a policy mapping query features to join
decisions in O(n^2) inference time (not O(3^n)), and it learns from actual
execution costs, bypassing cardinality estimation entirely.

**Key approaches:**
- **DQN (Deep Q-Network)**: Train a neural network to estimate Q(state, action)
  = expected future cost. State = current partial join plan. Action = next table
  to join. Use epsilon-greedy exploration.
- **PPO (Proximal Policy Optimization)**: Directly learn a policy network
  pi(action|state) that maps states to action probabilities. More stable
  training than DQN.
- **ReJOIN**: Left-deep join tree construction using DQN. State encodes joined
  tables and intermediate cardinalities. Action selects next table.
- **SkinnerDB**: Adaptive RL during query execution. Interleaves exploration
  and exploitation across tuples. Provides worst-case regret bounds.
- **Bao**: Steers the existing optimizer via hint sets rather than replacing it.
  Uses Thompson sampling over tree-structured plan representations.

**State representation:**
- **Query graph**: Nodes = tables, edges = join predicates with estimated
  cardinalities. Graph neural network encodes structure.
- **Join sequence**: List of tables joined so far + intermediate cardinalities.
  Variable-length encoding via LSTM or set transformer.
- **Plan tree**: Tree-structured plan representation processed by tree-CNN.

## Relational Algebra

```algebra
-- Traditional DP join ordering:
JoinOrder(R1, R2, ..., Rn) = min over all permutations:
  cost(build_left_deep_plan(permutation))
  -- O(n!) permutations, DP reduces to O(3^n)
  -- Requires accurate cardinality estimates for cost()

-- RL join ordering:
State_0 = {R1, R2, ..., Rn}  -- all tables unjoined
for step in 1..n:
  action = RL_policy(State_{step-1})  -- select next table
  State_step = join(State_{step-1}, action)
Plan = sequence of actions

-- Training:
for episode in 1..N_episodes:
  plan = RL_policy.generate_plan(query)
  execution_time = execute(plan)
  reward = -log(execution_time)  -- negative log cost
  RL_policy.update(reward)
```

## Implementation

```rust
/// RL-based join order optimizer
pub struct RLJoinOptimizer {
    /// Policy network (state -> action probabilities)
    policy: PolicyNetwork,
    /// Value network (state -> expected cost)
    value: ValueNetwork,
    /// Replay buffer for experience replay
    replay_buffer: ReplayBuffer,
    /// Training configuration
    config: RLConfig,
}

pub struct RLConfig {
    /// Discount factor for future rewards
    gamma: f64,
    /// Learning rate
    lr: f64,
    /// Exploration rate (epsilon-greedy)
    epsilon: f64,
    /// Minimum epsilon after decay
    epsilon_min: f64,
    /// Replay buffer capacity
    buffer_size: usize,
    /// Batch size for training
    batch_size: usize,
}

/// State: representation of partial join plan
pub struct JoinState {
    /// Set of tables joined so far
    joined_tables: Vec<TableRef>,
    /// Current intermediate result cardinality
    intermediate_cardinality: u64,
    /// Tables not yet joined
    remaining_tables: Vec<TableRef>,
    /// Query graph features
    graph_features: QueryGraphEncoding,
}

impl JoinState {
    /// Encode state as feature vector for neural network
    pub fn to_features(&self) -> Vec<f32> {
        let mut features = Vec::new();

        // Binary vector: which tables are joined
        for table in &self.graph_features.all_tables {
            let joined = self.joined_tables
                .contains(table) as u8;
            features.push(joined as f32);
        }

        // Log intermediate cardinality
        features.push(
            (self.intermediate_cardinality as f64)
                .log2() as f32,
        );

        // Join predicate features
        for edge in &self.graph_features.edges {
            let left_joined = self.joined_tables
                .contains(&edge.left_table);
            let right_joined = self.joined_tables
                .contains(&edge.right_table);
            features.push(left_joined as u8 as f32);
            features.push(right_joined as u8 as f32);
            features.push(
                edge.estimated_selectivity as f32,
            );
        }

        // Table cardinalities
        for table in &self.remaining_tables {
            features.push(
                (table.estimated_rows as f64)
                    .log2() as f32,
            );
        }

        features
    }
}

impl RLJoinOptimizer {
    /// Generate join order for a query
    pub fn optimize(
        &self,
        query: &QueryGraph,
    ) -> JoinOrder {
        let mut state = JoinState::initial(query);
        let mut order = Vec::new();

        while !state.remaining_tables.is_empty() {
            let action = if rand::random::<f64>()
                < self.config.epsilon
            {
                // Explore: random action
                let idx = rand::random::<usize>()
                    % state.remaining_tables.len();
                idx
            } else {
                // Exploit: use policy network
                let features = state.to_features();
                let q_values =
                    self.policy.forward(&features);

                // Mask invalid actions (already joined)
                let valid_actions: Vec<(usize, f32)> =
                    q_values.iter().enumerate()
                        .filter(|(i, _)| {
                            *i < state.remaining_tables.len()
                        })
                        .map(|(i, &q)| (i, q))
                        .collect();

                valid_actions.iter()
                    .max_by(|a, b| {
                        a.1.partial_cmp(&b.1).unwrap()
                    })
                    .map(|(i, _)| *i)
                    .unwrap_or(0)
            };

            let table = state.remaining_tables
                .remove(action);
            order.push(table.clone());

            // Update state
            state.joined_tables.push(table);
            state.intermediate_cardinality =
                estimate_join_cardinality(&state);
        }

        JoinOrder { tables: order }
    }

    /// Train on execution feedback
    pub fn train_on_feedback(
        &mut self,
        query: &QueryGraph,
        join_order: &JoinOrder,
        execution_time_ns: u64,
    ) {
        // Compute reward (negative log execution time)
        let reward = -(execution_time_ns as f64).log10();

        // Reconstruct state-action trajectory
        let trajectory = self.reconstruct_trajectory(
            query, join_order,
        );

        // Store in replay buffer
        for (state, action, next_state) in &trajectory {
            self.replay_buffer.push(Experience {
                state: state.to_features(),
                action: *action,
                reward: if next_state.remaining_tables
                    .is_empty()
                {
                    reward
                } else {
                    0.0 // intermediate step
                },
                next_state: next_state.to_features(),
                done: next_state.remaining_tables
                    .is_empty(),
            });
        }

        // Train on batch from replay buffer
        if self.replay_buffer.len()
            >= self.config.batch_size
        {
            let batch = self.replay_buffer
                .sample(self.config.batch_size);
            self.train_batch(&batch);
        }
    }

    /// DQN training step
    fn train_batch(&mut self, batch: &[Experience]) {
        for exp in batch {
            // Target Q-value: r + gamma * max_a' Q(s', a')
            let target = if exp.done {
                exp.reward
            } else {
                let next_q =
                    self.value.forward(&exp.next_state);
                let max_q = next_q.iter()
                    .cloned()
                    .fold(f32::MIN, f32::max);
                exp.reward
                    + self.config.gamma * max_q as f64
            };

            // Update Q-network toward target
            self.policy.train_step(
                &exp.state,
                exp.action,
                target as f32,
                self.config.lr as f32,
            );
        }
    }
}

/// SkinnerDB-style adaptive RL during execution
pub struct AdaptiveRLExecutor {
    /// Multi-armed bandit over join orderings
    bandit: UCBBandit,
    /// Timeout per join ordering attempt (tuples)
    budget_per_arm: usize,
    /// Best ordering found so far
    best_order: Option<JoinOrder>,
    best_throughput: f64,
}

impl AdaptiveRLExecutor {
    /// Execute with adaptive join order exploration
    pub fn execute_adaptive(
        &mut self,
        query: &QueryGraph,
        tables: &[Table],
    ) -> Vec<Row> {
        let mut results = Vec::new();
        let orderings = generate_candidate_orderings(
            query,
        );

        for round in 0.. {
            // UCB selects which ordering to try
            let arm = self.bandit.select_arm();
            let order = &orderings[arm];

            // Execute for a budget of tuples
            let start = std::time::Instant::now();
            let new_results = execute_with_order(
                order, tables, self.budget_per_arm,
            );
            let elapsed = start.elapsed().as_nanos();

            let throughput = new_results.len() as f64
                / elapsed as f64;
            self.bandit.update(arm, throughput);

            results.extend(new_results);

            if results.len()
                >= query.expected_output_size()
            {
                break;
            }

            // Double budget for best arm (exploit)
            if throughput > self.best_throughput {
                self.best_throughput = throughput;
                self.best_order = Some(order.clone());
                self.budget_per_arm *= 2;
            }
        }

        results
    }
}
```

**Restrictions:**
- Requires training data (cold start: first queries use heuristics)
- Training time: hours for DQN convergence on diverse workloads
- Inference overhead: 1-10ms per query for policy evaluation
- Generalization: may fail on query shapes not seen during training
- Left-deep only: most RL approaches only consider left-deep trees
- Non-deterministic: different runs may produce different plans
- SkinnerDB overhead: exploration wastes work on bad orderings

## Cost Model

```rust
fn rl_join_ordering_cost(
    num_tables: usize,
    num_training_queries: usize,
    inference_ms: f64,
) -> RLCostAnalysis {
    // Traditional DP cost
    let dp_cost_ms = (3.0_f64)
        .powi(num_tables as i32) * 0.001;
    let dp_feasible = num_tables <= 15;

    // Greedy cost
    let greedy_cost_ms = num_tables as f64
        * num_tables as f64 * 0.001;
    let greedy_quality = 0.7; // 70% of optimal

    // RL inference cost
    let rl_quality = 0.9 + 0.08
        * (1.0 - (-0.01 * num_training_queries as f64)
            .exp());

    RLCostAnalysis {
        dp_cost_ms,
        dp_feasible,
        greedy_cost_ms,
        greedy_plan_quality: greedy_quality,
        rl_inference_ms: inference_ms,
        rl_plan_quality: rl_quality,
        rl_training_queries: num_training_queries,
    }
}
```

**Typical performance:**
- Inference: 1-10ms per query (constant regardless of table count)
- Training: 1000-10000 queries for stable policy
- Plan quality: within 5% of DP optimal for seen query shapes
- Scaling: handles 20+ tables (where DP is infeasible)
- SkinnerDB: worst-case regret O(sqrt(T)) vs. optimal ordering

## Test Cases

### Positive: Complex star schema join (many tables)

```sql
SELECT d1.name, d2.category, d3.region,
       SUM(f.amount)
FROM fact f
JOIN dim1 d1 ON f.d1_id = d1.id
JOIN dim2 d2 ON f.d2_id = d2.id
JOIN dim3 d3 ON f.d3_id = d3.id
JOIN dim4 d4 ON f.d4_id = d4.id
JOIN dim5 d5 ON f.d5_id = d5.id
JOIN dim6 d6 ON f.d6_id = d6.id
JOIN dim7 d7 ON f.d7_id = d7.id
GROUP BY d1.name, d2.category, d3.region;
-- 8 tables: DP cost = 3^8 = 6561 plans to evaluate
-- RL: single forward pass through policy network (~2ms)
-- RL learns: join small dims first, fact table last
-- Plan quality: within 3% of DP optimal
```

### Positive: Workload with correlated predicates

```sql
-- Query pattern: always filters on status='active', city varies
SELECT * FROM orders o
JOIN customers c ON o.cust_id = c.id
JOIN products p ON o.prod_id = p.id
JOIN suppliers s ON p.supplier_id = s.id
WHERE c.city = 'NYC' AND o.status = 'active';
-- DP with wrong cardinality: joins customers first (10x wrong)
-- RL learns from execution: orders first (indexed on status)
-- After 50 similar queries: RL consistently outperforms DP
-- 5-10x faster plans than DP with bad cardinality estimates
```

### Positive: SkinnerDB adaptive execution

```sql
-- First execution of a new query shape
SELECT * FROM A JOIN B ON ... JOIN C ON ... JOIN D ON ...;
-- SkinnerDB: tries 4-6 different orderings simultaneously
-- After 100ms, identifies A-C-B-D as 5x faster
-- Remaining execution uses best ordering
-- No prior training needed (online adaptation)
-- Regret bound: at most 2x optimal in expectation
```

### Negative: Cold start (no training data)

```sql
-- New database, no workload history
-- RL policy is randomly initialized
-- First 100 queries: RL produces random join orders
-- Often worse than simple greedy heuristic
-- Must fall back to traditional optimizer during warm-up
```

### Negative: Novel query shape unseen in training

```sql
-- Training: all queries have 3-5 tables
-- Test query: 12 tables with complex predicates
-- RL generalizes poorly to unseen table counts
-- May produce join orders worse than greedy
-- Solution: combine RL with DP for small parts of plan
```

### Negative: OLTP point lookups

```sql
SELECT * FROM accounts WHERE id = 42;
-- Single table, no join ordering decision
-- RL inference overhead (1-5ms) exceeds entire query time
-- Traditional optimizer selects index scan instantly
-- RL adds latency with zero benefit
```

## References

**Academic papers:**
- Marcus, Papaemmanouil, "Towards a Hands-Free Query Optimizer through Deep Learning", CIDR 2019
- Krishnan, Yang, Goldberg, Hellerstein, Stoica, "Learning to Optimize Join Queries With Deep Reinforcement Learning", arXiv 2018
- Marcus et al., "Neo: A Learned Query Optimizer", VLDB 2019
- Marcus et al., "Bao: Making Learned Query Optimization Practical", SIGMOD 2021
- Trummer, Moseley, et al., "SkinnerDB: Regret-Bounded Query Processing via Reinforcement Learning", SIGMOD 2019
- Yu, Li, "Reinforcement Learning with Tree-LSTM for Join Order Selection", ICDE 2020
- Leis et al., "How Good Are Query Optimizers, Really?", VLDB 2015

**Implementation:**
- SkinnerDB: Open-source adaptive query processing
- NoisePage (CMU): Learned query optimizer components
- Bao: PostgreSQL extension for learned optimization
- PostgreSQL: pg_hint_plan for manual join order control
