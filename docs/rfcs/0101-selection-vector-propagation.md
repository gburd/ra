# RFC 0101: Selection Vector Propagation

- **Status**: Proposed
- **Priority**: High Impact (6-8 weeks)
- **Impact**: 2-5x memory bandwidth reduction for selective queries
- **Category**: Execution / Vectorization
- **Created**: 2026-03-28

## Summary

Maintain a bitmap or index array of valid row positions after filter operations instead of physically compacting data. Propagate selection vectors through operator pipelines to eliminate intermediate materializations, reducing memory bandwidth by 2-5x for selective queries with minimal implementation complexity.

## Motivation

**Problem**: Vectorized execution engines waste memory bandwidth on selective operations.

Consider a typical analytical query:
```sql
SELECT sum(price * quantity)
FROM orders
WHERE status = 'completed'
  AND region = 'US'
  AND amount &gt; 1000;
```

**Current approach (without selection vectors)**:
1. Filter 1 (status): Copy 30% of data → 300MB intermediate
2. Filter 2 (region): Copy 20% of remaining → 60MB intermediate
3. Filter 3 (amount): Copy 10% of remaining → 6MB intermediate
4. Project: Process 6MB final data

**Total memory bandwidth**: 300 + 60 + 6 = 366MB copied

**With selection vectors**:
1. Filter 1: Mark 30% as valid → 4KB bitmap
2. Filter 2: Mark 20% of 30% = 6% as valid → 4KB bitmap
3. Filter 3: Mark 10% of 6% = 0.6% as valid → 4KB bitmap
4. Compact once: Copy 6MB using final selection vector

**Total memory bandwidth**: 6MB + overhead = **~60x less intermediate copying**

### Real-World Evidence

**MonetDB X100** (Boncz et al. 2005):
- Selection vectors core to vectorized execution model
- 2-6x CPU throughput over Volcano iterator model
- Avoid compaction between pipeline stages

**DuckDB** (Raasveldt & Mühleisen 2019):
- Built-in selection vector support in execution engine
- 2-5x memory bandwidth reduction for selectivity &lt; 30%
- Standard practice for vectorized query processing

**ClickHouse**:
- SIMD filtering produces selection vectors
- Propagates through aggregation pipelines
- Critical for handling sparse data

### Key Metrics

| Selectivity | Traditional | Selection Vector | Speedup |
|-------------|-------------|------------------|---------|
| 5%          | 100% data copied | 5% data + vector overhead | **5x** |
| 10%         | 100% data copied | 10% data + vector overhead | **4x** |
| 30%         | 100% data copied | 30% data + vector overhead | **2x** |
| 80%         | 100% data copied | 80% data + vector overhead | **1.1x** |

**Break-even point**: 60-70% selectivity (tracking overhead equals compaction cost)

## Guide-level explanation

### What is a Selection Vector?

A selection vector tracks which rows in a data vector are valid after a filter operation:

**Representation 1: Index array (u32[])**
```
Original data: [10, 20, 30, 40, 50, 60, 70, 80]
After filter:  [10,     30,     50,     70    ]

Selection vector: [0, 2, 4, 6]  (indices of valid rows)
```

**Representation 2: Bitmap (bitvector)**
```
Original data: [10, 20, 30, 40, 50, 60, 70, 80]
Selection vector: [1, 0, 1, 0, 1, 0, 1, 0]  (1 = valid, 0 = invalid)
```

### When to Use Selection Vectors

**GOOD: Selective filters (&lt; 30% pass rate)**
```sql
WHERE rare_event = true  -- Selectivity: 2%
```
**Cost**: Tracking 98% filtered rows via bitmap &lt;&lt; copying 2% passing rows

**GOOD: Filter chains**
```sql
WHERE status = 'active'   -- Selectivity: 40%
  AND premium = true      -- Selectivity: 20% of 40% = 8%
  AND last_login &gt; '2024-01-01'  -- Selectivity: 5% of 8% = 0.4%
```
**Cost**: Three bitmap operations &lt;&lt; three data copies

