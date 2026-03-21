# Rule: Differential Incremental Stream Aggregation

**Category:** execution-models
**File:** `rules/execution-models/differential/differential-stream-aggregate.rra`

## Metadata

- **ID:** `differential-stream-aggregate`
- **Version:** 1.0.0
- **Databases:** Materialize, differential-dataflow, Noria
- **Tags:** execution, differential, aggregation, streaming, incremental, reduce
- **SQL Standard:** differential-dataflow
- **Authors:** Frank McSherry


# Differential Incremental Stream Aggregation

## Description

Incremental aggregation in differential dataflow maintains per-group aggregate state and updates it as changes arrive. When a row is inserted (+1), its value is added to the group's accumulator. When retracted (-1), the value is subtracted. The output is a changelog of changes to the aggregation result. This enables O(1) per-change cost for SUM/COUNT (vs O(N) for full recomputation) and correct handling of late data and retractions.

**Aggregate types and incrementability:**
- **SUM**: Fully incremental -- add/subtract value (+O(1) per change)
- **COUNT**: Fully incremental -- increment/decrement (+O(1) per change)
- **MIN/MAX**: Partially incremental -- retraction may require rescan (O(N) worst case)
- **AVG**: Incremental via SUM/COUNT decomposition
- **DISTINCT COUNT**: Incremental with counter per distinct value
- **TOP-K**: Maintain sorted buffer, retraction replaces element

**Key characteristics:**
- **Per-group state**: Hash table mapping group key to accumulator
- **Delta output**: Only emit when aggregate result changes
- **Retraction handling**: Subtract retracted values from accumulators
- **Correct for late data**: Late changes update aggregate, emit correction
- **Composable**: Aggregate output is itself a changelog

**Trade-offs:**
- MIN/MAX require maintaining full sorted set per group for correctness
- High-cardinality groups increase memory usage
- DISTINCT aggregates need per-distinct-value counters
- Complex aggregates (median, percentile) not incrementalizable

## Implementation

```rust
/// Incremental stream aggregation operator
pub struct IncrementalAggregate {
    group_keys: Vec<ColumnId>,
    agg_funcs: Vec<IncrementalAggFunc>,
    /// Per-group accumulator state
    groups: HashMap<GroupKey, GroupState>,
}

pub struct GroupState {
    accumulators: Vec<Accumulator>,
    /// For MIN/MAX: maintain sorted multiset of values
    sorted_values: Option<BTreeMap<Value, i64>>,
}

pub enum Accumulator {
    Sum(f64),
    Count(i64),
    SumCount(f64, i64), // For AVG
    MinMax {
        current_min: Option<Value>,
        current_max: Option<Value>,
    },
    DistinctCount(HashMap<Value, i64>),
}

impl IncrementalAggregate {
    /// Process a batch of changes, emit aggregate result changes
    pub fn process_changes(
        &mut self,
        changes: Vec<Change>,
    ) -> Vec<Change> {
        let mut output = Vec::new();

        for change in changes {
            let key = extract_group_key(
                &change.data, &self.group_keys,
            );
            let group = self.groups.entry(key.clone())
                .or_insert_with(|| GroupState::new(&self.agg_funcs));

            // Compute old output value
            let old_result = group.current_result(&self.agg_funcs);

            // Apply change to accumulators
            for (i, func) in self.agg_funcs.iter().enumerate() {
                let value = extract_value(&change.data, func.column);
                group.update(i, func, &value, change.diff);
            }

            // Compute new output value
            let new_result = group.current_result(&self.agg_funcs);

            // Emit delta if result changed
            if old_result != new_result {
                if let Some(old) = old_result {
                    output.push(Change {
                        data: (key.clone(), old),
                        time: change.time.clone(),
                        diff: -1, // Retract old result
                    });
                }
                if let Some(new) = new_result {
                    output.push(Change {
                        data: (key, new),
                        time: change.time,
                        diff: 1, // Insert new result
                    });
                }
            }
        }

        output
    }
}

impl GroupState {
    fn update(
        &mut self,
        idx: usize,
        func: &IncrementalAggFunc,
        value: &Value,
        diff: i64,
    ) {
        match &mut self.accumulators[idx] {
            Accumulator::Sum(sum) => {
                *sum += value.as_f64() * diff as f64;
            }
            Accumulator::Count(count) => {
                *count += diff;
            }
            Accumulator::SumCount(sum, count) => {
                *sum += value.as_f64() * diff as f64;
                *count += diff;
            }
            Accumulator::MinMax { current_min, current_max } => {
                // Must maintain full value multiset for correctness
                let sorted = self.sorted_values.as_mut().unwrap();
                *sorted.entry(value.clone()).or_default() += diff;

                // Remove entries with count <= 0
                sorted.retain(|_, count| *count > 0);

                // Recompute min/max from sorted set
                *current_min = sorted.keys().next().cloned();
                *current_max = sorted.keys().last().cloned();
            }
            Accumulator::DistinctCount(counter) => {
                *counter.entry(value.clone()).or_default() += diff;
                counter.retain(|_, count| *count > 0);
            }
        }
    }

    fn current_result(
        &self,
        funcs: &[IncrementalAggFunc],
    ) -> Option<Vec<Value>> {
        let mut result = Vec::new();
        for (i, func) in funcs.iter().enumerate() {
            let val = match &self.accumulators[i] {
                Accumulator::Sum(s) => Value::Float(*s),
                Accumulator::Count(c) => {
                    if *c <= 0 { return None; } // Group disappeared
                    Value::Int(*c)
                }
                Accumulator::SumCount(s, c) => {
                    if *c <= 0 { return None; }
                    Value::Float(*s / *c as f64)
                }
                Accumulator::MinMax { current_min, .. } => {
                    current_min.clone()?
                }
                Accumulator::DistinctCount(counter) => {
                    Value::Int(counter.len() as i64)
                }
            };
            result.push(val);
        }
        Some(result)
    }
}
```

