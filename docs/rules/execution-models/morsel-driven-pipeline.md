# Rule: Morsel-Driven Pipeline Scheduling

**Category:** execution-models
**File:** `rules/execution-models/morsel-driven/morsel-driven-pipeline.rra`

## Metadata

- **ID:** `morsel-driven-pipeline`
- **Version:** 1.0.0
- **Databases:** HyPer, DuckDB, Umbra
- **Tags:** execution, parallel, morsel, pipeline, scheduling, dependency
- **SQL Standard:** HyPer morsel model
- **Authors:** Viktor Leis, Thomas Neumann


# Morsel-Driven Pipeline Scheduling

## Description

In morsel-driven execution, a query plan is decomposed into pipelines separated by pipeline breakers (materializing operators). The pipeline scheduler determines the execution order of pipelines and manages dependencies between them. Within each pipeline, morsel-level parallelism applies. Between pipelines, barriers ensure that build-side materializations complete before probe-side pipelines begin.

**Pipeline scheduling concepts:**
- **Pipeline dependency graph**: DAG of pipelines with data dependencies
- **Pipeline barrier**: Synchronization point between dependent pipelines
- **Pipeline parallelism**: Independent pipelines can execute concurrently
- **Intra-pipeline parallelism**: Workers process morsels within one pipeline
- **Pipeline priority**: Short pipelines scheduled first for early output

**Key characteristics:**
- **Automatic pipeline identification**: Compiler splits plan at breakers
- **Dependency-driven scheduling**: Topological order of pipeline DAG
- **All workers on one pipeline**: Maximize intra-pipeline parallelism
- **Pipeline switching**: All workers move to next pipeline together
- **Inter-pipeline parallelism**: Multiple independent pipelines overlap

**Trade-offs:**
- Pipeline barriers introduce synchronization points
- Short pipelines may not utilize all workers efficiently
- Complex query plans produce many pipelines (scheduling overhead)
- Pipeline switching has coordination cost

## Relational Algebra

```
Query plan -> Pipeline decomposition:

SELECT c.name, SUM(o.amount)
FROM customers c
JOIN orders o ON c.id = o.cust_id
WHERE o.date > '2024-01-01'
GROUP BY c.name
ORDER BY SUM(o.amount) DESC;

Pipeline decomposition:
  P1: Scan(customers) -> Build HT_customers      [pipeline breaker]
  P2: Scan(orders) -> Filter(date) -> Probe HT_customers
      -> Aggregate(group_by name, SUM(amount))    [pipeline breaker]
  P3: Scan(agg_ht) -> Sort                        [pipeline breaker]
  P4: Scan(sorted) -> Output

Dependency graph:
  P1 -> P2 -> P3 -> P4

Execution schedule:
  Phase 1: All workers execute P1 (build customers HT)
  Barrier: Wait for P1 completion
  Phase 2: All workers execute P2 (probe + aggregate)
  Barrier: Wait for P2 completion
  Phase 3: All workers execute P3 (sort)
  Barrier: Wait for P3 completion
  Phase 4: All workers execute P4 (output)
```

## Implementation

