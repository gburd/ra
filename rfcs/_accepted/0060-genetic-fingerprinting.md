# RFC 0060: System State Fingerprinting with Genetic Parameter Tuning

- Start Date: 2026-03-23
- Author: Ra Team
- Status: Draft
- Tracking Issue: TBD

## Summary

Introduce a genetic algorithm framework that evolves optimizer parameters (`iter_limit`, `node_limit`, `time_limit_secs`, `large_join_threshold`, `cost_pruning_threshold`) to match observed system states (CPU-bound, I/O-bound, memory-bound, network-bound, balanced). The system continuously monitors host metrics, classifies the current operating regime, and recalls the best-performing parameter genome for that regime--falling back to defaults when no evolved genome exists. Over time this produces per-state optimizer configurations that outperform any single static default.

## Motivation

Ra's `OptimizerConfig` exposes a dozen knobs. The correct values depend on the environment: a memory-pressured host should use smaller e-graph node limits and more aggressive pruning, while an idle CPU-rich host can afford deeper search. Today these are set once at startup and never revisited.

Manual tuning is fragile:

1. Different deployment targets (embedded, cloud, laptop) have different sweet spots.
2. System load changes over the lifetime of a single connection.
3. The interaction between parameters is non-linear--changing `iter_limit` shifts the optimal `cost_pruning_threshold`.

Genetic algorithms handle exactly this class of problem: multi-dimensional, non-linear, noisy fitness landscapes where exhaustive search is infeasible. By maintaining a population of candidate configurations per system state and evolving them against measured outcomes, we converge on good parameter sets without requiring an operator to hand-tune.

### Goals

1. Classify runtime system state into discrete regimes using lightweight OS metrics.
2. Maintain a population of `OptimizerGenome` values per regime.
3. Evolve populations using standard GA operations (selection, crossover, mutation, elitism).
4. Evaluate fitness by measuring planning latency and plan quality under real workloads.
5. Persist winning genomes so restarts do not discard learned configurations.

### Non-Goals

- Tuning PostgreSQL/backend parameters (GUCs). This RFC targets Ra's own optimizer knobs.
- Real-time per-query adaptation. The genome is selected per system state, not per query.
- Replacing the adaptive search limits from RFC 0058. This RFC layers on top of those limits.

## Guide-level explanation

When Ra starts, a background `SystemMonitor` thread samples OS counters every 500 ms: CPU utilization, memory pressure, I/O wait, network throughput, and cache hit ratios. A lightweight classifier maps the latest sample to a `SystemState` enum. The `GeneticTuner` maintains a separate population of candidate `OptimizerGenome` values for each state.

Before each optimization pass the tuner is consulted:

```rust
let metrics = monitor.latest_metrics();
let state = classify(&metrics);
let genome = tuner.best_genome_for(state);
let config = genome.to_optimizer_config();
let plan = optimizer.optimize(query, &config);
```

After the plan executes, feedback is recorded:

```rust
tuner.record_fitness(state, genome_id, FitnessObservation {
    planning_time: plan_elapsed,
    estimated_cost: plan.cost(),
    actual_execution_time: exec_elapsed,
});
```

Periodically (every N observations or on a timer), the tuner evolves each population: selecting parents by tournament, producing offspring via crossover and mutation, and retaining elites. Populations converge toward parameter sets that minimize a weighted combination of planning time and plan cost for each system state.

### Example Usage

```rust
use ra_engine::genetic::{GeneticTuner, SystemMonitor};

// Start the monitor and tuner.
let monitor = SystemMonitor::start(Duration::from_millis(500));
let mut tuner = GeneticTuner::new(GeneticConfig::default());

// Optionally load previously evolved genomes from disk.
if let Ok(snapshot) = GeneticSnapshot::load("ra_genomes.bin") {
    tuner.restore(snapshot);
}

// On each query:
let state = monitor.classify();
let genome = tuner.best_genome_for(state);
let config = genome.to_optimizer_config();
let plan = optimizer.optimize(query, &config);

// After execution, feed back results.
tuner.record_fitness(state, genome.id, observation);

// Periodically persist.
tuner.snapshot().save("ra_genomes.bin")?;
```

