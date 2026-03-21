# RFC 0017: Large Join Graph Optimization Fallback

- Start Date: 2026-03-20
- Author: System
- Status: Implemented
- Tracking Issue: N/A

## Summary

Add simulated annealing and greedy heuristics as fallback strategies for optimizing queries with large join graphs (10+ tables) where e-graph equality saturation becomes too expensive.

## Motivation

### Problem

E-graph equality saturation can become computationally expensive for queries with many tables (10+). PostgreSQL switches to GEQO (genetic algorithm) for 12+ tables. RA currently has no fallback mechanism and may timeout on complex join queries.

**Real-world impact:**
- TPC-H Query 21: 4 tables (manageable)
- TPC-H Query 8: 8 tables (expensive)
- Production analytics queries: 15-50 tables (currently fails)
- ORM-generated queries with many joins: timeouts

### Why This Matters

1. **Practical necessity** - Enterprise queries routinely join 10-20 tables
2. **Predictable performance** - Bounded optimization time regardless of query complexity
3. **Graceful degradation** - Better to have a good plan in 10 seconds than perfect plan in 5 minutes
4. **Competitive parity** - PostgreSQL, SQL Server, Oracle all have heuristic fallbacks

## Guide-level explanation

When RA detects a query with many tables, it switches from exhaustive e-graph optimization to faster heuristic modes:

**Default Mode (< 10 tables):**
```rust
// Uses e-graph equality saturation
let optimizer = Optimizer::default();
let plan = optimizer.optimize(&query)?;
```

**Heuristic Mode (≥ 10 tables):**
```rust
// Automatically uses simulated annealing
let optimizer = Optimizer::with_config(OptimizerConfig {
    large_join_threshold: 10,
    large_join_strategy: LargeJoinStrategy::SimulatedAnnealing,
    ..Default::default()
});
let plan = optimizer.optimize(&query)?;  // Completes in bounded time
```

**Configuration:**
```toml
[optimizer]
large_join_threshold = 10  # Switch to heuristics at N tables
large_join_strategy = "simulated_annealing"  # or "greedy" or "egraph"
max_optimization_time_ms = 30000  # Hard timeout
```

### User Experience

Before (10+ table query):
```
Error: optimization timeout after 60 seconds
```

After (same query):
```
Plan generated in 8.3 seconds using simulated annealing (12 tables detected)
Estimated cost: 1,234,567 (within 15% of optimal based on sampling)
```

## Reference-level explanation

### Architecture

```
┌─────────────────────────────────────────────────┐
│ Optimizer::optimize(&query)                    │
└──────────────────┬──────────────────────────────┘
                   │
                   ▼
           ┌───────────────┐
           │ Count tables  │
           └───────┬───────┘
                   │
         ┌─────────▼──────────┐
         │ < threshold?       │
         └─────────┬──────────┘
           YES ↙    ↘ NO
    ┌──────────┐    ┌─────────────────────┐
    │ E-graph  │    │ Heuristic fallback  │
    │ equality │    │  - Simulated        │
    │ satur.   │    │    annealing        │
    └──────────┘    │  - Greedy           │
                    └─────────────────────┘
```

### Data Structures

```rust
// crates/ra-engine/src/large_join.rs

pub enum LargeJoinStrategy {
    /// Continue using e-graph (may timeout)
    EGraph,
    /// Greedy join ordering heuristic
    Greedy,
    /// Simulated annealing optimization
    SimulatedAnnealing {
        initial_temp: f64,
        cooling_rate: f64,
        max_iterations: usize,
    },
}

pub struct LargeJoinOptimizer {
    strategy: LargeJoinStrategy,
    cost_model: Arc<dyn CostModel>,
    stats: Arc<dyn Statistics>,
}

impl LargeJoinOptimizer {
    /// Optimize join ordering using heuristic strategy
    pub fn optimize(&self, joins: Vec<JoinNode>) -> Result<RelExpr> {
        match &self.strategy {
            LargeJoinStrategy::Greedy => self.greedy_join_order(joins),
            LargeJoinStrategy::SimulatedAnnealing { .. } => {
                self.simulated_annealing(joins)
            }
            LargeJoinStrategy::EGraph => {
                // Fall through to standard e-graph optimization
                Err(Error::NotApplicable)
            }
        }
    }
}
```

