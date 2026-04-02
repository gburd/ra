# RFC 0095: ASOF (As-Of) Join Optimization

- **Status**: Proposed
- **Priority**: High Impact (15-20 weeks)
- **Impact**: 50-100x improvement on time-series queries
- **Category**: Query Optimization / Join Algorithms
- **Created**: 2026-03-28

## Summary

Implement ASOF (As-Of) joins in Ra to enable efficient time-series inequality joins. ASOF joins match each left row with at most one right row based on inequality predicates (typically temporal), finding the "nearest" match based on an ordering column. This provides 50-100x speedup compared to self-join emulation on typical time-series workloads.

## Motivation

Time-series and event correlation queries frequently require matching records based on temporal proximity rather than exact equality. Current approaches using self-joins with inequality predicates result in O(n*m) complexity and poor performance.

### Current Limitations

**Problem 1: Inefficient Self-Join Emulation**

Users currently must write complex self-joins with inequality conditions:

```sql
-- Find the most recent price for each trade
SELECT t.trade_id, t.symbol, t.trade_time, t.quantity,
       (SELECT p.price
        FROM prices p
        WHERE p.symbol = t.symbol
          AND p.price_time &lt;= t.trade_time
        ORDER BY p.price_time DESC
        LIMIT 1) AS price
FROM trades t;
```

This pattern generates:
- O(n*m) complexity: Full nested loop for each trade
- No index exploitation: Cannot efficiently use B-tree indexes
- Poor cardinality estimates: Optimizer cannot model "nearest match" semantics

**Problem 2: Limited Time-Series Analytics**

Common use cases are difficult or impossible to optimize:
- Financial analytics: Attach the most recent stock price to each trade
- Sensor correlation: Match sensor readings to calibration events
- Log analysis: Join application events with system metrics based on timestamps
- IoT data fusion: Correlate device events with network conditions

**Problem 3: Missing DuckDB Feature Parity**

DuckDB provides native ASOF join syntax with optimized execution. Ra lacks this capability, creating a significant feature gap for analytical workloads.

### Expected Impact

| Pattern | Current (Self-Join) | With ASOF Join | Speedup |
|---------|---------------------|----------------|---------|
| Financial trades + prices (sorted) | 5000ms | 50ms | 100x |
| Sensor data correlation | 2000ms | 40ms | 50x |
| Log event matching | 3000ms | 60ms | 50x |
| IoT device correlation | 1500ms | 30ms | 50x |

## Guide-level Explanation

### Basic ASOF Join Syntax

**Standard inequality syntax:**
```sql
SELECT t.trade_id, t.symbol, t.trade_time,
       t.quantity, p.price
FROM trades t
ASOF JOIN prices p
  ON t.symbol = p.symbol
  AND t.trade_time &gt;= p.price_time;
```

**USING shorthand for common cases:**
```sql
SELECT *
FROM trades t
ASOF JOIN prices p
  USING (symbol)
  ON t.trade_time &gt;= p.price_time;
```

### Match Semantics

ASOF joins provide three matching modes based on the inequality operator:

**Forward matching (&gt;=, &gt;):** Match with the nearest earlier record
```sql
-- Find the most recent price before or at trade time
SELECT * FROM trades t
ASOF JOIN prices p
  ON t.symbol = p.symbol
  AND t.trade_time &gt;= p.price_time;
```

**Backward matching (&lt;=, &lt;):** Match with the nearest later record
```sql
-- Find the next scheduled maintenance after each sensor reading
SELECT * FROM sensor_readings s
ASOF JOIN maintenance_schedule m
  ON s.device_id = m.device_id
  AND s.reading_time &lt;= m.scheduled_time;
```

**Nearest match:** Find the closest record (requires extension)
```sql
-- Future extension: match to closest timestamp in either direction
SELECT * FROM events e
ASOF JOIN reference_data r
  ON e.device_id = r.device_id
  NEAREST e.event_time TO r.reference_time;
```

### Join Types

ASOF joins support LEFT, RIGHT, and INNER variants:

**LEFT ASOF JOIN:** Keep all left rows, NULL for unmatched
```sql
SELECT t.*, p.price
FROM trades t
LEFT ASOF JOIN prices p
  ON t.symbol = p.symbol
  AND t.trade_time &gt;= p.price_time;
-- All trades included, price=NULL if no prior price exists
```

**INNER ASOF JOIN:** Only matched rows
```sql
SELECT t.*, p.price
FROM trades t
INNER ASOF JOIN prices p
  ON t.symbol = p.symbol
  AND t.trade_time &gt;= p.price_time;
-- Only trades with prior prices
```

**RIGHT ASOF JOIN:** Keep all right rows (rarely used)

### Optimization Requirements

For optimal performance, ASOF joins require:

1. **Sorted inputs:** Both tables sorted by the ordering column (timestamp)
2. **Indexes:** B-tree indexes on ordering columns
3. **Partitioning:** Matching equality keys (e.g., symbol) benefit from partitioning

Ra will automatically inject sort operators when inputs are not pre-sorted.