## Reference-level explanation

### Implementation Details

#### System Metrics Collection

```rust
/// Raw counters sampled from the OS.
pub struct SystemMetrics {
    /// CPU utilization across all cores, 0.0..=1.0.
    pub cpu_utilization: f64,
    /// Fraction of physical memory in use, 0.0..=1.0.
    pub memory_pressure: f64,
    /// Fraction of time the CPU spent waiting on I/O, 0.0..=1.0.
    pub io_wait_fraction: f64,
    /// Network bytes received + sent per second.
    pub network_bytes_per_sec: u64,
    /// Buffer/page cache hit ratio, 0.0..=1.0.
    pub cache_hit_ratio: f64,
    /// Average disk queue depth over the sample window.
    pub disk_queue_depth: f64,
    /// Timestamp of this sample.
    pub sampled_at: Instant,
}
```

On Linux, values come from `/proc/stat`, `/proc/meminfo`, `/proc/diskstats`, and `/proc/net/dev`. On macOS, `host_statistics64` and `IOKit` provide equivalents. The monitor thread maintains a sliding window of the last 8 samples and exposes both instantaneous and averaged views.

#### System State Classification

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemState {
    CpuBound,
    IoBound,
    MemoryBound,
    NetworkBound,
    Balanced,
}

pub fn classify(metrics: &SystemMetrics) -> SystemState {
    if metrics.memory_pressure > 0.85 {
        return SystemState::MemoryBound;
    }
    if metrics.io_wait_fraction > 0.30 {
        return SystemState::IoBound;
    }
    if metrics.cpu_utilization > 0.80 && metrics.io_wait_fraction < 0.10 {
        return SystemState::CpuBound;
    }
    if metrics.network_bytes_per_sec > 500_000_000 {
        // >500 MB/s sustained
        return SystemState::NetworkBound;
    }
    SystemState::Balanced
}
```

Thresholds are intentionally simple. A future RFC may replace this with a trained decision tree, but hard-coded thresholds are easier to reason about, debug, and override.

#### System Fingerprint

The fingerprint summarizes the recent operating environment so that genomes can be recalled when conditions recur.

```rust
/// Compact descriptor of recent system conditions.
pub struct SystemFingerprint {
    /// Classified state over the last window.
    pub state: SystemState,
    /// Averaged metrics over the last window.
    pub avg_cpu: f64,
    pub avg_memory: f64,
    pub avg_io_wait: f64,
    pub avg_cache_hit: f64,
    /// Number of samples in the window.
    pub sample_count: u32,
}

impl SystemFingerprint {
    /// Euclidean distance between two fingerprints (ignoring state enum).
    pub fn distance(&self, other: &SystemFingerprint) -> f64 {
        let d_cpu = self.avg_cpu - other.avg_cpu;
        let d_mem = self.avg_memory - other.avg_memory;
        let d_io = self.avg_io_wait - other.avg_io_wait;
        let d_cache = self.avg_cache_hit - other.avg_cache_hit;
        (d_cpu * d_cpu
            + d_mem * d_mem
            + d_io * d_io
            + d_cache * d_cache)
            .sqrt()
    }
}
```

The distance metric allows nearest-neighbor lookup when the exact state has no evolved population yet.

#### Optimizer Genome

Each genome encodes the tunable parameters of `OptimizerConfig`:

```rust
pub struct OptimizerGenome {
    pub id: GenomeId,
    /// E-graph iteration limit (range: 3..=60).
    pub egraph_iter_limit: u32,
    /// Maximum e-graph nodes (range: 1_000..=200_000).
    pub node_limit: u32,
    /// Hard optimization timeout in milliseconds (range: 50..=5_000).
    pub timeout_ms: u32,
    /// Table count threshold for switching to large-join strategy
    /// (range: 4..=20).
    pub join_reorder_threshold: u32,
    /// How aggressively to prune dominated plans, 1.0 = no pruning,
    /// lower = more aggressive (range: 0.1..=3.0).
    pub pruning_aggressiveness: f64,
    /// Accumulated fitness observations.
    pub fitness: FitnessAccumulator,
}

