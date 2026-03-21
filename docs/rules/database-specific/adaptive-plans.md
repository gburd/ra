# Rule: Oracle Adaptive Query Plans

**Category:** database-specific/oracle
**File:** `rules/database-specific/oracle/adaptive-plans.rra`

## Metadata

- **ID:** `oracle-adaptive-plans`
- **Version:** "1.0.0"
- **Databases:** oracle
- **Tags:** database-specific, oracle, adaptive, statistics-collector, runtime
- **Authors:** "RA Contributors"


# Oracle Adaptive Query Plans

## Description

Defers final plan decisions until runtime by inserting statistics
collectors into the plan.  Oracle's adaptive optimizer can switch
between nested-loop join and hash join, or change the join order,
based on actual cardinalities observed during the first few batches
of execution.

**When to apply**: The optimizer is uncertain about cardinality
estimates (e.g., complex predicates, missing histograms, correlated
columns), and alternative plans exist with different cost profiles.

**Why it works**: Cardinality misestimates cause plan regressions.
By deferring the choice and observing actual row counts at a
statistics collector point, Oracle picks the plan branch that
matches reality, avoiding worst-case scenarios from bad estimates.

**Database version**: Oracle 12c+

## Relational Algebra

```algebra
-- Adaptive plan with decision point
statistics-collector[threshold=100](
    R -> {
        if actual_rows < threshold:
            nested-loop-join(R, index-scan(S))
        else:
            hash-join(R, full-scan(S))
    })
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("oracle-adaptive-join-plan";
    "(join inner ?cond ?left ?right)" =>
    "(adaptive-join ?cond ?left ?right
        (statistics-collector ?threshold)
        (nested-loop-join ?cond ?left (index-scan ?right))
        (hash-join ?cond ?left (full-scan ?right)))"
    if is_database("oracle")
    if cardinality_uncertain("?left")
    if both_plans_viable("?left", "?right")
),
```

## Preconditions

```rust
fn applicable(
    estimated_rows: f64,
    confidence: f64,
    plan_alternatives: usize,
) -> bool {
    confidence < 0.8 // uncertain estimate
    && plan_alternatives >= 2
    && estimated_rows > 100.0  // worth adapting
}
```

**Restrictions:**
- Adaptive plans have slight overhead from statistics collectors
- Only join method and certain distribution changes are adaptive
- OPTIMIZER_ADAPTIVE_PLANS must be TRUE (default in 12c+)
- Cannot adapt across query blocks

## Cost Model

```rust
fn adaptive_benefit(
    best_plan_cost: f64,
    worst_plan_cost: f64,
    misestimate_probability: f64,
) -> f64 {
    let expected_without_adaptive =
        best_plan_cost * (1.0 - misestimate_probability)
        + worst_plan_cost * misestimate_probability;
    let expected_with_adaptive = best_plan_cost * 1.05; // 5% overhead
    expected_without_adaptive - expected_with_adaptive
}
```

**Typical benefit**: Prevents 10x-100x plan regressions when
cardinality estimates are off by more than an order of magnitude.

## Test Cases

```sql
-- Positive: uncertain cardinality triggers adaptive plan
SELECT * FROM orders o JOIN customers c ON o.cust_id = c.id
WHERE c.signup_date > :date_param;
-- Cardinality of filtered customers uncertain; adaptive plan chosen
```

```sql
-- Negative: good statistics, high confidence
SELECT * FROM employees WHERE id = 100;
-- Primary key lookup; no uncertainty, no adaptive plan needed
```

## References

Oracle: Oracle Database SQL Tuning Guide, "Adaptive Query Plans"
Oracle: OPTIMIZER_ADAPTIVE_PLANS init parameter
Oracle: V$SQL_PLAN ADAPTIVE column