## Reference-level Explanation

### AST Representation

Add ASOF join variant to the join type enumeration:

```rust
// In crates/ra-core/src/algebra.rs
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum JoinType {
    Inner,
    Left,
    Right,
    Full,
    Semi,
    Anti,
    /// ASOF join: inequality join for time-series
    /// Requires at least one inequality condition on an ordering column
    AsOf {
        direction: AsOfDirection,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AsOfDirection {
    /// Forward matching: left.time &gt;= right.time
    /// Match with the nearest earlier right record
    Forward,
    /// Backward matching: left.time &lt;= right.time
    /// Match with the nearest later right record
    Backward,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AsOfCondition {
    /// Equality predicates (e.g., symbol = symbol)
    pub equality_keys: Vec&lt;(Expr, Expr)&gt;,
    /// Inequality predicate on ordering column
    pub ordering_left: Expr,
    pub ordering_right: Expr,
    pub direction: AsOfDirection,
}
```

### Physical Operators

Implement three ASOF join algorithms:

**1. Sort-Merge ASOF Join (Primary Strategy)**

```rust
pub struct SortMergeAsOfJoin {
    left: Box&lt;dyn PhysicalOperator&gt;,
    right: Box&lt;dyn PhysicalOperator&gt;,
    condition: AsOfCondition,
    join_type: JoinType,
}

impl PhysicalOperator for SortMergeAsOfJoin {
    fn execute(&self) -&gt; Result&lt;RecordBatch&gt; {
        // Requirements:
        // - Left sorted by (equality_keys, ordering_column)
        // - Right sorted by (equality_keys, ordering_column)

        let left_batches = self.left.execute()?;
        let right_batches = self.right.execute()?;

        let mut output = Vec::new();
        let mut left_iter = left_batches.iter();
        let mut right_iter = right_batches.iter().peekable();

        for left_row in left_iter {
            // Advance right iterator to matching partition
            while let Some(right_row) = right_iter.peek() {
                if !self.equality_matches(left_row, right_row) {
                    right_iter.next();
                    continue;
                }

                // Find nearest match within partition
                let matched = self.find_nearest_match(
                    left_row,
                    &mut right_iter,
                    self.condition.direction,
                );

                output.push(self.join_rows(left_row, matched));
                break;
            }
        }

        Ok(RecordBatch::from(output))
    }

    fn find_nearest_match(
        &self,
        left_row: &Row,
        right_iter: &mut Peekable&lt;Iter&lt;Row&gt;&gt;,
        direction: AsOfDirection,
    ) -&gt; Option&lt;&Row&gt; {
        let mut best_match = None;

        match direction {
            AsOfDirection::Forward =&gt; {
                // Find largest right.time where right.time &lt;= left.time
                while let Some(right_row) = right_iter.peek() {
                    if right_row.ordering &lt;= left_row.ordering {
                        best_match = Some(*right_row);
                        right_iter.next();
                    } else {
                        break;
                    }
                }
            }
            AsOfDirection::Backward =&gt; {
                // Find smallest right.time where right.time &gt;= left.time
                if let Some(right_row) = right_iter.peek() {
                    if right_row.ordering &gt;= left_row.ordering {
                        best_match = Some(*right_row);
                    }
                }
            }
        }

        best_match
    }
}
```

**Time Complexity:** O(n + m) with sorted inputs
**Space Complexity:** O(1) for the matching window

**2. Index-Based ASOF Join**

```rust
pub struct IndexAsOfJoin {
    left: Box&lt;dyn PhysicalOperator&gt;,
    right_table: TableHandle,
    right_index: BTreeIndex,  // Index on (equality_keys, ordering_column)
    condition: AsOfCondition,
}

impl PhysicalOperator for IndexAsOfJoin {
    fn execute(&self) -&gt; Result&lt;RecordBatch&gt; {
        let left_batches = self.left.execute()?;
        let mut output = Vec::new();

        for left_row in left_batches.iter() {
            // Build index lookup key
            let partition_key = self.extract_equality_keys(left_row);
            let ordering_value = self.extract_ordering(left_row);

            // Use index to find nearest match
            let matched = match self.condition.direction {
                AsOfDirection::Forward =&gt; {
                    // Range scan: find max(right.time) where right.time &lt;= left.time
                    self.right_index.range_lookup(
                        partition_key.clone(),
                        ..=ordering_value,
                    ).last()
                }
                AsOfDirection::Backward =&gt; {
                    // Range scan: find min(right.time) where right.time &gt;= left.time
                    self.right_index.range_lookup(
                        partition_key.clone(),
                        ordering_value..,
                    ).next()
                }
            };

            output.push(self.join_rows(left_row, matched));
        }

        Ok(RecordBatch::from(output))
    }
}
```

**Time Complexity:** O(n * log m) where n = left size, m = right size
**Space Complexity:** O(m) for the index

**3. Hash-Based Approximate ASOF (Optional Optimization)**