impl OptimizerGenome {
    /// Convert to an engine-level OptimizerConfig.
    pub fn to_optimizer_config(&self) -> OptimizerConfig {
        OptimizerConfig {
            iter_limit: self.egraph_iter_limit as usize,
            node_limit: self.node_limit as usize,
            time_limit_secs: (self.timeout_ms as u64 + 999) / 1000,
            large_join_threshold: self.join_reorder_threshold as usize,
            cost_pruning_threshold: self.pruning_aggressiveness,
            max_optimization_time_ms: u64::from(self.timeout_ms),
            use_adaptive_limits: true,
            use_cost_pruning: true,
            use_join_graph_filtering: true,
            ..OptimizerConfig::default()
        }
    }

    /// Create a random genome within valid parameter ranges.
    pub fn random(rng: &mut impl Rng) -> Self {
        Self {
            id: GenomeId::new(),
            egraph_iter_limit: rng.gen_range(3..=60),
            node_limit: rng.gen_range(1_000..=200_000),
            timeout_ms: rng.gen_range(50..=5_000),
            join_reorder_threshold: rng.gen_range(4..=20),
            pruning_aggressiveness: rng.gen_range(0.1..=3.0),
            fitness: FitnessAccumulator::default(),
        }
    }
}
```

#### Fitness Evaluation

Fitness combines planning speed and plan quality, weighted by system state:

```rust
pub struct FitnessObservation {
    pub planning_time_ms: f64,
    pub estimated_cost: f64,
    pub actual_execution_time_ms: Option<f64>,
}

pub struct FitnessAccumulator {
    pub observations: Vec<FitnessObservation>,
}

impl FitnessAccumulator {
    /// Compute aggregate fitness. Lower is better.
    pub fn score(&self, state: SystemState) -> f64 {
        if self.observations.is_empty() {
            return f64::MAX;
        }

        // Weights shift depending on system state.
        let (w_speed, w_quality) = match state {
            // Under CPU pressure, prefer faster planning.
            SystemState::CpuBound => (0.7, 0.3),
            // Under memory pressure, prefer smaller search
            // (correlated with speed).
            SystemState::MemoryBound => (0.8, 0.2),
            // When I/O-bound, plan quality matters more
            // because execution dominates.
            SystemState::IoBound => (0.3, 0.7),
            // Network-bound: execution cost matters.
            SystemState::NetworkBound => (0.3, 0.7),
            // Balanced: equal weight.
            SystemState::Balanced => (0.5, 0.5),
        };

        let n = self.observations.len() as f64;
        let avg_plan_time: f64 = self
            .observations
            .iter()
            .map(|o| o.planning_time_ms)
            .sum::<f64>()
            / n;
        let avg_cost: f64 = self
            .observations
            .iter()
            .map(|o| o.estimated_cost)
            .sum::<f64>()
            / n;

        // Normalize to roughly comparable scales.
        // Planning time: 0-5000 ms range, divide by 1000.
        // Estimated cost: already normalized by the cost model.
        let norm_speed = avg_plan_time / 1000.0;
        let norm_quality = avg_cost.ln().max(0.0);

        w_speed * norm_speed + w_quality * norm_quality
    }
}
```

The rationale for per-state weights:

- **CPU-bound**: The CPU is the bottleneck. Spending less time planning means more CPU available for execution and other queries. Favor fast planning even if plan quality drops slightly.
- **Memory-bound**: Large e-graphs consume memory. Smaller `node_limit` and fewer iterations reduce memory footprint. Speed is a proxy for memory efficiency here.
- **I/O-bound**: The CPU is idle waiting on disk. Spending more time planning to find a plan that reads fewer pages pays off because execution time dwarfs planning time.
- **Network-bound**: Similar to I/O-bound. A better plan that transfers less data across the network is worth the extra planning time.
- **Balanced**: No single resource dominates, so weight equally.

#### Genetic Algorithm Operations

```rust
pub struct GeneticConfig {
    /// Population size per system state.
    pub population_size: usize,      // default: 30
    /// Fraction of population preserved as elites.
    pub elitism_fraction: f64,       // default: 0.1
    /// Tournament size for parent selection.
    pub tournament_size: usize,      // default: 3
    /// Probability that a gene is mutated.
    pub mutation_rate: f64,          // default: 0.15
    /// Magnitude of mutation (fraction of parameter range).
    pub mutation_magnitude: f64,     // default: 0.2
    /// Minimum observations before a genome is evaluated.
    pub min_observations: usize,     // default: 5
    /// Observations between evolution generations.
    pub observations_per_generation: usize, // default: 50
}

