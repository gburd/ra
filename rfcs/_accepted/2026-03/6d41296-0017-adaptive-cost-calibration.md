# RFC 0017: Adaptive Cost Model Calibration

**Status:** Accepted
**Implemented:** 2026-03-20
**Commit:** 6d41296

## Summary

Implemented EWMA-based cost model calibration that adjusts per-operator correction factors from execution feedback, addressing cost model drift over time. The system automatically learns from actual query execution to improve future cost estimates.

## Motivation

Cost models have inherent inaccuracies due to:
- Hardware variability across deployments
- Workload characteristics changing over time
- Simplifying assumptions in the cost model
- Data distribution shifts

Without calibration, the optimizer makes increasingly poor decisions as the gap between estimated and actual costs widens. This was identified as RFC Proposal #2 with HIGH severity.

## Technical Design

### Architecture

The calibration system consists of three tiers:
1. **Static calibration**: Hardware-based factors from `CostCalibration`
2. **Dynamic calibration**: Track actual vs estimated per query
3. **Adaptive correction**: EWMA-based parameter adjustment

### Core Components

**`AdaptiveCalibrator`** - Main calibration engine
- Maintains per-operator correction factors
- Uses exponentially-weighted moving average (EWMA) for smoothing
- Detects systematic bias with configurable threshold (default 20%)
- Persists state to TOML for restart recovery

**`OperatorKind`** - Tracked operator categories
- Scan, Filter, HashJoin, MergeJoin, NestedLoopJoin
- Sort, Aggregate, IndexScan

**`CostFeedback`** - Execution observation ingestion
- Bridges from `ra_stats::timeline::ExecutionFeedback`
- Classifies PostgreSQL EXPLAIN output via `classify_operator`
- Tracks q-error metrics for estimation quality

### Algorithm

```rust
correction_factor = α × (actual/estimated) + (1-α) × old_factor
```

Where α = 0.2 (DEFAULT_ALPHA) provides stable convergence.

Corrections only apply after MIN_SAMPLES_FOR_CORRECTION (5) observations to avoid noise.

### Integration Points

- **Input**: Execution feedback from `ra_stats::timeline`
- **Output**: Correction factors applied in `CostModel::estimate`
- **Persistence**: TOML state file for calibration data
- **Monitoring**: Q-error tracking for quality metrics

## Implementation

### Key Files

- `crates/ra-engine/src/adaptive_calibration.rs` (1035 lines)
  - `AdaptiveCalibrator` struct and EWMA logic
  - `OperatorCalibration` per-operator state
  - `CostFeedback` ingestion pipeline
  - TOML serialization for persistence

### Dependencies

- `serde` for serialization
- `ra_stats` for execution feedback
- Integration with `CostModel` in ra-engine

## Testing

Comprehensive unit tests covering:
- EWMA convergence behavior
- Bias detection thresholds
- State persistence/recovery
- Operator classification
- Q-error calculation

## References

- Van Aken et al. "OtterTune: Automatic Database Management System Tuning" (2017)
- CMU 15-721 Lecture 18: Cost Models
- RFC Proposal #2: Cost Model Drift

## Future Work

- ML-based cost models using learned embeddings
- Workload-specific calibration profiles
- Online learning without restart
- Correlation detection between operators