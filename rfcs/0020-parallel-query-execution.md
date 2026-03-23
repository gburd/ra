# RFC 0020: Parallel Query Execution

- Start Date: 2026-03-20
- Author: System
- Status: Implemented

## Summary

Add parallel query execution support with parallel scans, joins, and aggregations using multiple worker processes/threads.

## Motivation

Modern CPUs have many cores. Single-threaded query execution underutilizes hardware:
- 16-core server running at 6% utilization (1 core busy)
- 5-minute query could complete in 20 seconds with parallelism

PostgreSQL supports parallel query since v9.6. RA has no parallel execution modeling.

## Technical design

### Parallel Operators

```rust
pub enum RelExpr {
    // ...
    ParallelScan {
        table: String,
        workers: usize,
    },
    ParallelHashJoin {
        join_type: JoinType,
        condition: Expr,
        left: Box<RelExpr>,
        right: Box<RelExpr>,
        workers: usize,
    },
    ParallelAggregate {
        group_by: Vec<Expr>,
        aggregates: Vec<AggregateExpr>,
        input: Box<RelExpr>,
        workers: usize,
    },
    Gather {
        input: Box<RelExpr>,
        workers: usize,
    },
}
```

### Parallelization Rules

```yaml
# rules/parallel/parallel-seq-scan.rra
name: parallel-seq-scan
pattern: |
  Scan(table)
condition: |
  table_size(table) > parallel_threshold &&
  hardware.cpu_cores >= 2
transform: |
  Gather(ParallelScan(table, workers = cpu_cores - 1))
cost_benefit: |
  scan_time / workers - gather_overhead
```

### Cost Model

```rust
impl CostModel {
    fn parallel_scan_cost(&self, table: &str, workers: usize) -> Cost {
        let seq_cost = self.seq_scan_cost(table);
        let parallel_speedup = self.parallel_efficiency(workers);

        Cost::new(
            seq_cost.io / parallel_speedup,
            seq_cost.cpu / parallel_speedup,
            0.0,
            seq_cost.memory * workers,  // Workers need separate buffers
        )
    }

    fn parallel_efficiency(&self, workers: usize) -> f64 {
        // Amdahl's law + coordination overhead
        // Perfect speedup: 1.0 * workers
        // Reality: diminishing returns
        let speedup = workers as f64 * 0.8;  // 80% efficiency
        let overhead = workers as f64 * 0.05;  // 5% overhead per worker
        (speedup - overhead).max(1.0)
    }
}
```

### Worker Configuration

```toml
[parallel]
max_parallel_workers = 8
max_parallel_workers_per_gather = 4
parallel_tuple_cost = 0.1
parallel_setup_cost = 1000.0
min_parallel_table_scan_size = 8_388_608  # 8 MB
```

## Challenges

1. **Memory management**: Workers need separate buffer pools
2. **Synchronization**: Lock contention, barrier synchronization
3. **Cost estimation**: Hard to predict parallel efficiency
4. **Testing**: Parallel execution is non-deterministic

## Implementation plan

- Week 1-2: Parallel scan and gather
- Week 3-4: Parallel hash join
- Week 5-6: Parallel aggregation
- Week 7-8: Cost model tuning and testing

## Gap addressed

Gap #6.1 (High severity) from postgres-planner-gaps.md - parallel query support