pub struct GeneticTuner {
    config: GeneticConfig,
    populations: HashMap<SystemState, Vec<OptimizerGenome>>,
    generation: HashMap<SystemState, u32>,
    rng: StdRng,
}

impl GeneticTuner {
    pub fn new(config: GeneticConfig) -> Self {
        let mut populations = HashMap::new();
        let mut rng = StdRng::from_entropy();
        for state in SystemState::all() {
            let pop: Vec<OptimizerGenome> = (0..config.population_size)
                .map(|_| OptimizerGenome::random(&mut rng))
                .collect();
            populations.insert(state, pop);
        }
        Self {
            config,
            populations,
            generation: HashMap::new(),
            rng,
        }
    }

    /// Return the best genome for the given state, or the one
    /// with the fewest observations (exploration).
    pub fn best_genome_for(
        &self,
        state: SystemState,
    ) -> &OptimizerGenome {
        let pop = &self.populations[&state];
        // Prefer under-explored genomes for exploration.
        let under_explored = pop.iter().find(|g| {
            g.fitness.observations.len() < self.config.min_observations
        });
        if let Some(genome) = under_explored {
            return genome;
        }
        // Otherwise return the genome with the best fitness.
        pop.iter()
            .min_by(|a, b| {
                a.fitness
                    .score(state)
                    .partial_cmp(&b.fitness.score(state))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .expect("population is never empty")
    }

    /// Run one generation of evolution for the given state.
    pub fn evolve_for_state(&mut self, state: SystemState) {
        let pop = self.populations.get_mut(&state)
            .expect("all states initialized");
        let pop_size = pop.len();

        // Sort by fitness (lower is better).
        pop.sort_by(|a, b| {
            a.fitness
                .score(state)
                .partial_cmp(&b.fitness.score(state))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Elitism: preserve top performers.
        let elite_count =
            (pop_size as f64 * self.config.elitism_fraction).ceil()
                as usize;
        let elites: Vec<OptimizerGenome> =
            pop[..elite_count].to_vec();

        // Build the next generation.
        let mut next_gen = elites;
        while next_gen.len() < pop_size {
            let parent_a = self.tournament_select(pop, state);
            let parent_b = self.tournament_select(pop, state);
            let mut child = self.crossover(parent_a, parent_b);
            self.mutate(&mut child);
            child.id = GenomeId::new();
            child.fitness = FitnessAccumulator::default();
            next_gen.push(child);
        }

        *pop = next_gen;
        *self.generation.entry(state).or_insert(0) += 1;
    }

    fn tournament_select<'a>(
        &mut self,
        pop: &'a [OptimizerGenome],
        state: SystemState,
    ) -> &'a OptimizerGenome {
        let mut best: Option<&OptimizerGenome> = None;
        for _ in 0..self.config.tournament_size {
            let idx = self.rng.gen_range(0..pop.len());
            let candidate = &pop[idx];
            match best {
                None => best = Some(candidate),
                Some(current) => {
                    if candidate.fitness.score(state)
                        < current.fitness.score(state)
                    {
                        best = Some(candidate);
                    }
                }
            }
        }
        best.expect("tournament_size >= 1")
    }

    /// Uniform crossover: each gene picked from either parent
    /// with equal probability.
    fn crossover(
        &mut self,
        a: &OptimizerGenome,
        b: &OptimizerGenome,
    ) -> OptimizerGenome {
        let pick = |x, y| if self.rng.gen_bool(0.5) { x } else { y };
        OptimizerGenome {
            id: GenomeId::new(),
            egraph_iter_limit: pick(
                a.egraph_iter_limit,
                b.egraph_iter_limit,
            ),
            node_limit: pick(a.node_limit, b.node_limit),
            timeout_ms: pick(a.timeout_ms, b.timeout_ms),
            join_reorder_threshold: pick(
                a.join_reorder_threshold,
                b.join_reorder_threshold,
            ),
            pruning_aggressiveness: pick(
                a.pruning_aggressiveness,
                b.pruning_aggressiveness,
            ),
            fitness: FitnessAccumulator::default(),
        }
    }

    /// Gaussian mutation: perturb each gene with probability
    /// `mutation_rate`.
    fn mutate(&mut self, genome: &mut OptimizerGenome) {
        let rate = self.config.mutation_rate;
        let mag = self.config.mutation_magnitude;

        if self.rng.gen_bool(rate) {
            let delta = self.rng.gen_range(-mag..=mag);
            let range = 60.0 - 3.0;
            let new_val = genome.egraph_iter_limit as f64
                + delta * range;
            genome.egraph_iter_limit =
                new_val.round().clamp(3.0, 60.0) as u32;
        }
        if self.rng.gen_bool(rate) {
            let delta = self.rng.gen_range(-mag..=mag);
            let range = 200_000.0 - 1_000.0;
            let new_val =
                genome.node_limit as f64 + delta * range;
            genome.node_limit =
                new_val.round().clamp(1_000.0, 200_000.0) as u32;
        }
        if self.rng.gen_bool(rate) {
            let delta = self.rng.gen_range(-mag..=mag);
            let range = 5_000.0 - 50.0;
            let new_val =
                genome.timeout_ms as f64 + delta * range;
            genome.timeout_ms =
                new_val.round().clamp(50.0, 5_000.0) as u32;
        }
        if self.rng.gen_bool(rate) {
            let delta = self.rng.gen_range(-mag..=mag);
            let range = 20.0 - 4.0;
            let new_val =
                genome.join_reorder_threshold as f64
                    + delta * range;
            genome.join_reorder_threshold =
                new_val.round().clamp(4.0, 20.0) as u32;
        }
        if self.rng.gen_bool(rate) {
            let delta = self.rng.gen_range(-mag..=mag);
            let range = 3.0 - 0.1;
            genome.pruning_aggressiveness = (genome
                .pruning_aggressiveness
                + delta * range)
                .clamp(0.1, 3.0);
        }
    }
}
```

#### The Adaptation Loop

The full loop runs as follows:

1. **Observe** -- `SystemMonitor` samples OS counters.
2. **Classify** -- `classify()` maps metrics to `SystemState`.
3. **Recall** -- `GeneticTuner::best_genome_for()` returns the best (or least-explored) genome for that state.
4. **Apply** -- The genome is converted to `OptimizerConfig` and passed to the optimizer.
5. **Measure** -- Planning time and plan cost are recorded as a `FitnessObservation`.
6. **Evolve** -- After `observations_per_generation` observations accumulate for a state, `evolve_for_state()` runs one GA generation.

This is a steady-state evolutionary loop. It does not require a separate training phase. The system learns during normal operation, with each query serving as both a production workload and a fitness evaluation.

### Integration Points

- **`OptimizerConfig`** (from `crates/ra-engine/src/egraph.rs:167`): The genome maps directly onto existing config fields. No changes to the optimizer are required.
- **RFC 0058 (Adaptive Search Limits)**: The GA tunes the base parameters; the adaptive limits module applies dynamic adjustments on top. The two are complementary: the GA finds a good starting point, and the per-query adaptive logic fine-tunes from there.
- **Persistence**: Genomes are serialized with `bincode`/`serde`. On startup, the tuner loads the last snapshot. On clean shutdown, it saves. A background timer also saves periodically to survive crashes.
- **Configuration**: The `GeneticConfig` struct is exposed through Ra's top-level configuration, allowing operators to disable the feature (`enabled: false`) or adjust population sizes and mutation rates.

### Error Handling

- **Metric collection failure**: If OS counters are unavailable (e.g., inside a restricted container), the monitor falls back to `SystemState::Balanced` and logs a warning. The GA still operates; it just has a single population.
- **Fitness evaluation with no data**: Genomes with zero observations return `f64::MAX` fitness, ensuring they are selected for exploration rather than exploitation.
- **Genome corruption on load**: If deserialization fails, the tuner logs a warning and starts fresh with random populations. No data loss beyond the cached genomes.
- **Divergent populations**: If a population converges prematurely (all genomes within 1% fitness), the tuner injects fresh random genomes to maintain diversity.

### Performance Considerations

- **Monitor overhead**: One thread, one syscall per metric source, every 500 ms. Negligible.
- **Classification**: A handful of comparisons per query. Negligible.
- **Genome lookup**: Linear scan of 30 genomes, once per query. Under 1 microsecond.
- **Evolution**: Runs every ~50 queries per state. One generation of 30 genomes takes <100 microseconds. Not on the hot path.
- **Memory**: 5 states x 30 genomes x ~200 bytes = ~30 KB. Negligible.
- **Persistence**: A snapshot is ~30 KB. Writing it once per minute is negligible.

## Drawbacks

- **Complexity**: Adds a GA framework that team members must understand when debugging optimizer behavior. The parameter space is small (5 genes), which limits the complexity of the GA itself, but the concept requires familiarity with evolutionary computation.
- **Non-determinism**: Two runs of the same workload may produce different evolved configurations. This complicates regression testing. Mitigation: seed the RNG deterministically in test mode.
- **Slow convergence**: With a population of 30 and generations every 50 queries, meaningful convergence may require thousands of queries. Short-lived connections or infrequent workloads may never converge. Mitigation: ship a set of pre-evolved genomes as defaults.
- **Metric availability**: Not all platforms expose the same OS counters. The monitor must degrade gracefully, reducing classification accuracy.
- **Overfitting to transient states**: A brief CPU spike might trigger CpuBound classification and apply a genome optimized for sustained CPU pressure. Mitigation: the sliding window of 8 samples (4 seconds) smooths transients.

## Rationale and alternatives

### Why This Design?

Genetic algorithms are a natural fit because:

1. **Small parameter space**: 5 continuous/integer parameters are well within GA capabilities. GAs handle mixed integer/float spaces without modification.
2. **Noisy fitness**: Query planning times vary with cache state, concurrent load, and query shape. GAs are inherently noise-tolerant because they evaluate populations, not single points.
3. **No gradient**: The mapping from optimizer parameters to plan quality has no closed-form derivative. GAs are gradient-free.
4. **Online learning**: GAs run continuously without a separate training phase. Each query is a fitness evaluation.
5. **Interpretability**: Unlike neural network based tuners, a genome is a readable set of parameter values. Operators can inspect, override, or seed populations.

### Alternative Approaches

- **Bayesian optimization (BO)**: BO (e.g., Gaussian processes) is more sample-efficient for small parameter spaces. However, BO assumes a single objective, requires careful kernel selection, and does not naturally partition by system state. A future RFC could replace the GA with BO per state if sample efficiency becomes critical.
- **Reinforcement learning**: An RL agent could learn a policy mapping system state to parameters. This requires more infrastructure (replay buffers, neural networks, gradient computation) and is harder to debug. The GA achieves similar results with simpler machinery.
- **Static profiles**: Ship 5 hand-tuned configs, one per system state. This is simpler but cannot adapt to the specific hardware, workload, or interaction effects of the deployment. The GA can be seeded with hand-tuned defaults and then improve from there.
- **Grid/random search**: Run a parameter sweep and pick the best. This requires a dedicated benchmarking phase and does not adapt at runtime.

### Impact of Not Doing This

Without this feature, operators must manually tune `OptimizerConfig` or accept the defaults. The defaults are a compromise that performs adequately but not well in any specific regime. Systems under memory pressure may OOM from large e-graphs; systems with idle CPUs leave optimization quality on the table.

## Prior art

### Academic Research

- **Self-Tuning Database Systems (Chaudhuri & Narasayya, 2007)**: Foundational work on automatic physical design tuning. Demonstrates that databases benefit from automated parameter search.
- **Adaptive Query Processing (Deshpande et al., 2007)**: Survey of techniques for adapting query execution to runtime conditions. Motivates per-state adaptation.

### Industry Solutions

- **OtterTune (Van Aken et al., 2017)**: Uses Gaussian processes and deep learning to tune database knobs. Demonstrated 35-60% throughput improvements on PostgreSQL and MySQL. OtterTune operates offline on collected workload traces; our approach operates online.
- **PostgreSQL GEQO**: Uses a genetic algorithm to optimize join ordering when the number of tables exceeds `geqo_threshold` (default 12). Demonstrates that GAs are practical inside a query optimizer, though GEQO optimizes a single query's join order rather than global parameters.
- **genetic-zaphod-cpu-scheduler** (https://codeberg.org/gregburd/genetic-zaphod-cpu-scheduler): A genetic algorithm for CPU scheduler parameter tuning. Demonstrates the same observe-classify-evolve loop we propose, applied to OS scheduling rather than query optimization. Directly inspired this RFC's architecture.
- **CDBTune (Zhang et al., 2019)**: Deep reinforcement learning for database configuration tuning. Achieves results comparable to expert DBAs but requires significant training time and GPU resources.

### What We Can Learn

1. **From OtterTune**: Factor metrics by workload type (OLTP vs OLAP). Our system state classification serves an analogous role.
2. **From GEQO**: Keep population sizes small (default `geqo_pool_size = 0`, meaning `2 * num_tables`). Larger populations slow convergence without proportional quality gains.
3. **From genetic-zaphod-cpu-scheduler**: The observe-classify-recall-evolve loop works in practice for system-level parameter tuning. Elitism is important to prevent regression. Persistence across restarts is necessary for the approach to be useful.
4. **From CDBTune**: Simple methods (even random search) often match sophisticated methods on small parameter spaces. Do not over-engineer the optimization algorithm.

## Unresolved questions

- **Metric sources on non-Linux platforms**: macOS and Windows provide different APIs. How much platform abstraction is worth building in the first version? Proposal: start with Linux (`/proc`) and macOS (`host_statistics64`), stub Windows to `Balanced`.
- **Population persistence format**: `bincode` is fast but not forward-compatible. Should we use a self-describing format like `postcard` or JSON for the genome snapshots?
- **Interaction with connection pooling**: If a connection pool routes queries from different applications, the system state is shared but workload characteristics differ. Should we fingerprint by (system state, workload type) instead of system state alone?
- **Cold start strategy**: How should the initial random population be seeded? Options: (a) pure random, (b) centered on current defaults with small perturbations, (c) ship pre-evolved genomes from benchmarks. Proposal: option (b) for the initial implementation.
- **Fitness decay**: Should older observations be down-weighted to adapt to changing hardware or workload? Proposal: exponential decay with a configurable half-life.

## Future possibilities

### Natural Extensions

- **Workload-aware fingerprinting**: Extend the fingerprint to include query shape statistics (average number of joins, filter selectivity, aggregation presence). This enables per-workload-type genomes within each system state.
- **Multi-objective evolution (NSGA-II)**: Instead of scalarizing planning time and plan quality, maintain a Pareto front and let operators choose their preferred trade-off point.
- **Transfer learning between instances**: A fleet of Ra instances could share evolved genomes via a central registry, bootstrapping new instances from collective experience.
- **Automatic parameter range discovery**: Instead of hard-coding gene ranges, use initial random exploration to identify the feasible region, then tighten ranges to focus the search.
- **Integration with cost model calibration**: The GA currently treats the cost model as fixed. Co-evolving cost model weights alongside optimizer parameters could yield further improvements.

### Long-term Vision

This RFC is one step toward a fully self-tuning query optimizer. Combined with RFC 0058 (adaptive search limits) and future cost model calibration, Ra could automatically adapt to any deployment environment without operator intervention. The genetic tuning framework is general enough to accommodate new parameters as they are added to `OptimizerConfig`, requiring only the addition of new genes to the genome.
