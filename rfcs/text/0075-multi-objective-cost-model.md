# RFC 0075: Multi-Objective Cost Model

- **Status**: Proposed
- **Priority**: Long-term (4-6 months)
- **Impact**: Pareto-optimal plans for competing goals
- **Category**: Cost Model / Multi-Objective
- **Created**: 2026-03-25

## Summary

Optimize for multiple objectives simultaneously (time, memory, I/O, cost, energy) and expose Pareto-optimal plans. Addresses the problem that single-objective optimization ignores important tradeoffs.

## Motivation

**Current**: Minimize execution time only

**Reality**: Multiple conflicting goals
- Cloud: Minimize scan cost (charged per GB)
- Green computing: Minimize energy
- Memory-constrained: Minimize memory usage
- Interactive: Minimize latency

**Example tradeoff**:
- Plan A: 10s execution, 1GB memory, 100GB I/O
- Plan B: 20s execution, 100MB memory, 10GB I/O
- Which is better? Depends on context!

### Evidence

**Pareto-Optimal Plans** (Trummer et al., VLDB 2014):
- Enumerate plans on Pareto frontier (time vs memory)
- Let user/system choose based on context
- Result: 2-5x improvement by exposing tradeoffs

**AWS Athena**: Optimizes for scan cost → 5-10x cost reduction on S3 scans

## Proposal

### Multi-Dimensional Cost

```rust
pub struct MultiCost {
    pub time_ms: f64,
    pub memory_mb: f64,
    pub io_gb: f64,
    pub cpu_ms: f64,
    pub energy_joules: f64,
}

impl MultiCost {
    pub fn dominates(&self, other: &MultiCost) -> bool {
        self.time_ms <= other.time_ms &&
        self.memory_mb <= other.memory_mb &&
        self.io_gb <= other.io_gb &&
        self.cpu_ms <= other.cpu_ms &&
        self.energy_joules <= other.energy_joules &&
        (self.time_ms < other.time_ms ||
         self.memory_mb < other.memory_mb ||
         self.io_gb < other.io_gb ||
         self.cpu_ms < other.cpu_ms ||
         self.energy_joules < other.energy_joules)
    }
}
```

### Pareto Frontier

```rust
pub struct ParetoFrontier {
    plans: Vec<(PhysicalPlan, MultiCost)>,
}

impl ParetoFrontier {
    pub fn add(&mut self, plan: PhysicalPlan, cost: MultiCost) {
        // Remove dominated plans
        self.plans.retain(|(_, c)| !cost.dominates(c));

        // Add if not dominated
        if !self.plans.iter().any(|(_, c)| c.dominates(&cost)) {
            self.plans.push((plan, cost));
        }
    }

    pub fn choose(&self, weights: &ObjectiveWeights) -> PhysicalPlan {
        // Weighted sum to choose from Pareto set
        self.plans.iter()
            .min_by_key(|(_, cost)| {
                (weights.time * cost.time_ms +
                 weights.memory * cost.memory_mb +
                 weights.io * cost.io_gb +
                 weights.cpu * cost.cpu_ms +
                 weights.energy * cost.energy_joules) as i64
            })
            .map(|(plan, _)| plan.clone())
            .unwrap()
    }
}
```

### Objective Weights

```rust
pub struct ObjectiveWeights {
    pub time: f64,     // Latency weight
    pub memory: f64,   // Memory weight
    pub io: f64,       // I/O weight
    pub cpu: f64,      // CPU weight
    pub energy: f64,   // Energy weight
}

impl ObjectiveWeights {
    pub fn minimize_time() -> Self {
        Self { time: 1.0, memory: 0.0, io: 0.0, cpu: 0.0, energy: 0.0 }
    }

    pub fn minimize_cost() -> Self {
        // Cloud pricing: I/O is expensive
        Self { time: 0.1, memory: 0.1, io: 1.0, cpu: 0.1, energy: 0.0 }
    }

    pub fn minimize_energy() -> Self {
        Self { time: 0.0, memory: 0.0, io: 0.0, cpu: 0.0, energy: 1.0 }
    }

    pub fn balanced() -> Self {
        Self { time: 0.4, memory: 0.2, io: 0.2, cpu: 0.1, energy: 0.1 }
    }
}
```

## Implementation Plan

### Phase 1: Multi-Dimensional Cost (Month 1-2)
1. Extend cost model to track all dimensions
2. Compute multi-cost for each operator
3. Test: verify cost accuracy per dimension

### Phase 2: Pareto Frontier (Month 3-4)
1. Implement Pareto frontier enumeration
2. Add domination check
3. Test: verify Pareto property (no dominated plans)

### Phase 3: Objective Selection (Month 5-6)
1. Add objective weight configuration
2. Implement weighted sum selection
3. Expose Pareto frontier to user (optional)

## Expected Impact

- **Pareto optimality**: No regression on any objective
- **New use cases**: Cloud cost optimization, green computing
- **Flexibility**: Context-dependent plan selection

## Prior Art

- Trummer et al., VLDB 2014: Pareto-optimal plans
- AWS Athena: Scan cost optimization
- PowerGraph (Chen et al., CIDR 2023): Energy optimization
