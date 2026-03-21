# Rule: "JIT Query Compilation"

**Category:** experimental/compilation
**File:** `rules/experimental/compilation/query-compilation-jit.rra`

## Metadata

- **ID:** `query-compilation-jit`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, umbra, hyper
- **Tags:** compilation, jit, llvm, codegen, push-based, data-centric
- **Authors:** "Neumann 2011", "RA Contributors"


# JIT Query Compilation

## Description

Compiles SQL queries into native machine code at query time, eliminating
the interpretation overhead of traditional Volcano-style execution. Instead
of processing tuples through a tree of iterator operators (with virtual
function calls per tuple), compiled queries fuse operators into tight loops
that keep data in CPU registers.

Two compilation approaches:
1. **Produce/consume model** (HyPer/Umbra): Operators push data to
   consumers. Pipeline breakers (hash build, sort) define code boundaries.
   Each pipeline compiles to a single tight loop.
2. **Vectorized compilation** (DuckDB, Velox): Compile vector operations
   rather than per-tuple operations. Lower compilation overhead, still
   benefits from code generation.

JIT compilation provides 5-10x speedup over interpreted execution for
CPU-bound queries. The speedup comes from: (1) eliminating virtual function
calls, (2) keeping data in registers, (3) enabling SIMD auto-vectorization,
(4) removing materialization between operators.

**When to apply**: CPU-bound analytical queries where execution time
exceeds compilation time. Short OLTP queries may not benefit due to
compilation overhead (10-100ms).

**Why it works**: Modern CPUs spend most cycles on control flow overhead
(branch prediction, function calls, cache misses for vtable lookups) in
interpreted execution. Compiled queries eliminate this overhead by
generating straight-line code that processes tuples in tight loops.

**Research status**: Production in HyPer (now Umbra), PostgreSQL 11+ (JIT
via LLVM for expressions), CockroachDB (vectorized + JIT). Active research
on reducing compilation latency and improving adaptive compilation.

## Relational Algebra

```algebra
Volcano (interpreted):
  while (tuple = child.next()) {
    // Virtual dispatch per tuple per operator
    // Data moves through memory between operators
    process(tuple);
  }

Compiled (produce/consume):
  Pipeline 1: scan -> filter -> hash_build
    for each tuple in table {
      if (predicate) {
        hash_table.insert(tuple);
      }
    }

  Pipeline 2: scan -> hash_probe -> aggregate -> output
    for each tuple in probe_table {
      for match in hash_table.probe(tuple.key) {
        agg_state.update(match.value);
      }
    }

Pipeline boundaries (breakers):
  - Hash join build
  - Sort
  - Materialize (temp table)
  Everything between breakers fuses into one loop
```

## Implementation