```rust
pub struct HashAsOfJoin {
    left: Box&lt;dyn PhysicalOperator&gt;,
    right: Box&lt;dyn PhysicalOperator&gt;,
    condition: AsOfCondition,
    bucket_size: Duration,  // Time bucket size for approximation
}

impl PhysicalOperator for HashAsOfJoin {
    fn execute(&self) -&gt; Result&lt;RecordBatch&gt; {
        // Phase 1: Bucket right side by time ranges
        let right_buckets = self.build_time_buckets(
            self.right.execute()?,
            self.bucket_size,
        );

        // Phase 2: Probe with left side
        let left_batches = self.left.execute()?;
        let mut output = Vec::new();

        for left_row in left_batches.iter() {
            let bucket_id = left_row.ordering / self.bucket_size;

            // Search current bucket and adjacent bucket
            let candidates = right_buckets
                .get(&bucket_id)
                .chain(right_buckets.get(&(bucket_id - 1)));

            let matched = self.find_nearest_in_candidates(
                left_row,
                candidates,
            );

            output.push(self.join_rows(left_row, matched));
        }

        Ok(RecordBatch::from(output))
    }
}
```

**Time Complexity:** O(n + m) average case
**Space Complexity:** O(m) for hash table
**Tradeoff:** Approximate matches (may miss boundary cases)

### Cost Model

Add cost estimation for ASOF joins:

```rust
impl CostModel {
    pub fn estimate_asof_join_cost(
        &self,
        left_cardinality: f64,
        right_cardinality: f64,
        left_sorted: bool,
        right_sorted: bool,
        index_available: bool,
    ) -&gt; f64 {
        // Sort cost if inputs not pre-sorted
        let left_sort_cost = if !left_sorted {
            left_cardinality * left_cardinality.log2() * CPU_OPERATOR_COST
        } else {
            0.0
        };

        let right_sort_cost = if !right_sorted {
            right_cardinality * right_cardinality.log2() * CPU_OPERATOR_COST
        } else {
            0.0
        };

        // Join cost
        let join_cost = if index_available {
            // Index-based: O(n * log m)
            left_cardinality * right_cardinality.log2() * CPU_OPERATOR_COST * 2.0
        } else {
            // Sort-merge: O(n + m)
            (left_cardinality + right_cardinality) * CPU_OPERATOR_COST
        };

        left_sort_cost + right_sort_cost + join_cost
    }

    pub fn estimate_self_join_cost(
        &self,
        left_cardinality: f64,
        right_cardinality: f64,
    ) -&gt; f64 {
        // Self-join with inequality: O(n * m) nested loop
        left_cardinality * right_cardinality * CPU_OPERATOR_COST * 10.0
    }
}
```

### Optimization Rules

**Rule 1: ASOF Join Detection**

```rust
pub struct DetectAsOfJoinRule;

impl OptimizationRule for DetectAsOfJoinRule {
    fn apply(&self, plan: RelExpr) -&gt; Result&lt;RelExpr&gt; {
        // Detect pattern:
        // Join(Inner)
        //   Filter(equality_keys AND inequality_on_time)
        //   SubQuery(ORDER BY time LIMIT 1) or DISTINCT ON pattern

        if let RelExpr::Join { join_type, left, right, condition } = plan {
            if let Some(asof_condition) = self.extract_asof_pattern(&condition) {
                return Ok(RelExpr::AsOfJoin {
                    join_type: JoinType::AsOf {
                        direction: asof_condition.direction,
                    },
                    left,
                    right,
                    condition: asof_condition,
                });
            }
        }

        Ok(plan)
    }

    fn extract_asof_pattern(&self, condition: &Expr) -&gt; Option&lt;AsOfCondition&gt; {
        // Parse condition like:
        // symbol = symbol AND trade_time &gt;= price_time

        let predicates = split_conjunction(condition);
        let mut equality_keys = Vec::new();
        let mut inequality_pred = None;

        for pred in predicates {
            match pred {
                Expr::BinaryOp { op: Op::Eq, left, right } =&gt; {
                    equality_keys.push((left, right));
                }
                Expr::BinaryOp { op: Op::GtEq, left, right }
                | Expr::BinaryOp { op: Op::Gt, left, right } =&gt; {
                    inequality_pred = Some((left, right, AsOfDirection::Forward));
                }
                Expr::BinaryOp { op: Op::LtEq, left, right }
                | Expr::BinaryOp { op: Op::Lt, left, right } =&gt; {
                    inequality_pred = Some((left, right, AsOfDirection::Backward));
                }
                _ =&gt; return None,
            }
        }

        inequality_pred.map(|(left, right, direction)| AsOfCondition {
            equality_keys,
            ordering_left: left,
            ordering_right: right,
            direction,
        })
    }
}
```

**Rule 2: Sort Injection**

