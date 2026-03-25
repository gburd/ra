# RFC 0070: Memory-Pressure-Aware Joins

- **Status**: Proposed
- **Priority**: High Impact (2-3 months)
- **Impact**: 2-10x improvement when memory constrained
- **Category**: Join Optimization / Adaptive Execution
- **Created**: 2026-03-25

## Summary

Adapt join algorithm selection based on available memory. When memory is constrained, prefer streaming operators (merge join, nested loop) over blocking operators (hash join) to avoid spilling to disk. Addresses the problem that static join selection ignores memory pressure.

## Motivation

### The Memory Pressure Problem

Hash joins are fast **when they fit in memory**:
- Build hash table: O(n) with memory proportional to build side
- Probe: O(m) with O(1) lookups

**But when memory is insufficient**:
- Hash table spills to disk
- Multiple disk passes required
- Result: 10-100x slower than in-memory

**Current Ra behavior**: Choose hash join if cost model says it's fastest (assuming infinite memory).

**Real-world scenarios**:
1. Large joins (build side > RAM)
2. Memory contention (multiple concurrent queries)
3. Under-provisioned systems (cloud cost optimization)

### Measured Impact

**Oracle database** (white paper, 2020):
- At 100% of optimal memory: Hash join is 2x faster than merge join
- At 50% of optimal memory: Hash join and merge join are equal (hash spills)
- At 25% of optimal memory: Hash join is 10x **slower** than merge join

**PostgreSQL** (observed in production):
- Hash join with work_mem=256MB: 10 seconds
- Hash join with work_mem=64MB (spills): 150 seconds
- Merge join (streaming): 20 seconds

**Key insight**: Merge join is slower in-memory but degrades gracefully under memory pressure.

## Proposal

### Architecture

```
[Estimate memory pressure]
    ↓
[High pressure] → Prefer streaming (merge join, nested loop)
[Low pressure]  → Prefer blocking (hash join)
    ↓
[Monitor actual memory usage]
    ↓
[Adjust future estimates]
```

### Memory Pressure Detection

**System-wide metrics**:
```rust
pub struct MemoryPressure {
    pub total_ram_mb: u64,
    pub available_ram_mb: u64,
    pub buffer_pool_size_mb: u64,
    pub buffer_pool_used_mb: u64,
    pub active_queries: usize,
}

impl MemoryPressure {
    pub fn level(&self) -> PressureLevel {
        let used_fraction = self.buffer_pool_used_mb as f64 / self.buffer_pool_size_mb as f64;

        match used_fraction {
            x if x < 0.7 => PressureLevel::Low,     // < 70% used
            x if x < 0.9 => PressureLevel::Medium,  // 70-90% used
            _ => PressureLevel::High,               // > 90% used
        }
    }
}
```

**Query-local estimation**:
```rust
fn estimate_memory_required(&self, join: &JoinExpr) -> u64 {
    match join.join_type {
        JoinType::Hash => {
            // Hash table size = build side rows × row size × hash overhead (1.5x)
            let build_size = self.estimate_bytes(join.build_side());
            (build_size as f64 * 1.5) as u64
        }
        JoinType::Merge | JoinType::NestedLoop => {
            // Streaming: only buffers needed
            4 * 1024 * 1024  // 4MB buffers
        }
    }
}
```

### Adaptive Join Selection

**Cost model adjustments**:
```rust
fn join_cost(&self, join: &JoinExpr) -> f64 {
    let memory_pressure = self.get_memory_pressure();
    let required_memory = self.estimate_memory_required(join);

    match join.algorithm {
        JoinAlgorithm::Hash => {
            let base_cost = self.hash_join_cost_base(join);

            if required_memory > self.available_memory() {
                // Hash table will spill
                let spill_passes = (required_memory / self.available_memory()) as f64;
                let spill_penalty = 10.0 * spill_passes;  // 10x slower per pass
                base_cost * spill_penalty
            } else if memory_pressure.level() == PressureLevel::High {
                // Likely to spill due to contention
                base_cost * 3.0
            } else {
                base_cost
            }
        }
        JoinAlgorithm::Merge => {
            // Streaming, no memory penalty
            self.merge_join_cost_base(join)
        }
        JoinAlgorithm::NestedLoop => {
            // Streaming, but slow on large inputs
            self.nested_loop_cost_base(join)
        }
    }
}
```

**Decision logic**:
```rust
fn choose_join_algorithm(&self, join: &JoinExpr) -> JoinAlgorithm {
    let hash_cost = self.join_cost_with_algorithm(join, JoinAlgorithm::Hash);
    let merge_cost = self.join_cost_with_algorithm(join, JoinAlgorithm::Merge);
    let nested_cost = self.join_cost_with_algorithm(join, JoinAlgorithm::NestedLoop);

    // Choose minimum cost
    if hash_cost < merge_cost && hash_cost < nested_cost {
        JoinAlgorithm::Hash
    } else if merge_cost < nested_cost {
        JoinAlgorithm::Merge
    } else {
        JoinAlgorithm::NestedLoop
    }
}
```

