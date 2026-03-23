# RFC 0030: Cardinality Estimation Enhancement

- Start Date: 2026-03-21
- Author: RA Contributors
- Status: Accepted
- Tracking Issue: TBD

## Summary

Enhance cardinality estimation with Most Common Values (MCV) lists, improved semi-join/anti-join formulas, estimation error detection, and safety margins for deep join trees to reduce plan regression incidents caused by estimation errors.

## Motivation

Cardinality estimation errors compound exponentially through join trees (Leis et al. 2015 showed errors of 1000x or more for 5+ table joins). RA's current statistics model lacks MCV lists and has limited selectivity estimation. Without these enhancements:
- Equality predicates on skewed columns produce wildly inaccurate estimates
- Semi-join and anti-join cardinalities use heuristics instead of probability formulas
- There is no feedback mechanism to detect when estimates are systematically wrong
- Deep join trees have no safety margin against compounding errors

## Guide-level explanation

The enhanced estimation system works at four levels:

1. **MCV Lists**: Track the most frequent values and their frequencies for each column. Equality predicates check MCV first for accurate selectivity.
2. **Improved Join Formulas**: Semi-join and anti-join use probability-based formulas instead of heuristics.
3. **Error Detection**: After execution, compare estimated vs actual row counts and flag operators with >10x error.
4. **Safety Margins**: For join trees deeper than 3 tables, apply increasing safety factors to prefer robust plans.

## Reference-level explanation

### Implementation Details

**MCV Lists**:
```rust
pub struct McvList {
    pub values: Vec<(Value, f64)>,  // (value, frequency)
    pub null_fraction: f64,
    pub other_distinct_count: f64,  // NDV of non-MCV values
}

impl McvList {
    fn equality_selectivity(&self, value: &Value) -> f64 {
        // Check MCV list first
        if let Some((_, freq)) = self.values.iter().find(|(v, _)| v == value) {
            return *freq;
        }
        // Fall back to uniform assumption for non-MCV values
        (1.0 - self.mcv_frequency()) / self.other_distinct_count
    }
}
```

**Semi-Join / Anti-Join Cardinality**:
- Semi-join: `sel = 1 - (1 - 1/ndv_inner)^n_outer`
- Anti-join: `sel = (1 - 1/ndv_inner)^n_inner_per_key`

**Estimation Error Detection**:
- Post-execution comparison of estimated vs actual row counts per operator
- Flag operators with >10x error
- Recommend ANALYZE when statistics appear stale

**Safety Margins**:
- For join trees > 3 tables, multiply cardinality by a safety factor
- Factor increases with join depth: 1.5x per additional join beyond 3
- Prefer robust plans (hash join) over fragile plans (nested loop) under uncertainty

## Drawbacks

- MCV lists increase memory usage for statistics
- Safety margins can cause the optimizer to choose suboptimal plans when estimates are accurate
- Error detection requires execution feedback infrastructure

## Rationale and alternatives

### Why This Design?

MCV lists and probability-based formulas are proven techniques from PostgreSQL. The layered approach (accurate estimation + error detection + safety margins) provides defense in depth.

### Alternative Approaches

- **Sampling-based estimation**: Accurate but expensive at optimization time
- **Machine learning models**: Require training data and add complexity
- **Extended statistics (multi-column)**: Future enhancement, not in initial scope

## Prior art

- Leis et al., "How Good Are Query Optimizers, Really?" (2015)
- PostgreSQL row estimation formulas and extended statistics (v14)
- MySQL histograms (v8.0)
- CockroachDB multi-column statistics

## Unresolved questions

- Optimal MCV list size (PostgreSQL uses 100 entries; is this sufficient?)
- Safety margin dampening to avoid over-correction
- Integration with histogram-based range estimation

## Future possibilities

- Multi-column MCV lists for correlated predicates
- Adaptive safety margins based on observed estimation accuracy
- Query-specific cardinality bounds using functional dependencies