```rust
pub struct InjectAsOfSortRule;

impl OptimizationRule for InjectAsOfSortRule {
    fn apply(&self, plan: RelExpr) -&gt; Result&lt;RelExpr&gt; {
        if let RelExpr::AsOfJoin { left, right, condition, .. } = plan {
            // Check if inputs are already sorted
            let left_sorted = self.is_sorted(&left, &condition.equality_keys, &condition.ordering_left);
            let right_sorted = self.is_sorted(&right, &condition.equality_keys, &condition.ordering_right);

            let sorted_left = if !left_sorted {
                self.inject_sort(left, &condition.equality_keys, &condition.ordering_left)
            } else {
                left
            };

            let sorted_right = if !right_sorted {
                self.inject_sort(right, &condition.equality_keys, &condition.ordering_right)
            } else {
                right
            };

            Ok(RelExpr::AsOfJoin {
                left: sorted_left,
                right: sorted_right,
                condition,
                ..plan
            })
        } else {
            Ok(plan)
        }
    }
}
```

**Rule 3: Index Selection**

```rust
pub struct AsOfIndexSelectionRule;

impl OptimizationRule for AsOfIndexSelectionRule {
    fn apply(&self, plan: RelExpr) -&gt; Result&lt;RelExpr&gt; {
        if let RelExpr::AsOfJoin { right, condition, .. } = &plan {
            // Check for index on (equality_keys, ordering_column)
            let required_columns = condition.equality_keys.iter()
                .map(|(_, right_col)| right_col)
                .chain(std::iter::once(&condition.ordering_right))
                .collect::&lt;Vec&lt;_&gt;&gt;();

            if let Some(index) = self.find_matching_index(&right, &required_columns) {
                // Use index-based ASOF join
                return Ok(RelExpr::IndexAsOfJoin {
                    left: plan.left,
                    right_table: extract_table(right),
                    right_index: index,
                    condition: condition.clone(),
                });
            }
        }

        Ok(plan)
    }
}
```

**Rule 4: Partition-Wise ASOF Join**

```rust
pub struct PartitionWiseAsOfRule;

impl OptimizationRule for PartitionWiseAsOfRule {
    fn apply(&self, plan: RelExpr) -&gt; Result&lt;RelExpr&gt; {
        if let RelExpr::AsOfJoin { left, right, condition, .. } = plan {
            // If both sides are partitioned by equality keys,
            // perform partition-wise ASOF join

            if self.is_partitioned(&left, &condition.equality_keys) &&
               self.is_partitioned(&right, &condition.equality_keys) {
                // Generate parallel ASOF joins per partition
                return Ok(self.generate_partition_wise_asof(
                    left,
                    right,
                    condition,
                ));
            }
        }

        Ok(plan)
    }
}
```

**Rule 5: Predicate Pushdown**

```rust
pub struct AsOfPredicatePushdownRule;

impl OptimizationRule for AsOfPredicatePushdownRule {
    fn apply(&self, plan: RelExpr) -&gt; Result&lt;RelExpr&gt; {
        // Pattern: Filter(AsOfJoin(...))
        // Push predicates that reference only left or only right side

        if let RelExpr::Filter { predicate, input } = plan {
            if let RelExpr::AsOfJoin { left, right, condition, .. } = &**input {
                let (left_preds, right_preds, join_preds) =
                    self.partition_predicates(&predicate, left, right);

                let filtered_left = if !left_preds.is_empty() {
                    Box::new(RelExpr::Filter {
                        predicate: conjunction(left_preds),
                        input: left.clone(),
                    })
                } else {
                    left.clone()
                };

                let filtered_right = if !right_preds.is_empty() {
                    Box::new(RelExpr::Filter {
                        predicate: conjunction(right_preds),
                        input: right.clone(),
                    })
                } else {
                    right.clone()
                };

                let asof = RelExpr::AsOfJoin {
                    left: filtered_left,
                    right: filtered_right,
                    condition: condition.clone(),
                };

                if !join_preds.is_empty() {
                    return Ok(RelExpr::Filter {
                        predicate: conjunction(join_preds),
                        input: Box::new(asof),
                    });
                }

                return Ok(asof);
            }
        }

        Ok(plan)
    }
}
```

### Property Tracking

Track sort order as a physical property:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct SortOrder {
    pub columns: Vec&lt;Expr&gt;,
    pub ascending: Vec&lt;bool&gt;,
}

impl PhysicalProperty for SortOrder {
    fn satisfied_by(&self, operator: &PhysicalOperator) -&gt; bool {
        match operator {
            PhysicalOperator::Scan { ordering, .. } =&gt; {
                ordering.as_ref() == Some(self)
            }
            PhysicalOperator::Sort { order, .. } =&gt; order == self,
            PhysicalOperator::IndexScan { index, .. } =&gt; {
                self.matches_index_order(index)
            }
            _ =&gt; false,
        }
    }
}

pub trait PhysicalOperator {
    fn output_properties(&self) -&gt; PhysicalProperties {
        // Default: no properties guaranteed
        PhysicalProperties::default()
    }
}

