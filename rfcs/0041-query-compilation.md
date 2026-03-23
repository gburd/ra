# RFC 0041: Query Compilation and Code Generation

- Start Date: 2026-03-21
- Author: RA Contributors
- Status: Proposed
- Tracking Issue: TBD

## Summary

Implement query compilation that generates specialized Rust code or WASM bytecode for query execution, replacing the interpreted Volcano-style iterator model with compiled push-based execution for CPU-bound analytical queries.

## Motivation

The iterator (Volcano) execution model has per-tuple overhead from virtual function calls and branch mispredictions. For CPU-bound analytical queries processing millions of rows, this overhead dominates execution time. Query compilation generates tight loops without virtual dispatch, achieving 5-10x speedup for scan-heavy workloads.

This is the execution model used by HyPer/Umbra, DuckDB (vectorized + compiled), and modern analytical engines.

## Guide-level explanation

For a query like:
```sql
SELECT SUM(amount)
FROM orders
WHERE status = 'completed' AND amount > 100;
```

Instead of interpreting a tree of iterator nodes, the optimizer generates a compiled function equivalent to:
```rust
let mut sum = 0.0;
for row in orders.scan() {
    if row.status == "completed" && row.amount > 100.0 {
        sum += row.amount;
    }
}
```

The compiled code eliminates:
- Virtual function calls between operators
- Per-tuple type checking
- Branch mispredictions from operator dispatch
- Unnecessary materialization between operators

## Reference-level explanation

### Implementation Details

Two compilation targets:
1. **Native Rust**: Generate `.rs` source, compile with `rustc`, load as dynamic library
2. **WASM**: Generate WASM bytecode via `wasm-encoder`, execute with `wasmtime`

### Compilation Pipeline

```
Logical Plan -> Physical Plan -> Operator Pipeline -> Code Template -> Compiled Code
```

Pipeline breaking operators (hash join build, sort, group by) split the plan into pipeline fragments. Each fragment is compiled as a single push-based function.

### Operator Fusion

Within a pipeline, operators are fused:
- Scan + Filter -> fused scan with inline predicate
- Filter + Project -> single pass with column selection
- Aggregate -> running accumulator in tight loop

### Fallback

Not all operators are compilable. When a pipeline contains an uncompilable operator, fall back to interpreted execution for that segment.

## Drawbacks

- Compilation latency adds overhead for simple queries
- Generated code complexity for maintenance
- WASM compilation target has lower peak performance than native
- Dynamic library loading has security implications

## Rationale and alternatives

### Why This Design?

Hybrid compilation (compile hot pipelines, interpret the rest) provides the best tradeoff. It captures the majority of the performance benefit (scan + filter + aggregate pipelines) without requiring full query compilation.

### Alternative Approaches

- **Vectorized execution**: Processes batches instead of tuples; lower compilation cost but lower peak performance
- **LLVM JIT**: More mature but heavier dependency than WASM
- **Interpretation only**: Current approach; CPU-bound for analytical queries

## Prior art

- HyPer/Umbra: data-centric compilation with LLVM
- DuckDB: vectorized execution with adaptive morsel-driven parallelism
- DataFusion: Row-based but exploring compilation
- CMU 15-721: Query Compilation lectures

## Unresolved questions

- Compilation cache strategy (LRU by plan fingerprint?)
- Threshold for compilation vs interpretation (row count? plan complexity?)
- WASM vs native Rust compilation tradeoffs

## Future possibilities

- Adaptive compilation (interpret first, compile after N executions)
- GPU code generation for massively parallel operations
- SIMD intrinsics in generated code for vectorized predicates