**BAD: High selectivity (&gt; 80%)**
```sql
WHERE created_at IS NOT NULL  -- Selectivity: 99.9%
```
**Cost**: Tracking 0.1% invalid rows &gt; copying 99.9% valid rows

**BAD: Immediately followed by hash join**
```sql
SELECT * FROM t1
WHERE t1.rare_flag = true  -- Selectivity: 1%
JOIN t2 ON t1.id = t2.id   -- Hash join needs dense data
```
**Cost**: Materialize before hash join anyway (random access pattern)

### Operator Support

**Producers (create selection vectors)**:
- `Filter`: Primary producer, evaluates predicates and marks valid rows
- `Scan`: Can produce selection vector from zone map pruning

**Consumers (process with selection vectors)**:
- `Project`: Apply expressions only to valid rows
- `Aggregate`: Skip invalid rows in hash table build
- `Filter` (chained): Intersect with existing selection vector

**Materializers (compact data)**:
- `HashJoin`: Probe side needs dense data for hash table lookup
- `Sort`: Sorting algorithms require contiguous data
- `Window`: Frame computations need dense data
- `Network`: Shipping data to remote nodes

## Reference-level explanation

### Data Structures

```rust
/// Selection vector tracking valid rows in a data batch
#[derive(Clone)]
pub enum SelectionVector {
    /// All rows valid (identity selection)
    All { count: usize },

    /// Sparse indices (&lt; 50% selectivity)
    Indices {
        indices: Vec&lt;u32&gt;,  // Valid row positions
        capacity: usize,    // Original batch size
    },

    /// Dense bitmap (50-90% selectivity)
    Bitmap {
        bits: BitVec,       // 1 bit per row
    },
}

impl SelectionVector {
    /// Create selection vector from predicate evaluation
    pub fn from_filter(
        predicate: &BooleanArray,
        capacity: usize,
    ) -&gt; Self {
        let valid_count = predicate.true_count();
        let selectivity = valid_count as f64 / capacity as f64;

        if selectivity &gt;= 0.99 {
            // Nearly all valid, use identity
            SelectionVector::All { count: capacity }
        } else if selectivity &lt; 0.5 {
            // Sparse, use index array
            let indices: Vec&lt;u32&gt; = predicate
                .iter()
                .enumerate()
                .filter_map(|(i, v)| v.then_some(i as u32))
                .collect();
            SelectionVector::Indices { indices, capacity }
        } else {
            // Dense, use bitmap
            let bits = BitVec::from_iter(predicate.iter().map(|v| v.unwrap_or(false)));
            SelectionVector::Bitmap { bits }
        }
    }

    /// Intersect with another selection vector (AND operation)
    pub fn intersect(&self, other: &SelectionVector) -&gt; Self {
        match (self, other) {
            (SelectionVector::All { count }, s) =&gt; s.clone(),
            (s, SelectionVector::All { count }) =&gt; s.clone(),

            (SelectionVector::Bitmap { bits: a }, SelectionVector::Bitmap { bits: b }) =&gt; {
                // Bitwise AND
                let bits = a & b;
                SelectionVector::Bitmap { bits }
            }

            (SelectionVector::Indices { indices, capacity }, other) =&gt; {
                // Filter indices through second selection
                let new_indices: Vec&lt;u32&gt; = indices
                    .iter()
                    .filter(|&&i| other.is_valid(i as usize))
                    .copied()
                    .collect();
                SelectionVector::Indices { indices: new_indices, capacity: *capacity }
            }

            _ =&gt; {
                // Convert both to common representation and retry
                self.to_bitmap().intersect(&other.to_bitmap())
            }
        }
    }

    /// Check if row at index is valid
    #[inline]
    pub fn is_valid(&self, index: usize) -&gt; bool {
        match self {
            SelectionVector::All { .. } =&gt; true,
            SelectionVector::Indices { indices, .. } =&gt; {
                indices.binary_search(&(index as u32)).is_ok()
            }
            SelectionVector::Bitmap { bits } =&gt; {
                bits.get(index).unwrap_or(false)
            }
        }
    }

    /// Number of valid rows
    pub fn len(&self) -&gt; usize {
        match self {
            SelectionVector::All { count } =&gt; *count,
            SelectionVector::Indices { indices, .. } =&gt; indices.len(),
            SelectionVector::Bitmap { bits } =&gt; bits.count_ones(),
        }
    }

    /// Selectivity (fraction of valid rows)
    pub fn selectivity(&self) -&gt; f64 {
        match self {
            SelectionVector::All { .. } =&gt; 1.0,
            SelectionVector::Indices { indices, capacity } =&gt; {
                indices.len() as f64 / *capacity as f64
            }
            SelectionVector::Bitmap { bits } =&gt; {
                bits.count_ones() as f64 / bits.len() as f64
            }
        }
    }
}
```

