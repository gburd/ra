# RFC 0071: Workload Classification

- **Status**: Proposed
- **Priority**: Medium (1-2 months)
- **Impact**: 5-20% improvement via specialized strategies
- **Category**: Query Optimization / Workload-Aware
- **Created**: 2026-03-25

## Summary

Classify queries into workload types (OLTP, OLAP, hybrid) and apply specialized optimization strategies per class. OLTP queries benefit from different techniques than OLAP queries: shorter timeouts, index-driven plans, plan caching vs full exploration, scan-heavy plans, complex join reordering.

## Motivation

**OLTP characteristics**:
- Simple queries (1-3 tables)
- High concurrency (1000s QPS)
- Latency-sensitive (< 10ms)
- Index-driven (point lookups, range scans)

**OLAP characteristics**:
- Complex queries (5-20 tables)
- Low concurrency (10s queries)
- Throughput-oriented (seconds to minutes)
- Scan-heavy (full table scans, large aggregates)

**Problem**: One-size-fits-all optimization is suboptimal.

### Evidence

**PostgreSQL query classification** (Unterbrunner et al., VLDB 2009):
- OLTP queries: 10-30% faster with specialized optimizer settings
- OLAP queries: 5-20% faster with full exploration, no timeout

**Snowflake workload separation** (Dageville et al., SIGMOD 2016):
- Separate warehouses for OLTP vs OLAP
- 2-5x improvement via specialization

## Proposal

### Classification Algorithm

```rust
pub enum WorkloadClass {
    Oltp,      // Transactional
    Olap,      // Analytical
    Hybrid,    // Mix
}

fn classify_query(query: &RelExpr) -> WorkloadClass {
    let table_count = count_tables(query);
    let has_aggregates = has_aggregates(query);
    let has_complex_predicates = has_complex_predicates(query);
    let estimated_rows = estimate_output_size(query);

    // Simple heuristic classification
    if table_count <= 2 && !has_aggregates && estimated_rows < 100 {
        WorkloadClass::Oltp
    } else if table_count >= 5 || has_aggregates {
        WorkloadClass::Olap
    } else {
        WorkloadClass::Hybrid
    }
}
```

### Strategy Selection

**OLTP optimization strategy**:
```rust
impl OptimizerConfig {
    pub fn for_oltp() -> Self {
        Self {
            timeout: Duration::from_millis(10),  // Fast timeout
            enable_plan_cache: true,             // Cache parameterized queries
            prefer_index_scans: true,            // Index-driven
            max_join_reordering: 5,              // Limited exploration
            enable_complex_rules: false,         // Simple rules only
        }
    }
}
```

**OLAP optimization strategy**:
```rust
impl OptimizerConfig {
    pub fn for_olap() -> Self {
        Self {
            timeout: Duration::from_secs(5),     // Longer timeout
            enable_plan_cache: false,            // Fresh optimization
            prefer_index_scans: false,           // Scan-heavy
            max_join_reordering: 20,             // Full exploration
            enable_complex_rules: true,          // All rules
            enable_parallelism: true,            // Parallel execution
        }
    }
}
```

### Integration

```rust
impl Optimizer {
    pub fn optimize(&mut self, query: &RelExpr) -> Result<PhysicalPlan> {
        let workload_class = classify_query(query);
        let config = match workload_class {
            WorkloadClass::Oltp => OptimizerConfig::for_oltp(),
            WorkloadClass::Olap => OptimizerConfig::for_olap(),
            WorkloadClass::Hybrid => OptimizerConfig::default(),
        };

        self.optimize_with_config(query, &config)
    }
}
```

## Implementation Plan

### Phase 1: Classification (Weeks 1-2)
1. Implement `classify_query()` with heuristics
2. Add tests with known OLTP/OLAP queries
3. Validate classification accuracy on JOB benchmark

### Phase 2: Strategy Selection (Weeks 3-4)
1. Create `OptimizerConfig::for_oltp()` and `for_olap()`
2. Update optimizer to use config per workload class
3. Add tests: OLTP query with OLTP config vs OLAP config

### Phase 3: Validation (Weeks 5-6)
1. Run JOB benchmark with classification
2. Measure: OLTP queries faster? OLAP queries better plans?
3. Tune classification thresholds based on results

## Expected Impact

- OLTP: 10-30% faster (shorter timeout, plan cache, index preference)
- OLAP: 5-20% better plans (longer timeout, full exploration)

## Prior Art

- PostgreSQL query workload classification (Unterbrunner et al., VLDB 2009)
- Snowflake workload management (Dageville et al., SIGMOD 2016)
- CockroachDB admission control (CockroachDB Blog, 2022)
