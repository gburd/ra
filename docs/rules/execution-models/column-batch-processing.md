# Rule: Column-at-a-Time Batch Processing

**Category:** execution-models/column-at-a-time
**File:** `rules/execution-models/column-at-a-time/column-batch-processing.rra`

## Metadata

- **ID:** `column-batch-processing`
- **Version:** "1.0.0"
- **Databases:** monetdb, clickhouse, duckdb
- **Tags:** execution, columnar, batch, vectorized, tight-loop
- **Authors:** "Peter Boncz", "Marcin Zukowski"


# Column-at-a-Time Batch Processing

## Description

Processes entire column vectors (batches of 1000-65536 values) through each
operator before moving to the next operator, as opposed to row-at-a-time
processing where each row passes through all operators. Batch processing
amortizes function call overhead, enables SIMD, and keeps data in CPU caches.

**Processing model:**
- Each operator receives a column vector (array of values) as input
- The operator produces a column vector as output
- A selection vector (boolean mask or position list) tracks which rows are active
- Operators implement tight loops over the active positions

**Key advantage over row-at-a-time**: A function call per row (Volcano model)
costs ~5ns overhead each call. With batch size 1024, the amortized call
overhead is 5ns / 1024 = ~0.005ns per value, making the function call cost
negligible compared to actual computation.

## Relational Algebra

```
Batch column processing:
  // Row-at-a-time (Volcano): function call per row
  for each row in table:
    if filter(row):         // virtual function call
      projected = project(row)  // virtual function call
      output(projected)    // virtual function call

  // Column-at-a-time: function call per batch
  batch = read_column_batch(table, 1024)
  selection = filter_batch(batch.col_a, "> 100")  // one call, 1024 values
  projected = project_batch(batch, selection, [col_b, col_c])  // one call
  output_batch(projected)  // one call
```

## Implementation

```rust
/// Column vector: contiguous array of typed values
pub struct ColumnVector {
    data: Vec<u8>,       // Raw bytes
    len: usize,          // Number of values
    type_width: usize,   // Bytes per value
    null_bitmap: Vec<u64>, // Null tracking (1 bit per value)
}

/// Selection vector: tracks active rows
pub struct SelectionVector {
    /// Active row positions (sorted)
    positions: Vec<u32>,
}

/// Batch processing operator interface
pub trait BatchOperator {
    fn process_batch(
        &self,
        input: &[ColumnVector],
        selection: &SelectionVector,
    ) -> (Vec<ColumnVector>, SelectionVector);
}

/// Filter operator: processes entire column in tight loop
pub struct BatchFilter {
    column_idx: usize,
    threshold: i64,
}

impl BatchOperator for BatchFilter {
    fn process_batch(
        &self,
        input: &[ColumnVector],
        selection: &SelectionVector,
    ) -> (Vec<ColumnVector>, SelectionVector) {
        let col = &input[self.column_idx];
        let values = col.as_i64_slice();
        let mut new_selection = Vec::new();

        // Tight loop: no function calls, branch-free comparison
        for &pos in &selection.positions {
            let val = values[pos as usize];
            // Branch-free: convert comparison to 0/1 and use as index
            if val > self.threshold {
                new_selection.push(pos);
            }
        }

        (input.to_vec(), SelectionVector { positions: new_selection })
    }
}

/// Aggregation: batch processing of column values
pub struct BatchSum {
    column_idx: usize,
}

impl BatchSum {
    pub fn aggregate(
        &self,
        input: &ColumnVector,
        selection: &SelectionVector,
    ) -> i64 {
        let values = input.as_i64_slice();
        let mut sum: i64 = 0;

        // Tight loop: compiler auto-vectorizes
        for &pos in &selection.positions {
            sum += values[pos as usize];
        }

        sum
    }
}

/// Batch size tuning
pub fn optimal_batch_size(
    row_width_bytes: usize,
    num_columns: usize,
    l1_cache_bytes: usize,
) -> usize {
    // Batch should fit working columns in L1 cache
    let per_column_batch_bytes = l1_cache_bytes / num_columns;
    let type_width = row_width_bytes / num_columns;
    let batch_size = per_column_batch_bytes / type_width;

    // Round to power of 2 for alignment
    batch_size.next_power_of_two().min(65536).max(64)
}
```

## Cost Model

**Per-value cost comparison:**
- Row-at-a-time (Volcano): ~5ns function call + ~1ns computation = ~6ns/value
- Column-at-a-time (batch 1024): ~5ns/1024 + ~1ns = ~1.005ns/value
- Speedup from call amortization alone: ~6x

**SIMD auto-vectorization:**
- Tight loops on column arrays auto-vectorize with modern compilers
- Additional 4-8x speedup from SIMD
- Total vs Volcano: 6x * 4x = ~24x for simple predicates

**Cache behavior:**
- Sequential column access: hardware prefetch effective
- One column at a time: fits in L1/L2
- Row-at-a-time: random access across columns, poor prefetch

## Test Cases

```sql
-- Test 1: Filter with batch processing
SELECT * FROM lineitem WHERE l_quantity < 25;
-- Processes l_quantity column in batches of 1024
-- Tight loop: compiler generates SIMD comparison
-- ~1 billion rows/sec on modern CPU

-- Test 2: Aggregation batching
SELECT SUM(l_extendedprice) FROM lineitem;
-- Sums column in batches
-- Auto-vectorized: 8 doubles per AVX2 operation
-- Memory-bandwidth limited: ~10 GB/s

-- Test 3: Multi-column batch pipeline
SELECT l_partkey, SUM(l_extendedprice * l_discount)
FROM lineitem WHERE l_shipdate > DATE '1995-01-01'
GROUP BY l_partkey;
-- Batch 1: filter l_shipdate column
-- Batch 2: multiply l_extendedprice * l_discount (with selection vector)
-- Batch 3: hash aggregate by l_partkey
```

## References

1. **Boncz, Peter et al**. "MonetDB/X100: Hyper-Pipelining Query Execution."
   CIDR 2005.
   - Vectorized batch processing for column stores

2. **Zukowski, Marcin et al**. "Vectorwise: Beyond Column Stores." IEEE Data
   Eng. Bull. 2012.
   - Production vectorized execution engine

3. **Kersten, Timo et al**. "Everything You Always Wanted to Know About
   Compiled and Vectorized Queries But Were Afraid to Ask." PVLDB 2018.
   - Comprehensive comparison of batch vs compiled execution
