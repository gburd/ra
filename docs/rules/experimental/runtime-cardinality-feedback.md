# Rule: Runtime Cardinality Feedback Loop

**Category:** experimental/adaptive
**File:** `rules/experimental/adaptive/runtime-cardinality-feedback.rra`

## Metadata

- **ID:** `runtime-cardinality-feedback`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle, cockroachdb
- **Tags:** adaptive, cardinality, feedback, learning, statistics
- **Authors:** "Stillger et al. 2001", "Chaudhuri 2009", "RA Contributors"


# Runtime Cardinality Feedback Loop

## Description

After query execution, compares actual operator cardinalities with
estimates and feeds corrections back to the optimizer's statistics.
Subsequent executions of the same or similar queries benefit from
improved estimates. This creates a feedback loop: execute -> measure ->
correct -> re-optimize -> execute with better plan.

**When to apply**: Parameterized queries and prepared statements that
execute repeatedly with different parameter values. Also useful for
correcting persistent estimation biases (correlated columns, skewed
distributions).

**Why it works**: Traditional statistics (histograms, samples) miss
correlations and complex predicates. By observing actual cardinalities
during execution, the system learns corrections that capture
data-specific patterns statistics cannot represent.

## Relational Algebra

```algebra
-- Execution 1: uses estimate, records actual
join[R.a = S.a](filter[R.x > ?param](R), S)
  estimated_card: 1000, actual_card: 50000

-- Feedback stored: correction_factor(R, x > ?, S, a) = 50.0

-- Execution 2: uses corrected estimate
join[R.a = S.a](filter[R.x > ?param2](R), S)
  corrected_estimate: traditional_est * correction_factor
  -> may choose different join algorithm or order
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// This is a meta-rule: modifies the optimizer's statistics
// rather than the query plan directly

struct CardinalityFeedback {
    query_signature: QuerySignature,
    operator_estimates: Vec<(OperatorId, f64)>,
    operator_actuals: Vec<(OperatorId, f64)>,
    correction_factors: HashMap<OperatorSignature, f64>,
}

impl CardinalityFeedback {
    fn record_execution(
        &mut self,
        plan: &ExecutedPlan,
    ) {
        for op in plan.operators() {
            let sig = op.signature();
            let estimate = op.estimated_cardinality();
            let actual = op.actual_cardinality();

            // Exponential moving average of correction factor
            let correction = actual / estimate;
            let entry = self.correction_factors
                .entry(sig)
                .or_insert(1.0);

            let alpha = 0.3; // Learning rate
            *entry = *entry * (1.0 - alpha) + correction * alpha;
        }
    }

    fn corrected_estimate(
        &self,
        op: &Operator,
        traditional_estimate: f64,
    ) -> f64 {
        let sig = op.signature();
        let correction = self.correction_factors
            .get(&sig)
            .copied()
            .unwrap_or(1.0);

        traditional_estimate * correction
    }
}

// Integration with optimizer
rw!("apply-cardinality-feedback";
    "(cardinality ?op)" =>
    "(corrected_cardinality ?op (lookup_feedback ?op))"
    if feedback_available("?op")
),

// Operator signature for feedback matching
struct OperatorSignature {
    operator_type: OpType,    // join, filter, aggregate
    table_set: Vec<TableId>,  // tables involved
    predicate_template: String, // predicate with params replaced
}
```

## Preconditions

```rust
fn applicable(
    query: &RelExpr,
    feedback_store: &FeedbackStore,
) -> bool {
    // Query must have been executed before (or similar query)
    let sig = compute_query_signature(query);

    if let Some(feedback) = feedback_store.get(&sig) {
        // Correction factor must be significant
        feedback.max_correction_factor() > 2.0
    } else {
        false
    }
}
```

**Restrictions:**
- Requires execution history (cold start problem)
- Feedback may be stale if data distribution changes
- Parameterized queries need parameter-aware feedback (not one-size-fits-all)
- Over-correction risk: single bad execution can bias estimates
- Need expiration/decay for stale feedback

## Cost Model

```rust
fn estimated_benefit(
    current_q_error: f64,
    feedback_q_error: f64,
) -> f64 {
    // Plan quality improvement from better estimates
    // Q-error of 10x typically causes 5-10x plan cost increase
    let current_plan_penalty = (current_q_error.log2()).powi(2);
    let feedback_plan_penalty = (feedback_q_error.log2()).powi(2);

    if current_plan_penalty > feedback_plan_penalty {
        (current_plan_penalty - feedback_plan_penalty)
            / current_plan_penalty
    } else {
        0.0
    }
}
```

**Typical benefit**: 2x-10x improvement for repeated queries with
systematic estimation errors. Most impactful for correlated predicates.

## Test Cases

### Positive: Correlated columns causing persistent errors

```sql
-- Executed daily with different date ranges
-- category and price are correlated (electronics more expensive)
SELECT * FROM products p
JOIN orders o ON p.id = o.product_id
WHERE p.category = 'electronics' AND p.price > 500;

-- Execution 1: estimate 200K, actual 48K -> correction 0.24
-- Execution 2: uses corrected estimate -> better join order
```

### Positive: Prepared statement with varying selectivity

```sql
PREPARE q AS
SELECT * FROM events e JOIN users u ON e.user_id = u.id
WHERE e.type = $1 AND e.timestamp > $2;

-- $1 = 'login': 10M events (common)
-- $1 = 'upgrade': 1000 events (rare)
-- Feedback learns per-parameter correction factors
```

### Negative: One-off ad-hoc query

```sql
-- Executed once, never repeated
SELECT * FROM temp_analysis WHERE complex_condition();
-- No feedback history, no benefit
```

## References

**Academic papers:**
- Stillger et al., "LEO - DB2's LEarning Optimizer", VLDB 2001
- Chaudhuri, "Self-Tuning Database Systems: A Decade of Progress", VLDB 2007
- Wu et al., "Predicting Query Execution Time", VLDB 2013

**Implementation:**
- Oracle: Adaptive Cursor Sharing, Automatic SQL Plan Management
- DB2: LEO (Learning Optimizer) since v8
- mssql: CE Feedback (since 2022), memory grant feedback
- PostgreSQL: pg_plan_guarantee extension (community)

**Key insights:**
- LEO (DB2) was the first production cardinality feedback system
- Exponential moving average prevents oscillation from outlier executions
- Operator signatures enable knowledge transfer between similar queries
- Feedback granularity: per-operator is more precise than per-query