### Operator Metadata

Extend `RelExpr` with selection vector tracking:

```rust
pub struct PhysicalProperties {
    // ... existing fields ...

    /// Selection vector state for this operator's output
    pub selection_vector: SelectionVectorState,
}

pub enum SelectionVectorState {
    /// No selection vector (all rows valid, dense data)
    None,

    /// Selection vector present
    Present {
        /// Estimated selectivity
        selectivity: f64,

        /// Cost of materializing (compacting) data
        materialize_cost: f64,

        /// Cost of tracking selection vector through next operator
        tracking_cost: f64,
    },
}
```

### Cost Model

**Decision: Materialize or propagate?**

```rust
impl CostModel {
    fn should_materialize(
        &self,
        selection: &SelectionVectorState,
        next_operator: &PhysicalOperator,
    ) -&gt; bool {
        match selection {
            SelectionVectorState::None =&gt; false,
            SelectionVectorState::Present { selectivity, materialize_cost, tracking_cost } =&gt; {
                // Always materialize for operators needing dense data
                if requires_dense_data(next_operator) {
                    return true;
                }

                // High selectivity: compaction is cheap
                if *selectivity &gt; 0.7 {
                    return true;
                }

                // Compare costs
                let propagate_cost = tracking_cost * next_operator.row_count as f64;
                let compact_cost = materialize_cost;

                compact_cost &lt; propagate_cost
            }
        }
    }
}

fn requires_dense_data(op: &PhysicalOperator) -&gt; bool {
    matches!(
        op,
        PhysicalOperator::HashJoin { .. }
            | PhysicalOperator::Sort { .. }
            | PhysicalOperator::Window { .. }
            | PhysicalOperator::Exchange { .. }  // Network shuffle
    )
}

fn estimate_materialize_cost(
    row_count: usize,
    selectivity: f64,
    row_width: usize,
) -&gt; f64 {
    // Cost = read sparse + write dense
    let read_cost = row_count as f64 * MEMORY_READ_COST;
    let write_cost = (row_count as f64 * selectivity) * row_width as f64 * MEMORY_WRITE_COST;
    read_cost + write_cost
}

fn estimate_tracking_cost(
    selectivity: f64,
    operation_cost: f64,
) -&gt; f64 {
    // Overhead of checking selection vector per row
    let check_cost = SELECTION_CHECK_COST;

    // If very sparse (&lt; 10%), benefit from skipping work
    if selectivity &lt; 0.1 {
        operation_cost * selectivity  // Only process valid rows
    } else {
        operation_cost + check_cost  // Process all + check overhead
    }
}
```

### Physical Operator Execution

**Filter operator (producer)**:
```rust
impl FilterExec {
    pub fn execute(&self, batch: RecordBatch) -&gt; Result&lt;RecordBatch&gt; {
        // Evaluate predicate
        let mask = self.predicate.evaluate(&batch)?;

        // Create selection vector
        let selection = SelectionVector::from_filter(&mask, batch.num_rows());

        // Should we materialize or propagate?
        if self.should_compact(&selection) {
            // Compact data immediately
            compact_batch(&batch, &selection)
        } else {
            // Attach selection vector, keep data unchanged
            batch.with_selection(selection)
        }
    }

    fn should_compact(&self, selection: &SelectionVector) -&gt; bool {
        // Materialize if selectivity &gt; 70% (compaction is cheap)
        selection.selectivity() &gt; 0.7
    }
}
```

