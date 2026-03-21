# Rule: Approximate Percentile via t-digest/DDSketch

**Category:** experimental/approximate
**File:** `rules/experimental/approximate/approximate-percentile.rra`

## Metadata

- **ID:** `approximate-percentile`
- **Version:** "1.0.0"
- **Databases:** clickhouse, duckdb, presto, spark, datadog
- **Tags:** approximate, percentile, quantile, sketch, t-digest, ddsketch
- **Authors:** "Dunning, Ted", "Masson, Charles"


# Approximate Percentile via t-digest/DDSketch

## Description

Replaces exact percentile computation (which requires sorting all data)
with approximate sketch-based algorithms. t-digest and DDSketch maintain
compact data structures that can estimate any quantile with bounded
relative error. These sketches are mergeable, enabling parallel and
distributed computation.

**When to apply**: PERCENTILE_CONT, PERCENTILE_DISC, or MEDIAN queries
on large datasets where approximate results are acceptable.

## Relational Algebra

```algebra
-- Before: exact percentile (requires full sort)
gamma[; PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency)](requests)

-- After: approximate via t-digest
gamma[; APPROX_PERCENTILE(latency, 0.95, compression=200)](requests)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("tdigest-percentile";
    "(aggregate ?groups (percentile ?quantile ?col) ?input)" =>
    "(aggregate ?groups
        (tdigest-percentile ?quantile ?col (compression 200))
        ?input)"
    if approximate_mode_enabled()
),
```

## Preconditions

```rust
fn applicable(agg: &Aggregate, config: &QueryConfig) -> bool {
    config.allows_approximate_results()
        && agg.has_percentile_function()
        && agg.input_cardinality() > 10000
}
```

**Restrictions:**
- Relative error depends on compression parameter
- Extreme quantiles (p99.9) have higher relative error with t-digest
- DDSketch provides better guarantees for extreme quantiles
- Not suitable for exact quantile requirements

## Cost Model

```rust
fn estimated_benefit(
    rows: f64,
    compression: f64,
) -> f64 {
    let exact_cost = rows * rows.log2(); // sort-based
    let sketch_cost = rows * compression.log2();
    exact_cost - sketch_cost
}
```

**Typical benefit**: 30-90% for large datasets.

## Test Cases

```sql
-- Positive: P95 latency monitoring
SELECT APPROX_PERCENTILE(response_time, 0.95)
FROM api_requests
WHERE ts > NOW() - INTERVAL '1 hour';

-- Positive: distributed percentile (mergeable sketches)
SELECT endpoint, APPROX_PERCENTILE(latency, 0.99)
FROM distributed_logs GROUP BY endpoint;

-- Negative: small dataset
SELECT PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY score)
FROM exam_results;
-- Only 100 rows: exact sort is trivial
```

## References

- Dunning, T., Ertl, O. "Computing Extremely Accurate Quantiles Using t-Digests" (2019)
- Masson, C., Rim, J.E., Lee, H.K. "DDSketch: A Fast and Fully-Mergeable Quantile Sketch" (VLDB 2019)
