# Rule: Adaptive Aggregation

**Category:** physical/aggregation-strategies
**File:** `rules/physical/aggregation-strategies/adaptive-aggregation.rra`

## Metadata

- **ID:** `adaptive-aggregation`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, clickhouse, cockroachdb, mssql, oracle
- **Tags:** aggregation, adaptive, runtime
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(aggregate ?input ?groups ?aggs)"
    description: "Aggregation with runtime strategy adaptation"
  - type: "fact"
    fact_type: "statistics.cardinality"
    table: "?input"
    comparator: "exists"
    description: "Runtime cardinality monitoring for strategy switching"
  - type: "capability"
    database: "current"
    requires: "adaptive_execution"
    description: "Database supports adaptive query execution"
```


# Adaptive Aggregation

## Metadata
- **Rule ID**: `adaptive-aggregation`
- **Category**: Physical / Aggregation Strategies
- **Complexity**: O(n) with runtime adaptation
- **Introduced**: Modern systems (2015+)
- **Prerequisites**: Runtime statistics, adaptive execution framework
- **Alternatives**: Static aggregation strategy selection

## Description

Adaptive aggregation monitors runtime characteristics (cardinality, memory usage, skew) and dynamically adjusts strategy. May switch from hash to sort, enable spilling, or trigger repartitioning mid-execution.

**Adaptation triggers:**
- Memory pressure -> spill to disk
- Cardinality exceeds estimate -> switch to sort
- Data skew detected -> repartition
- Early termination possible -> streaming

**When to use:**
- Highly variable workloads
- Unknown data characteristics
- Production systems requiring robustness
- Cloud environments with variable resources

## Relational Algebra

```
$\gamma$_{g; AGG(v)}(R)
-> AdaptiveAggregation(R, g, AGG, initial_strategy)
  where strategy adapts based on runtime feedback
```

## Implementation

```rust
pub struct AdaptiveAggregation {
    input: Box<dyn Operator>,
    group_cols: Vec<usize>,
    agg_funcs: Vec<AggregateFunction>,
    strategy: Box<dyn AggregationStrategy>,
    stats: RuntimeStats,
    adaptation_threshold: usize,
}

struct RuntimeStats {
    rows_processed: u64,
    groups_seen: u64,
    memory_used: usize,
    last_check: Instant,
}

trait AggregationStrategy {
    fn process(&mut self, tuple: Tuple) -> Result<(), AdaptationNeeded>;
    fn finalize(&mut self) -> Vec<Tuple>;
}

enum AdaptationNeeded {
    SwitchToSort,
    EnableSpilling,
    Repartition,
}

impl Operator for AdaptiveAggregation {
    fn next(&mut self) -> Option<Tuple> {
        loop {
            match self.strategy.process(self.input.next()?) {
                Ok(()) => {
                    // Check if adaptation needed
                    if self.should_check_adaptation() {
                        if let Some(new_strategy) = self.evaluate_adaptation() {
                            self.adapt(new_strategy);
                        }
                    }
                }
                Err(AdaptationNeeded::SwitchToSort) => {
                    self.switch_to_sort_aggregation();
                }
                Err(AdaptationNeeded::EnableSpilling) => {
                    self.enable_spilling();
                }
                Err(AdaptationNeeded::Repartition) => {
                    self.repartition();
                }
            }
        }
    }

    fn adapt(&mut self, new_strategy: Box<dyn AggregationStrategy>) {
        // Transfer state from old to new strategy
        let old_state = self.strategy.extract_state();
        self.strategy = new_strategy;
        self.strategy.import_state(old_state);
    }
}
```

## Adaptation Policies

```rust
impl AdaptiveAggregation {
    fn evaluate_adaptation(&self) -> Option<Box<dyn AggregationStrategy>> {
        // Policy 1: Memory pressure
        if self.stats.memory_used > self.memory_limit * 0.8 {
            if self.is_hash_based() {
                return Some(Box::new(SortAggregation::new()));
            }
        }

        // Policy 2: Cardinality explosion
        let cardinality_rate = self.stats.groups_seen as f64 / self.stats.rows_processed as f64;
        if cardinality_rate > 0.9 {
            // Nearly unique: sorting more efficient
            return Some(Box::new(SortAggregation::new()));
        }

        // Policy 3: Skew detected
        if self.detect_skew() {
            return Some(Box::new(SkewHandlingAggregation::new()));
        }

        None
    }

    fn detect_skew(&self) -> bool {
        // Check if largest group > 10x average
        let avg_group_size = self.stats.rows_processed / self.stats.groups_seen;
        let max_group_size = self.get_largest_group_size();
        max_group_size > avg_group_size * 10
    }
}
```

## Cost Model

```rust
pub fn expected_cost_adaptive_aggregation(
    input_card: u64,
    estimated_groups: u64,
    adaptation_probability: f64,
) -> Cost {
    // Expected cost = weighted average of strategies
    let hash_cost = cost_hash_aggregation(input_card, estimated_groups);
    let sort_cost = cost_sort_aggregation(input_card, estimated_groups);
    let adaptation_overhead = Cost::cpu(input_card / 1000); // Periodic checks

    let expected_strategy_cost = hash_cost * (1.0 - adaptation_probability)
        + sort_cost * adaptation_probability;

    expected_strategy_cost + adaptation_overhead
}
```

## Test Cases

### Test 1: Adaptation from hash to sort
```sql
-- Estimated: 1000 groups (hash-based chosen)
-- Actual: 10M groups (adaptation triggers)

SELECT user_id, COUNT(*)
FROM events
WHERE event_date = '2024-01-01'
GROUP BY user_id;

-- Runtime adaptation:
-- 1. Start with hash aggregation
-- 2. Detect cardinality explosion after 1M rows
-- 3. Switch to sort aggregation
-- 4. Spill hash table to disk
-- 5. Continue with sort-based approach
```

### Test 2: Skew handling
```sql
-- Most users have <10 events, but user_id=123 has 1M events

SELECT user_id, COUNT(*)
FROM events
GROUP BY user_id;

-- Adaptation:
-- 1. Detect skewed key (user_id=123)
-- 2. Process skewed key separately
-- 3. Use hash for remaining keys
```

### Test 3: Memory pressure adaptation
```sql
-- Memory limit: 1GB
-- Estimated groups: 100K (fits in memory)
-- Actual groups: 10M (exceeds memory)

SELECT product_id, AVG(price)
FROM transactions
GROUP BY product_id;

-- Adaptation:
-- 1. Hash table grows to 800MB (80% threshold)
-- 2. Trigger spilling mechanism
-- 3. Partition hash table to disk
-- 4. Continue with hybrid hash/sort approach
```

## References

1. **Dutt et al. (2018)**: "Adaptive Query Execution in Modern Database Systems"
2. **mssql**: Adaptive query processing
   - https://docs.microsoft.com/sql/relational-databases/performance/adaptive-query-processing
3. **Presto**: Adaptive execution framework
4. **Spark 3.0**: Adaptive Query Execution (AQE)
   - https://spark.apache.org/docs/latest/sql-performance-tuning.html#adaptive-query-execution

## Tags
`physical`, `aggregation`, `adaptive`, `runtime`, `robust`
