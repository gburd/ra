# Rule: Vectorized Execution - Batch Table Scan

**Category:** execution-models
**File:** `rules/execution-models/vectorized/vectorized-scan.rra`

## Metadata

- **ID:** `vectorized-scan`
- **Version:** 1.0.0
- **Databases:** duckdb, clickhouse, spark
- **Tags:** execution, vectorized, batch, scan, simd
- **SQL Standard:** MonetDB X100
- **Authors:** Peter Boncz, MonetDB/X100 Team


# Vectorized Execution - Batch Table Scan

## Description

Vectorized table scan processes data in batches (vectors) of tuples instead of one-at-a-time. Batch sizes typically 1024-4096 tuples enable SIMD operations, improve CPU cache locality, and amortize function call overhead. This is the foundation of modern analytical query engines like DuckDB and ClickHouse.

**Key advantages:**
- **SIMD**: Operate on multiple values simultaneously (4-8x speedup)
- **Cache efficiency**: Better spatial locality, prefetching
- **Reduced overhead**: One function call per batch vs per tuple
- **Compilation friendly**: Tight loops, predictable branches

**Trade-offs:**
- Larger memory footprint (batch buffers)
- More complex operator implementations
- Requires columnar or decomposed storage

## Relational Algebra

```
VectorizedScan(table) → Iterator<Batch>

Batch = {
  columns: Vec<Column>  // Columnar layout
  size: usize           // Number of tuples
  selection: Option<Vec<usize>>  // Selection vector for filtered rows
}

Column = {
  data: Vec<Value>      // Contiguous array
  nulls: Bitset         // NULL indicators
}

VectorizedScanIterator {
  cursor: TableCursor
  batch_size: usize

  fn next_batch() → Batch | None {
    batch = Batch::new(batch_size)

    for col in table.columns {
      batch.columns.push(
        cursor.read_column(col, batch_size)
      )
    }

    if batch.size == 0 {
      return None
    }

    cursor.advance(batch.size)
    return batch
  }
}
```

## Implementation

```rust
use ra_core::algebra::RelExpr;

const DEFAULT_BATCH_SIZE: usize = 1024;

/// Vectorized batch of tuples in columnar format
pub struct Batch {
    /// Columnar data
    columns: Vec<Column>,
    /// Number of valid tuples
    size: usize,
    /// Optional selection vector for filtered rows
    selection: Option<Vec<usize>>,
}

pub struct Column {
    /// Column data (typed array)
    data: Box<dyn Array>,
    /// NULL bitmap
    nulls: Bitset,
}

/// Vectorized scan iterator
pub struct VectorizedScanIterator {
    table: String,
    cursor: TableCursor,
    batch_size: usize,
    columns: Vec<ColumnId>,
}

impl VectorizedScanIterator {
    pub fn new(table: String, batch_size: usize) -> Self {
        Self {
            table,
            cursor: TableCursor::default(),
            batch_size,
            columns: vec![],
        }
    }

    pub fn next_batch(&mut self) -> Result<Option<Batch>> {
        if !self.cursor.has_more() {
            return Ok(None);
        }

        let mut batch = Batch::new(self.batch_size);

        // Read columns in batch
        for col_id in &self.columns {
            let column_data = self.cursor.read_column_batch(
                *col_id,
                self.batch_size,
            )?;
            batch.add_column(column_data);
        }

        if batch.size == 0 {
            return Ok(None);
        }

        self.cursor.advance(batch.size)?;
        Ok(Some(batch))
    }
}

/// SIMD-accelerated filter on batch
pub fn vectorized_filter(batch: &mut Batch, predicate: &Expr) -> Result<()> {
    let mut selection = Vec::with_capacity(batch.size);

    // Evaluate predicate on entire batch (SIMD-friendly)
    let results = eval_predicate_vectorized(predicate, batch)?;

    // Build selection vector
    for (i, &result) in results.iter().enumerate() {
        if result {
            selection.push(i);
        }
    }

    batch.selection = Some(selection);
    Ok(())
}

/// Cost model for vectorized scan
pub fn vectorized_scan_cost(
    row_count: f64,
    row_size: usize,
    batch_size: usize,
) -> f64 {
    let num_batches = (row_count / batch_size as f64).ceil();

    // Much lower per-tuple cost due to batching
    let cpu_cost_per_batch = 0.01; // ms (amortized overhead)
    let cpu_cost_per_tuple = 0.0001; // Per tuple processing

    // I/O cost similar to tuple-at-a-time but better cache behavior
    let page_size = 8192;
    let num_pages = ((row_count * row_size as f64) / page_size as f64).ceil();
    let io_cost = num_pages * 0.08; // Slightly faster due to prefetching

    let cpu_cost = num_batches * cpu_cost_per_batch + row_count * cpu_cost_per_tuple;

    cpu_cost + io_cost
}
```

## Cost Model

**CPU Cost:**
- **Function call overhead:** O(N / batch_size) - 1000x reduction
- **SIMD speedup:** 4-8x for arithmetic, comparisons
- **Cache efficiency:** ~2x improvement from locality
- **Total CPU:** `(row_count / batch_size) × batch_overhead + row_count × per_tuple_cost / SIMD_width`

**I/O Cost:**
- Similar to tuple-at-a-time for sequential scans
- Better prefetching: ~20% improvement
- Columnar storage: read only needed columns

**Memory:**
- **Batch buffers:** `batch_size × row_size × num_columns`
- Typically 1-8 MB per batch
- Multiple batches in flight for pipelining

**Batch Size Tuning:**
- Too small: overhead dominates
- Too large: cache thrashing, memory pressure
- Optimal: 1024-4096 tuples (cache-friendly)

## Test Cases

```sql
-- Test 1: Large analytical scan
SELECT * FROM events WHERE timestamp > '2024-01-01';
-- Expected: Vectorized scan with batch processing
-- Cost: ~10x faster than Volcano for large scans

-- Test 2: Filtered scan
SELECT user_id, amount FROM transactions WHERE amount > 1000;
-- Expected: Vectorized filter on batch
-- Cost: SIMD predicate evaluation, selection vectors

-- Test 3: Columnar projection
SELECT SUM(revenue) FROM sales;
-- Expected: Only read revenue column
-- Cost: Minimal I/O, vectorized aggregation

-- Test 4: Small result set
SELECT * FROM config WHERE key = 'setting';
-- Expected: May still use batching
-- Cost: Overhead not significant for small tables
```

## References

1. **Boncz, Peter A.; Zukowski, Marcin; Nes, Niels**. "MonetDB/X100: Hyper-Pipelining Query Execution." CIDR 2005.
   - Original vectorized execution paper
   - Batch processing model, cache-conscious design

2. **Zukowski, Marcin et al**. "Super-Scalar RAM-CPU Cache Compression." ICDE 2006.
   - Vectorized compression techniques

3. **Kersten, Timo et al**. "Everything You Always Wanted to Know About Compiled and Vectorized Queries But Were Afraid to Ask." VLDB 2018.
   - Comparison of vectorized vs compiled execution

4. **DuckDB Source**: `src/execution/physical_operator/scan/physical_table_scan.cpp`
   - Modern vectorized scan implementation

5. **ClickHouse Source**: `src/Processors/Sources/SourceFromInputStream.cpp`
   - Vectorized block processing
