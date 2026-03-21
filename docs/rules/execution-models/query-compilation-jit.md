# Rule: Experimental Query Compilation Approaches

**Category:** execution-models/experimental
**File:** `rules/execution-models/experimental/query-compilation-jit.rra`

## Metadata

- **ID:** `query-compilation-jit`
- **Version:** "1.0.0"
- **Databases:** hyper, umbra, duckdb, cockroachdb, spark
- **Tags:** execution, experimental, research, compilation, jit, ir, weld, mlir, cranelift
- **Authors:** Thomas Neumann, Andre Kohn, Amir Shaikhha


# Experimental Query Compilation Approaches

## Description

Beyond the established LLVM-based JIT compilation approach (HyPer/Umbra), recent
research explores alternative compilation strategies that address LLVM's
limitations: high compilation latency (10-100ms per pipeline), large binary size,
and complexity. Experimental approaches include lightweight IR compilation,
tiered interpretation-to-compilation, domain-specific IRs, and WebAssembly-based
portable compilation.

**When to apply**: Systems that need the performance benefits of compiled
queries but cannot tolerate LLVM's compilation overhead or dependency footprint.
Particularly relevant for embedded databases (DuckDB), cloud-native systems, and
mixed OLTP/OLAP workloads with varying query durations.

**Why new approaches are needed**: LLVM provides excellent steady-state
performance (optimized native code) but has significant downsides:
- Compilation latency: 10-100ms per pipeline, unacceptable for OLTP
- Binary size: LLVM libraries add 100+ MB to the database binary
- Complexity: LLVM API is large and evolving, maintenance burden
- Fixed compilation: once compiled, cannot adapt to runtime conditions
- All-or-nothing: must compile the entire pipeline before execution

**Experimental compilation strategies:**

1. **Adaptive execution (Umbra)**: Start with bytecode interpretation, JIT-compile
   hot pipelines after threshold. Best of both worlds: low latency for short
   queries, native performance for long queries.

2. **Lightweight code generation (Cranelift)**: Use simpler, faster compilers
   designed for JIT use cases. Cranelift compiles 10x faster than LLVM with
   ~80% of the code quality.

3. **Weld IR**: Domain-specific intermediate representation for data-parallel
   operations. Optimizes across operator boundaries with data-parallel primitives.

4. **MLIR/Linalg**: Multi-level IR that supports progressive lowering from
   high-level relational algebra to machine code. Each level enables different
   optimizations.

5. **WebAssembly compilation**: Compile to WASM for portable, sandboxed query
   execution. Enables shipping compiled queries between systems.

6. **Micro-adaptive execution**: Compile at the expression level (not the
   pipeline level). Each expression becomes a compiled kernel that can be
   mixed with interpreted operators.

## Relational Algebra

```algebra
-- Traditional JIT (HyPer):
Query Plan -> LLVM IR -> LLVM Optimization -> Native Code -> Execute
  -- Compilation: 10-100ms
  -- Execution: 5ns/tuple (optimal)

-- Adaptive (Umbra):
Query Plan -> Bytecode
  -> Interpret (immediate start)
  -> After 10K tuples: JIT compile hot loop
  -> Native Code (seamless switch)
  -- First tuple latency: <1ms
  -- Steady-state: ~7ns/tuple (near-optimal)

-- Cranelift:
Query Plan -> Cranelift IR -> Cranelift Compile -> Native Code -> Execute
  -- Compilation: 1-10ms (10x faster than LLVM)
  -- Execution: ~8ns/tuple (80% of LLVM quality)

-- Weld:
Query Plan -> Weld IR -> Weld Optimization -> LLVM IR -> Native Code
  -- Cross-operator optimization at Weld level
  -- Then standard LLVM optimization
  -- Benefit: Weld captures data-parallel patterns

-- MLIR:
Query Plan -> Relational Dialect -> Linalg Dialect -> LLVM Dialect -> Native
  -- Progressive lowering enables level-specific optimization
  -- Relational: predicate pushdown, join reordering
  -- Linalg: loop tiling, vectorization
  -- LLVM: register allocation, instruction selection
```

## Implementation

