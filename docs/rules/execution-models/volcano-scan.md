# Rule: Volcano Iterator Model - Table Scan

**Category:** execution-models
**File:** `rules/execution-models/volcano/volcano-scan.rra`

## Metadata

- **ID:** `volcano-scan`
- **Version:** 1.0.0
- **Databases:** postgresql, mysql, oracle, sqlite, mssql
- **Tags:** execution, iterator, volcano, scan, tuple-at-a-time
- **SQL Standard:** Volcano model
- **Authors:** Goetz Graefe


# Volcano Iterator Model - Table Scan

## Description

The Volcano iterator model implements table scanning using the classic `open()`, `next()`, `close()` iterator interface. Each call to `next()` returns a single tuple, providing the foundation for tuple-at-a-time pipelined execution. This model enables efficient composition of operators and natural support for pipelining.

**Key characteristics:**
- **Pull-based**: Parent operators call `next()` on child operators
- **Lazy evaluation**: Tuples produced on-demand
- **Pipelining**: No materialization between operators
- **Memory efficiency**: Only one tuple in flight at a time
- **Simplicity**: Clean operator interface, easy to implement

**Trade-offs:**
- High per-tuple function call overhead
- Poor CPU cache locality (random tuple access)
- Limited SIMD/vectorization opportunities
- Iterator state management overhead

## Relational Algebra

```
Scan(table) -> Iterator<Tuple>

interface Iterator {
  open() -> void
  next() -> Tuple | None
  close() -> void
}

ScanIterator implements Iterator {
  cursor: TableCursor

  fn open() {
    cursor = table.begin()
  }

  fn next() -> Tuple | None {
    if cursor.valid() {
      tuple = cursor.current()
      cursor.advance()
      return tuple
    }
    return None
  }

  fn close() {
    cursor.release()
  }
}
```

## Implementation

```rust
use ra_core::algebra::RelExpr;
use ra_core::expr::Expr;

/// Volcano-style scan iterator
pub struct ScanIterator {
    table: String,
    cursor: Option<TableCursor>,
    filter: Option<Expr>,
}

impl ScanIterator {
    pub fn new(table: String, filter: Option<Expr>) -> Self {
        Self {
            table,
            cursor: None,
            filter,
        }
    }
}

impl Iterator for ScanIterator {
    type Item = Tuple;

    fn open(&mut self) -> Result<()> {
        self.cursor = Some(TableCursor::open(&self.table)?);
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Tuple>> {
        let cursor = self.cursor.as_mut().unwrap();

        loop {
            if !cursor.valid() {
                return Ok(None);
            }

            let tuple = cursor.current()?;
            cursor.advance()?;

            // Apply optional filter predicate
            if let Some(ref filter) = self.filter {
                if !eval_predicate(filter, &tuple)? {
                    continue; // Skip filtered tuple
                }
            }

            return Ok(Some(tuple));
        }
    }

    fn close(&mut self) -> Result<()> {
        if let Some(cursor) = self.cursor.take() {
            cursor.close()?;
        }
        Ok(())
    }
}

/// Cost model for volcano scan
pub fn volcano_scan_cost(
    row_count: f64,
    row_size: usize,
    filter_selectivity: f64,
) -> f64 {
    // Base cost: CPU cost per tuple + I/O cost
    let cpu_cost_per_tuple = 0.001; // ms per tuple (iterator overhead)
    let io_cost_per_page = 0.1; // ms per page read

    let page_size = 8192; // bytes
    let tuples_per_page = page_size / row_size;
    let num_pages = (row_count / tuples_per_page as f64).ceil();

    let cpu_cost = row_count * cpu_cost_per_tuple;
    let io_cost = num_pages * io_cost_per_page;
    let output_tuples = row_count * filter_selectivity;

    cpu_cost + io_cost
}
```

## Cost Model

**CPU Cost:**
- Iterator state management: `O(1)` per next() call
- Function call overhead: ~1-10 ns per tuple
- Predicate evaluation (if filter): 10-100 ns per tuple
- Total CPU: `row_count $\times$ (call_overhead + predicate_cost)`

**I/O Cost:**
- Sequential scan: read every page once
- Page reads: `⌈row_count / tuples_per_page⌉`
- Random access penalty if not sequential
- Total I/O: `num_pages $\times$ page_read_latency`

**Memory:**
- Iterator state: O(1) - just cursor position
- No buffering: minimal memory footprint
- Cache behavior: poor (single tuple)

**Total Cost:** `CPU_cost + I/O_cost`

## Test Cases

```sql
-- Test 1: Simple scan
SELECT * FROM orders;
-- Expected: Sequential scan, all tuples returned
-- Cost: row_count $\times$ tuple_cost + page_count $\times$ io_cost

-- Test 2: Scan with filter
SELECT * FROM orders WHERE amount > 1000;
-- Expected: Scan with predicate evaluation per tuple
-- Cost: increased CPU (predicate), same I/O

-- Test 3: Scan small table
SELECT * FROM config WHERE key = 'setting';
-- Expected: Fast scan, few tuples
-- Cost: minimal, dominated by I/O setup

-- Test 4: Scan large table
SELECT * FROM events LIMIT 10;
-- Expected: Can stop early after 10 tuples (with limit pushdown)
-- Cost: Only read pages needed for 10 tuples
```

## References

1. **Graefe, Goetz**. "Volcano: An Extensible and Parallel Query Evaluation System." IEEE TKDE, 1994.
   - Original Volcano iterator model paper
   - Defines open/next/close interface

2. **Graefe, Goetz**. "Encapsulation of Parallelism in the Volcano Query Processing System." SIGMOD 1990.
   - Parallel extensions to iterator model
   - Exchange operators for parallelism

3. **Graefe, Goetz; McKenna, William J**. "The Volcano Optimizer Generator: Extensibility and Efficient Search." ICDE 1993.
   - Optimizer integration with iterator model

4. **PostgreSQL Source**: `src/backend/executor/nodeSeqscan.c`
   - Production Volcano-style scan implementation
   - Demonstrates practical optimizations

5. **MySQL Source**: `storage/innobase/row/row0sel.cc`
   - InnoDB row-by-row scan implementation
