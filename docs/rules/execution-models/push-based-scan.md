# Rule: Push-Based Compiled Table Scan

**Category:** execution-models
**File:** `rules/execution-models/push-based/push-based-scan.rra`

## Metadata

- **ID:** `push-based-scan`
- **Version:** 1.0.0
- **Databases:** HyPer, Umbra, SingleStore
- **Tags:** execution, push-based, compilation, scan, jit
- **SQL Standard:** HyPer model
- **Authors:** Thomas Neumann


# Push-Based Compiled Table Scan

## Description

In push-based execution, the table scan is the only "source" operator -- it drives the entire pipeline by pushing tuples into downstream operators. Unlike the Volcano model where parent operators pull via `next()`, a compiled scan generates a tight loop that iterates over the table and calls consumer functions inline. JIT compilation eliminates virtual function dispatch and enables the compiler to keep intermediate values in CPU registers.

**Key characteristics:**
- **Push-driven**: Scan pushes tuples into the pipeline, not pulled
- **Compiled loop**: Single tight `for` loop over table pages
- **Register-resident**: Tuple fields kept in CPU registers between operators
- **No iterator overhead**: No virtual dispatch, no state machines
- **Branch-friendly**: Predictable loop structure for CPU branch predictor

**Trade-offs:**
- Compilation latency (1-100ms) before first tuple
- Generated code complexity harder to debug
- Code cache pressure for many concurrent queries
- No natural "pause" point for cooperative scheduling

## Relational Algebra

```
Push-based scan compilation:

produce(Scan(table)):
  for each page p in table:
    for each tuple t in p:
      consume(t)    // Inline downstream operators

vs. Volcano (pull-based):
  fn next() -> Tuple:
    t = cursor.next()   // Virtual dispatch
    return t             // Copy to caller's frame
```

## Implementation

```rust
use ra_core::algebra::RelExpr;
use ra_core::expr::Expr;

/// Push-based compiled scan
pub struct CompiledScan {
    table: String,
    columns: Vec<ColumnId>,
}

impl CompiledScan {
    /// Generate code for a push-based scan pipeline
    pub fn produce(&self, codegen: &mut CodeGen) {
        // Emit page iteration loop
        codegen.emit_loop_header("page", &self.table);

        // Emit tuple iteration within page
        codegen.emit_loop_header("tuple", "page");

        // Extract only needed columns into registers
        for col in &self.columns {
            codegen.emit_register_load(col);
        }

        // Call consumer (next operator in pipeline)
        codegen.emit_consume_call();

        codegen.emit_loop_footer(); // tuple loop
        codegen.emit_loop_footer(); // page loop
    }
}

/// Generated code for a scan-filter-project pipeline
/// Compiles to approximately:
///
///   for page in table.pages():
///     for (col_a, col_b) in page.rows():
///       if col_a > 100:        // filter inlined
///         result = col_b * 2   // project inlined
///         emit(result)
///
fn compile_scan_pipeline(
    table: &str,
    filter: Option<&Expr>,
    projection: &[ColumnId],
) -> CompiledFunction {
    let mut codegen = CodeGen::new();

    codegen.emit("fn execute(table: &Table, output: &mut Vec<Row>) {");
    codegen.emit("  for page in table.pages() {");
    codegen.emit("    for row_idx in 0..page.num_rows() {");

    // Load only columns referenced by filter + projection
    for col in projection {
        codegen.emit(&format!(
            "      let c{} = page.column({}).get(row_idx);",
            col.index(), col.index()
        ));
    }

    // Inline filter predicate
    if let Some(pred) = filter {
        codegen.emit(&format!("      if {} {{", pred.to_code()));
    }

    // Inline projection
    codegen.emit("        output.push(Row::new(");
    for col in projection {
        codegen.emit(&format!("          c{},", col.index()));
    }
    codegen.emit("        ));");

    if filter.is_some() {
        codegen.emit("      }"); // close if
    }

    codegen.emit("    }"); // row loop
    codegen.emit("  }"); // page loop
    codegen.emit("}");

    codegen.compile()
}

/// Cost model for push-based compiled scan
pub fn compiled_scan_cost(
    row_count: f64,
    row_size: usize,
    num_columns_accessed: usize,
) -> f64 {
    // Per-tuple cost is near zero: no function calls, register-resident
    let cpu_cost_per_tuple = 0.00005; // ~5 ns (vs ~100 ns for Volcano)

    // I/O cost identical to any scan
    let page_size = 8192;
    let tuples_per_page = page_size / row_size;
    let num_pages = (row_count / tuples_per_page as f64).ceil();
    let io_cost_per_page = 0.1;

    // Compilation cost (amortized)
    let compilation_cost = 5.0; // ~5ms typical

    let cpu_cost = row_count * cpu_cost_per_tuple;
    let io_cost = num_pages * io_cost_per_page;

    compilation_cost + cpu_cost + io_cost
}
```

