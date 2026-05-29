# RFC 0026: Adaptive Cost Model Calibration

- Start Date: 2026-03-21
- Author: RA Contributors
- Status: Implemented (MVP, 2026-05-29) — see INDEX.md for scope boundaries
- Tracking Issue: TBD

## Summary

Implement a three-tier cost model calibration system (static hardware benchmarks, dynamic execution feedback, adaptive correction factors) that replaces fixed cost parameters with self-tuning estimates matched to the actual hardware and workload.

## Motivation

RA's cost model uses fixed parameters that may not match the actual hardware and workload. PostgreSQL requires manual tuning of `seq_page_cost`, `random_page_cost`, etc. There is no feedback loop from actual execution to cost model adjustment.

Miscalibrated cost models cause:
- Hash joins chosen when merge joins would be faster (or vice versa)
- Sequential scans chosen over index scans (or vice versa)
- Nested loop joins chosen for large relations
- Memory spill not accounted for in cost estimates

## Guide-level explanation

The calibration system operates at three levels:

1. **Static**: On first run, micro-benchmarks measure actual I/O, CPU, and memory costs. These are stored as a hardware profile.
2. **Dynamic**: After each query, estimated costs are compared to actual execution metrics. Running statistics track estimation accuracy.
3. **Adaptive**: When systematic bias is detected (e.g., hash join cost consistently underestimated by 3x), correction factors are applied automatically.

Users can trigger recalibration with:
```sql
SELECT ra_calibrate();  -- Run micro-benchmarks
SELECT ra_calibration_report();  -- Show current calibration state
```

## Reference-level explanation

### Implementation Details

```rust
pub struct CostCalibration {
    pub seq_page_cost: f64,
    pub random_page_cost: f64,
    pub cpu_tuple_cost: f64,
    pub cpu_index_tuple_cost: f64,
    pub cpu_operator_cost: f64,
    pub hash_build_cost_factor: f64,
    pub sort_cost_factor: f64,
}

pub struct ExecutionFeedback {
    pub estimated_rows: f64,
    pub actual_rows: u64,
    pub estimated_cost: f64,
    pub actual_time_ms: f64,
    pub operator_type: OperatorType,
}

pub struct CorrectionFactors {
    pub factors: HashMap<OperatorType, f64>,
    pub sample_count: HashMap<OperatorType, usize>,
    pub confidence: HashMap<OperatorType, f64>,
}
```

### Cost Model Extensions

- **Correlation-aware index cost**: `cost = random_cost * (1 - corr^2) + seq_cost * corr^2`
- **Cache-aware random I/O**: Reduce effective `random_page_cost` based on working set size vs available cache
- **Memory spill threshold**: When hash table exceeds memory budget, add 2x I/O cost for spill-to-disk

### Integration Points

- Post-execution hook collects `ExecutionFeedback` for each operator
- Correction factors applied during cost estimation in the optimizer
- Hardware profile stored in persistent configuration

## Drawbacks

- Micro-benchmarks add startup cost on first run
- Feedback loop could oscillate if not dampened
- Correction factors may not generalize across different query shapes

## Rationale and alternatives

### Why This Design?

A layered approach allows each tier to compensate for the others. Static calibration provides a good baseline, dynamic feedback catches systematic errors, and adaptive correction handles workload-specific patterns.

### Alternative Approaches

- **Manual tuning**: Current approach in PostgreSQL; error-prone and static
- **ML-based tuning (OtterTune)**: Requires training data and model infrastructure
- **Fixed parameters**: Simple but inaccurate across hardware

## Prior art

- CMU 15-721 Lecture 16: Cost Models
- DuckDB cost model calibration approach
- Van Aken et al., "OtterTune: Automatic Database Management System Tuning" (2017)
- PostgreSQL `pg_stat_statements` for execution feedback

## Unresolved questions

- Dampening factor for correction updates to prevent oscillation
- How to handle cost model changes across PostgreSQL versions
- Cold-start behavior before enough feedback is collected

## Future possibilities

- Workload-aware calibration profiles (OLTP vs OLAP)
- Per-table cost adjustments based on storage characteristics
- Automatic detection of hardware changes requiring recalibration