**Project operator (consumer)**:
```rust
impl ProjectExec {
    pub fn execute(&self, batch: RecordBatch) -&gt; Result&lt;RecordBatch&gt; {
        match batch.selection_vector() {
            None =&gt; {
                // Dense data, normal projection
                self.project_dense(&batch)
            }
            Some(selection) =&gt; {
                // Evaluate expressions only on valid rows
                self.project_with_selection(&batch, selection)
            }
        }
    }

    fn project_with_selection(
        &self,
        batch: &RecordBatch,
        selection: &SelectionVector,
    ) -&gt; Result&lt;RecordBatch&gt; {
        let output_arrays: Vec&lt;ArrayRef&gt; = self.exprs
            .iter()
            .map(|expr| {
                // Evaluate expression using selection vector
                let arr = expr.evaluate(batch)?;

                // Result inherits selection vector
                Ok(arr)
            })
            .collect::&lt;Result&lt;_&gt;&gt;()?;

        RecordBatch::try_new(self.schema.clone(), output_arrays)
            .map(|b| b.with_selection(selection.clone()))
    }
}
```

**Materialize operator (consumer)**:
```rust
impl MaterializeExec {
    pub fn execute(&self, batch: RecordBatch) -&gt; Result&lt;RecordBatch&gt; {
        match batch.selection_vector() {
            None =&gt; Ok(batch),  // Already dense
            Some(selection) =&gt; compact_batch(&batch, selection),
        }
    }
}

fn compact_batch(
    batch: &RecordBatch,
    selection: &SelectionVector,
) -&gt; Result&lt;RecordBatch&gt; {
    let compacted_arrays: Vec&lt;ArrayRef&gt; = batch
        .columns()
        .iter()
        .map(|arr| compact_array(arr, selection))
        .collect::&lt;Result&lt;_&gt;&gt;()?;

    RecordBatch::try_new(batch.schema(), compacted_arrays)
}

fn compact_array(
    array: &ArrayRef,
    selection: &SelectionVector,
) -&gt; Result&lt;ArrayRef&gt; {
    match selection {
        SelectionVector::All { .. } =&gt; Ok(array.clone()),

        SelectionVector::Indices { indices, .. } =&gt; {
            // Use Arrow take kernel (optimized for sparse selection)
            let indices_array = UInt32Array::from(indices.clone());
            compute::take(array.as_ref(), &indices_array, None)
        }

        SelectionVector::Bitmap { bits } =&gt; {
            // Use Arrow filter kernel (optimized for dense bitmap)
            let mask = BooleanArray::from(bits.iter().collect::&lt;Vec&lt;_&gt;&gt;());
            compute::filter(array.as_ref(), &mask)
        }
    }
}
```

### Optimization Rules

**Rule 1: Selection Vector Creation**
```
Filter(predicate, input)
  WHERE selectivity(predicate) &lt; 0.7
  ↓
FilterWithSelection(predicate, input)
  produces SelectionVector
```

**Rule 2: Selection Vector Propagation**
```
Project(exprs, FilterWithSelection(pred, input))
  WHERE NOT requires_dense(Project)
  ↓
ProjectWithSelection(exprs, FilterWithSelection(pred, input))
  propagates SelectionVector
```

**Rule 3: Selection Vector Intersection**
```
Filter(pred2, FilterWithSelection(pred1, input))
  ↓
FilterWithSelection(pred1 AND pred2, input)
  SelectionVector = intersect(sv1, sv2)
```

**Rule 4: Forced Materialization**
```
HashJoin(FilterWithSelection(...), ...)
  ↓
HashJoin(Materialize(FilterWithSelection(...)), ...)
  explicit compaction before hash join
```

**Rule 5: Selection Vector Elimination**
```
FilterWithSelection(pred, input)
  WHERE selectivity(pred) &gt; 0.7
  ↓
Filter(pred, input)
  compact immediately, no selection vector
```