## Cost Model

**CPU Cost:**
- Per-tuple overhead: ~1-5 CPU cycles (vs 50-100 for Volcano)
- No virtual dispatch: all calls inlined
- Register allocation: intermediate values never spill to memory
- Branch prediction: tight loop is highly predictable
- Total CPU: `row_count x 5ns + compilation_cost`

**I/O Cost:**
- Identical to Volcano for sequential scans
- Page reads: `ceil(row_count / tuples_per_page)`
- Prefetching: compiler can insert prefetch instructions
- Column pruning reduces bytes read

**Memory:**
- Minimal: no iterator state, no tuple copying
- Working set: page buffer + CPU registers
- Code cache: generated function typically 1-10 KB

**Compilation Cost:**
- LLVM IR generation: ~1ms
- LLVM optimization passes: ~5-50ms
- Code generation: ~1-10ms
- Amortized over millions of tuples: negligible

## Test Cases

```sql
-- Test 1: Simple full scan (push-based vs Volcano)
SELECT * FROM lineitem;
-- Expected: Compiled tight loop, no iterator overhead
-- Speedup: 10-20x over Volcano for CPU-bound scans

-- Test 2: Scan with filter pushed into loop
SELECT l_orderkey, l_quantity
FROM lineitem
WHERE l_shipdate > '1998-01-01';
-- Expected: Filter compiled inline within scan loop
-- No separate filter operator, predicate in same loop body

-- Test 3: Multi-column projection
SELECT l_orderkey, l_extendedprice * l_discount AS revenue
FROM lineitem WHERE l_quantity < 25;
-- Expected: Expression compiled to register arithmetic
-- Revenue computation uses CPU multiply, no expression tree walking

-- Test 4: Pipeline breaker awareness
SELECT l_returnflag, SUM(l_quantity)
FROM lineitem
GROUP BY l_returnflag;
-- Expected: Scan pushes into aggregation hash table
-- Pipeline breaks at GROUP BY (hash table materialization)
```

## Comparison with Other Models

| Aspect | Push-Based Scan | Volcano Scan | Vectorized Scan |
|--------|----------------|-------------|-----------------|
| Per-tuple cost | ~5 ns | ~100 ns | ~10 ns (amortized) |
| Compilation | Required (1-100ms) | None | None |
| Code complexity | High (codegen) | Low (interface) | Medium (batch ops) |
| Debuggability | Hard (generated) | Easy (stack trace) | Medium |
| SIMD usage | Via compiler auto-vectorization | None | Explicit |
| Cache behavior | Good (registers) | Poor (virtual calls) | Good (batch) |

## References

1. **Neumann, Thomas**. "Efficiently Compiling Efficient Query Plans for Modern Hardware." VLDB 2011.
   - Foundational push-based compilation paper
   - Data-centric code generation approach

2. **Kersten, Timo et al**. "Everything You Always Wanted to Know About Compiled and Vectorized Queries But Were Afraid to Ask." VLDB 2018.
   - Detailed comparison of push-based vs vectorized execution

3. **Shaikhha, Amir et al**. "How to Architect a Query Compiler, Revisited." SIGMOD 2018.
   - Survey of compilation strategies for query engines

4. **HyPer Source**: Code generation for table scans
   - Production push-based scan implementation
