# RFC 0035: Genetic Query Optimizer for Large Join Graphs

## Status
PROPOSED

## Summary
Implement a genetic algorithm-based query optimizer as a fallback for queries with 12+ table joins, where exhaustive dynamic programming becomes computationally infeasible.

## Motivation
RA currently uses e-graph equality saturation for query optimization, which works well for most queries but can struggle with very large join graphs. PostgreSQL switches to a genetic algorithm (GEQO) for queries with 12+ tables. Without a similar fallback, RA may timeout or produce poor plans for complex analytical queries.

## Design

### Algorithm Components

```rust
pub struct GeneticOptimizer {
    population_size: usize,
    generations: usize,
    mutation_rate: f64,
    crossover_rate: f64,
    selection_pressure: f64,
}

pub struct JoinChromosome {
    join_order: Vec<TableId>,
    fitness: Option<Cost>,
}
```

### Genetic Operations

1. **Encoding**: Represent join order as permutation of table IDs
2. **Fitness**: Query execution cost from cost model
3. **Selection**: Tournament selection with configurable pressure
4. **Crossover**: Order crossover (OX) preserving valid permutations
5. **Mutation**: Swap mutation to explore new orderings
6. **Elitism**: Keep best solutions across generations

### Integration Points

- Trigger when join count >= configurable threshold (default 12)
- Fall back from e-graph when saturation timeout detected
- Use existing cost model for fitness evaluation
- Generate standard plan nodes for execution

## Implementation Plan

1. Create genetic algorithm framework
2. Implement join order encoding/decoding
3. Add genetic operators (crossover, mutation)
4. Integrate with existing cost model
5. Add configuration parameters
6. Benchmark against exhaustive search

## Alternatives Considered

- **Simulated Annealing**: Less parallelizable than GA
- **Random Sampling**: Poor convergence for large spaces
- **Greedy Join Ordering**: Too simplistic, misses good plans

## Success Criteria

- Optimize 20+ table joins within 1 second
- Plan quality within 20% of exhaustive search (when feasible to compare)
- Configurable parameters for different workloads