impl SortMergeAsOfJoin {
    fn required_properties(&self) -&gt; Vec&lt;PhysicalProperties&gt; {
        vec![
            PhysicalProperties {
                sort_order: Some(SortOrder {
                    columns: self.condition.equality_keys.iter()
                        .map(|(left, _)| left.clone())
                        .chain(std::iter::once(self.condition.ordering_left.clone()))
                        .collect(),
                    ascending: vec![true; self.condition.equality_keys.len() + 1],
                }),
            },
            PhysicalProperties {
                sort_order: Some(SortOrder {
                    columns: self.condition.equality_keys.iter()
                        .map(|(_, right)| right.clone())
                        .chain(std::iter::once(self.condition.ordering_right.clone()))
                        .collect(),
                    ascending: vec![true; self.condition.equality_keys.len() + 1],
                }),
            },
        ]
    }
}
```

### Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum AsOfJoinError {
    #[error(
        "ASOF join requires exactly one inequality condition on an ordering column, \
         found {found} inequality predicates"
    )]
    InvalidInequalityCount { found: usize },

    #[error(
        "ASOF join ordering column must be comparable, found type {found}"
    )]
    NonComparableOrderingType { found: String },

    #[error(
        "ASOF join with mixed inequality directions: {left_op} and {right_op}"
    )]
    MixedInequalityDirections {
        left_op: String,
        right_op: String,
    },

    #[error(
        "ASOF join input not sorted by required columns: {required:?}"
    )]
    UnsortedInput { required: Vec&lt;String&gt; },
}
```

## Performance Analysis

### Theoretical Complexity

| Algorithm | Time Complexity | Space Complexity | Best Use Case |
|-----------|----------------|------------------|---------------|
| Self-Join (Current) | O(n * m) | O(m) | Never (always worse) |
| Sort-Merge ASOF | O(n log n + m log m) unsorted&lt;br&gt;O(n + m) sorted | O(1) | Pre-sorted data, sequential scans |
| Index-Based ASOF | O(n * log m) | O(m) | Small left, large indexed right |
| Hash-Based ASOF | O(n + m) | O(m) | Approximate matches acceptable |

### Expected Speedups

Based on DuckDB benchmarks and analytical modeling:

**Scenario 1: Financial Data (trades + prices)**
- Left: 1M trades
- Right: 100K prices
- Pre-sorted by timestamp
- Current: Nested loop = 1M * 100K * 10μs = 16,667 seconds
- ASOF: Sort-merge = (1M + 100K) * 1μs = 1.1 seconds
- **Speedup: 15,000x**

**Scenario 2: Sensor Data (readings + calibrations)**
- Left: 10M sensor readings
- Right: 1M calibration events
- Unsorted
- Current: Nested loop with index = 10M * log(1M) * 100μs = 2,000 seconds
- ASOF: Sort + merge = 10M * log(10M) * 2μs + 11M * 1μs = 234 seconds
- **Speedup: 8.5x**

**Scenario 3: Log Correlation (app events + system metrics)**
- Left: 5M application events
- Right: 2M system metrics
- Partially sorted (recent data sorted)
- Current: Hash join with inequality filter = 5M * 2M * 1μs = 10,000 seconds
- ASOF: Index-based = 5M * log(2M) * 10μs = 1,050 seconds
- **Speedup: 9.5x**

### Real-World Performance Targets

Based on DuckDB ASOF join benchmarks:

- **Small datasets (&lt; 100K rows):** 2-5x speedup
- **Medium datasets (100K - 10M rows):** 10-50x speedup
- **Large datasets (&gt; 10M rows, sorted):** 50-100x speedup
- **Large datasets (&gt; 10M rows, unsorted):** 5-20x speedup

### Optimization Opportunities

1. **Exploit Existing Sort Orders**
   - Table scanned with ORDER BY → already sorted
   - Index scan → inherits index order
   - Partition-local sorting → cheaper than global sort

2. **Index Usage for Range Lookups**
   - B-tree index on (partition_key, timestamp) → O(log m) lookup
   - Skip-scan optimization when few partitions

3. **Partition-Wise ASOF Joins**
   - Partitioned by equality keys → parallel ASOF per partition
   - Reduces memory footprint and enables parallelism

4. **Pushdown Predicates Before ASOF Matching**
   - Filter left side: reduces ASOF probes
   - Filter right side: reduces build table size

5. **Late Materialization**
   - Only materialize matched columns after ASOF matching
   - Reduces data movement during join phase

## Implementation Plan

### Phase 1: Core ASOF Join Operator (Weeks 1-4)

**Week 1-2: AST and Logical Plan**
- [ ] Add `JoinType::AsOf` variant to `algebra.rs`
- [ ] Implement `AsOfDirection` enum
- [ ] Add `AsOfCondition` struct
- [ ] Update logical plan serialization/deserialization
- [ ] Add unit tests for AST representation

**Week 3-4: Physical Operator Implementation**
- [ ] Implement `SortMergeAsOfJoin` physical operator
- [ ] Add `find_nearest_match()` algorithm
- [ ] Implement equality partition matching
- [ ] Add execution tests with synthetic data
- [ ] Validate correctness with property-based tests

### Phase 2: Optimization Rules (Weeks 5-8)

