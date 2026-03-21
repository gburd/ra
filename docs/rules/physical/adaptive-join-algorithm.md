# Rule: Adaptive Join Algorithm

**Category:** physical/join-algorithms
**File:** `rules/physical/join-algorithms/adaptive-join-algorithm.rra`

## Metadata

- **ID:** `adaptive-join-algorithm`
- **Version:** "1.0.0"
- **Databases:** duckdb, oracle, mssql
- **Tags:** join, adaptive, runtime, switching
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(join ?type ?cond ?left ?right)"
    description: "Join with runtime algorithm selection"
  - type: "capability"
    database: "current"
    requires: "adaptive_execution"
    description: "Database supports runtime algorithm switching"
```


# Adaptive Join Algorithm

## Metadata
- **Rule ID**: `adaptive-join-algorithm`
- **Category**: Physical / Join Algorithms
- **Complexity**: Varies -- adapts at runtime between O(n+m) and O(n*m)
- **Introduced**: Oracle 12c adaptive plans, mssql adaptive joins
- **Prerequisites**: Runtime cardinality feedback mechanism
- **Alternatives**: hash-join, nested-loop-join, sort-merge-join

## Description

Adaptive join defers the final algorithm choice until runtime, starting
execution with one strategy and switching mid-query if statistics prove
inaccurate. The optimizer sets a cardinality threshold: if the build side
exceeds it, the join switches from nested-loop to hash join (or vice versa).

**When to use:**
- Cardinality estimates are unreliable (skewed data, correlated predicates)
- First execution of parameterized queries
- Complex predicates where selectivity is hard to estimate
- Queries with multiple possible optimal plans

**Advantages:**
- Robust against estimation errors
- Self-correcting at runtime
- No need for perfect statistics
- Amortized overhead across repeated executions

**Disadvantages:**
- Switching overhead during execution
- Memory pre-allocation must handle both strategies
- More complex operator implementation
- Cannot adapt once past the point of no return

## Relational Algebra

```
Join(R, S, theta)
-> AdaptiveJoin(R, S, theta,
     threshold = T,
     strategy_below = NestedLoop,
     strategy_above = HashJoin)

Runtime behavior:
  Start building with strategy_below
  if rows_seen > threshold:
    switch to strategy_above (reuse buffered rows)
  else:
    continue with strategy_below
```

## Implementation (egg rewrite rules)

```lisp
;; Mark joins as adaptive when cardinality is uncertain
(rewrite (join ?left ?right ?cond)
  (adaptive-join ?left ?right ?cond
    :threshold (cardinality-threshold ?left)
    :low-card-strategy nested-loop
    :high-card-strategy hash-join)
  :if (uncertain-cardinality ?left)
  :if (> (cardinality-error-bound ?left) 3.0))

;; Oracle-style: adapt between nested-loop and hash
(rewrite (join ?left ?right ?cond)
  (adaptive-join ?left ?right ?cond
    :threshold (adaptive-threshold ?left ?right)
    :low-card-strategy index-nested-loop
    :high-card-strategy hash-join)
  :if (has-index ?right ?cond)
  :if (uncertain-cardinality ?left))

;; Revert to fixed strategy after statistics stabilize
(rewrite (adaptive-join ?left ?right ?cond
           :threshold ?t :low-card-strategy ?ls
           :high-card-strategy ?hs)
  (hash-join ?left ?right ?cond)
  :if (known-cardinality ?left)
  :if (> (cardinality ?left) ?t))
```

## Implementation Pattern

```rust
pub struct AdaptiveJoin {
    left: Box<dyn Operator>,
    right: Box<dyn Operator>,
    condition: JoinCondition,
    threshold: u64,
    rows_buffered: Vec<Tuple>,
    current_strategy: JoinStrategy,
    switched: bool,
}

enum JoinStrategy {
    NestedLoop(NestedLoopState),
    HashJoin(HashJoinState),
}

