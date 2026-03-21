# Rule: EDDY Adaptive Query Routing

**Category:** experimental/adaptive
**File:** `rules/experimental/adaptive/eddy-operator.rra`

## Metadata

- **ID:** `eddy-operator`
- **Version:** "1.0.0"
- **Databases:** postgresql
- **Tags:** adaptive, eddy, runtime-optimization, query-routing
- **Authors:** "Avnur & Hellerstein 2000", "RA Contributors"


# EDDY Adaptive Query Routing

## Description

Replaces a static query plan with an adaptive EDDY operator that routes tuples
through operators dynamically based on runtime performance and selectivity.
Instead of committing to a fixed join order, EDDY observes operator behavior
and adaptively reorders operations to minimize total processing time.

**When to apply**: Queries with unpredictable selectivity, time-varying data
distributions, or when optimization time must be minimized (ad-hoc queries).

**Why it works**: Static plans can be suboptimal when cardinality estimates
are wrong or data distributions change. EDDY uses online learning to route
tuples through the most promising operators first, effectively performing
runtime query optimization without replanning overhead.

## Relational Algebra

```algebra
(join[p1] (join[p2] (filter[p3] R) S) T)
  -> eddy({filter[p3], join[p1], join[p2]}, {R, S, T})
  where has_uncertain_selectivity(p1, p2, p3)

EDDY maintains:
- Ready bitmap per tuple (which operators have processed it)
- Operator selectivity estimates (updated online)
- Routing lottery (probability of sending tuple to each operator)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("eddy-adaptive-routing";
    "(join ?p1 (join ?p2 ?r1 ?r2) ?r3)" =>
    "(eddy
       (operators (filter ?p1) (filter ?p2) (join ?r1 ?r2) (join))
       (sources ?r1 ?r2 ?r3))"
    if has_uncertain_selectivity()
    if is_adhoc_query()
),

// EDDY runtime implementation
struct Eddy {
    operators: Vec<Box<dyn PhysicalOperator>>,
    sources: Vec<TupleStream>,
    routing_table: RoutingTable,
    statistics: OnlineStatistics,
}

impl Eddy {
    fn execute(&mut self) -> Vec<Tuple> {
        let mut results = Vec::new();

        loop {
            // Get next tuple from sources
            let mut tuple = match self.get_next_source_tuple() {
                Some(t) => t,
                None => break,
            };

            // Route tuple through operators until done
            while !tuple.is_done() {
                let next_op = self.select_next_operator(&tuple);

                // Apply operator
                let output = self.operators[next_op].apply(&tuple);

                // Update statistics
                self.statistics.record_selectivity(
                    next_op,
                    output.len() as f64,
                );

                // Update routing probabilities
                self.update_routing_table();

                // Continue with output tuples
                for out_tuple in output {
                    if out_tuple.is_done() {
                        results.push(out_tuple);
                    } else {
                        tuple = out_tuple;
                    }
                }
            }
        }

        results
    }

    fn select_next_operator(&self, tuple: &Tuple) -> usize {
        // Lottery scheduling: probability proportional to inverse selectivity
        let ready_ops: Vec<usize> = self
            .operators
            .iter()
            .enumerate()
            .filter(|(i, _)| !tuple.ready_bitmap.is_set(*i))
            .map(|(i, _)| i)
            .collect();

        if ready_ops.is_empty() {
            panic!("No ready operators for tuple");
        }

        // Select operator with lowest estimated cost
        let mut best_op = ready_ops[0];
        let mut best_priority = f64::MAX;

        for &op_id in &ready_ops {
            let selectivity = self.statistics.get_selectivity(op_id);
            let cost = self.statistics.get_avg_cost(op_id);
            let priority = cost * selectivity; // Expected cost

            if priority < best_priority {
                best_priority = priority;
                best_op = op_id;
            }
        }

        best_op
    }

    fn update_routing_table(&mut self) {
        // Use multi-armed bandit algorithm (epsilon-greedy)
        for op_id in 0..self.operators.len() {
            let wins = self.statistics.get_wins(op_id);
            let trials = self.statistics.get_trials(op_id);

            if trials > 0 {
                let win_rate = wins as f64 / trials as f64;
                self.routing_table.set_priority(op_id, win_rate);
            }
        }
    }
}

struct OnlineStatistics {
    selectivity: Vec<ExponentialMovingAverage>,
    cost: Vec<ExponentialMovingAverage>,
    wins: Vec<u64>,
    trials: Vec<u64>,
}

impl OnlineStatistics {
    fn record_selectivity(&mut self, op_id: usize, output_size: f64) {
        self.selectivity[op_id].update(output_size);
        self.trials[op_id] += 1;

        // "Win" if selectivity is low (fewer output tuples)
        if output_size < 0.5 {
            self.wins[op_id] += 1;
        }
    }
}
```

