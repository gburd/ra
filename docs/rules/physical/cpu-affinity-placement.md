# Rule: CPU Affinity Placement

**Category:** physical/parallelization
**File:** `rules/physical/parallelization/cpu-affinity-placement.rra`

## Metadata

- **ID:** `cpu-affinity-placement`
- **Version:** "1.0.0"
- **Databases:** hyper, umbra, oracle, sap-hana, clickhouse
- **Tags:** parallelization, affinity, thread-pinning, cache, scheduling
- **Authors:** "RA Contributors"


# CPU Affinity Placement

## Metadata
- **Rule ID**: `cpu-affinity-placement`
- **Category**: Physical / Parallelization
- **Complexity**: O(n/p) with improved constant factor from cache effects
- **Introduced**: HyPer, Oracle RAC, sap-hana thread management
- **Prerequisites**: OS CPU affinity API, known CPU topology
- **Alternatives**: numa-aware-scheduling, work-stealing-parallelism

## Description

CPU affinity placement pins query worker threads to specific CPU cores,
preventing the OS scheduler from migrating threads across cores. This
preserves L1/L2 cache contents, avoids TLB flushes from migration, and
ensures predictable performance.

The placement strategy considers CPU topology: threads sharing data should
be placed on cores sharing an L2/L3 cache, while independent pipeline
segments should use separate cache domains to avoid contention.

**When to use:**
- Long-running queries where cache warming matters
- Pipeline operators that share intermediate data
- Latency-sensitive queries requiring predictable performance
- Systems with heterogeneous core types (P-cores vs E-cores)

**Advantages:**
- Preserves L1/L2 cache across operator invocations
- Avoids TLB flush from thread migration
- Predictable latency (no OS scheduling jitter)
- Enables co-location of communicating operators

**Disadvantages:**
- Reduces OS scheduler flexibility
- Can cause imbalance if pinned cores are shared with other processes
- Requires accurate CPU topology information
- Over-pinning can prevent beneficial migration

## Relational Algebra

```
Pipeline(op1, op2, op3) on worker W:
  pin(W, core_id)
  // All three operators execute on same core
  // op1's output in L1 cache when op2 reads it
  // No migration between operators

Placement rules:
  - Pipeline stages: same core (cache sharing)
  - Independent pipelines: different L3 domains (no contention)
  - Build/probe: same NUMA node (memory locality)
```

## Implementation (egg rewrite rules)

```lisp
;; Pin pipeline workers to cores
(rewrite (parallel-pipeline ?ops ?workers)
  (affinity-pipeline ?ops ?workers
    :placement co-locate-pipeline
    :policy pin-to-core)
  :if (> (pipeline-length ?ops) 2)
  :if (> (estimated-runtime-ms ?ops) 100))

;; Place communicating operators on cores sharing L2 cache
(rewrite (producer-consumer ?producer ?consumer ?workers)
  (affinity-pair ?producer ?consumer ?workers
    :constraint same-l2-cache)
  :if (> (data-volume ?producer ?consumer) (* 1024 1024)))

;; Separate independent pipelines to different cache domains
(rewrite (parallel-independent ?pipeline1 ?pipeline2)
  (affinity-separate ?pipeline1 ?pipeline2
    :constraint different-l3-cache)
  :if (no-shared-data ?pipeline1 ?pipeline2))

;; Heterogeneous cores: place compute-heavy on P-cores
(rewrite (parallel-execute ?op ?workers)
  (affinity-execute ?op ?workers
    :core-type performance)
  :if (is-compute-heavy ?op)
  :if (has-heterogeneous-cores))
```

## Implementation Pattern

```rust
pub struct AffinityPlacement {
    topology: CpuTopology,
    assignments: HashMap<WorkerId, CoreId>,
}

struct CpuTopology {
    cores: Vec<CoreInfo>,
    l2_groups: Vec<Vec<usize>>,   // Cores sharing L2
    l3_groups: Vec<Vec<usize>>,   // Cores sharing L3
    numa_groups: Vec<Vec<usize>>, // Cores per NUMA node
}

struct CoreInfo {
    id: usize,
    socket: usize,
    physical_core: usize,
    is_hyperthread: bool,
    core_type: CoreType, // Performance or Efficiency
}

enum CoreType {
    Performance,
    Efficiency,
}

impl AffinityPlacement {
    fn place_pipeline(
        &mut self,
        pipeline: &Pipeline,
        num_workers: usize,
    ) {
        // Assign workers to cores, keeping pipeline on same core
        let available = self.topology.available_cores();

        for (i, worker) in pipeline.workers().enumerate() {
            let core = available[i % available.len()];
            self.assignments.insert(worker.id, core);

            // Pin thread to core via OS API
            set_thread_affinity(worker.thread_id, core);
        }
    }

    fn place_communicating_pair(
        &mut self,
        producer: &Operator,
        consumer: &Operator,
    ) {
        // Find cores sharing L2 cache
        for l2_group in &self.topology.l2_groups {
            if l2_group.len() >= 2 {
                let prod_core = l2_group[0];
                let cons_core = l2_group[1];

                self.assignments.insert(
                    producer.worker_id(), prod_core
                );
                self.assignments.insert(
                    consumer.worker_id(), cons_core
                );
                return;
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn set_thread_affinity(tid: u64, core: usize) {
    // Linux: sched_setaffinity via libc
    let mut cpuset = libc::cpu_set_t::default();
    unsafe {
        libc::CPU_SET(core, &mut cpuset);
        libc::sched_setaffinity(
            tid as i32,
            std::mem::size_of::<libc::cpu_set_t>(),
            &cpuset,
        );
    }
}
```

