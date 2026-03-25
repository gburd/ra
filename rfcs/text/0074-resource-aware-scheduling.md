# RFC 0074: Resource-Aware Scheduling

- **Status**: Proposed
- **Priority**: Medium (3-4 months)
- **Impact**: 20-50% system-wide throughput improvement
- **Category**: Scheduling / Resource Management
- **Created**: 2026-03-25

## Summary

Schedule queries based on resource usage (CPU, memory, I/O) to maximize system-wide throughput. Interleave cheap queries, serialize expensive queries, prevent resource exhaustion from runaway queries.

## Motivation

**Problem**: Concurrent expensive queries compete for resources
- 10 concurrent scan-heavy queries → I/O contention → all 10x slower
- Better: Run 2 at a time → each finishes 2x slower → 5x better throughput

**CockroachDB admission control**: 50-100% P99 latency improvement under load

## Proposal

### Query Resource Estimation

```rust
pub struct ResourceEstimate {
    pub cpu_ms: u64,
    pub memory_mb: u64,
    pub io_mb: u64,
}

fn estimate_resources(&self, query: &RelExpr) -> ResourceEstimate {
    ResourceEstimate {
        cpu_ms: self.estimate_cpu_time(query),
        memory_mb: self.estimate_memory_usage(query),
        io_mb: self.estimate_io_volume(query),
    }
}
```

### Scheduler

```rust
pub struct ResourceAwareScheduler {
    queues: HashMap<QueryClass, VecDeque<Query>>,
    limits: ResourceLimits,
    current_usage: ResourceUsage,
}

impl ResourceAwareScheduler {
    fn can_admit(&self, query: &Query) -> bool {
        let estimate = query.resource_estimate;
        let new_usage = self.current_usage + estimate;

        new_usage.cpu_ms < self.limits.cpu_ms &&
        new_usage.memory_mb < self.limits.memory_mb &&
        new_usage.io_mb < self.limits.io_mb
    }

    fn schedule(&mut self) -> Option<Query> {
        // Priority: cheap queries first
        for class in [QueryClass::Cheap, QueryClass::Medium, QueryClass::Expensive] {
            if let Some(query) = self.queues.get_mut(&class).and_then(|q| q.pop_front()) {
                if self.can_admit(&query) {
                    self.current_usage += query.resource_estimate;
                    return Some(query);
                } else {
                    // Put back, try next queue
                    self.queues.get_mut(&class).unwrap().push_front(query);
                }
            }
        }

        None
    }
}
```

### Query Classification

```rust
fn classify_by_resources(&self, estimate: &ResourceEstimate) -> QueryClass {
    if estimate.cpu_ms < 100 && estimate.memory_mb < 10 && estimate.io_mb < 10 {
        QueryClass::Cheap      // Fast queries: < 100ms CPU
    } else if estimate.cpu_ms < 1000 && estimate.memory_mb < 100 {
        QueryClass::Medium     // Medium queries: < 1s CPU
    } else {
        QueryClass::Expensive  // Expensive queries: > 1s CPU
    }
}
```

## Implementation Plan

### Phase 1: Resource Estimation (Month 1)
1. Implement `estimate_resources()` for each operator
2. Sum resources across query plan
3. Validate estimates via execution feedback

### Phase 2: Scheduler (Month 2)
1. Implement `ResourceAwareScheduler`
2. Add admission control with resource limits
3. Test with synthetic workloads (cheap + expensive mix)

### Phase 3: Integration (Month 3)
1. Integrate scheduler with query executor
2. Add backpressure (queue full → reject query)
3. Monitor: throughput, latency, resource utilization

## Expected Impact

**Under load** (concurrent queries):
- 20-50% throughput improvement (CockroachDB results)
- P99 latency improvement (prioritize cheap queries)
- Prevent resource exhaustion (runaway queries)

## Prior Art

- CockroachDB admission control (CockroachDB Blog, 2022): 50-100% P99 improvement