**Week 5-6: Detection and Rewriting**
- [ ] Implement `DetectAsOfJoinRule`
- [ ] Pattern matching for self-join + inequality
- [ ] Pattern matching for DISTINCT ON + ORDER BY
- [ ] Extract equality keys and ordering columns
- [ ] Add rule tests with TPC-H-style queries

**Week 7-8: Sort and Property Management**
- [ ] Implement `SortOrder` physical property
- [ ] Add property tracking to logical plan
- [ ] Implement `InjectAsOfSortRule`
- [ ] Exploit existing sort orders from index scans
- [ ] Test with pre-sorted and unsorted inputs

### Phase 3: Cost Model Integration (Weeks 9-11)

**Week 9-10: Cost Estimation**
- [ ] Add `estimate_asof_join_cost()` to cost model
- [ ] Model sort costs for unsorted inputs
- [ ] Model merge costs for sorted inputs
- [ ] Compare ASOF vs. self-join costs
- [ ] Validate estimates against actual execution

**Week 11: Cardinality Estimation**
- [ ] Estimate ASOF join output cardinality
- [ ] Model match selectivity based on time ranges
- [ ] Handle NULL propagation in LEFT ASOF joins
- [ ] Add histogram-based selectivity estimation

### Phase 4: Index-Based ASOF (Weeks 12-14)

**Week 12-13: Index Selection**
- [ ] Implement `IndexAsOfJoin` physical operator
- [ ] Add B-tree range scan for nearest match
- [ ] Implement `AsOfIndexSelectionRule`
- [ ] Cost comparison: index vs. sort-merge
- [ ] Test with compound indexes

**Week 14: Index Optimization**
- [ ] Support skip-scan for sparse partitions
- [ ] Batch index lookups for better cache utilization
- [ ] Test performance with various index types

### Phase 5: Advanced Optimizations (Weeks 15-17)

**Week 15: Predicate Pushdown**
- [ ] Implement `AsOfPredicatePushdownRule`
- [ ] Push filters below ASOF join
- [ ] Handle predicates on ordering column
- [ ] Test with complex filter combinations

**Week 16: Partition-Wise ASOF**
- [ ] Implement `PartitionWiseAsOfRule`
- [ ] Detect co-partitioned inputs
- [ ] Generate parallel ASOF per partition
- [ ] Validate with partitioned tables

**Week 17: Additional Optimizations**
- [ ] Late materialization of join results
- [ ] Column pruning through ASOF join
- [ ] Incremental ASOF for streaming data

### Phase 6: Testing and Validation (Weeks 18-20)

**Week 18: Functional Testing**
- [ ] Correctness tests with known results
- [ ] Edge cases: empty inputs, no matches, duplicates
- [ ] NULL handling in equality keys and ordering
- [ ] Property-based testing with proptest

**Week 19: Performance Benchmarking**
- [ ] Benchmark suite: financial, sensor, log data
- [ ] Compare against self-join baseline
- [ ] Compare against DuckDB ASOF join
- [ ] Measure speedups across data sizes

**Week 20: Integration and Documentation**
- [ ] Integration tests with full query optimizer
- [ ] User documentation and examples
- [ ] Developer documentation for optimizer rules
- [ ] Performance tuning guide

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asof_join_forward_matching() {
        let left = vec![
            row("AAPL", "2024-01-01 10:00:00", 100),
            row("AAPL", "2024-01-01 10:05:00", 200),
        ];

        let right = vec![
            row("AAPL", "2024-01-01 09:55:00", 150.0),
            row("AAPL", "2024-01-01 10:02:00", 151.0),
        ];

        let result = asof_join_forward(left, right, "symbol", "time");

        assert_eq!(result, vec![
            joined_row("AAPL", "2024-01-01 10:00:00", 100, 150.0),
            joined_row("AAPL", "2024-01-01 10:05:00", 200, 151.0),
        ]);
    }

    #[test]
    fn test_asof_join_no_match() {
        let left = vec![row("AAPL", "2024-01-01 09:00:00", 100)];
        let right = vec![row("AAPL", "2024-01-01 10:00:00", 150.0)];

        // Left time before any right time → no match
        let result = left_asof_join_forward(left, right, "symbol", "time");

        assert_eq!(result, vec![
            joined_row("AAPL", "2024-01-01 09:00:00", 100, null),
        ]);
    }

    #[test]
    fn test_asof_join_multiple_partitions() {
        let left = vec![
            row("AAPL", "2024-01-01 10:00:00", 100),
            row("GOOGL", "2024-01-01 10:00:00", 50),
        ];

        let right = vec![
            row("AAPL", "2024-01-01 09:55:00", 150.0),
            row("GOOGL", "2024-01-01 09:58:00", 2800.0),
        ];

        let result = asof_join_forward(left, right, "symbol", "time");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].get("price"), 150.0);
        assert_eq!(result[1].get("price"), 2800.0);
    }
}
```

### Property-Based Tests

```rust
#[cfg(test)]
mod proptests {
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn asof_join_preserves_left_cardinality(
            left in vec(trade_row(), 0..1000),
            right in vec(price_row(), 0..1000),
        ) {
            let result = left_asof_join(&left, &right);
            assert_eq!(result.len(), left.len());
        }