## Cost Model

```rust
pub fn cost_with_affinity(
    base_cost: Cost,
    pipeline_length: usize,
    data_volume_bytes: u64,
    cache_hit_improvement: f64,
    hardware: &HardwareModel,
) -> Cost {
    // Cache benefit: data stays in L1/L2 across pipeline stages
    let cache_savings = data_volume_bytes as f64
        * pipeline_length as f64
        * cache_hit_improvement
        * (hardware.l2_latency_ns() - hardware.l1_latency_ns());

    // Migration avoidance: no TLB flushes
    let migration_savings = Cost::cpu(
        pipeline_length as u64 * 1000, // ~1us per avoided migration
    );

    base_cost - Cost::cpu(cache_savings as u64) - migration_savings
}

pub fn affinity_benefit(
    l1_hit_rate_before: f64,
    l1_hit_rate_after: f64,
    l1_latency_ns: f64,
    l2_latency_ns: f64,
    memory_accesses: u64,
) -> f64 {
    let before = memory_accesses as f64 * (
        l1_hit_rate_before * l1_latency_ns
            + (1.0 - l1_hit_rate_before) * l2_latency_ns
    );
    let after = memory_accesses as f64 * (
        l1_hit_rate_after * l1_latency_ns
            + (1.0 - l1_hit_rate_after) * l2_latency_ns
    );
    (before - after) / before
}
```

## Test Cases

### Test 1: Multi-stage pipeline benefits from core pinning
```sql
SELECT customer_id, SUM(amount)
FROM orders
WHERE date >= '2025-01-01'
GROUP BY customer_id
HAVING SUM(amount) > 1000;

-- Pipeline: Scan -> Filter -> Aggregate -> Having
-- Expected: Pin entire pipeline to single core per worker
-- Filter output stays in L1 when aggregate reads it
-- No cache eviction from thread migration
```

### Test 2: Producer-consumer on shared L2
```sql
SELECT *
FROM (SELECT id, expensive_calc(data) as result FROM raw_data) sub
WHERE result > threshold;

-- Expected: Pin producer (subquery) and consumer (filter)
--   to cores sharing L2 cache
-- expensive_calc output (~64 bytes/row) stays in L2
-- Consumer reads from L2 instead of L3/memory
```

### Test 3: Heterogeneous cores (Intel Alder Lake / Apple M-series)
```sql
-- System with P-cores and E-cores
SELECT complex_aggregation(*)
FROM large_analytics_table;

-- Expected: Pin compute-heavy aggregation to P-cores
-- E-cores handle background I/O prefetch
-- P-cores get full clock speed and cache for computation
```

### Test 4: Negative -- short query, overhead not justified
```sql
SELECT * FROM small_table WHERE id = 42;

-- NOT suitable: query runs in microseconds
-- Affinity setup overhead exceeds any cache benefit
-- Let OS scheduler handle normally
```

## Performance Characteristics

| Scenario | Without Affinity | With Affinity | Improvement |
|----------|-----------------|---------------|-------------|
| 4-stage pipeline, 1M rows | Baseline | 10-20% faster | Cache hits |
| P-core vs E-core placement | Baseline | 30-50% faster | Clock speed |
| Long-running OLAP | Baseline | 5-15% faster | No migration |
| Short OLTP lookup | Baseline | ~0% (overhead) | None |

## References

1. **Leis et al.**: "Morsel-Driven Parallelism"
   - SIGMOD 2014, thread-to-core assignment in HyPer

2. **Balkesen et al.**: "Multi-Core, Main-Memory Joins: Sort vs. Hash Revisited"
   - VLDB 2013, cache-conscious thread placement for joins

3. **ClickHouse**: Thread pool and CPU affinity configuration
   - https://clickhouse.com/docs/en/operations/server-configuration-parameters

4. **Intel**: "Threading Building Blocks -- Task Affinity"
   - Thread affinity for cache-conscious scheduling