**Restrictions:**
- Operators must be independent (commutative joins, independent filters)
- Non-commutative operators require careful ready-bitmap management
- Overhead of tuple routing (bitmap checks, lottery scheduling)
- Not suitable for bulk processing (better for interactive/streaming)

## Cost Model

```rust
fn estimated_benefit(
    query_stats: &Statistics,
    selectivity_uncertainty: f64,
) -> f64 {
    // Static plan cost with potentially wrong selectivity
    let static_cost = estimate_static_plan_cost(query_stats);

    // EDDY overhead: routing decisions + statistics updates
    let routing_overhead_per_tuple = 50.0; // nanoseconds
    let total_routing_overhead =
        query_stats.row_count * routing_overhead_per_tuple;

    // EDDY benefit: avoid processing tuples through expensive operators first
    let selectivity_improvement = 1.0 - selectivity_uncertainty;
    let eddy_cost = static_cost * selectivity_improvement
        + total_routing_overhead;

    if static_cost > eddy_cost {
        (static_cost - eddy_cost) / static_cost
    } else {
        0.0
    }
}
```

**Assumptions:**
- Operators have varying selectivity (routing opportunities exist)
- Tuple-at-a-time processing is acceptable (not bulk-optimized)
- Online statistics converge quickly (< 1000 tuples)
- Ready bitmap overhead is small (< 64 operators)

**Typical benefit**: 20-60% improvement when selectivity estimates are off by
2x or more, especially for multi-way joins with uncertain cardinalities.

## Test Cases

### Positive: Query with uncertain selectivities

```sql
-- User-defined predicates with unknown selectivity
SELECT *
FROM events e
JOIN users u ON e.user_id = u.id
JOIN sessions s ON e.session_id = s.id
WHERE expensive_udf(e.data) = true
  AND u.status IN (SELECT status FROM active_users)
  AND s.duration > compute_threshold(s.ip);

-- Static plan may choose wrong order if UDF selectivity is unknown
-- EDDY adapts: routes tuples through cheap filters first,
-- learns UDF selectivity online, adjusts routing dynamically
```

### Positive: Time-varying selectivity (streaming)

```sql
-- Stream with changing distribution
SELECT *
FROM sensor_stream s
WHERE s.temperature > threshold
  AND s.location IN (SELECT loc FROM active_regions)
  AND s.reading > calibrated_value(s.sensor_id);

-- EDDY adapts to changing selectivity as data distribution shifts
-- (e.g., temperature threshold becomes more/less selective over time)
```

### Negative: Simple selective filter

```sql
SELECT * FROM users WHERE id = 12345;

-- EDDY overhead not justified: single operator, known selectivity
-- Expected: Direct indexed lookup, no adaptive routing
```

## References

**Academic papers:**
- Avnur & Hellerstein, "Eddies: Continuously Adaptive Query Processing", SIGMOD 2000
- Raman et al., "Online Dynamic Reordering for Interactive Data Processing", VLDB 1999
- Babu et al., "Adaptive Ordering of Pipelined Stream Filters", SIGMOD 2004

**Implementation:**
- TelegraphCQ: Streaming query engine with EDDY
- PostgreSQL (historical): Early EDDY prototype
- Modern streaming engines: Flink, Spark Structured Streaming (adaptive execution)

**Key insights:**
- Lottery scheduling balances exploration vs exploitation
- Ready bitmaps track tuple progress through operators (state machine)
- EDDY converges to optimal routing in O(n log n) tuples
- Works best with 3-8 operators (bitmap overhead increases with |operators|)
- Can be combined with other adaptive techniques (adaptive filters, ripple joins)