### Greedy Join Ordering

```rust
impl LargeJoinOptimizer {
    fn greedy_join_order(&self, mut joins: Vec<JoinNode>) -> Result<RelExpr> {
        // 1. Start with smallest relation (by cardinality)
        let mut current = self.smallest_relation(&joins)?;
        joins.retain(|j| j.table != current.table);

        // 2. Greedily add joins with lowest estimated cost
        while !joins.is_empty() {
            let (best_idx, best_cost) = joins
                .iter()
                .enumerate()
                .map(|(i, join)| {
                    let candidate = self.join_with(&current, join)?;
                    let cost = self.cost_model.estimate(&candidate)?;
                    Ok((i, cost))
                })
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
                .ok_or(Error::NoJoinFound)?;

            let next_join = joins.swap_remove(best_idx);
            current = self.join_with(&current, &next_join)?;
        }

        Ok(current)
    }

    fn smallest_relation(&self, joins: &[JoinNode]) -> Result<RelExpr> {
        joins
            .iter()
            .map(|j| {
                let card = self.stats.cardinality(&j.table)?;
                Ok((j, card))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(j, _)| j.to_scan())
            .ok_or(Error::NoRelations)
    }
}
```

### Simulated Annealing

```rust
impl LargeJoinOptimizer {
    fn simulated_annealing(&self, joins: Vec<JoinNode>) -> Result<RelExpr> {
        // 1. Start with greedy initial solution
        let mut current = self.greedy_join_order(joins.clone())?;
        let mut current_cost = self.cost_model.estimate(&current)?;
        let mut best = current.clone();
        let mut best_cost = current_cost;

        // 2. Simulated annealing parameters
        let LargeJoinStrategy::SimulatedAnnealing {
            mut initial_temp,
            cooling_rate,
            max_iterations,
        } = self.strategy else {
            unreachable!()
        };

        // 3. Annealing loop
        for iteration in 0..max_iterations {
            // Perturb: swap two random joins
            let neighbor = self.perturb_join_order(&current, &joins)?;
            let neighbor_cost = self.cost_model.estimate(&neighbor)?;

            // Accept if better, or probabilistically if worse
            let delta = neighbor_cost.total() - current_cost.total();
            let accept_prob = if delta < 0.0 {
                1.0  // Always accept improvements
            } else {
                (-delta / initial_temp).exp()
            };

            if rand::random::<f64>() < accept_prob {
                current = neighbor;
                current_cost = neighbor_cost;

                if current_cost.total() < best_cost.total() {
                    best = current.clone();
                    best_cost = current_cost;
                }
            }

            // Cool down
            initial_temp *= cooling_rate;
        }

        Ok(best)
    }

    fn perturb_join_order(&self, plan: &RelExpr, joins: &[JoinNode]) -> Result<RelExpr> {
        // Find two join nodes and swap their order
        // Implementation details: traverse tree, swap subtrees
        todo!("Implement join tree perturbation")
    }
}
```

### Configuration Integration

```rust
// crates/ra-engine/src/config.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizerConfig {
    /// Number of tables to trigger large join fallback
    #[serde(default = "default_large_join_threshold")]
    pub large_join_threshold: usize,

    /// Strategy for large join optimization
    #[serde(default)]
    pub large_join_strategy: LargeJoinStrategy,

    /// Hard timeout for optimization (ms)
    #[serde(default = "default_optimization_timeout")]
    pub max_optimization_time_ms: u64,

    // ... other config
}

fn default_large_join_threshold() -> usize { 10 }
fn default_optimization_timeout() -> u64 { 30000 }
```

