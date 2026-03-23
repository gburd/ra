# Rule: CPU Cost Model

**Category:** cost-models
**File:** `rules/cost-models/cpu-cost-model.rra`

## Metadata

- **ID:** `cpu-cost-model`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, clickhouse, cockroachdb, mssql, oracle
- **Tags:** cost, cpu, estimation, modeling
- **Authors:** "RA Contributors"


# CPU Cost Model

## Description

Estimates CPU processing cost for query operators based on operation type,
input cardinality, and hardware characteristics. Fundamental for cost-based
query optimization to compare different execution plans.

**When to apply**: Cost-based optimization needs CPU cost estimates for
operators (scan, filter, join, aggregate, sort).

**Why it works**: Accurate CPU cost estimation enables selecting efficient
execution strategies. Model accounts for per-tuple overhead, cache effects,
and SIMD vectorization potential.

## Relational Algebra

```algebra
Cost_CPU(Op) = f(op_type, cardinality, hardware_profile)

Examples:
- Cost_CPU(Scan(n)) = n * tuple_processing_cost
- Cost_CPU(Filter(n, sel)) = n * predicate_eval_cost
- Cost_CPU(HashJoin(n, m)) = n * build_cost + m * probe_cost
- Cost_CPU(Sort(n)) = n * log(n) * comparison_cost
```

## Implementation

```rust
use ra_hardware::{CpuModel, HardwareProfile};

struct CpuCostModel {
    hardware: HardwareProfile,
    // Cost parameters (nanoseconds per operation)
    tuple_copy_ns: f64,        // 10-20ns
    predicate_eval_ns: f64,    // 5-50ns depending on complexity
    hash_computation_ns: f64,  // 30-50ns
    comparison_ns: f64,        // 2-5ns
}

impl CpuCostModel {
    fn scan_cost(&self, row_count: f64, row_size: u64) -> f64 {
        // Sequential scan: dominated by memory bandwidth
        let cache_miss_penalty = self.estimate_cache_misses(row_count, row_size);
        let base_cost = row_count * self.tuple_copy_ns;
        base_cost + cache_miss_penalty
    }

    fn filter_cost(&self, row_count: f64, predicate_complexity: u32) -> f64 {
        let pred_cost = match predicate_complexity {
            1 => self.predicate_eval_ns,      // Simple: x > 10
            2..=5 => self.predicate_eval_ns * 2.0,  // AND/OR combo
            _ => self.predicate_eval_ns * 5.0,      // Complex expression
        };
        row_count * pred_cost
    }

    fn hash_join_cost(&self, build_rows: f64, probe_rows: f64) -> f64 {
        // Build: hash + insert into hash table
        let build_cost = build_rows * (
            self.hash_computation_ns +
            100.0  // Hash table insertion with collision handling
        );

        // Probe: hash + lookup + comparison
        let probe_cost = probe_rows * (
            self.hash_computation_ns +
            50.0  // Hash table lookup
        );

        // Account for cache effects
        let cache_benefit = if build_rows < 100_000.0 {
            0.7  // Hash table fits in L3 cache
        } else {
            1.0
        };

        (build_cost + probe_cost) * cache_benefit
    }

    fn sort_cost(&self, row_count: f64) -> f64 {
        // Quicksort: O(n log n) comparisons
        let comparisons = row_count * row_count.log2();
        comparisons * self.comparison_ns
    }

    fn aggregate_cost(&self, input_rows: f64, group_count: f64) -> f64 {
        // Hash aggregation: hash + update aggregates
        let hash_cost = input_rows * self.hash_computation_ns;
        let update_cost = input_rows * 80.0;  // Update aggregate state
        hash_cost + update_cost
    }

    fn estimate_cache_misses(&self, rows: f64, row_size: u64) -> f64 {
        let l3_size = self.hardware.l3_cache_bytes as f64;
        let working_set = rows * row_size as f64;

        if working_set < l3_size {
            0.0  // Fits in cache
        } else {
            // Cache misses: DRAM latency penalty
            let miss_rate = 1.0 - (l3_size / working_set);
            rows * miss_rate * self.hardware.dram_latency_ns
        }
    }
}
```

**Restrictions:**
- Assumes modern out-of-order CPU with speculative execution
- Cache model is simplified (actual behavior more complex)
- SIMD vectorization not explicitly modeled (implicit in per-tuple cost)
- Multi-core parallelism handled separately

## Cost Model

```rust
fn estimated_benefit(
    accurate_model: &CpuCostModel,
    simple_model: &SimpleCostModel,
    query: &Query,
) -> f64 {
    // Accurate model considers hardware characteristics
    let accurate_plan = optimize_with_model(query, accurate_model);
    let accurate_cost = accurate_plan.estimated_cost();

    // Simple model uses fixed costs
    let simple_plan = optimize_with_model(query, simple_model);
    let simple_cost = simple_plan.estimated_cost();

    if simple_cost > accurate_cost {
        (simple_cost - accurate_cost) / simple_cost
    } else {
        0.0
    }
}
```

**Assumptions:**
- CPU operates at advertised clock frequency (no thermal throttling)
- Branch prediction ~95% accurate for sorted data
- L1/L2/L3 cache hit rates follow working set model
- Memory bandwidth saturates at advertised rate

**Typical benefit**: 30-70% better plan selection when hardware characteristics
significantly affect operator costs (cache size, memory bandwidth).

## Test Cases

### Test 1: Small table scan (fits in cache)

```sql
SELECT * FROM small_table; -- 10K rows, 100 bytes/row = 1MB

-- Expected CPU cost:
-- Cache: Fits in L3 (1MB < 64MB)
-- Cost: 10K * 20ns = 200$\mu$s (no cache miss penalty)
```

### Test 2: Large table scan (exceeds cache)

```sql
SELECT * FROM large_table; -- 10M rows, 100 bytes/row = 1GB

-- Expected CPU cost:
-- Cache: Exceeds L3, ~95% miss rate
-- Cost: 10M * 20ns + 9.5M * 90ns = 1.06s (DRAM latency dominates)
```

### Test 3: Hash join cost estimation

```sql
SELECT * FROM orders o JOIN lineitem l ON o.orderkey = l.orderkey;
-- Orders: 1.5M rows (build side)
-- Lineitem: 60M rows (probe side)

-- Expected CPU cost:
-- Build: 1.5M * 130ns = 195ms
-- Probe: 60M * 80ns = 4.8s
-- Total: ~5s CPU time
```

## References

**Cost model theory:**
- Selinger et al., "Access Path Selection in a RDBMS", SIGMOD 1979
- Graefe, "Query Evaluation Techniques for Large Databases", ACM Comp. Surveys 1993

**Modern implementations:**
- PostgreSQL: `src/backend/optimizer/path/costsize.c`
- MySQL: `sql/opt_costmodel.cc`
- Apache Calcite: `RelOptCostImpl.java`

**Hardware considerations:**
- Agner Fog, "Instruction tables" - CPU instruction latencies
- Intel/AMD optimization manuals - Cache behavior
- Hardware models from `ra-hardware` crate