## Cost Model

**Per-Change Cost by Aggregate Type:**
- SUM: O(1) -- single addition
- COUNT: O(1) -- single increment
- AVG: O(1) -- update sum and count
- MIN/MAX: O(log G) where G = group cardinality (BTreeMap operation)
- DISTINCT COUNT: O(1) amortized (HashMap insert)

**Memory per Group:**
- SUM: 8 bytes (one f64)
- COUNT: 8 bytes (one i64)
- MIN/MAX: O(G) per group (sorted multiset of all values)
- DISTINCT COUNT: O(D) per group (counter per distinct value)

**Output Rate:** Changes per input change = 0-2 (retract old + insert new)

## Test Cases

```sql
-- Test 1: Incremental SUM
CREATE MATERIALIZED VIEW revenue AS
SELECT region, SUM(amount) FROM orders GROUP BY region;
-- INSERT: region_sum += amount, emit new total
-- DELETE: region_sum -= amount, emit corrected total

-- Test 2: Incremental COUNT with retraction
CREATE MATERIALIZED VIEW active_counts AS
SELECT status, COUNT(*) FROM orders GROUP BY status;
-- UPDATE status: retract from old group, insert to new group
-- Two groups change: old_count-1, new_count+1

-- Test 3: MIN with retraction (expensive case)
CREATE MATERIALIZED VIEW cheapest AS
SELECT category, MIN(price) FROM products GROUP BY category;
-- DELETE cheapest product: must rescan group for new MIN
-- Maintains BTreeMap per group for O(log N) re-min

-- Test 4: AVG via SUM/COUNT decomposition
CREATE MATERIALIZED VIEW avg_order AS
SELECT customer_id, AVG(total) FROM orders GROUP BY customer_id;
-- Maintained as SUM(total)/COUNT(*) per customer
-- Each change: update sum and count, recompute average
```

## References

1. **McSherry, Frank et al**. "differential-dataflow." CIDR 2013.
2. **Koch, Christoph et al**. "DBToaster: Higher-Order Delta Processing for Dynamic, Frequently Fresh Views." VLDB 2014.
3. **Materialize Documentation**. "Aggregation and Reduction."
4. **Ahmad, Yanif; Koch, Christoph**. "DBToaster: A SQL Compiler for High-Performance Delta Processing." VLDB 2009.
