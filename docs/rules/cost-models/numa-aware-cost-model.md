# Rule: "NUMA-Aware Operator Placement Cost Model"

**Category:** cost-models
**File:** `rules/cost-models/numa-aware-cost-model.rra`

## Metadata

- **ID:** `numa-aware-cost-model`
- **Version:** "1.0.0"
- **Databases:** singlestore, sap-hana, oracle, postgresql
- **Tags:** cost, numa, placement, memory-locality, socket, morsel, parallel
- **Authors:** "Leis et al. 2014 - morsel-driven parallelism", "Li et al. 2013 - NUMA-aware databases"


# NUMA-Aware Operator Placement Cost Model

## Description

Models the cost difference between local and remote memory access in
Non-Uniform Memory Architecture (NUMA) systems. Modern multi-socket
servers have 2-8 NUMA nodes, each with local DRAM attached. Local
memory access costs ~100ns; remote access via interconnect (UPI/Infinity
Fabric) costs ~150-300ns, a 1.5-3x penalty. For memory-bandwidth-bound
query operators, NUMA placement can determine whether performance
scales linearly with sockets or hits a bandwidth wall.

**When to apply**: Any parallel operator on a multi-socket system.
Determines data partitioning across NUMA nodes, thread-to-core
affinity, and morsel scheduling.

**Why it works**: Partitioning data so each NUMA node processes its
local portion eliminates cross-socket memory traffic. For hash joins,
this means partitioning the build side across nodes and scheduling
probe morsels to match. The morsel-driven execution model (HyPer/Umbra)
was designed specifically for NUMA-awareness.

## Relational Algebra

```algebra
-- NUMA cost model:
numa_cost(op, placement) =
  local_accesses(op, placement) * LOCAL_LATENCY
  + remote_accesses(op, placement) * REMOTE_LATENCY

-- Bandwidth model:
numa_bandwidth(op, placement) =
  min(
    local_bw * local_fraction + remote_bw * remote_fraction,
    interconnect_bw  -- UPI/IF bottleneck
  )

-- Optimal placement: partition data across NUMA nodes
partition_cost(R, num_nodes) =
  BYTES(R) / (num_nodes * local_bw)  -- ideal linear scaling

-- Suboptimal: all data on one node, all nodes access it
contended_cost(R, num_nodes) =
  BYTES(R) / min(local_bw, interconnect_bw * (num_nodes - 1))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

struct NumaTopology {
    num_nodes: usize,
    local_latency_ns: f64,
    remote_latency_ns: f64,
    local_bw_gbps: f64,
    remote_bw_gbps: f64,
    interconnect_bw_gbps: f64,
    cores_per_node: usize,
}

impl NumaTopology {
    fn memory_access_cost(
        &self,
        data_bytes: f64,
        local_fraction: f64,
    ) -> f64 {
        let remote_fraction = 1.0 - local_fraction;

        let local_time =
            data_bytes * local_fraction
                / (self.local_bw_gbps * 1e9)
                * 1e9;

        let remote_time =
            data_bytes * remote_fraction
                / (self.remote_bw_gbps * 1e9)
                * 1e9;

        local_time + remote_time
    }

    fn hash_join_placement(
        &self,
        build_bytes: f64,
        probe_bytes: f64,
    ) -> NumaPlacement {
        let per_node_build = build_bytes / self.num_nodes as f64;

        // Option 1: Replicate build on each node
        let replicate_cost =
            build_bytes * self.num_nodes as f64 // memory cost
                / (self.local_bw_gbps * 1e9) * 1e9
            + probe_bytes // probe is local
                / (self.local_bw_gbps * 1e9
                    * self.num_nodes as f64)
                * 1e9;

        // Option 2: Partition build, route probes
        let partition_cost =
            build_bytes
                / (self.local_bw_gbps * 1e9
                    * self.num_nodes as f64)
                * 1e9
            + probe_bytes // some probes are remote
                / (self.remote_bw_gbps * 1e9
                    * self.num_nodes as f64)
                * 1e9;

        // Option 3: Partition both, co-locate
        let colocate_cost =
            (build_bytes + probe_bytes)
                / (self.local_bw_gbps * 1e9
                    * self.num_nodes as f64)
                * 1e9;

        if colocate_cost <= replicate_cost
            && colocate_cost <= partition_cost
        {
            NumaPlacement::ColocatePartitioned
        } else if replicate_cost <= partition_cost {
            NumaPlacement::ReplicateBuild
        } else {
            NumaPlacement::PartitionBuild
        }
    }

    fn morsel_scheduling_cost(
        &self,
        data_bytes: f64,
        morsel_size: usize,
        steal_rate: f64, // fraction stolen from other nodes
    ) -> f64 {
        let local_time = data_bytes * (1.0 - steal_rate)
            / (self.local_bw_gbps * 1e9)
            * 1e9;

        let stolen_time = data_bytes * steal_rate
            / (self.remote_bw_gbps * 1e9)
            * 1e9;

        local_time + stolen_time
    }

    fn scaling_efficiency(
        &self,
        data_bytes: f64,
        local_fraction: f64,
    ) -> f64 {
        let ideal = data_bytes
            / (self.local_bw_gbps * 1e9
                * self.num_nodes as f64);

        let actual = self.memory_access_cost(
            data_bytes / self.num_nodes as f64,
            local_fraction,
        );

        if actual > 0.0 { ideal / actual } else { 1.0 }
    }
}
```