impl Operator for AdaptiveJoin {
    fn next(&mut self) -> Option<Tuple> {
        if !self.switched {
            // Buffer rows and check threshold
            while self.rows_buffered.len() < self.threshold as usize {
                match self.left.next() {
                    Some(tuple) => self.rows_buffered.push(tuple),
                    None => {
                        // Input smaller than threshold: use NL
                        return self.finish_nested_loop();
                    }
                }
            }
            // Exceeded threshold: switch to hash join
            self.switched = true;
            self.build_hash_table_from_buffer();
            self.continue_hash_build();
        }
        self.probe_hash_table()
    }
}
```

## Cost Model

```rust
pub fn cost_adaptive_join(
    estimated_left: u64,
    right_card: u64,
    threshold: u64,
    hardware: &HardwareModel,
) -> Cost {
    let switching_overhead = Cost::cpu(threshold * 2); // Buffer + rebuild

    // Expected cost is weighted average of strategies
    let p_below = probability_below_threshold(estimated_left, threshold);
    let nl_cost = cost_nested_loop(threshold, right_card, hardware);
    let hj_cost = cost_hash_join(estimated_left, right_card, hardware);

    let expected = Cost::weighted(
        p_below, nl_cost,
        1.0 - p_below, hj_cost + switching_overhead,
    );

    expected
}

fn adaptive_threshold(
    estimated_card: u64,
    right_card: u64,
    available_memory: u64,
) -> u64 {
    // Threshold where NL cost equals HJ cost
    // NL: threshold * right_card comparisons
    // HJ: right_card (build) + estimated_card (probe)
    // Break-even: threshold * right_card = right_card + estimated_card
    // threshold = 1 + estimated_card / right_card
    let break_even = 1 + estimated_card / right_card.max(1);
    break_even.min(available_memory / 100) // Memory-bounded
}
```

## Test Cases

### Test 1: Cardinality underestimate triggers switch
```sql
CREATE TABLE orders (id INT, customer_id INT, status TEXT);
CREATE TABLE customers (id INT PRIMARY KEY, name TEXT);

-- Optimizer estimates 100 matching orders, actual is 50,000
SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id
WHERE o.status = 'pending';

-- Expected: AdaptiveJoin
-- Starts with NestedLoop (estimated 100 rows)
-- Switches to HashJoin when actual rows exceed threshold
```

### Test 2: Stays with nested-loop when estimate correct
```sql
SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id
WHERE o.id = 42;

-- Expected: AdaptiveJoin starts with NestedLoop
-- Only 1 row from orders: stays with NestedLoop (optimal)
-- No switching overhead incurred
```

### Test 3: Parameterized query with varying selectivity
```sql
PREPARE order_lookup AS
SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id
WHERE o.status = $1;

EXECUTE order_lookup('archived');  -- 1M rows -> hash join
EXECUTE order_lookup('urgent');    -- 5 rows -> nested loop

-- Expected: AdaptiveJoin handles both cases
-- First execution: switches to hash join
-- Feedback updates statistics for future executions
```

### Test 4: Negative -- stable well-known cardinality
```sql
SELECT * FROM small_lookup_table t1
JOIN small_lookup_table t2 ON t1.key = t2.key;

-- NOT suitable: both tables have known small cardinality
-- Fixed hash join or nested loop is simpler and equally fast
```

## Performance Characteristics

| Scenario | Fixed NL | Fixed HJ | Adaptive |
|----------|----------|----------|----------|
| Estimate accurate, small | Optimal | Overhead | Near-optimal |
| Estimate accurate, large | Slow | Optimal | Near-optimal |
| 10x underestimate | Very slow | Optimal | Good (switch) |
| 10x overestimate | Optimal | Wasted memory | Good (no switch) |

## References

1. **Oracle Adaptive Plans**: "Adaptive Query Processing" (Oracle 12c)
   - Runtime plan adaptation based on actual cardinalities

2. **mssql Adaptive Joins**: "Adaptive query processing in SQL databases"
   - https://learn.microsoft.com/en-us/sql/relational-databases/performance/adaptive-query-processing

3. **Babu & Bizarro**: "Adaptive Query Processing in the Looking Glass"
   - Foundations of Trends in Databases, 2005

4. **Deshpande et al.**: "Adaptive Query Processing"
   - DOI: 10.1561/1900000001