## Implementation Plan

### Phase 1: Core Infrastructure (Weeks 1-2)

**Deliverables**:
1. `SelectionVector` data structure with index array and bitmap variants
2. `RecordBatch` extension to carry selection vectors
3. Unit tests for selection vector operations (intersect, compact)

**Files**:
- `crates/ra-core/src/execution/selection_vector.rs`
- `crates/ra-core/src/execution/record_batch_ext.rs`

**Validation**: Selection vector operations correct, compact produces identical results

### Phase 2: Filter Operator (Weeks 3-4)

**Deliverables**:
1. `FilterExec` produces selection vectors for selectivity &lt; 70%
2. Cost model to decide materialize vs propagate
3. Integration tests: filter chains with selection vectors

**Files**:
- `crates/ra-engine/src/physical_plan/filter.rs`
- `crates/ra-engine/src/cost_model/selection_vector.rs`

**Validation**: Filter chains avoid intermediate copies, 2-3x memory bandwidth reduction

### Phase 3: Project/Aggregate Consumers (Weeks 5-6)

**Deliverables**:
1. `ProjectExec` evaluates expressions using selection vectors
2. `AggregateExec` builds hash table using selection vectors
3. Performance tests: end-to-end query speedup

**Files**:
- `crates/ra-engine/src/physical_plan/project.rs`
- `crates/ra-engine/src/physical_plan/aggregate.rs`

**Validation**: 2-5x speedup on selective queries (TPC-H Q6, Q19)

### Phase 4: Optimization Rules (Weeks 7-8)

**Deliverables**:
1. Rules for selection vector creation, propagation, elimination
2. Cost-based materialization decisions
3. Integration with existing optimization framework

**Files**:
- `rules/physical/execution/selection-vector-propagation.rra`
- `rules/physical/execution/forced-materialization.rra`

**Validation**: Optimizer chooses correct materialize/propagate strategy

## Performance Analysis

### Baseline vs Selection Vectors

**Query**: TPC-H Q6 (selective scan + aggregate)
```sql
SELECT sum(l_extendedprice * l_discount) AS revenue
FROM lineitem
WHERE l_shipdate &gt;= '1994-01-01'
  AND l_shipdate &lt; '1995-01-01'
  AND l_discount BETWEEN 0.05 AND 0.07
  AND l_quantity &lt; 24;
```

**Selectivity**: ~2% (3 filters each ~25% selective)

**Baseline (no selection vectors)**:
- Filter 1: Scan 6GB → copy 1.5GB (25% pass)
- Filter 2: Read 1.5GB → copy 375MB (25% of 25%)
- Filter 3: Read 375MB → copy 75MB (25% of 6.25%)
- Aggregate: Read 75MB

**Total bandwidth**: 6 + 1.5 + 375 + 75 + 75 = **7.95GB**

**With selection vectors**:
- Filter 1: Scan 6GB → bitmap 768KB
- Filter 2: Scan 6GB → intersect bitmap (768KB)
- Filter 3: Scan 6GB → intersect bitmap (768KB)
- Materialize: Copy 120MB (2% of 6GB)
- Aggregate: Read 120MB

**Total bandwidth**: 6 + 6 + 6 + 120 + 120 = **6.24GB** (only scan overhead)

**Effective reduction**: ~1.3x (scan dominates)

**Better example**: Multi-column projection after filter
```sql
SELECT col1, col2, col3, col4, col5
FROM t
WHERE rare_flag = true;  -- 5% selectivity
```

**Baseline**: Scan 1GB → copy 50MB (5%) → project 5 columns → 5x50MB = 250MB

**Selection vector**: Scan 1GB → bitmap 128KB → project with selection → compact once → 50MB

**Bandwidth reduction**: 250MB / 50MB = **5x**

### Expected Speedup by Workload