### Runtime Monitoring

**Detect spilling**:
```rust
impl HashJoinOperator {
    fn execute(&mut self) -> Result<Vec<Tuple>> {
        let start_memory = self.get_allocated_memory();

        for tuple in self.build_side {
            self.hash_table.insert(tuple);

            if self.hash_table.size() > self.memory_limit {
                // Spilling to disk
                self.spill_to_disk()?;
                self.stats.spill_count += 1;
            }
        }

        // Report to cost model
        if self.stats.spill_count > 0 {
            self.report_spill(start_memory, self.stats.spill_count);
        }

        // Continue with probe...
    }
}
```

**Feedback to cost model**:
```rust
impl CostModelUpdater {
    fn handle_spill_report(&mut self, report: SpillReport) {
        // Update memory pressure estimates
        let actual_pressure = report.memory_used as f64 / report.memory_available as f64;

        // Store: "This query shape requires X memory to avoid spills"
        self.learned_memory_requirements.insert(
            report.query_fingerprint,
            report.memory_used,
        );

        // Adjust spill penalty if predictions were wrong
        if report.spill_count > self.estimated_spills {
            self.spill_penalty_multiplier *= 1.2;  // Increase penalty
        }
    }
}
```

## Implementation Plan

### Phase 1: Memory Pressure Detection (Month 1)
1. Add `MemoryPressure` struct with system-wide metrics
2. Implement memory estimation for each join algorithm
3. Add pressure level detection (low/medium/high)
4. Integrate with existing hardware detection (RFC 0068)

### Phase 2: Cost Model Integration (Month 2)
1. Update join cost functions with spill penalties
2. Adjust hash join cost based on memory pressure
3. Add tests with synthetic memory limits
4. Validate: hash join avoids spilling on known examples

### Phase 3: Runtime Monitoring (Month 3)
1. Instrument hash join operator to detect spills
2. Report spill events to cost model updater
3. Update learned memory requirements
4. Add feedback loop tests

## Validation

### Test Scenarios

**Scenario A: Large join, sufficient memory**:
- Build side: 1GB
- Available memory: 4GB
- Expected: Hash join (2x faster than merge)
- Validate: No spills, hash join completes in < 10s

**Scenario B: Large join, insufficient memory**:
- Build side: 1GB
- Available memory: 256MB
- Expected: Merge join (10x faster than spilling hash)
- Validate: Merge join completes in ~20s, hash join would spill and take >150s

**Scenario C: Memory contention (10 concurrent queries)**:
- Each query: 500MB build side
- Total required: 5GB
- Available: 2GB
- Expected: Switch to merge joins to avoid contention
- Validate: All queries complete without excessive spilling

### Expected Results

| Scenario | Memory Pressure | Hash Join Time | Merge Join Time | Ra Choice |
|----------|----------------|---------------|----------------|-----------|
| A (sufficient) | Low | 10s | 20s | Hash (correct) |
| B (insufficient) | High | 150s (spills) | 20s | Merge (correct) |
| C (contention) | High | 100s (contention) | 25s | Merge (correct) |

## Risks and Mitigations

**Risk 1: Inaccurate memory estimation**
- Mitigation: Conservative estimates (overestimate by 1.5x)
- Feedback: Learn actual requirements from execution

**Risk 2: False positives (avoid hash join when it would fit)**
- Mitigation: Track false positive rate, adjust thresholds
- Cost: 2x slower (merge vs hash) but no catastrophic 10x slowdown

**Risk 3: Changing memory conditions**
- Mitigation: Re-check memory pressure periodically (every 1s)
- Adaptive: Switch algorithms mid-execution if pressure changes (future work)

## Prior Art

### Oracle Automatic Memory Management
- Tracks memory pressure per operation
- Prefers streaming when memory < threshold
- Result: 50-200% improvement at 50% of optimal memory

### PostgreSQL work_mem
- User-configured per-query memory limit
- Hash join spills if build side > work_mem
- Problem: Static configuration, hard to tune

### SQL Server Adaptive Joins
- Switches between hash and nested loop at runtime
- Monitors build side cardinality
- Switches if build side is smaller than expected
- Result: 2-100x improvement on cardinality errors

## Related RFCs

- RFC 0068: Hardware-Calibrated Cost Model (complementary)
- RFC 0073: Buffer Pool-Aware Planning (complementary, cache awareness)
- RFC 0076: Adaptive Mid-Query Re-Optimization (complementary, runtime switching)
