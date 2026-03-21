# Rule: Morsel-Driven Parallel Execution Framework

**Category:** execution-models
**File:** `rules/execution-models/morsel-driven/morsel-driven-parallelism.rra`

## Metadata

- **ID:** `morsel-driven-parallelism`
- **Version:** 1.0.0
- **Databases:** HyPer, DuckDB, Umbra
- **Tags:** execution, parallel, morsel, work-stealing, multi-core, elasticity
- **SQL Standard:** HyPer morsel model
- **Authors:** Viktor Leis, Peter Boncz, Alfons Kemper, Thomas Neumann


# Morsel-Driven Parallel Execution Framework

## Description

Morsel-driven parallelism is a framework for parallelizing query execution on modern multi-core CPUs. It divides input data into small chunks called "morsels" (typically 10K-100K tuples) and distributes them across worker threads using work-stealing. Unlike traditional exchange-operator parallelism (Volcano), morsel-driven execution provides elastic parallelism -- workers can be dynamically reassigned between concurrent queries, and the degree of parallelism adapts to the actual workload.

**Core concepts:**
- **Morsel**: A small, fixed-size chunk of tuples that represents a unit of work
- **Pipeline task**: A complete pipeline applied to one morsel
- **Dispatcher**: Assigns morsels to worker threads, enables work-stealing
- **Elastic scheduling**: Workers migrate between queries based on load
- **NUMA-aware**: Morsels scheduled on cores local to their data

**Key advantages over exchange-operator parallelism:**
- **No partition skew**: All workers process equal-sized morsels
- **Elastic**: Degree of parallelism changes at runtime
- **No startup cost**: No repartitioning needed
- **Cache-friendly**: Morsel fits in L2/L3 cache
- **Load balanced**: Work-stealing handles variable per-tuple cost

**Trade-offs:**
- Shared data structures need synchronization (hash tables, aggregates)
- Morsel size tuning affects efficiency
- Memory overhead for per-worker state (thread-local hash tables)
- Not suitable for very small queries (scheduling overhead)

## Relational Algebra

```
Morsel-driven query execution:

execute_query(plan, num_workers):
  pipelines = identify_pipelines(plan)

  for pipeline in pipelines:
    // Create morsel source for this pipeline
    source = MorselSource::new(pipeline.input, morsel_size)

    // Dispatch morsels to workers
    dispatcher = Dispatcher::new(source, num_workers)

    parallel_for worker in 0..num_workers:
      loop:
        morsel = dispatcher.get_morsel(worker)
        if morsel is None: break
        pipeline.execute(morsel)

    // Finalize pipeline (merge thread-local state)
    pipeline.finalize()
```

## Implementation

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Morsel-driven parallel execution framework
pub struct MorselDrivenExecutor {
    num_workers: usize,
    morsel_size: usize,
    workers: Vec<Worker>,
    dispatcher: Arc<Dispatcher>,
}

/// Morsel: unit of parallel work
pub struct Morsel {
    pub start_row: usize,
    pub end_row: usize,
    pub data: MorselData,
}

/// Dispatcher: distributes morsels to workers
pub struct Dispatcher {
    /// Atomic counter for morsel allocation
    next_morsel: AtomicUsize,
    total_morsels: usize,
    morsel_size: usize,
    table_size: usize,
}

impl Dispatcher {
    /// Get next morsel for a worker (lock-free)
    pub fn get_morsel(&self) -> Option<Morsel> {
        let morsel_idx = self.next_morsel.fetch_add(
            1, Ordering::Relaxed,
        );
        if morsel_idx >= self.total_morsels {
            return None;
        }

        let start = morsel_idx * self.morsel_size;
        let end = (start + self.morsel_size).min(self.table_size);

        Some(Morsel {
            start_row: start,
            end_row: end,
            data: MorselData::Range(start, end),
        })
    }
}

impl MorselDrivenExecutor {
    pub fn new(num_workers: usize, morsel_size: usize) -> Self {
        let workers = (0..num_workers)
            .map(|id| Worker::new(id))
            .collect();

        Self {
            num_workers,
            morsel_size,
            workers,
            dispatcher: Arc::new(Dispatcher::default()),
        }
    }

    /// Execute a query plan using morsel-driven parallelism
    pub fn execute(&mut self, plan: &QueryPlan) -> Result<Vec<Batch>> {
        let pipelines = plan.identify_pipelines();
        let mut final_results = Vec::new();

        for pipeline in &pipelines {
            // Initialize dispatcher for this pipeline
            let source_size = pipeline.estimate_input_rows();
            let total_morsels = (source_size + self.morsel_size - 1)
                / self.morsel_size;

            let dispatcher = Arc::new(Dispatcher {
                next_morsel: AtomicUsize::new(0),
                total_morsels,
                morsel_size: self.morsel_size,
                table_size: source_size,
            });

            // Initialize per-worker state (e.g., thread-local HTs)
            let pipeline_state = pipeline.create_shared_state()?;

            // Execute pipeline in parallel
            let handles: Vec<_> = (0..self.num_workers)
                .map(|worker_id| {
                    let disp = dispatcher.clone();
                    let state = pipeline_state.clone();
                    let p = pipeline.clone();

                    std::thread::spawn(move || {
                        let mut local_state = p.create_local_state();
                        while let Some(morsel) = disp.get_morsel() {
                            p.execute_morsel(
                                &morsel, &state, &mut local_state,
                            );
                        }
                        local_state
                    })
                })
                .collect();

            // Collect and merge thread-local state
            let local_states: Vec<_> = handles
                .into_iter()
                .map(|h| h.join().expect("Worker panicked"))
                .collect();

            let result = pipeline.finalize(
                pipeline_state, local_states,
            )?;
            final_results.extend(result);
        }

        Ok(final_results)
    }
}

