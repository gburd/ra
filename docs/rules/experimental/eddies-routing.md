# Rule: Eddies Adaptive Query Routing

**Category:** experimental/adaptive
**File:** `rules/experimental/adaptive/eddies-routing.rra`

## Metadata

- **ID:** `eddies-routing`
- **Version:** "1.0.0"
- **Databases:** research, telegraphcq
- **Tags:** adaptive, eddies, routing, runtime, operator-ordering
- **Authors:** "Avnur, Ron", "Hellerstein, Joseph"


# Eddies Adaptive Query Routing

## Description

Replaces static operator ordering with a runtime routing mechanism that
continuously adapts the order in which tuples flow through operators.
An "eddy" operator sits between data sources and query operators, routing
each tuple (or batch) to operators based on observed selectivities and
costs. Operators that are more selective or cheaper are given priority,
and the routing adapts as data characteristics change during execution.

**When to apply**: Multi-operator queries where selectivities are unknown
or changing, particularly in streaming or federated scenarios.

## Relational Algebra

```algebra
-- Before: static operator order
sigma[pred3](sigma[pred2](sigma[pred1](R)))

-- After: eddy routes tuples adaptively
Eddy(R, {pred1, pred2, pred3})
-- Routes tuples to most selective predicate first
-- Adapts routing as selectivities are learned
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("eddy-multi-filter";
    "(filter ?p1 (filter ?p2 (filter ?p3 ?input)))" =>
    "(eddy ?input [?p1, ?p2, ?p3]
        (routing lottery)
        (window 100))"
    if selectivities_unknown("?p1", "?p2", "?p3")
),
```

## Preconditions

```rust
fn applicable(plan: &Plan) -> bool {
    let filters = plan.consecutive_filters();
    // Need at least 2 reorderable operators
    filters.len() >= 2
        // Selectivities must be uncertain
        && filters.iter().any(|f| f.selectivity_confidence() < 0.5)
        // Operators must be independently applicable
        && filters.are_independent()
}
```

**Restrictions:**
- Per-tuple routing overhead (mitigated by batch routing)
- Cannot reorder operators with dependencies
- Convergence time: needs enough tuples to learn good routing
- Join ordering via eddies (SteMs) more complex than filter ordering

## Cost Model

```rust
fn estimated_benefit(
    rows: f64,
    operators: usize,
    selectivity_variance: f64,
) -> f64 {
    // Benefit from learning optimal order vs. worst-case static order
    let worst_case_factor = operators as f64;
    let learned_factor = 1.5; // near-optimal after learning
    rows * (worst_case_factor - learned_factor) * selectivity_variance
}
```

**Typical benefit**: 20-80% when selectivities are unknown or varying.

## Test Cases

```sql
-- Positive: multiple filters with unknown selectivities
SELECT * FROM remote_table
WHERE expensive_udf(col1) AND col2 > threshold AND col3 LIKE pattern;
-- Unknown UDF cost and selectivity: eddy learns optimal order

-- Positive: streaming with changing data distribution
SELECT * FROM event_stream
WHERE region = 'US' AND category = 'tech' AND score > 0.8;
-- Data distribution shifts over time: eddy adapts

-- Negative: known selectivities, static order is fine
SELECT * FROM orders WHERE status = 'pending' AND amount > 100;
-- Both selectivities well-estimated from statistics
```

## References

- Avnur, R., Hellerstein, J.M. "Eddies: Continuously Adaptive Query Processing" (SIGMOD 2000)
- Deshpande, A. et al. "Adaptive Query Processing" (Foundations and Trends in Databases 2007)