| Query Type | Selectivity | Speedup | Notes |
|------------|-------------|---------|-------|
| Selective scan + aggregate | 5% | **3-4x** | Avoid intermediate copies |
| Multi-filter chains | 10% | **2-3x** | Intersection instead of copy |
| Filter + projection | 15% | **2-3x** | Skip unused rows in expressions |
| Filter + hash join | 20% | **1.5-2x** | Materialize once before join |
| High selectivity (&gt; 70%) | 80% | **1.0x** | Compaction overhead = benefit |

## Cross-Database Applicability

### MonetDB (Core Feature)
- **X100 vectorization model**: Selection vectors are fundamental
- **OID-based execution**: Selection vector = list of OIDs
- **Ra integration**: Extend MonetDB rules to leverage selection vectors

### DuckDB (Built-in Support)
- **Selection vectors in execution engine**: Standard practice
- **Filter pushdown**: Parquet row group filtering produces selection vectors
- **Ra integration**: Optimize selection vector materialization points

### ClickHouse (SIMD Filtering)
- **SIMD filters**: AVX2/AVX-512 produces selection vectors efficiently
- **Sparse columns**: Selection vectors critical for sparse data
- **Ra integration**: Model SIMD selection vector generation costs

### PostgreSQL (Limited Support)
- **Partial support**: Bitmap heap scans use selection-like mechanism
- **Vectorization opportunity**: PGXN extensions could leverage selection vectors
- **Ra integration**: Recommend vectorized extensions when appropriate

### Spark/Presto (Vectorization Trend)
- **Arrow-based execution**: Selection vectors natural in columnar format
- **Filter pushdown**: ORC/Parquet predicate pushdown benefits from selection vectors
- **Ra integration**: Distributed materialization strategies

## Testing Strategy

### Unit Tests
- Selection vector creation from predicates
- Intersection, union, negation operations
- Compaction correctness (dense vs sparse)
- Selectivity estimation accuracy

### Integration Tests
- Filter chains with varying selectivities
- Project expressions with selection vectors
- Aggregate with selection vectors
- Materialize before hash joins

### Performance Tests
- TPC-H queries with selective filters (Q6, Q19)
- Synthetic workloads: 5%, 10%, 30%, 50%, 70%, 90% selectivity
- Memory bandwidth measurement (perf counters)
- Comparison: baseline vs selection vectors

### Correctness Validation
- Property test: `compact(batch, selection) ≡ filter(batch, predicate)`
- Fuzz test: Random selection vectors with all operators
- Edge cases: Empty selection, all valid, single valid row

## Drawbacks

**1. Complexity in operator implementations**
- Every operator must handle dense and sparse data paths
- Increases code maintenance burden
- **Mitigation**: Centralize selection vector logic in execution framework

**2. Overhead for high selectivity**
- Tracking overhead when > 70% of rows pass filter
- **Mitigation**: Cost model automatically compacts at high selectivity

**3. Cache locality degradation**
- Sparse data access patterns reduce cache hit rate
- **Mitigation**: Materialize before operators with sequential access patterns (sort, scan)

**4. Increased planning complexity**
- Cost model must reason about selection vector propagation
- **Mitigation**: Conservative defaults, calibrate costs from real workloads

## Rationale and alternatives

### Alternative 1: Always Compact (Baseline)
**Pros**: Simple, no tracking overhead
**Cons**: 2-5x memory bandwidth waste for selective queries

### Alternative 2: Lazy Evaluation (Apache Arrow)
**Pros**: Zero-copy filter chains
**Cons**: Expression evaluation overhead on every access, complex lifetime management

### Alternative 3: Late Materialization (MonetDB)
**Pros**: Minimal data movement until projection
**Cons**: Requires OID tracking, not applicable to non-columnar engines

**Why Selection Vectors?**
1. **Low complexity**: Minimal changes to operators (if-check per row)
2. **Broad applicability**: Works with any vectorized engine
3. **Measurable impact**: 2-5x bandwidth reduction for common workloads
4. **Incremental adoption**: Can deploy operator-by-operator

## Prior Art

**MonetDB X100** (Boncz et al. VLDB 2005):
- Introduced vectorized execution with selection vectors
- Proved 2-6x throughput improvement over Volcano model
- Industry standard for OLAP query processing