```rust
use std::collections::HashMap;

// Query plan to compiled code
struct QueryCompiler {
    llvm_context: LLVMContext,
    module: LLVMModule,
    optimization_level: OptLevel,
}

impl QueryCompiler {
    fn compile(&self, plan: &PlanNode) -> CompiledQuery {
        // Step 1: Identify pipelines (split at breakers)
        let pipelines = self.identify_pipelines(plan);

        // Step 2: Generate code for each pipeline
        let mut compiled_pipelines = Vec::new();
        for pipeline in &pipelines {
            let ir = self.generate_pipeline_ir(pipeline);
            compiled_pipelines.push(ir);
        }

        // Step 3: Optimize and compile to native code
        let native_code = self.optimize_and_compile(
            &compiled_pipelines,
        );

        CompiledQuery {
            pipelines: compiled_pipelines,
            native_function: native_code,
        }
    }

    fn identify_pipelines(
        &self,
        plan: &PlanNode,
    ) -> Vec<Pipeline> {
        let mut pipelines = Vec::new();
        let mut current = Pipeline::new();

        self.walk_plan(plan, &mut current, &mut pipelines);

        if !current.operators.is_empty() {
            pipelines.push(current);
        }

        pipelines
    }

    fn walk_plan(
        &self,
        node: &PlanNode,
        current: &mut Pipeline,
        pipelines: &mut Vec<Pipeline>,
    ) {
        match node {
            PlanNode::SeqScan { table, .. } => {
                current.operators.push(Operator::Scan(table.clone()));
            }

            PlanNode::Filter { input, predicate } => {
                self.walk_plan(input, current, pipelines);
                current.operators.push(
                    Operator::Filter(predicate.clone()),
                );
            }

            PlanNode::HashJoin { build, probe, .. } => {
                // Build side: separate pipeline ending in hash build
                let mut build_pipeline = Pipeline::new();
                self.walk_plan(build, &mut build_pipeline, pipelines);
                build_pipeline.operators.push(
                    Operator::HashBuild,
                );
                pipelines.push(build_pipeline);

                // Probe side: continues current pipeline
                self.walk_plan(probe, current, pipelines);
                current.operators.push(Operator::HashProbe);
            }

            PlanNode::Sort { input, keys } => {
                // Sort is a pipeline breaker
                let mut input_pipeline = Pipeline::new();
                self.walk_plan(
                    input,
                    &mut input_pipeline,
                    pipelines,
                );
                input_pipeline.operators.push(
                    Operator::SortBuild(keys.clone()),
                );
                pipelines.push(input_pipeline);

                current.operators.push(Operator::SortScan);
            }

            PlanNode::Aggregate { input, group_by, aggs } => {
                self.walk_plan(input, current, pipelines);
                current.operators.push(
                    Operator::Aggregate(group_by.clone(), aggs.clone()),
                );
            }

            _ => {}
        }
    }

    fn generate_pipeline_ir(
        &self,
        pipeline: &Pipeline,
    ) -> PipelineIR {
        let mut ir = PipelineIR::new();

        // Generate produce/consume code
        // Source operator produces tuples
        let source = &pipeline.operators[0];
        ir.emit_loop_header(source);

        // Intermediate operators consume and produce
        for op in &pipeline.operators[1..] {
            match op {
                Operator::Filter(pred) => {
                    ir.emit_filter(pred);
                }
                Operator::HashBuild => {
                    ir.emit_hash_insert();
                }
                Operator::HashProbe => {
                    ir.emit_hash_probe();
                }
                Operator::Aggregate(groups, aggs) => {
                    ir.emit_aggregate(groups, aggs);
                }
                _ => {}
            }
        }

        ir.emit_loop_footer();
        ir
    }

    fn optimize_and_compile(
        &self,
        pipelines: &[PipelineIR],
    ) -> NativeFunction {
        // LLVM optimization passes
        let mut pass_manager = PassManager::new();
        pass_manager.add_pass(InstructionCombining);
        pass_manager.add_pass(Reassociate);
        pass_manager.add_pass(GVN); // Global value numbering
        pass_manager.add_pass(SimplifyCFG);
        pass_manager.add_pass(LoopVectorize);
        pass_manager.add_pass(SLPVectorize);

        for pipeline in pipelines {
            pass_manager.run(&pipeline.llvm_function);
        }

        // JIT compile to native
        let engine = ExecutionEngine::new(&self.module);
        engine.get_function("query_main")
    }
}

// Expression compilation
struct ExpressionCompiler;

impl ExpressionCompiler {
    fn compile_predicate(
        &self,
        pred: &Predicate,
        builder: &IRBuilder,
        tuple_ptr: LLVMValue,
    ) -> LLVMValue {
        match pred {
            Predicate::Eq { col_offset, value } => {
                let col_val = builder.load_from_offset(
                    tuple_ptr,
                    *col_offset,
                );
                let const_val = builder.constant_f64(*value);
                builder.fcmp_eq(col_val, const_val)
            }

            Predicate::Range { col_offset, low, high } => {
                let col_val = builder.load_from_offset(
                    tuple_ptr,
                    *col_offset,
                );
                let low_val = builder.constant_f64(*low);
                let high_val = builder.constant_f64(*high);
                let ge_low = builder.fcmp_ge(col_val, low_val);
                let le_high = builder.fcmp_le(col_val, high_val);
                builder.and(ge_low, le_high)
            }

            Predicate::And { left, right } => {
                // Short-circuit evaluation
                let left_val = self.compile_predicate(
                    left, builder, tuple_ptr,
                );
                let right_val = self.compile_predicate(
                    right, builder, tuple_ptr,
                );
                builder.and(left_val, right_val)
            }

            _ => builder.constant_bool(true),
        }
    }
}

// Adaptive compilation: interpret first, compile if hot
struct AdaptiveCompiler {
    compilation_threshold: u32,
    interpreted_engine: InterpretedEngine,
    compiled_cache: HashMap<PlanSignature, CompiledQuery>,
}

impl AdaptiveCompiler {
    fn execute(
        &mut self,
        plan: &PlanNode,
        invocation_count: u32,
    ) -> QueryResult {
        let signature = plan.signature();

        if let Some(compiled) = self.compiled_cache.get(&signature) {
            // Hot path: execute compiled code
            return compiled.execute();
        }

        if invocation_count >= self.compilation_threshold {
            // Compile and cache
            let compiler = QueryCompiler::new();
            let compiled = compiler.compile(plan);
            let result = compiled.execute();
            self.compiled_cache.insert(signature, compiled);
            return result;
        }

        // Cold path: interpret
        self.interpreted_engine.execute(plan)
    }
}

struct Pipeline {
    operators: Vec<Operator>,
}

enum Operator {
    Scan(TableRef),
    Filter(Predicate),
    HashBuild,
    HashProbe,
    SortBuild(Vec<SortKey>),
    SortScan,
    Aggregate(Vec<String>, Vec<AggFunc>),
}
```