```rust
/// Adaptive compilation engine (Umbra-style)
pub struct AdaptiveCompiler {
    /// Bytecode interpreter
    interpreter: BytecodeInterpreter,
    /// JIT compiler (Cranelift or LLVM)
    jit: Box<dyn JITCompiler>,
    /// Compilation threshold (tuples processed)
    compile_threshold: u64,
    /// Compiled functions cache
    compiled_cache: HashMap<PipelineId, CompiledFn>,
}

impl AdaptiveCompiler {
    /// Execute a pipeline adaptively
    pub fn execute_pipeline(
        &mut self,
        pipeline: &Pipeline,
        input: &mut dyn DataSource,
    ) -> Vec<OutputRow> {
        // Phase 1: Generate bytecode (fast, <1ms)
        let bytecode = self.generate_bytecode(pipeline);
        let mut results = Vec::new();
        let mut tuples_processed: u64 = 0;

        // Phase 2: Interpret until threshold
        while tuples_processed < self.compile_threshold {
            match input.next_batch() {
                None => return results,
                Some(batch) => {
                    let out = self.interpreter.execute(
                        &bytecode, &batch,
                    );
                    tuples_processed += batch.len() as u64;
                    results.extend(out);
                }
            }
        }

        // Phase 3: JIT compile in background
        let compiled = self.jit.compile(pipeline);
        self.compiled_cache.insert(
            pipeline.id(), compiled.clone(),
        );

        // Phase 4: Execute compiled code
        while let Some(batch) = input.next_batch() {
            let out = compiled.execute(&batch);
            results.extend(out);
        }

        results
    }

    /// Generate compact bytecode for interpretation
    fn generate_bytecode(
        &self,
        pipeline: &Pipeline,
    ) -> Bytecode {
        let mut bc = Bytecode::new();

        for op in pipeline.operators() {
            match op {
                Op::Scan { table, columns } => {
                    for col in columns {
                        bc.emit(Instruction::LoadColumn {
                            table: *table,
                            col: *col,
                        });
                    }
                }
                Op::Filter { predicate } => {
                    self.compile_predicate(&mut bc, predicate);
                    bc.emit(Instruction::BranchIfFalse {
                        offset: 0, // patched later
                    });
                }
                Op::Project { columns } => {
                    for col in columns {
                        bc.emit(Instruction::ExtractColumn {
                            col: *col,
                        });
                    }
                }
                Op::HashProbe { table, key } => {
                    bc.emit(Instruction::HashLookup {
                        table: *table,
                        key: *key,
                    });
                    bc.emit(Instruction::BranchIfNull {
                        offset: 0,
                    });
                }
                Op::Emit => {
                    bc.emit(Instruction::OutputRow);
                }
            }
        }

        bc.patch_branches();
        bc
    }
}

/// Bytecode interpreter for fast startup
pub struct BytecodeInterpreter {
    /// Register file (values during execution)
    registers: Vec<Value>,
    /// Instruction pointer
    ip: usize,
}

impl BytecodeInterpreter {
    pub fn execute(
        &mut self,
        bytecode: &Bytecode,
        batch: &Batch,
    ) -> Vec<OutputRow> {
        let mut output = Vec::new();

        for row_idx in 0..batch.len() {
            self.ip = 0;
            while self.ip < bytecode.len() {
                match bytecode.instruction(self.ip) {
                    Instruction::LoadColumn { table, col } => {
                        self.registers.push(
                            batch.get(row_idx, *col),
                        );
                        self.ip += 1;
                    }
                    Instruction::Compare { op } => {
                        let b = self.registers.pop().unwrap();
                        let a = self.registers.pop().unwrap();
                        let result = compare(&a, &b, *op);
                        self.registers.push(
                            Value::Bool(result),
                        );
                        self.ip += 1;
                    }
                    Instruction::BranchIfFalse { offset } => {
                        let cond = self.registers
                            .pop().unwrap();
                        if !cond.as_bool() {
                            self.ip = *offset;
                        } else {
                            self.ip += 1;
                        }
                    }
                    Instruction::OutputRow => {
                        output.push(OutputRow::from_registers(
                            &self.registers,
                        ));
                        self.ip += 1;
                    }
                    _ => { self.ip += 1; }
                }
            }
            self.registers.clear();
        }

        output
    }
}

/// Cranelift-based lightweight JIT compiler
pub struct CraneliftCompiler {
    /// Function builder module
    module: JITModule,
}

impl JITCompiler for CraneliftCompiler {
    fn compile(
        &self,
        pipeline: &Pipeline,
    ) -> CompiledFn {
        let mut builder = FunctionBuilder::new();
        let entry = builder.create_block();
        builder.switch_to_block(entry);

        // Generate function signature
        let sig = builder.import_signature(Signature {
            params: vec![
                AbiParam::new(types::I64),  // batch ptr
                AbiParam::new(types::I64),  // batch len
                AbiParam::new(types::I64),  // output ptr
            ],
            returns: vec![
                AbiParam::new(types::I64),  // output count
            ],
        });

        // Generate loop over batch
        let loop_header = builder.create_block();
        let loop_body = builder.create_block();
        let loop_exit = builder.create_block();

        builder.ins().jump(loop_header, &[]);
        builder.switch_to_block(loop_header);

        let idx = builder.append_block_param(
            loop_header, types::I64,
        );
        let batch_len = builder.block_params(entry)[1];
        let done = builder.ins().icmp(
            IntCC::UnsignedGreaterThanOrEqual,
            idx, batch_len,
        );
        builder.ins().brif(
            done, loop_exit, &[], loop_body, &[],
        );

        // Generate operator code in loop body
        builder.switch_to_block(loop_body);
        for op in pipeline.operators() {
            self.generate_operator(
                &mut builder, op, idx,
            );
        }

        // Increment and loop
        let next_idx = builder.ins().iadd_imm(idx, 1);
        builder.ins().jump(loop_header, &[next_idx]);

        builder.switch_to_block(loop_exit);
        let count = builder.append_block_param(
            loop_exit, types::I64,
        );
        builder.ins().return_(&[count]);

        // Compile (much faster than LLVM)
        let func = builder.finalize();
        self.module.define_function(func);
        let code = self.module.compile().unwrap();

        CompiledFn { code }
    }
}

/// Weld-style domain-specific IR for data-parallel ops
pub struct WeldIRGenerator;

impl WeldIRGenerator {
    /// Generate Weld IR from query plan
    pub fn generate(
        &self,
        pipeline: &Pipeline,
    ) -> WeldProgram {
        let mut program = WeldProgram::new();

        // Map each operator to Weld primitives
        for op in pipeline.operators() {
            match op {
                Op::Scan { table, .. } => {
                    program.emit(WeldOp::Iter {
                        data: table.to_string(),
                    });
                }
                Op::Filter { predicate } => {
                    program.emit(WeldOp::Filter {
                        predicate: self.pred_to_weld(
                            predicate,
                        ),
                    });
                }
                Op::Project { columns } => {
                    program.emit(WeldOp::Map {
                        func: format!(
                            "|e| {{{}}}",
                            columns.iter()
                                .map(|c| format!(
                                    "e.{}", c.name,
                                ))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    });
                }
                Op::Emit => {
                    program.emit(WeldOp::Result);
                }
                _ => {}
            }
        }

        // Weld optimizer fuses operations
        program.optimize();
        program
    }
}
```

