# Rule: Differential Incremental Aggregation

**Category:** execution-models/differential
**File:** `rules/execution-models/differential/differential-incremental-aggregation.rra`

## Metadata

- **ID:** `differential-incremental-aggregation`
- **Version:** "1.0.0"
- **Databases:** materialize, differential-dataflow
- **Tags:** execution, differential, aggregation, reduce, incremental
- **Authors:** "Frank McSherry"


# Differential Incremental Aggregation

## Description

Maintains aggregate results (GROUP BY with SUM, COUNT, MIN, MAX, AVG, etc.)
incrementally by processing only the changes to each group. Instead of
recomputing the aggregate from all rows in the group, the operator maintains
per-group state and applies deltas from incoming changes.

**Aggregate categories by incremental difficulty:**
- **Algebraic** (SUM, COUNT, AVG): State is fixed-size, O(1) update per change
- **Distributive** (MIN, MAX): State may need full recomputation on retraction
  of the current extreme value
- **Holistic** (MEDIAN, PERCENTILE): State must store all values

**Key challenge**: Retracting the current MIN/MAX requires knowing the next
value. Differential maintains a full multiplicity map for each group to handle
this correctly.

## Relational Algebra

```
Incremental aggregation:
  State per group: multiplicity_map[value -> count], aggregate_result

  On change (group_key, value, time, diff):
    old_result = state[group_key].aggregate_result

    // Update multiplicity map
    state[group_key].multiplicities[value] += diff
    if state[group_key].multiplicities[value] == 0:
      remove value from map

    // Recompute aggregate from updated map
    new_result = compute_aggregate(state[group_key].multiplicities)

    if old_result != new_result:
      emit (group_key, old_result, time, -1)  // retract old
      emit (group_key, new_result, time, +1)  // insert new
```

## Implementation

```rust
/// Incremental aggregate operator
pub struct IncrementalAggregate<T: Timestamp> {
    /// Per-group state: value multiplicities and current result
    groups: HashMap<Key, GroupState>,
    /// Aggregate function type
    agg_fn: AggregateFn,
}

/// State for one group
pub struct GroupState {
    /// Map of (value -> multiplicity count)
    multiplicities: BTreeMap<Value, i64>,
    /// Current aggregate result
    current_result: Option<AggResult>,
    /// Total row count for this group
    total_count: i64,
}

impl GroupState {
    pub fn apply_change(
        &mut self,
        value: &Value,
        diff: Diff,
        agg_fn: &AggregateFn,
    ) -> (Option<AggResult>, Option<AggResult>) {
        let old = self.current_result.clone();

        // Update multiplicities
        let entry = self.multiplicities.entry(value.clone()).or_insert(0);
        *entry += diff as i64;
        if *entry == 0 {
            self.multiplicities.remove(value);
        }
        self.total_count += diff as i64;

        // Recompute aggregate
        self.current_result = if self.total_count <= 0 {
            None // Group is empty
        } else {
            Some(match agg_fn {
                AggregateFn::Count => AggResult::Int(self.total_count),

                AggregateFn::Sum => {
                    let sum: f64 = self.multiplicities.iter()
                        .map(|(v, &mult)| v.as_f64() * mult as f64)
                        .sum();
                    AggResult::Float(sum)
                }

                AggregateFn::Min => {
                    // First key in BTreeMap with positive multiplicity
                    self.multiplicities.iter()
                        .find(|(_, &mult)| mult > 0)
                        .map(|(v, _)| AggResult::Value(v.clone()))
                        .unwrap_or(AggResult::Null)
                }

                AggregateFn::Max => {
                    // Last key in BTreeMap with positive multiplicity
                    self.multiplicities.iter()
                        .rev()
                        .find(|(_, &mult)| mult > 0)
                        .map(|(v, _)| AggResult::Value(v.clone()))
                        .unwrap_or(AggResult::Null)
                }

                AggregateFn::Avg => {
                    let sum: f64 = self.multiplicities.iter()
                        .map(|(v, &mult)| v.as_f64() * mult as f64)
                        .sum();
                    AggResult::Float(sum / self.total_count as f64)
                }
            })
        };

        (old, self.current_result.clone())
    }
}

impl<T: Timestamp> IncrementalAggregate<T> {
    pub fn process_changes(
        &mut self,
        changes: Vec<(Key, Value, T, Diff)>,
    ) -> Vec<(Key, AggResult, T, Diff)> {
        let mut output = Vec::new();

        // Batch changes by group key
        let mut by_group: HashMap<Key, Vec<(Value, T, Diff)>> = HashMap::new();
        for (key, val, time, diff) in changes {
            by_group.entry(key).or_default().push((val, time, diff));
        }

        for (key, group_changes) in by_group {
            let state = self.groups.entry(key.clone())
                .or_insert_with(GroupState::default);

            for (val, time, diff) in group_changes {
                let (old, new) = state.apply_change(&val, diff, &self.agg_fn);

                if old != new {
                    if let Some(old_result) = old {
                        output.push((key.clone(), old_result, time.clone(), -1));
                    }
                    if let Some(new_result) = new {
                        output.push((key.clone(), new_result, time, 1));
                    }
                }
            }
        }

        output
    }
}
```

## Cost Model

**Per-change:**
- SUM/COUNT: O(1) state update, O(1) result computation
- MIN/MAX: O(log G) where G = distinct values in group (BTreeMap lookup)
- AVG: O(1) (maintained as sum/count)
- MEDIAN: O(log G) with order-statistics tree

**Output amplification:**
- Each input change emits 0 or 2 output changes (retract old + insert new)
- No change if aggregate value unchanged (e.g., adding duplicate to COUNT DISTINCT)

**Memory per group:**
- Algebraic (SUM, COUNT): O(1) fixed state
- Distributive (MIN, MAX): O(G) multiplicity map
- Holistic (MEDIAN): O(G) all values stored

**Comparison with recomputation:**
- Recomputation: O(group_size) per change
- Incremental: O(1) to O(log G) per change
- Speedup: group_size / log(G), typically 100-10000x

## Test Cases

```sql
-- Test 1: Incremental COUNT
CREATE MATERIALIZED VIEW dept_sizes AS
SELECT department, COUNT(*) AS size FROM employees GROUP BY department;
-- INSERT: size goes from 50 to 51 (one output change pair)
-- DELETE: size goes from 51 to 50 (one output change pair)

-- Test 2: Incremental MIN with retraction
CREATE MATERIALIZED VIEW cheapest AS
SELECT category, MIN(price) AS min_price FROM products GROUP BY category;
-- DELETE cheapest product: must find next-cheapest from multiplicity map
-- If min was 9.99 and next is 12.99:
--   emit (category, 9.99, time, -1) and (category, 12.99, time, +1)

-- Test 3: SUM with high update rate
CREATE MATERIALIZED VIEW balances AS
SELECT account_id, SUM(amount) AS balance FROM transactions GROUP BY account_id;
-- 10K transactions/sec: each updates one group in O(1)
-- Total cost: O(10K) per second regardless of table size

-- Test 4: Empty group handling
-- DELETE last employee in a department
-- Group transitions from COUNT=1 to empty
-- Emit (dept, 1, time, -1) -- retract last result, no insertion
```

## References

1. **McSherry, Frank et al**. "differential-dataflow." CIDR 2013.
   - Incremental aggregation semantics in differential dataflow

2. **Koch, Christoph et al**. "Incremental View Maintenance." CACM 2015.
   - Survey of IVM techniques for different aggregate classes

3. **Materialize Source**. `src/compute/src/render/reduce.rs`
   - Production incremental reduce implementation