## Preconditions

```rust
fn applicable(system: &SystemConfig) -> bool {
    system.numa_nodes() > 1
}
```

**Restrictions:**
- NUMA topology varies: 2-socket (common) to 8-socket (high-end)
- AMD EPYC has NUMA domains within a single socket (CCX/CCD)
- OS memory allocation policy (interleave, first-touch, bind) affects placement
- Work stealing across NUMA nodes trades locality for load balance
- Small data sets (<L3 cache) make NUMA effects negligible

## Cost Model

```rust
fn numa_penalty_ratio(
    num_nodes: usize,
    local_bw: f64,
    remote_bw: f64,
) -> f64 {
    // Worst case: all remote access
    // Best case: all local access
    // Ratio indicates max penalty for poor placement
    local_bw / remote_bw
}
```

**Typical NUMA penalties:**
- Intel Xeon 2-socket (UPI): 1.5-2x for remote access
- AMD EPYC 2-socket (IF): 1.3-1.8x for remote access
- AMD EPYC intra-socket (CCD): 1.1-1.3x for cross-CCD
- 4-socket systems: 2-3x for 2-hop remote access

## Test Cases

### Positive: Hash join with NUMA-aware partitioning

```sql
-- 4-socket system, 100GB hash table
-- Without NUMA awareness: random placement, 50% remote
-- Access cost: 100GB * (0.5 * 100ns + 0.5 * 200ns) = 15s
-- With NUMA partitioning: 25GB per node, all local
-- Access cost: 25GB * 100ns per node (parallel) = 2.5s
-- 6x improvement from NUMA-aware placement
SELECT * FROM orders o JOIN lineitem l ON o.orderkey = l.orderkey;
```

### Positive: Morsel-driven scan with affinity

```sql
-- 2-socket, 200GB table, first-touch allocation
-- NUMA-aware morsels: each core scans local memory
-- Achieves 95% local access, 5% work-stealing
-- Scaling: 1.9x on 2 sockets (95% efficiency)
SELECT SUM(amount) FROM transactions WHERE year = 2025;
```

### Negative: Small table (fits in L3)

```sql
-- 100KB table: fits in L3 cache shared across cores
-- NUMA effects negligible, cache locality dominates
SELECT * FROM config WHERE key = 'timeout';
```

## References

**NUMA-aware databases:**
- Leis et al., "Morsel-Driven Parallelism: A NUMA-Aware Query Evaluation Framework", SIGMOD 2014
  - Morsel scheduling with NUMA-local dispatch
- Li et al., "NUMA-Aware Algorithms: the Case of Data Shuffling", CIDR 2013
  - NUMA-conscious partitioning algorithms

**NUMA effects on databases:**
- Psaroudakis et al., "Scaling Up Concurrent Main-Memory Column-Store Scans", VLDB 2015
  - NUMA effects on concurrent scans in sap-hana
- Balkesen et al., "NUMA-Aware Hash Joins on Multi-Core CPUs", VLDB 2015
  - NUMA-partitioned hash join implementations