**Restrictions:**
- Adaptive: overhead from monitoring and switching
- Cranelift: less optimization than LLVM (no auto-vectorization)
- Weld: limited to data-parallel patterns
- MLIR: still experimental, API instability
- WASM: sandboxing overhead, limited SIMD support
- All: complexity of maintaining multiple compilation paths

## Cost Model

```rust
fn compilation_approach_comparison(
    plan_size: usize,
    num_rows: u64,
) -> CompilationComparison {
    // Interpretation
    let interp_startup_ms = 0.1;
    let interp_per_row_ns = 50.0;
    let interp_total = interp_startup_ms * 1e6
        + num_rows as f64 * interp_per_row_ns;

    // LLVM JIT
    let llvm_compile_ms = plan_size as f64 * 5.0;
    let llvm_per_row_ns = 5.0;
    let llvm_total = llvm_compile_ms * 1e6
        + num_rows as f64 * llvm_per_row_ns;

    // Cranelift JIT
    let cranelift_compile_ms = plan_size as f64 * 0.5;
    let cranelift_per_row_ns = 8.0;
    let cranelift_total = cranelift_compile_ms * 1e6
        + num_rows as f64 * cranelift_per_row_ns;

    // Adaptive (interpret then compile)
    let threshold_rows = 10_000;
    let adaptive_total = if num_rows < threshold_rows as u64 {
        interp_startup_ms * 1e6
            + num_rows as f64 * interp_per_row_ns
    } else {
        interp_startup_ms * 1e6
            + threshold_rows as f64 * interp_per_row_ns
            + cranelift_compile_ms * 1e6
            + (num_rows - threshold_rows as u64) as f64
                * cranelift_per_row_ns
    };

    CompilationComparison {
        interpretation_ns: interp_total as u64,
        llvm_ns: llvm_total as u64,
        cranelift_ns: cranelift_total as u64,
        adaptive_ns: adaptive_total as u64,
        llvm_break_even_rows: (llvm_compile_ms * 1e6
            / (interp_per_row_ns - llvm_per_row_ns))
            as u64,
        cranelift_break_even_rows:
            (cranelift_compile_ms * 1e6
            / (interp_per_row_ns - cranelift_per_row_ns))
            as u64,
    }
}
```