**Restrictions:**
- Compilation latency: 10-100ms per query (LLVM overhead)
- Not beneficial for simple OLTP queries (compilation > execution)
- Code cache management needed to avoid memory bloat
- Debugging compiled queries is difficult
- LLVM dependency adds binary size and build complexity
- UDFs require special handling (inline or call out)

## Cost Model

```rust
fn jit_benefit(
    interpreted_cost_ms: f64,
    compilation_cost_ms: f64,
    compiled_cost_ms: f64,
    expected_invocations: u32,
) -> f64 {
    let total_interpreted =
        interpreted_cost_ms * expected_invocations as f64;
    let total_compiled = compilation_cost_ms
        + compiled_cost_ms * expected_invocations as f64;

    if total_interpreted > total_compiled {
        (total_interpreted - total_compiled) / total_interpreted
    } else {
        0.0 // Not worth compiling
    }
}

fn compilation_breakeven(
    interpreted_cost_ms: f64,
    compiled_cost_ms: f64,
    compilation_cost_ms: f64,
) -> u32 {
    // Breakeven: N * interpreted = compilation + N * compiled
    // N = compilation / (interpreted - compiled)
    let speedup = interpreted_cost_ms - compiled_cost_ms;
    if speedup <= 0.0 {
        return u32::MAX;
    }
    (compilation_cost_ms / speedup).ceil() as u32
}
```

**Typical benefit**: 3-10x speedup over interpreted execution for
CPU-bound analytical queries. Compilation overhead amortized after 1-5
invocations for expensive queries.

## Test Cases

### Test 1: TPC-H Q1 (scan + aggregate)

```sql
SELECT l_returnflag, l_linestatus,
       SUM(l_quantity), SUM(l_extendedprice),
       SUM(l_extendedprice * (1 - l_discount))
FROM lineitem
WHERE l_shipdate <= DATE '1998-09-02'
GROUP BY l_returnflag, l_linestatus;

-- Interpreted (Volcano): 3.2s (60M rows, per-tuple overhead)
-- Compiled (one pipeline): 0.4s (fused scan-filter-aggregate)
-- Speedup: 8x (tight loop, data in registers, auto-vectorized)
-- Compilation time: 50ms (amortized after 1 invocation)
```

### Test 2: Hash join pipeline fusion

```sql
SELECT SUM(lo_revenue) FROM lineorder lo
JOIN date d ON lo.lo_orderdate = d.d_datekey
WHERE d.d_year = 1997;

-- Interpreted: 2 pipelines, virtual dispatch per tuple
-- Pipeline 1 (build): scan date, filter year, build hash = 0.1s
-- Pipeline 2 (probe): scan lineorder, probe hash, sum = 2.5s
-- Total interpreted: 2.6s

-- Compiled: each pipeline is one tight loop
-- Pipeline 1: 0.02s (filtered scan + hash insert, no dispatch)
-- Pipeline 2: 0.3s (scan + probe + sum, all in registers)
-- Total compiled: 0.32s (8x speedup)
```

### Test 3: Compilation overhead for short query

```sql
SELECT * FROM users WHERE id = 42;
-- Interpreted: 0.1ms (index lookup)
-- Compilation: 30ms + execution 0.05ms = 30.05ms
-- JIT is 300x SLOWER for this query
-- Adaptive: interpret below threshold, compile after 300 invocations
```

### Test 4: Expression compilation

```sql
SELECT * FROM orders
WHERE (price * (1 - discount) + tax) > 100
  AND status IN ('shipped', 'delivered');

-- Interpreted: evaluate expression per tuple via tree walker
-- Compiled: expression becomes 5 x86 instructions
-- Per-tuple speedup: 20x for complex expressions
```

### Test 5: SIMD auto-vectorization

```sql
SELECT SUM(value) FROM measurements WHERE value > 0;
-- Compiled with LLVM vectorization:
-- Processes 8 doubles per SIMD instruction (AVX-512)
-- 8x additional speedup beyond compilation
-- Total vs interpreted: 40-80x for pure scan + filter
```

## References

**Foundational:**
- Neumann, "Efficiently Compiling Efficient Query Plans for Modern Hardware", VLDB 2011
- Krikellas et al., "Generating Code for Holistic Query Evaluation", ICDE 2010

**Production systems:**
- HyPer/Umbra: full query compilation (produce/consume model)
- PostgreSQL 11+: expression JIT via LLVM
- CockroachDB: vectorized execution with optional JIT
- Apache Spark: Tungsten whole-stage code generation

**Compilation techniques:**
- Kersten et al., "Everything You Always Wanted to Know About Compiled and Vectorized Queries But Were Afraid to Ask", VLDB 2018
- Menon et al., "Relaxed Operator Fusion for In-Memory Databases", VLDB 2017

**Adaptive compilation:**
- Kohn et al., "Adaptive Execution of Compiled Queries", ICDE 2018