**DuckDB** (Raasveldt & Mühleisen SIGMOD 2019):
- Built-in selection vector support in all operators
- Automatic materialization decisions based on operator type
- Demonstrates practicality for production systems

**ClickHouse** (Yandex):
- SIMD-optimized filter produces selection vectors
- Critical for handling sparse clickstream data
- Shows importance for real-world analytics workloads

**Apache Arrow**:
- Optional selection vector support in compute kernels
- Demonstrates cross-language applicability
- Validates design for interoperability

## Unresolved Questions

1. **SIMD optimization**: Can we use AVX-512 for bitmap intersection?
2. **Distributed execution**: How to ship selection vectors across network?
3. **Parquet integration**: Should zone map pruning produce selection vectors?
4. **GPU execution**: Do selection vectors transfer to GPU kernels?

## Future Possibilities

**Phase 2 Extensions** (beyond initial implementation):

1. **SIMD-optimized selection vectors**
   - AVX-512 bitmap operations (8x faster intersection)
   - Hardware popcount for selectivity estimation
   - Autovectorization for filter evaluation

2. **Nested selection vectors**
   - Hierarchical bitmaps for multi-level filtering
   - Sparse index trees for very low selectivity (< 1%)
   - Efficient representation for deeply nested filters

3. **Selection vector caching**
   - Cache selection vectors for repeated subqueries
   - Zone map → selection vector precomputation
   - Index scan → selection vector materialization

4. **Adaptive materialization**
   - Runtime decision based on actual selectivity
   - Switch strategy mid-pipeline if estimate wrong
   - Feedback loop from execution to optimizer

5. **Cross-operator fusion**
   - Fuse filter + project + filter into single kernel
   - Generate specialized code for selection vector chains
   - JIT compile selection vector operations

## References

1. Boncz, P., Zukowski, M., & Nes, N. (2005). MonetDB/X100: Hyper-Pipelining Query Execution. *CIDR*.
2. Raasveldt, M., & Mühleisen, H. (2019). DuckDB: an Embeddable Analytical Database. *SIGMOD*.
3. Idreos, S., et al. (2011). Database Cracking. *CIDR*.
4. Leis, V., et al. (2014). Morsel-Driven Parallelism: A NUMA-Aware Query Evaluation Framework. *SIGMOD*.


## Referenced By

This RFC is referenced by:

- [RFC 101: Selection Vector Propagation](/maintainers/rfcs/0101-selection-vector-propagation)


## Referenced By

This RFC is referenced by:

- [RFC 101: Selection Vector Propagation](/maintainers/rfcs/0101-selection-vector-propagation)


## Referenced By

This RFC is referenced by:

- [RFC 101: Selection Vector Propagation](/maintainers/rfcs/0101-selection-vector-propagation)


## Referenced By

This RFC is referenced by:

- [RFC 101: Selection Vector Propagation](/maintainers/rfcs/0101-selection-vector-propagation)


## Referenced By

This RFC is referenced by:

- [RFC 101: Selection Vector Propagation](/maintainers/rfcs/0101-selection-vector-propagation)


## Referenced By

This RFC is referenced by:

- [RFC 101: Selection Vector Propagation](/maintainers/rfcs/0101-selection-vector-propagation)


## Referenced By

This RFC is referenced by:

- [RFC 101: Selection Vector Propagation](/maintainers/rfcs/0101-selection-vector-propagation)


## Referenced By

This RFC is referenced by:

- [RFC 101: Selection Vector Propagation](/maintainers/rfcs/0101-selection-vector-propagation)


## Referenced By

This RFC is referenced by:

- [RFC 101: Selection Vector Propagation](/maintainers/rfcs/0101-selection-vector-propagation)


## Referenced By

This RFC is referenced by:

- [RFC 101: Selection Vector Propagation](/maintainers/rfcs/0101-selection-vector-propagation)


## Referenced By

This RFC is referenced by:

- [RFC 101: Selection Vector Propagation](/maintainers/rfcs/0101-selection-vector-propagation)