```rust
use std::sync::{Arc, Barrier};

/// Pipeline: a maximal chain of pipelineable operators
pub struct Pipeline {
    id: PipelineId,
    operators: Vec<Operator>,
    source: PipelineSource,
    sink: PipelineSink,
    dependencies: Vec<PipelineId>,
}

/// Pipeline scheduler for morsel-driven execution
pub struct PipelineScheduler {
    pipelines: Vec<Pipeline>,
    dependency_graph: DependencyGraph,
    num_workers: usize,
}

impl PipelineScheduler {
    /// Decompose query plan into pipelines
    pub fn decompose(plan: &QueryPlan) -> Vec<Pipeline> {
        let mut pipelines = Vec::new();
        let mut current = Pipeline::new();
        let mut next_id = 0;

        plan.walk_bottom_up(|op| {
            if op.is_pipeline_breaker() {
                // End current pipeline at this breaker
                current.sink = PipelineSink::Materialize(op.id());
                current.id = PipelineId(next_id);
                next_id += 1;
                pipelines.push(current);

                // Start new pipeline from breaker's output
                current = Pipeline::new();
                current.source = PipelineSource::from_breaker(op.id());
                current.dependencies.push(PipelineId(next_id - 1));
            } else {
                current.operators.push(op.clone());
            }
        });

        // Add final pipeline
        if !current.operators.is_empty() {
            current.id = PipelineId(next_id);
            pipelines.push(current);
        }

        pipelines
    }

    /// Execute pipelines respecting dependencies
    pub fn execute(&self) -> Result<Vec<Batch>> {
        // Topological sort of pipeline DAG
        let execution_order = self.dependency_graph
            .topological_sort();

        let mut results = Vec::new();

        for pipeline_id in execution_order {
            let pipeline = &self.pipelines[pipeline_id.0];

            // Check: can we overlap with other pipelines?
            let independent = self.find_independent_pipelines(
                pipeline_id,
                &execution_order,
            );

            if independent.is_empty() {
                // Execute single pipeline with all workers
                let batch_results = self.execute_pipeline(
                    pipeline,
                    self.num_workers,
                )?;
                results.extend(batch_results);
            } else {
                // Execute independent pipelines concurrently
                let total_work: f64 = std::iter::once(pipeline)
                    .chain(independent.iter().map(|id| &self.pipelines[id.0]))
                    .map(|p| p.estimated_work())
                    .sum();

                // Allocate workers proportional to estimated work
                for p in std::iter::once(pipeline_id)
                    .chain(independent.iter().copied())
                {
                    let pip = &self.pipelines[p.0];
                    let share = pip.estimated_work() / total_work;
                    let workers = ((share * self.num_workers as f64)
                        .round() as usize)
                        .max(1);

                    let r = self.execute_pipeline(pip, workers)?;
                    results.extend(r);
                }
            }
        }

        Ok(results)
    }

    /// Execute a single pipeline with given number of workers
    fn execute_pipeline(
        &self,
        pipeline: &Pipeline,
        num_workers: usize,
    ) -> Result<Vec<Batch>> {
        let source = pipeline.create_morsel_source();
        let shared_state = pipeline.create_shared_state()?;
        let barrier = Arc::new(Barrier::new(num_workers));

        let handles: Vec<_> = (0..num_workers)
            .map(|worker_id| {
                let src = source.clone();
                let state = shared_state.clone();
                let bar = barrier.clone();
                let ops = pipeline.operators.clone();

                std::thread::spawn(move || {
                    let mut local = ThreadLocalState::new();

                    while let Some(morsel) = src.next_morsel() {
                        // Execute pipeline operators on morsel
                        for op in &ops {
                            op.process_morsel(
                                &morsel, &state, &mut local,
                            );
                        }
                    }

                    bar.wait(); // Pipeline barrier
                    local
                })
            })
            .collect();

        // Collect thread-local state
        let locals: Vec<_> = handles
            .into_iter()
            .map(|h| h.join().expect("Worker panicked"))
            .collect();

        // Finalize pipeline (merge thread-local state)
        pipeline.finalize(shared_state, locals)
    }
}

/// Cost model for pipeline scheduling
pub fn pipeline_schedule_cost(
    pipelines: &[Pipeline],
    num_workers: usize,
) -> f64 {
    let mut total_cost = 0.0;

    for pipeline in pipelines {
        // Pipeline execution cost (parallel)
        let sequential_cost = pipeline.estimated_work();
        let parallel_cost = sequential_cost / num_workers as f64;

        // Barrier cost
        let barrier_cost = 0.01; // ~10 us per barrier

        // Thread coordination cost
        let coord_cost = num_workers as f64 * 0.001;

        total_cost += parallel_cost + barrier_cost + coord_cost;
    }

    total_cost
}
```

## Cost Model

**Pipeline Execution:**
- Intra-pipeline: morsel-level parallelism (near-linear speedup)
- Barrier overhead: ~10 us per pipeline boundary
- Workers idle at barriers: proportional to work imbalance

**Pipeline Scheduling Overhead:**
- Decomposition: O(plan_size) one-time
- Dependency resolution: O(pipelines) topological sort
- Worker coordination: O(workers) per pipeline switch

**Inter-Pipeline Parallelism:**
- Independent pipelines overlap: reduces total execution time
- Example: two separate joins can build HTs concurrently
- Benefit: depends on plan shape (bushy plans have more independence)

**Total Query Cost:**
- Sum of pipeline costs + barrier overhead + coordination
- Dominated by longest pipeline (critical path)

## Test Cases

```sql
-- Test 1: Linear pipeline chain
SELECT * FROM lineitem WHERE l_quantity < 25 ORDER BY l_shipdate;
-- Pipelines: P1(scan+filter -> sort), P2(scan sorted -> output)
-- Sequential execution: P1 then P2

-- Test 2: Bushy plan with independent pipelines
SELECT *
FROM (SELECT * FROM A JOIN B ON A.id = B.a_id) AB
JOIN (SELECT * FROM C JOIN D ON C.id = D.c_id) CD
ON AB.id = CD.ab_id;
-- P1: Build HT(B) | P2: Build HT(D)  -- independent, overlap
-- P3: Probe A->B  | P4: Probe C->D  -- independent, overlap
-- P5: Build HT(CD), P6: Probe AB->CD

-- Test 3: Many small pipelines
SELECT DISTINCT region FROM orders;
-- P1: scan -> aggregate (few groups)
-- P2: scan HT -> output
-- Short pipelines: most time at barriers

-- Test 4: Pipeline with high-selectivity filter
SELECT COUNT(*) FROM lineitem WHERE l_quantity = 1;
-- One pipeline: scan+filter -> aggregate
-- ~1% selectivity: morsels finish quickly
-- Work-stealing handles imbalance from early-terminating morsels
```

## References

1. **Leis, Viktor et al**. "Morsel-Driven Parallelism." SIGMOD 2014.
   - Pipeline decomposition and scheduling

2. **Neumann, Thomas**. "Efficiently Compiling Efficient Query Plans for Modern Hardware." VLDB 2011.
   - Pipeline identification for compiled execution

3. **Raasveldt, Mark; Muhleisen, Hannes**. "DuckDB: an Embeddable Analytical Database." SIGMOD 2019.
   - Pipeline scheduling in DuckDB

4. **Graefe, Goetz**. "Volcano: An Extensible and Parallel Query Evaluation System." IEEE TKDE 1994.
   - Exchange-based pipeline parallelism (predecessor)
