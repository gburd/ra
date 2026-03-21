# Rule: Push-Based Adaptive Compilation Strategy

**Category:** execution-models
**File:** `rules/execution-models/push-based/push-based-adaptive-compilation.rra`

## Metadata

- **ID:** `push-based-adaptive-compilation`
- **Version:** 1.0.0
- **Databases:** Umbra, PostgreSQL, CockroachDB, SingleStore
- **Tags:** execution, push-based, adaptive, compilation, interpretation, tiered
- **SQL Standard:** Umbra model
- **Authors:** Thomas Neumann, Timo Kersten


# Push-Based Adaptive Compilation Strategy

## Description

Adaptive compilation addresses the tension between compilation latency and execution speed. For short-running queries, LLVM compilation (5-100ms) can exceed the query's execution time, making interpretation faster overall. Adaptive systems use tiered execution: start with fast interpretation or lightweight compilation, then JIT-compile hot pipelines as they prove to be long-running. This mirrors the approach used by JVM (C1/C2) and JavaScript (V8 TurboFan) engines.

**Tiered execution strategy:**
1. **Tier 0 (Interpretation)**: Immediate start, no compilation
2. **Tier 1 (Lightweight codegen)**: Fast compilation (~1ms), moderate speed
3. **Tier 2 (Full LLVM JIT)**: Slow compilation (~50ms), maximum speed

**Key characteristics:**
- **No compilation penalty**: Short queries start instantly
- **Progressive optimization**: Hot code compiled on demand
- **Morsel-aware switching**: Tier transitions happen at morsel boundaries
- **Profile-guided**: Runtime statistics inform compilation decisions
- **Concurrent compilation**: JIT runs in background thread

**Trade-offs:**
- State transfer complexity between tiers
- Warm-up period before optimal execution speed
- Memory overhead for multiple code versions
- Decision heuristic tuning required

## Relational Algebra

```
Adaptive execution decision:

execute(pipeline, estimated_rows):
  if estimated_rows < 1000:
    // Short query: interpret directly
    interpret(pipeline)

  else if estimated_rows < 100000:
    // Medium query: lightweight codegen
    code = quick_compile(pipeline)  // ~1ms
    execute_compiled(code)

  else:
    // Long query: start interpreted, JIT in background
    jit_future = async { llvm_compile(pipeline) }
    morsels_processed = 0

    while has_more_morsels():
      if jit_future.is_ready():
        // Switch to compiled execution
        code = jit_future.get()
        execute_compiled_remaining(code)
        return

      interpret_morsel(pipeline)
      morsels_processed += 1
```

## Implementation

```rust
use ra_core::algebra::RelExpr;

/// Adaptive compilation engine with tiered execution
pub struct AdaptiveCompiler {
    interpreter: PipelineInterpreter,
    quick_compiler: QuickCompiler,
    llvm_compiler: LLVMCompiler,
    compilation_thread: ThreadPool,
}

/// Execution tier selection
enum ExecutionTier {
    Interpret,
    QuickCompile,
    FullJIT,
    AdaptiveSwitch,
}

impl AdaptiveCompiler {
    /// Select execution tier based on estimated cost
    pub fn select_tier(
        &self,
        pipeline: &Pipeline,
        stats: &TableStats,
    ) -> ExecutionTier {
        let estimated_rows = stats.estimated_cardinality(pipeline);
        let pipeline_complexity = pipeline.operators.len();

        // Thresholds calibrated per system
        let interpret_threshold = 1_000;
        let quick_compile_threshold = 100_000;

        // Factor in compilation cost vs execution savings
        let interpret_cost = estimated_rows as f64 * 0.0001;
        let compile_cost = pipeline_complexity as f64 * 10.0;
        let compiled_exec_cost = estimated_rows as f64 * 0.000005;

        if estimated_rows < interpret_threshold {
            ExecutionTier::Interpret
        } else if estimated_rows < quick_compile_threshold {
            if compile_cost < interpret_cost - compiled_exec_cost {
                ExecutionTier::QuickCompile
            } else {
                ExecutionTier::Interpret
            }
        } else {
            ExecutionTier::AdaptiveSwitch
        }
    }

    /// Execute with adaptive tier switching
    pub fn execute_adaptive(
        &mut self,
        pipeline: &Pipeline,
    ) -> Result<Vec<Batch>> {
        let tier = self.select_tier(pipeline, &pipeline.stats());

        match tier {
            ExecutionTier::Interpret => {
                self.interpreter.execute(pipeline)
            }
            ExecutionTier::QuickCompile => {
                let code = self.quick_compiler.compile(pipeline)?;
                code.execute()
            }
            ExecutionTier::FullJIT => {
                let code = self.llvm_compiler.compile(pipeline)?;
                code.execute()
            }
            ExecutionTier::AdaptiveSwitch => {
                self.execute_with_switch(pipeline)
            }
        }
    }

    /// Start interpreted, switch to JIT when ready
    fn execute_with_switch(
        &mut self,
        pipeline: &Pipeline,
    ) -> Result<Vec<Batch>> {
        // Launch LLVM compilation in background
        let pipeline_clone = pipeline.clone();
        let jit_handle = self.compilation_thread.spawn(move || {
            LLVMCompiler::new().compile(&pipeline_clone)
        });

        let mut results = Vec::new();
        let mut morsel_source = pipeline.morsel_source();

        // Interpret morsels while waiting for JIT
        while let Some(morsel) = morsel_source.next_morsel() {
            // Check if JIT is ready
            if let Some(compiled) = jit_handle.try_get() {
                // Switch to compiled execution
                let code = compiled?;
                while let Some(m) = morsel_source.next_morsel() {
                    results.extend(code.execute_morsel(&m)?);
                }
                return Ok(results);
            }

            // Continue interpreting
            results.extend(
                self.interpreter.execute_morsel(pipeline, &morsel)?
            );
        }

        Ok(results)
    }
}

/// Lightweight quick compiler (Tier 1)
/// Uses simple code generation without LLVM overhead
pub struct QuickCompiler;

impl QuickCompiler {
    /// Generate code via direct assembly or bytecode
    pub fn compile(&self, pipeline: &Pipeline) -> Result<QuickCode> {
        let mut asm = AssemblyBuilder::new();

        // Generate simpler but faster-to-compile code
        for op in &pipeline.operators {
            match op {
                RelExpr::Scan { table, .. } => {
                    asm.emit_scan_loop(table);
                }
                RelExpr::Filter { predicate, .. } => {
                    asm.emit_branch(predicate);
                }
                RelExpr::Project { exprs, .. } => {
                    asm.emit_compute(exprs);
                }
                _ => {
                    // Fall back to function call for complex ops
                    asm.emit_call(op);
                }
            }
        }

        Ok(asm.finalize())
    }
}

/// Cost model for adaptive compilation
pub fn adaptive_compilation_cost(
    estimated_rows: f64,
    pipeline_complexity: usize,
) -> f64 {
    let interpret_cost_per_row = 0.0001; // ~100 ns
    let compiled_cost_per_row = 0.000005; // ~5 ns
    let quick_compile_cost = 1.0; // ~1ms
    let full_jit_cost = pipeline_complexity as f64 * 10.0; // 10-100ms

    // Short query: interpret is cheapest
    if estimated_rows < 1000.0 {
        return estimated_rows * interpret_cost_per_row;
    }

    // Medium query: quick compile
    if estimated_rows < 100_000.0 {
        return quick_compile_cost
            + estimated_rows * compiled_cost_per_row * 5.0;
    }

    // Long query: adaptive switch
    // Interpret some morsels, then switch to JIT
    let morsels_before_switch = full_jit_cost / 10.0; // ~morsel count
    let rows_interpreted = morsels_before_switch * 10000.0;
    let rows_compiled = estimated_rows - rows_interpreted;

    rows_interpreted * interpret_cost_per_row
        + rows_compiled * compiled_cost_per_row
}
```