/// Elastic worker pool for concurrent query scheduling
pub struct ElasticScheduler {
    workers: Vec<WorkerThread>,
    active_queries: Vec<QueryExecution>,
}

impl ElasticScheduler {
    /// Reassign workers between queries based on progress
    pub fn rebalance(&mut self) {
        let total_workers = self.workers.len();
        let total_remaining: f64 = self.active_queries.iter()
            .map(|q| q.remaining_work())
            .sum();

        for query in &mut self.active_queries {
            let share = query.remaining_work() / total_remaining;
            let assigned = (share * total_workers as f64).round()
                as usize;
            query.set_worker_count(assigned.max(1));
        }
    }
}

/// Cost model for morsel-driven execution
pub fn morsel_driven_cost(
    sequential_cost: f64,
    num_workers: usize,
    morsel_size: usize,
    total_rows: usize,
) -> f64 {
    let num_morsels = (total_rows + morsel_size - 1) / morsel_size;

    // Scheduling overhead per morsel
    let scheduling_cost = num_morsels as f64 * 0.001;

    // Work imbalance: last batch of morsels may leave workers idle
    let imbalance_factor = if num_morsels > num_workers * 10 {
        1.02 // Negligible with many morsels
    } else {
        1.0 + 1.0 / (num_morsels as f64 / num_workers as f64)
    };

    // Synchronization cost for shared state
    let sync_cost = sequential_cost * 0.05; // ~5% overhead

    (sequential_cost / num_workers as f64) * imbalance_factor
        + scheduling_cost + sync_cost
}
```

## Cost Model

**Parallel Speedup:**
- Ideal: `sequential_time / num_workers`
- Practical: ~85-95% of ideal (scheduling + sync overhead)
- Memory-bandwidth bound: speedup plateaus at ~8-16 cores for scans

**Morsel Scheduling Overhead:**
- Per-morsel dispatch: ~100 ns (atomic fetch-add)
- Work-stealing: ~500 ns per steal attempt
- Total overhead: `num_morsels x 100 ns`

**Load Balancing:**
- Equal-size morsels: good balance for uniform work
- Variable per-tuple cost: work-stealing compensates
- Imbalance factor: < 2% with > 10x morsels per worker

**Elastic Scheduling:**
- Rebalancing interval: every pipeline stage
- Migration cost: ~0 (no state transfer for pipelined operators)
- Benefit: fair resource sharing under concurrent load

## Test Cases

```sql
-- Test 1: Embarrassingly parallel scan
SELECT * FROM lineitem WHERE l_quantity < 25;
-- Expected: Linear speedup, each worker gets equal morsels
-- 6M rows / 100K morsel = 60 morsels, 16 workers => 4 morsels each

-- Test 2: Parallel with pipeline breaker
SELECT l_returnflag, SUM(l_extendedprice)
FROM lineitem GROUP BY l_returnflag;
-- Expected: Parallel scan+aggregate with thread-local HTs
-- Finalize: merge 16 thread-local HTs into one

-- Test 3: Concurrent queries (elasticity)
-- Q1: SELECT COUNT(*) FROM lineitem; (long running)
-- Q2: SELECT * FROM orders WHERE id = 42; (short)
-- Expected: Q2 gets workers quickly, Q1 yields workers

-- Test 4: Skewed workload (work-stealing benefit)
SELECT * FROM events WHERE expensive_udf(payload);
-- Per-tuple cost varies 10x depending on payload
-- Expected: Work-stealing balances unequal morsel processing times
```

## Comparison with Other Models

| Aspect | Morsel-Driven | Exchange (Volcano) | MapReduce |
|--------|--------------|-------------------|-----------|
| Parallelism unit | Morsel (10K-100K) | Partition (1/N) | Split (64MB) |
| Load balancing | Work-stealing | Static partition | Task retry |
| Elasticity | Per-pipeline | Fixed at plan time | None |
| Skew handling | Automatic | Manual repartition | Speculative exec |
| Shared memory | Yes | Exchange channels | No (distributed) |

## References

1. **Leis, Viktor; Boncz, Peter; Kemper, Alfons; Neumann, Thomas**. "Morsel-Driven Parallelism: A NUMA-Aware Query Evaluation Framework for the Many-Core Age." SIGMOD 2014.
   - Foundational paper on morsel-driven parallelism

2. **Raasveldt, Mark; Muhleisen, Hannes**. "DuckDB: an Embeddable Analytical Database." SIGMOD 2019.
   - Morsel-driven parallelism in DuckDB

3. **Graefe, Goetz**. "Encapsulation of Parallelism in the Volcano Query Processing System." SIGMOD 1990.
   - Exchange-operator parallelism (predecessor approach)

4. **Cieslewicz, John; Ross, Kenneth A**. "Adaptive Aggregation on Chip Multiprocessors." VLDB 2007.
   - Parallel aggregation strategies on multi-core
