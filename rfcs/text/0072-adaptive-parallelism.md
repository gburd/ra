# RFC 0072: Adaptive Parallelism

- **Status**: Proposed
- **Priority**: High Impact (3-4 months)
- **Impact**: 2-4x improvement on multi-core systems
- **Category**: Execution / Parallelism
- **Created**: 2026-03-25

## Summary

Automatically determine optimal degree of parallelism (DOP) per query and operator based on workload, hardware, and system load. Addresses the problem that static parallelism settings waste resources or miss opportunities.

## Motivation

**Problem**: How many cores should a query use?
- Too few: Underutilize hardware (2x slowdown)
- Too many: Overhead exceeds benefit (coordination cost)

**Current Ra**: Sequential execution only

**DuckDB**: Work-stealing with adaptive DOP → 4-10x speedup on OLAP

## Proposal

### DOP Estimation

```rust
fn estimate_dop(&self, query: &RelExpr) -> usize {
    let data_size = self.estimate_bytes(query);
    let parallelizable_ops = count_parallelizable_ops(query);
    let available_cores = num_cpus::get();
    let active_queries = self.get_active_query_count();

    // Heuristic: 1 core per 10MB of data, capped by available cores
    let ideal_dop = (data_size / (10 * 1024 * 1024)) as usize;
    let max_dop = available_cores / (active_queries + 1);

    ideal_dop.min(max_dop).max(1)
}
```

### Parallel Operators

**Parallel scan**:
```rust
impl ScanOperator {
    fn execute_parallel(&self, dop: usize) -> Vec<Tuple> {
        let chunks = self.partition_data(dop);

        chunks.par_iter()
            .flat_map(|chunk| self.scan_chunk(chunk))
            .collect()
    }
}
```

**Parallel hash join**:
```rust
impl HashJoinOperator {
    fn execute_parallel(&self, dop: usize) -> Vec<Tuple> {
        // Phase 1: Parallel build (partition by hash)
        let partitions = self.build_side.par_iter()
            .fold(|| vec![HashMap::new(); dop],
                  |mut parts, tuple| {
                      let hash = self.hash_key(tuple);
                      parts[hash % dop].insert(tuple.key, tuple);
                      parts
                  })
            .reduce(|| vec![HashMap::new(); dop],
                    |a, b| merge_partitions(a, b));

        // Phase 2: Parallel probe
        self.probe_side.par_iter()
            .flat_map(|tuple| {
                let hash = self.hash_key(tuple);
                partitions[hash % dop].get(&tuple.key)
            })
            .collect()
    }
}
```

### Work-Stealing Scheduler

```rust
pub struct WorkStealingScheduler {
    work_queues: Vec<Mutex<VecDeque<Task>>>,
    workers: Vec<JoinHandle<()>>,
}

impl WorkStealingScheduler {
    fn worker_loop(&self, worker_id: usize) {
        loop {
            // Try local queue first
            if let Some(task) = self.work_queues[worker_id].lock().pop_front() {
                task.execute();
                continue;
            }

            // Steal from other workers
            for victim_id in 0..self.work_queues.len() {
                if victim_id == worker_id { continue; }

                if let Some(task) = self.work_queues[victim_id].lock().pop_back() {
                    task.execute();
                    break;
                }
            }
        }
    }
}
```

## Implementation Plan

### Phase 1: DOP Estimation (Month 1)
1. Implement `estimate_dop()` with heuristics
2. Add parallel scan operator
3. Test with synthetic workloads

### Phase 2: Parallel Joins (Month 2)
1. Implement parallel hash join (partitioned)
2. Add work-stealing scheduler
3. Validate: 2-4x speedup on large joins

### Phase 3: Adaptive Adjustment (Month 3)
1. Monitor core utilization
2. Adjust DOP if under/over-utilized
3. Add feedback loop

## Expected Impact

- Scan-heavy queries: 2-4x speedup
- Join-heavy queries: 2-3x speedup (coordination overhead)
- OLTP queries: No change (too small to benefit)

## Prior Art

- DuckDB work-stealing (Raasveldt & Mühleisen, CIDR 2019): 4-10x OLAP speedup
- PostgreSQL parallel query: 2-8x speedup, fixed DOP