## Cost Model

**Tier Selection Decision:**
- Interpret if: `rows x interpret_cost < compile_cost + rows x compiled_cost`
- Quick compile if: `quick_compile_cost + rows x quick_cost < rows x interpret_cost`
- Full JIT if: `jit_cost + rows x jit_cost < rows x interpret_cost`
- Adaptive switch for large/unknown cardinality

**Break-even Points (typical):**
- Interpret vs Quick compile: ~1,000 rows
- Quick compile vs Full JIT: ~100,000 rows
- Interpretation cost: ~100 ns/tuple
- Quick-compiled cost: ~20 ns/tuple
- Full JIT cost: ~5 ns/tuple

**Adaptive Switch Overhead:**
- Background compilation thread: ~50ms
- State transfer between tiers: ~0.1ms per morsel boundary
- Wasted interpretation: first N morsels (until JIT ready)
- Net benefit: positive for queries >50ms total execution

## Test Cases

```sql
-- Test 1: Point query (Tier 0 - Interpret)
SELECT * FROM users WHERE id = 42;
-- Expected: Immediate execution, no compilation
-- Latency: <1ms total

-- Test 2: Medium scan (Tier 1 - Quick compile)
SELECT name, email FROM users WHERE active = true;
-- Expected: 1ms compile, then fast execution
-- Table: ~50K rows, compile cost amortized

-- Test 3: TPC-H Q1 (Tier 2 - Full JIT)
SELECT l_returnflag, l_linestatus,
       SUM(l_quantity), SUM(l_extendedprice)
FROM lineitem
WHERE l_shipdate <= DATE '1998-12-01' - INTERVAL '90' DAY
GROUP BY l_returnflag, l_linestatus;
-- Expected: Start interpreted, switch to JIT after ~5ms
-- 6M rows: JIT compilation amortized over remaining rows

-- Test 4: Adaptive switch verification
SELECT * FROM large_table WHERE col > ?;
-- Expected: Cardinality unknown (parameterized)
-- Start interpreted, switch to JIT if scan is long
-- Verify: no regression vs always-interpret for small results
```

## Comparison with Other Models

| Aspect | Adaptive | Always JIT | Always Interpret |
|--------|---------|-----------|-----------------|
| Point query | <1ms | 5-100ms overhead | <1ms |
| OLAP query | Near JIT speed | Optimal | 10-100x slower |
| Decision overhead | Minimal (~1us) | None | None |
| Memory | Multiple versions | JIT only | Interpreter only |
| Complexity | High | Medium | Low |

## References

1. **Kohn, Andre; Leis, Viktor; Neumann, Thomas**. "Adaptive Execution of Compiled Queries." ICDE 2018.
   - Umbra's adaptive compilation with morsel-level switching

2. **Kersten, Timo et al**. "Everything You Always Wanted to Know About Compiled and Vectorized Queries But Were Afraid to Ask." VLDB 2018.
   - Analysis of compilation overhead vs execution benefit

3. **Menon, Prashanth et al**. "Relaxed Operator Fusion for In-Memory Databases." VLDB 2017.
   - Hybrid execution strategies

4. **Palczewski, Roee et al**. "The Adaptive Radix Tree: ARTful Indexing for Main-Memory Databases." ICDE 2013.
   - Adaptive data structure strategies applicable to execution tier selection

5. **PostgreSQL Documentation**. "JIT Compilation."
   - PostgreSQL's LLVM JIT with cost-based activation thresholds