**Typical performance:**
- LLVM: 10-100ms compile, 5ns/tuple execution, break-even ~200K rows
- Cranelift: 1-10ms compile, 8ns/tuple execution, break-even ~25K rows
- Adaptive: <1ms first tuple, converges to ~7ns/tuple after threshold
- Interpretation: <0.1ms startup, 50ns/tuple (10x slower than compiled)
- Weld: cross-operator fusion adds 20-30% speedup over pipeline-only

## Test Cases

### Positive: Short OLTP query (adaptive wins)

```sql
SELECT balance FROM accounts WHERE id = 42;
-- 1 row: interpretation is instant
-- LLVM: 10ms compile + 0.001ms execute = 10.001ms total
-- Cranelift: 1ms compile + 0.001ms execute = 1.001ms
-- Adaptive: 0.05ms interpret = 0.05ms total
-- Adaptive is 200x faster than LLVM for this query
```

### Positive: Long OLAP query (compiled wins)

```sql
SELECT region, SUM(amount * tax_rate)
FROM transactions WHERE date > '2024-01-01'
GROUP BY region;
-- 100M rows
-- Interpretation: 50ns * 100M = 5000ms
-- LLVM: 20ms compile + 5ns * 100M = 520ms
-- Cranelift: 3ms compile + 8ns * 100M = 803ms
-- Adaptive: 0.5ms interpret 10K + 3ms compile +
--   8ns * 99.99M = 803ms (same as Cranelift)
-- All compiled approaches ~6x faster than interpretation
```

### Positive: Cranelift for embedded database

```sql
-- DuckDB-like embedded analytics
-- Cannot ship 100MB LLVM dependency
-- Cranelift: 5MB binary addition
-- Compile quality: 80% of LLVM
-- Compile speed: 10x faster than LLVM
-- Good tradeoff for embedded use case
```

### Negative: Highly complex expression tree

```sql
SELECT CASE WHEN ... THEN ... ELSE
  CASE WHEN ... THEN ...
    CASE WHEN ... END
  END
END AS complex_calc
FROM data;
-- 50-node expression tree
-- Cranelift: limited optimization of complex expressions
-- LLVM: GVN, DCE, and SROA significantly reduce work
-- LLVM execution: 3ns/tuple
-- Cranelift execution: 15ns/tuple (5x slower)
-- For very complex expressions, LLVM quality matters
```

### Negative: Adaptive overhead for medium queries

```sql
SELECT ... FROM medium_table WHERE ...;
-- 50K rows: right at the adaptive threshold
-- Interpretation: processes 10K rows (warmup)
-- Then compiles (3ms overhead)
-- Then processes 40K rows compiled
-- Total: interpretation cost + compile cost + compiled cost
-- Pure Cranelift: 3ms compile + all rows compiled
-- Adaptive overhead from two phases = ~10% slower
```

### Negative: WASM sandbox overhead

```sql
-- WebAssembly compilation target
-- Portable across architectures
-- But: WASM sandbox adds 15-30% overhead vs native
-- No direct SIMD (WASM SIMD is limited)
-- Memory management through linear memory model
-- Not competitive for maximum performance
-- Use case: UDF sandboxing, not full query compilation
```

## References

**Academic papers:**
- Neumann, "Efficiently Compiling Efficient Query Plans for Modern Hardware", VLDB 2011
- Kohn, Leis, Neumann, "Adaptive Execution of Compiled Queries", ICDE 2018
- Shaikhha, Klonatos, Pirk, Koch, "How to Architect a Query Compiler, Revisited", SIGMOD 2018
- Palkar et al., "Weld: A Common Runtime for High Performance Data Analytics", CIDR 2017
- Kersten et al., "Everything You Always Wanted to Know About Compiled and Vectorized Queries But Were Afraid to Ask", VLDB 2018
- Grulich et al., "Babelfish: Efficient Execution of Polyglot Queries", VLDB 2022

**Implementation:**
- Umbra: Adaptive bytecode interpretation + compilation
- DuckDB: Vectorized execution with expression compilation
- CockroachDB: Vectorized execution engine
- Spark Tungsten: Whole-stage Java bytecode generation
- Cranelift: Compiler backend designed for JIT use cases
- Weld: Stanford research runtime for data analytics