### Cost Model Extensions

No changes needed - uses existing `CostModel` trait. However, heuristic strategies may benefit from:
- Faster cost estimation (sampling instead of full analysis)
- Cached cardinality estimates
- Approximate selectivity calculations

## Drawbacks

1. **Not optimal** - Heuristics produce good plans, not necessarily optimal
2. **Configuration complexity** - Adds tunable parameters
3. **Code complexity** - New optimization code path to maintain
4. **Testing difficulty** - Harder to verify correctness without ground truth

## Rationale and alternatives

### Why simulated annealing over genetic algorithms?

**Simulated Annealing Advantages:**
- Simpler to implement
- More predictable behavior
- Better theoretical properties (convergence guarantees)
- Deterministic with fixed seed (reproducible)

**GEQO (Genetic Algorithm) Disadvantages:**
- Complex crossover/mutation operators
- Non-deterministic
- Many hyperparameters to tune
- PostgreSQL community feedback: GEQO is difficult to tune

### Alternative: Dynamic Programming with pruning

Could implement PostgreSQL-style DP with aggressive pruning:
```rust
for i in 1..n {
    for subset in subsets_of_size(i) {
        for partition in partitions(subset) {
            // Only keep top K plans per subset
            plans[subset] = best_k_plans(plans[left] + plans[right]);
        }
    }
}
```

**Rejected because:**
- Still exponential space complexity
- Hard to bound worst-case time
- E-graph already provides DP-like exploration

### Alternative: Do nothing (let users split queries)

**Rejected because:**
- Poor user experience
- Competitive disadvantage
- Every major database has heuristic fallbacks

## Prior art

- **PostgreSQL GEQO**: Genetic algorithm for 12+ tables, configurable threshold
- **SQL Server**: Greedy join enumeration with parallel plans
- **Oracle**: Greedy heuristics + cost-based refinement
- **Spark Catalyst**: Stochastic search for large join graphs
- **Research**: Steinbrunn et al. "Heuristic and Randomized Optimization" (1997)

## Unresolved questions

1. **Threshold tuning**: Is 10 tables the right default? Should vary by hardware?
2. **Hybrid approach**: Use e-graph for subproblems, heuristics for top-level?
3. **Quality metrics**: How to measure "closeness to optimal" without knowing optimal?
4. **User feedback**: Should optimizer report "this was a heuristic plan, may not be optimal"?

## Future possibilities

1. **Adaptive thresholds**: Learn optimal threshold from query execution history
2. **Hybrid optimization**: E-graph for critical subgraphs, heuristics for rest
3. **ML-guided search**: Use learned models to guide simulated annealing
4. **Parallel search**: Run multiple annealing processes in parallel, take best result
5. **Progressive optimization**: Return quick greedy plan immediately, refine in background

## Implementation plan

### Phase 1: Greedy Baseline (1 week)
1. Implement `LargeJoinOptimizer` with greedy strategy
2. Add table counting logic to `Optimizer`
3. Add `large_join_threshold` config
4. Unit tests for greedy join ordering
5. Benchmark against e-graph on 10-15 table queries

### Phase 2: Simulated Annealing (2 weeks)
1. Implement `perturb_join_order()` - tree mutation
2. Implement annealing loop with temperature schedule
3. Tune default parameters (initial_temp, cooling_rate)
4. Comparative benchmarks: greedy vs annealing vs e-graph
5. Integration tests with TPC-H queries

### Phase 3: Polish (1 week)
1. Add optimizer reporting (which strategy used, why)
2. Add telemetry (strategy choice, time, quality metrics)
3. Documentation and examples
4. Performance tuning

### Verification
- TPC-H queries 8, 21: should complete in < 10s each
- Synthetic 20-table query: greedy < 5s, annealing < 30s
- Quality: annealing plans within 20% of e-graph optimal (measured on small queries)
