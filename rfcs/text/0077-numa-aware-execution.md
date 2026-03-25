# RFC 0077: NUMA-Aware Execution

- **Status**: Proposed
- **Priority**: Specialized (4-6 months)
- **Impact**: 20-40% improvement on NUMA systems
- **Category**: Execution / Hardware-Aware
- **Created**: 2026-03-25

## Summary

Bind worker threads to NUMA nodes, allocate memory from local nodes, and partition data by NUMA topology. Addresses the problem that remote memory access on NUMA systems is 2-3x slower than local access, but default execution ignores topology.

## Motivation

### NUMA Architecture

**NUMA** (Non-Uniform Memory Access):
- Multi-socket servers (2-8 sockets)
- Each socket has local memory
- Remote memory access: 2-3x latency penalty

**Example**: 2-socket system
- Socket 0: Cores 0-15, 64GB local memory
- Socket 1: Cores 16-31, 64GB local memory
- Local access: 100ns
- Remote access: 300ns (3x slower)

**Problem**: Default thread placement is random
- Worker on socket 0 accesses data on socket 1 → 3x slower

### Evidence

**Greenplum NUMA Optimization** (white paper, 2020):
- Bind workers to NUMA nodes
- Allocate memory from local node
- Result: 20-40% improvement on NUMA systems

**MongoDB NUMA Warning** (documentation):
- Disables NUMA interleaving by default
- Recommends `numactl --interleave=all` for better performance

## Proposal

### NUMA Topology Detection

```rust
pub struct NumaTopology {
    pub node_count: usize,
    pub nodes: Vec<NumaNode>,
}

pub struct NumaNode {
    pub node_id: usize,
    pub cpu_ids: Vec<usize>,
    pub memory_gb: u64,
}

impl NumaTopology {
    pub fn detect() -> Option<Self> {
        // Use libnuma or hwloc to detect topology
        #[cfg(target_os = "linux")]
        {
            let node_count = unsafe { numa_num_configured_nodes() };
            if node_count <= 1 {
                return None;  // Not a NUMA system
            }

            let nodes = (0..node_count).map(|node_id| {
                NumaNode {
                    node_id,
                    cpu_ids: get_cpus_for_node(node_id),
                    memory_gb: get_node_memory(node_id),
                }
            }).collect();

            Some(NumaTopology { node_count, nodes })
        }

        #[cfg(not(target_os = "linux"))]
        None
    }
}
```

### Worker Thread Binding

```rust
impl WorkerPool {
    pub fn new_numa_aware(topology: &NumaTopology) -> Self {
        let workers_per_node = num_cpus::get() / topology.node_count;

        let workers = topology.nodes.iter().flat_map(|node| {
            (0..workers_per_node).map(move |i| {
                let cpu_id = node.cpu_ids[i % node.cpu_ids.len()];
                Worker::new_pinned(cpu_id, node.node_id)
            })
        }).collect();

        Self { workers }
    }
}

impl Worker {
    fn new_pinned(cpu_id: usize, numa_node: usize) -> Self {
        let handle = std::thread::spawn(move || {
            // Bind thread to CPU
            unsafe {
                let mut cpuset: libc::cpu_set_t = std::mem::zeroed();
                libc::CPU_SET(cpu_id, &mut cpuset);
                libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &cpuset);
            }

            // Set NUMA memory policy to prefer local node
            #[cfg(target_os = "linux")]
            unsafe {
                numa_set_preferred(numa_node as i32);
            }

            Self::worker_loop()
        });

        Self { handle, numa_node }
    }
}
```

### Data Partitioning

```rust
pub struct NumaPartitionedTable {
    pub partitions: Vec<TablePartition>,
}

impl NumaPartitionedTable {
    pub fn partition_by_numa(table: &Table, topology: &NumaTopology) -> Self {
        let rows_per_node = table.row_count / topology.node_count as u64;

        let partitions = topology.nodes.iter().enumerate().map(|(i, node)| {
            let start = i as u64 * rows_per_node;
            let end = if i == topology.node_count - 1 {
                table.row_count
            } else {
                (i + 1) as u64 * rows_per_node
            };

            TablePartition {
                node_id: node.node_id,
                rows: table.rows[start as usize..end as usize].to_vec(),
            }
        }).collect();

        Self { partitions }
    }
}
```

### NUMA-Aware Execution

```rust
impl Executor {
    fn execute_numa_aware(&mut self, plan: &PhysicalPlan) -> Result<Vec<Tuple>> {
        let topology = NumaTopology::detect().unwrap();
        let worker_pool = WorkerPool::new_numa_aware(&topology);

        // Partition data by NUMA node
        let partitioned_data = NumaPartitionedTable::partition_by_numa(
            &self.table,
            &topology,
        );

        // Execute on each node (workers access local partition)
        let results: Vec<_> = worker_pool.workers.iter()
            .zip(partitioned_data.partitions.iter())
            .map(|(worker, partition)| {
                worker.execute_on_partition(plan, partition)
            })
            .collect();

        // Merge results
        Ok(results.into_iter().flatten().collect())
    }
}
```

## Implementation Plan

### Phase 1: NUMA Detection (Month 1-2)
1. Add libnuma or hwloc dependency
2. Implement topology detection
3. Test on NUMA systems (2-socket, 4-socket)

### Phase 2: Thread Binding (Month 3-4)
1. Implement worker thread pinning
2. Set NUMA memory policy
3. Validate: local memory access dominates

### Phase 3: Data Partitioning (Month 5-6)
1. Partition tables by NUMA node
2. Assign partitions to workers
3. Measure: 20-40% improvement on NUMA systems

## Expected Impact

**On NUMA systems** (2-8 sockets):
- 20-40% improvement (Greenplum results)
- Local memory access: 90%+ (vs 50% without NUMA awareness)

**On non-NUMA systems**: No change (detection fails, fallback to default)

## Risks and Mitigations

**Risk 1: Portability** (Linux-specific)
- Mitigation: Conditional compilation, fallback on non-Linux
- Alternative: Use hwloc (cross-platform)

**Risk 2: Unbalanced partitions** (data skew)
- Mitigation: Dynamic work-stealing across nodes
- Cost: Remote access, but better than idle cores

**Risk 3: Complexity** (hard to debug)
- Mitigation: Expose NUMA statistics, visualize topology
- Configuration: Allow disable via config

## Prior Art

### Greenplum NUMA Optimization
- Worker binding + local memory allocation
- 20-40% improvement on NUMA systems

### PostgreSQL parallel_setup_cost
- No explicit NUMA support
- Relies on OS scheduler (suboptimal)

### MongoDB NUMA Recommendations
- Disable NUMA interleaving
- Use `numactl --interleave=all` for memory allocation

## Related RFCs

- RFC 0068: Hardware-Calibrated Cost Model (complementary, NUMA detection)
- RFC 0072: Adaptive Parallelism (complementary, worker pool)
