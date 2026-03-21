# Rule: Differential Time-Aware Operators

**Category:** execution-models/differential
**File:** `rules/execution-models/differential/differential-time-aware-operators.rra`

## Metadata

- **ID:** `differential-time-aware-operators`
- **Version:** "1.0.0"
- **Databases:** materialize, differential-dataflow
- **Tags:** execution, differential, time, lattice, operators, temporal
- **Authors:** "Frank McSherry"


# Differential Time-Aware Operators

## Description

Implements operators that are aware of the partially-ordered time structure
(lattice) in differential dataflow. Unlike traditional streaming systems with
totally-ordered timestamps, differential dataflow operators must handle times
that form a lattice (e.g., (epoch, iteration) pairs for iterative computations).
Time-aware operators correctly process updates at arbitrary points in this lattice
and produce outputs with the correct output timestamps.

**Key concepts:**
- **Time lattice**: Timestamps form a partial order, not total order
- **Capability**: An operator holds a capability at time T, meaning it may
  produce output at time T or later in the lattice
- **Antichain**: A frontier (set of incomparable times) representing the
  minimum times at which future updates may arrive
- **Join of times**: When combining data from two inputs at times T1 and T2,
  the output timestamp is join(T1, T2) = least upper bound in the lattice

**Why this matters**: Incorrect time handling causes wrong results in
iterative computations (e.g., fixed-point iterations) and multi-input operators
(e.g., joins where inputs advance at different rates).

## Relational Algebra

```
Time-aware join:
  For (data_L, time_L, diff_L) from left input,
      (data_R, time_R, diff_R) from right input:
    if join_predicate(data_L, data_R):
      output_time = time_L.join(time_R)  // lattice join
      emit (joined_data, output_time, diff_L * diff_R)

Time-aware map:
  For (data, time, diff) from input:
    emit (f(data), time, diff)  // time preserved

Time-aware filter:
  For (data, time, diff) from input:
    if predicate(data):
      emit (data, time, diff)  // time preserved
```

## Implementation

```rust
/// Partially ordered timestamp (lattice element)
pub trait Timestamp: PartialOrd + Clone {
    /// Least upper bound of two timestamps
    fn join(&self, other: &Self) -> Self;
    /// Greatest lower bound of two timestamps
    fn meet(&self, other: &Self) -> Self;
    /// Minimum possible timestamp
    fn minimum() -> Self;
}

/// Product timestamp for iterative computations
#[derive(Clone, PartialEq, Eq)]
pub struct Product<T1: Timestamp, T2: Timestamp> {
    pub outer: T1,  // e.g., epoch
    pub inner: T2,  // e.g., iteration number
}

impl<T1: Timestamp, T2: Timestamp> Timestamp for Product<T1, T2> {
    fn join(&self, other: &Self) -> Self {
        Product {
            outer: self.outer.join(&other.outer),
            inner: self.inner.join(&other.inner),
        }
    }

    fn meet(&self, other: &Self) -> Self {
        Product {
            outer: self.outer.meet(&other.outer),
            inner: self.inner.meet(&other.inner),
        }
    }

    fn minimum() -> Self {
        Product {
            outer: T1::minimum(),
            inner: T2::minimum(),
        }
    }
}

/// Time-aware join operator
pub struct TimeAwareJoin<T: Timestamp> {
    left_arrangement: Arrangement<T>,
    right_arrangement: Arrangement<T>,
}

impl<T: Timestamp> TimeAwareJoin<T> {
    pub fn process_left_change(
        &mut self,
        key: Key,
        value_l: Value,
        time_l: T,
        diff: Diff,
    ) -> Vec<(JoinedRow, T, Diff)> {
        let mut output = Vec::new();

        // Probe right arrangement at all times
        for (value_r, time_r, diff_r) in
            self.right_arrangement.lookup(&key)
        {
            // Output time is lattice join of both input times
            let output_time = time_l.join(&time_r);
            let output_diff = diff * diff_r;

            output.push((
                join_values(&value_l, &value_r),
                output_time,
                output_diff,
            ));
        }

        output
    }
}

/// Capability tracking for operators
pub struct CapabilitySet<T: Timestamp> {
    /// Antichain of times we may still produce output at
    held: Vec<T>,
}

impl<T: Timestamp> CapabilitySet<T> {
    /// Downgrade capability: promise not to produce output
    /// before new_time
    pub fn downgrade(&mut self, new_time: &T) {
        self.held.retain(|t| !t.less_equal(new_time));
        if !self.held.iter().any(|t| t.less_equal(new_time)) {
            self.held.push(new_time.clone());
        }
    }

    /// Drop all capabilities (operator is done)
    pub fn drop_all(&mut self) {
        self.held.clear();
    }
}
```

## Cost Model

**Time join computation:**
- Per change: O(matches * log T) for lattice join operations
- Lattice join: O(1) for simple timestamps, O(D) for product of D dimensions

**Capability tracking:**
- Antichain maintenance: O(A) per downgrade where A = antichain width
- For most workloads: A = 1 (single active time), so O(1)
- For iterative: A proportional to iteration depth

**Memory:**
- Arrangements store full time history until compaction
- Product timestamps increase per-entry overhead by 2x vs simple timestamps

## Test Cases

```sql
-- Test 1: Simple time-aware join
-- Left input at time 100, right at time 200
-- Output at time max(100, 200) = 200
CREATE MATERIALIZED VIEW joined AS
SELECT l.*, r.value
FROM left_source l JOIN right_source r ON l.key = r.key;

-- Test 2: Iterative computation (product timestamps)
-- Fixed-point reachability: (epoch, iteration) timestamps
-- Each iteration's results have time (epoch, iter+1)
-- Converged results visible when inner frontier reaches max_iter

-- Test 3: Multi-input with different rates
-- Fast source updates at time 1000/sec
-- Slow source updates at time 1/sec
-- Join output times dominated by slow source timestamps
```

## References

1. **McSherry, Frank et al**. "differential-dataflow." CIDR 2013.
   - Lattice-based timestamp model for incremental computation

2. **Murray, Derek G. et al**. "Naiad: A Timely Dataflow System." SOSP 2013.
   - Pointstamp protocol for progress tracking on lattice times

3. **Abadi, Daniel et al**. "Materialize: A Streaming Database." VLDB 2022.
   - Production time-aware operator implementation