        #[test]
        fn asof_join_matches_are_valid(
            left in vec(trade_row(), 0..100),
            right in vec(price_row(), 0..100),
        ) {
            let result = asof_join_forward(&left, &right);

            for row in result {
                if let Some(matched_time) = row.right_time {
                    // Forward match: matched_time &lt;= left_time
                    assert!(matched_time &lt;= row.left_time);

                    // Nearest match: no closer match exists
                    for candidate in &right {
                        if candidate.symbol == row.symbol &&
                           candidate.time &lt;= row.left_time &&
                           candidate.time &gt; matched_time {
                            panic!("Found closer match");
                        }
                    }
                }
            }
        }
    }
}
```

### Integration Tests

```rust
#[test]
fn test_asof_join_with_tpch_lineitem() {
    // Simulate: match orders to prior price updates
    let query = "
        SELECT o.o_orderkey, o.o_orderdate, p.ps_supplycost
        FROM orders o
        ASOF JOIN partsupp p
          ON o.o_custkey = p.ps_partkey
          AND o.o_orderdate &gt;= p.ps_availqty_updated
        WHERE o.o_orderdate BETWEEN '1995-01-01' AND '1995-12-31'
    ";

    let result = execute_query(query);

    // Validate: every order has at most one matching price
    assert!(result.len() &lt;= orders_count);

    // Validate: matched prices are before order dates
    for row in result {
        assert!(row.ps_availqty_updated &lt;= row.o_orderdate);
    }
}
```

### Performance Benchmarks

```rust
#[bench]
fn bench_asof_join_vs_self_join(b: &mut Bencher) {
    let trades = generate_trades(1_000_000);
    let prices = generate_prices(100_000);

    // Baseline: self-join with correlated subquery
    b.iter(|| {
        execute_query("
            SELECT t.*,
                   (SELECT p.price
                    FROM prices p
                    WHERE p.symbol = t.symbol
                      AND p.time &lt;= t.time
                    ORDER BY p.time DESC
                    LIMIT 1) AS price
            FROM trades t
        ")
    });

    // ASOF join
    b.iter(|| {
        execute_query("
            SELECT t.*, p.price
            FROM trades t
            ASOF JOIN prices p
              ON t.symbol = p.symbol
              AND t.time &gt;= p.time
        ")
    });
}
```

## Drawbacks

**1. Increased optimizer complexity**
- New join type requires additional rule implementations
- Sort property tracking adds complexity to physical planning
- More optimization decisions increase search space

**2. Semantic restrictions**
- ASOF join requires exactly one inequality predicate
- Inputs must be sortable by ordering column
- NULL handling in ordering column is undefined (must be explicit)

**3. Potential performance regression**
- Incorrectly detected ASOF patterns could choose sort-merge over hash join
- Sort injection for unsorted inputs may be expensive
- Need fallback to self-join when ASOF assumptions violated

**4. Limited SQL standard support**
- ASOF join is not in SQL standard (DuckDB extension)
- Portability concerns for users migrating queries
- May require query rewrites when switching databases

## Rationale and Alternatives

### Why ASOF Join vs. Self-Join Emulation

**Self-join approach:**
```sql
SELECT t.*, p.price
FROM trades t
JOIN LATERAL (
    SELECT price
    FROM prices p
    WHERE p.symbol = t.symbol AND p.time &lt;= t.time
    ORDER BY p.time DESC
    LIMIT 1
) p ON true
```

**Drawbacks:**
- O(n * m) complexity: Full scan for each left row
- Poor cardinality estimates: Optimizer cannot model LIMIT 1 pattern
- No index exploitation: Correlation fence prevents index pushdown
- Materialization overhead: Subquery results materialized per row

**ASOF join advantages:**
- O(n + m) complexity with sorted inputs
- Accurate cardinality estimates: At most n output rows
- Index-aware: Can use B-tree for range scans
- Streaming execution: No materialization required

### Why Not Window Functions

**Window function approach:**
```sql
SELECT DISTINCT ON (trade_id)
    t.*,
    FIRST_VALUE(p.price) OVER (
        PARTITION BY t.trade_id
        ORDER BY p.time DESC
    ) AS price
FROM trades t
JOIN prices p ON t.symbol = p.symbol AND p.time &lt;= t.time
```

**Drawbacks:**
- Still requires full cross-product join before windowing
- Window sorting overhead even with pre-sorted data
- Cannot exploit sort merge or index scans efficiently

### Alternative: Specialized Time-Series Operators

Instead of generalizing to ASOF join, implement time-series specific operators like:
- `LAST_VALUE_BEFORE(timestamp)`
- `INTERPOLATE(timestamp, value)`

**Rejected because:**
- ASOF join is more general and composable
- Specialized operators proliferate the function catalog
- ASOF join aligns with DuckDB semantics (compatibility)

## Prior Art

### DuckDB ASOF Joins
- Native ASOF join syntax and execution
- Sort-merge algorithm with partition-wise execution
- Reported speedups: 50-100x on time-series queries
- Implementation: https://github.com/duckdb/duckdb/pull/7182

### Timescale DB
- Time-bucket joins for aggregated data
- Continuous aggregates with join support
- No native ASOF join syntax (uses self-joins)

### InfluxDB IOx
- Time-series joins with inequality predicates
- Vectorized execution on Arrow data
- Optimized for sorted timestamp columns

### Apache Flink
- Interval joins for stream processing
- Forward/backward matching semantics
- Event-time based correlation

### Databricks Delta
- AS OF clauses for time-travel (different semantics)
- Merge joins for sorted data
- No inequality join optimization

## Unresolved Questions

1. **Nearest match extension:** Should Ra support bidirectional nearest matching (find closest in either direction)? This requires additional complexity but enables interpolation use cases.

2. **Multi-column ordering:** Should ASOF joins support multiple ordering columns (e.g., sort by location then time)? How to define "nearest" with multiple dimensions?

3. **Tolerance bounds:** Should Ra support ASOF joins with tolerance windows (e.g., match within ±5 minutes)? This requires range-based matching logic.

4. **Null ordering:** How should NULL values in the ordering column be handled? Exclude from matching, treat as minimum/maximum, or error?

5. **Streaming ASOF joins:** Can ASOF joins be implemented for streaming queries where data arrives out-of-order? Requires buffering and late-arriving data handling.

6. **Approximate ASOF:** When is hash-based approximate ASOF acceptable? Should Ra detect cases where exact matching is not required?

## Future Possibilities

### 1. Temporal Interpolation

Extend ASOF joins to support interpolation between boundary points:

```sql
SELECT t.*,
       INTERPOLATE(p.price, t.time) AS interpolated_price
FROM trades t
ASOF JOIN prices p
  ON t.symbol = p.symbol
  NEAREST t.time TO p.time
WITH INTERPOLATION LINEAR;
```

### 2. Multi-Table ASOF Joins

Support ASOF joins across more than two tables:

```sql
SELECT *
FROM trades t
ASOF JOIN prices p ON t.symbol = p.symbol AND t.time &gt;= p.time
ASOF JOIN volumes v ON t.symbol = v.symbol AND t.time &gt;= v.time;
```

### 3. Incremental ASOF Maintenance

For materialized views with ASOF joins, maintain incrementally as new data arrives:

```sql
CREATE MATERIALIZED VIEW enriched_trades AS
SELECT t.*, p.price
FROM trades t
ASOF JOIN prices p ON t.symbol = p.symbol AND t.time &gt;= p.time;
```

### 4. ASOF Aggregation

Combine ASOF matching with aggregation:

```sql
SELECT t.symbol,
       AVG(p.price) AS avg_matched_price
FROM trades t
ASOF JOIN prices p ON t.symbol = p.symbol AND t.time &gt;= p.time
GROUP BY t.symbol;
```

### 5. Distributed ASOF Joins

Extend to distributed execution:
- Partition both tables by equality keys
- Perform local ASOF joins per partition
- Merge results without global coordination

### 6. GPU-Accelerated ASOF

Implement ASOF join on GPU for massive parallelism:
- Vectorized merge using CUDA
- Warp-level primitives for nearest match
- Expected 10-50x additional speedup on large datasets

## Referenced By

This RFC is referenced by:
- [RFC 0065](/maintainers/rfcs/0065-time-series-query-optimization): Time-Series Query Optimization (ASOF joins complement chunk-aware optimization)

## References

- [DuckDB ASOF Join Documentation](https://duckdb.org/docs/sql/query_syntax/from.html#as-of-joins)
- [DuckDB ASOF Join Implementation](https://github.com/duckdb/duckdb/pull/7182)
- [Time-Series Joins in Databases (Research Paper)](https://cs.brown.edu/~ugur/tsjoins.pdf)
- DUCKDB_FEATURES_ANALYSIS.md (Section 1: ASOF Joins)


## Referenced By

This RFC is referenced by:

- [RFC 95: ASOF (As-Of) Join Optimization](/maintainers/rfcs/0095-asof-join-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 95: ASOF (As-Of) Join Optimization](/maintainers/rfcs/0095-asof-join-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 95: ASOF (As-Of) Join Optimization](/maintainers/rfcs/0095-asof-join-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 95: ASOF (As-Of) Join Optimization](/maintainers/rfcs/0095-asof-join-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 95: ASOF (As-Of) Join Optimization](/maintainers/rfcs/0095-asof-join-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 95: ASOF (As-Of) Join Optimization](/maintainers/rfcs/0095-asof-join-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 95: ASOF (As-Of) Join Optimization](/maintainers/rfcs/0095-asof-join-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 95: ASOF (As-Of) Join Optimization](/maintainers/rfcs/0095-asof-join-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 95: ASOF (As-Of) Join Optimization](/maintainers/rfcs/0095-asof-join-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 95: ASOF (As-Of) Join Optimization](/maintainers/rfcs/0095-asof-join-optimization)
