# Rule: NUMA-Aware Data Partitioning

**Category:** hardware/accelerator
**File:** `rules/hardware/accelerator/numa-aware-partitioning.rra`

## Metadata

- **ID:** `numa-aware-partitioning`
- **Version:** "1.0.0"
- **Databases:** hyper, umbra, singlestore, sap-hana
- **Tags:** numa, partitioning, locality, multi-socket, morsel
- **Authors:** "RA Contributors"


# NUMA-Aware Data Partitioning

## Description

Partitions data and assigns query operators to NUMA nodes so that
each core accesses local memory. On multi-socket systems, remote
memory access has 1.5-3x higher latency than local access. By
hash-partitioning tables across NUMA nodes and scheduling operators
to process local partitions, the optimizer avoids the remote memory
penalty.

**When to apply**: The system has multiple NUMA nodes (multi-socket
server) and the query processes enough data that memory access latency
is a bottleneck (not register/L1-resident).

**Why it works**: NUMA systems have asymmetric memory access times.
Local memory access takes ~80ns; remote access takes ~130-200ns.
For memory-bandwidth-bound operations (scans, hash probes), remote
access reduces throughput by 30-50%. NUMA-aware placement keeps
data accesses local, recovering that throughput.

## Relational Algebra

```algebra
op(R) -> numa_partition(R, num_nodes) |> parallel_op(per_node)
  where num_numa_nodes > 1
    AND size(R) > L3_cache_size
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("numa-aware-scan";
    "(scan ?table)" =>
    "(numa_parallel_scan ?table)"
    if multi_numa_system()
    if table_exceeds_cache("?table")
),

rw!("numa-aware-hash-join";
    "(join inner ?cond ?left ?right)" =>
    "(numa_partitioned_join inner ?cond ?left ?right)"
    if multi_numa_system()
    if either_side_exceeds_cache("?left", "?right")
),

rw!("numa-aware-aggregation";
    "(aggregate ?group_by ?aggs ?input)" =>
    "(numa_local_aggregate ?group_by ?aggs ?input)"
    if multi_numa_system()
    if input_exceeds_cache("?input")
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    hw.numa_nodes > 1
        && stats.row_count as u64 * stats.avg_row_size
            > hw.l3_cache_bytes
}
```

**Restrictions:**
- Only benefits multi-socket systems (no effect on single-socket)
- Hash partitioning for joins requires matching partition keys
- Skewed data causes NUMA node imbalance
- Table must be large enough that cache effects dominate

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> f64 {
    let data_bytes =
        stats.row_count as u64 * stats.avg_row_size;

    if data_bytes <= hw.l3_cache_bytes {
        return 0.0; // Data fits in cache, NUMA irrelevant
    }

    // Remote access penalty as fraction of total accesses
    // Without NUMA awareness: ~50% remote on 2-socket
    let remote_fraction =
        1.0 - 1.0 / hw.numa_nodes as f64;
    let remote_penalty = 0.5; // 50% slower for remote access
    let unaware_slowdown =
        1.0 + remote_fraction * remote_penalty;

    // With NUMA awareness: ~95% local
    let aware_remote_fraction = 0.05;
    let aware_slowdown =
        1.0 + aware_remote_fraction * remote_penalty;

    (unaware_slowdown - aware_slowdown) / unaware_slowdown
}
```

**Typical benefit**: 15-40% on dual-socket systems, more on 4+ socket
systems. Critical for in-memory analytics on large datasets.

## Test Cases

### Positive: Large scan on multi-socket system

```sql
-- 2-socket server, lineitem: 72 GB (exceeds L3 cache)
-- Without NUMA: 50% remote accesses at 1.5x latency
-- With NUMA: partitioned scan, 95% local accesses
SELECT * FROM lineitem WHERE l_quantity < 24;

-- Plan: NumaParallelScan(lineitem, partitions=2,
--        pred=l_quantity < 24)
```

### Positive: NUMA-partitioned hash join

```sql
-- Both tables large, hash-partition by join key across NUMA nodes
SELECT * FROM lineitem l
JOIN orders o ON l.l_orderkey = o.o_orderkey;

-- Plan: NumaPartitionedJoin(
--        partition_key=orderkey, numa_nodes=2,
--        left=lineitem, right=orders)
```

### Negative: Small table fits in L3 cache

```sql
-- nation: 25 rows, ~2KB, fits in L1 cache
SELECT * FROM nation WHERE n_regionkey = 1;

-- Plan: Scan(nation, pred=n_regionkey=1) -- no NUMA optimization
```

## References

**Implementation in databases:**
- HyPer/Umbra: Morsel-driven parallelism with NUMA awareness
- sap-hana: NUMA-aware memory allocation
- SingleStore (MemSQL): NUMA-local partitioning

**Academic papers:**
- Leis et al., "Morsel-Driven Parallelism: A NUMA-Aware Query Evaluation Framework for the Many-Core Age", SIGMOD 2014
- Li et al., "NUMA-aware Algorithms: the Case of Data Shuffling", CIDR 2013
- Psaroudakis et al., "Scaling Up Concurrent Main-Memory Column-Store Scans: Towards Adaptive NUMA-aware Data and Task Placement", VLDB 2015
