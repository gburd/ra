# Rule: Push-Based JIT Code Generation

**Category:** execution-models/push-based
**File:** `rules/execution-models/push-based/push-based-code-generation.rra`

## Metadata

- **ID:** `push-based-code-generation`
- **Version:** "1.0.0"
- **Databases:** hyper, umbra, mssql, cockroachdb
- **Tags:** execution, push-based, jit, compilation, llvm, codegen
- **Authors:** "RA Contributors"


# Push-Based JIT Code Generation

## Description

Just-in-time compiles query plans to native machine code, generating
specialized functions that eliminate interpretation overhead, enable
register allocation across operators, and allow the hardware's branch
predictor and instruction cache to work optimally. This is the core
technique behind the HyPer and Umbra database systems.

**When to apply**: Any query pipeline that will process enough rows
to amortize compilation cost (typically >10K rows). Short OLTP queries
may skip JIT and use interpretation.

**Why it works**: In the Volcano model, each tuple crosses multiple
virtual function boundaries (open/next/close per operator). Each
boundary costs ~20 CPU cycles (indirect call, pipeline stall, icache miss).
For a 5-operator pipeline processing 1M rows, that's 100M wasted cycles.
JIT compilation fuses the entire pipeline into a single tight loop where
intermediate values live in CPU registers, branches are predictable, and
the instruction stream fits in L1 icache.

**Compilation pipeline:**
1. **Query plan** -> Identify pipeline breakers (hash builds, sorts)
2. **Pipeline segmentation** -> Split plan at breakers into pipelines
3. **Code generation** -> Each pipeline becomes a function
4. **LLVM optimization** -> Standard compiler optimizations (O2)
5. **Native code** -> Directly executable machine code
6. **Execution** -> Call generated functions

**Key concepts:**
- **Pipeline breaker**: An operator that must materialize its full input
  before producing output (hash table build, sort). Pipelines end at breakers.
- **Produce/consume interface**: Operators implement `produce()` (request
  tuples from children) and `consume()` (process a tuple from a child).
  Code generation traverses the plan calling produce/consume to emit code.
- **Pipeline fusion**: All non-breaking operators in a pipeline are fused
  into a single loop body.

## Relational Algebra

```algebra
-- Pipeline identification:
-- Plan: Project -> Filter -> HashJoin(build=Scan_dim, probe=Scan_fact)
-- Pipeline 1 (build): Scan_dim -> HashBuild
-- Pipeline 2 (probe): Scan_fact -> HashProbe -> Filter -> Project -> Emit

-- Generated code for Pipeline 2:
for page in fact_table.pages():
  for tuple in page.tuples():
    match = hash_table.probe(tuple.join_key)
    if match != NULL:
      joined = concatenate(tuple, match)
      if eval_predicate(joined):
        result = project(joined, output_cols)
        output_buffer.append(result)
```

## Implementation

```rust
/// JIT compiler for query pipelines
pub struct PipelineCompiler {
    context: LLVMContext,
    module: LLVMModule,
    builder: LLVMBuilder,
    /// Mapping from plan nodes to generated LLVM values
    value_map: HashMap<PlanNodeId, LLVMValue>,
}

impl PipelineCompiler {
    /// Compile a full query plan into executable pipelines
    pub fn compile(plan: &QueryPlan) -> CompiledQuery {
        let pipelines = Self::identify_pipelines(plan);
        let mut compiled = Vec::new();

        for pipeline in &pipelines {
            let compiler = PipelineCompiler::new();
            let func = compiler.compile_pipeline(pipeline);
            compiled.push(func);
        }

        CompiledQuery { pipelines: compiled }
    }

    /// Identify pipeline boundaries at materialization points
    fn identify_pipelines(plan: &QueryPlan) -> Vec<Pipeline> {
        let mut pipelines = Vec::new();
        let mut current = Pipeline::new();

        plan.visit_bottom_up(|node| {
            if node.is_pipeline_breaker() {
                // End current pipeline, start new one
                pipelines.push(current.clone());
                current = Pipeline::new();
            }
            current.add_operator(node);
        });

        pipelines.push(current);
        pipelines
    }

    /// Generate code for a single pipeline
    fn compile_pipeline(&self, pipeline: &Pipeline) -> CompiledFunction {
        let func_type = self.context.function_type(
            self.context.void_type(),
            &[self.context.ptr_type()], // runtime context
        );
        let func = self.module.add_function("pipeline", func_type);
        let entry = self.context.append_basic_block(func, "entry");
        self.builder.position_at_end(entry);

        // Generate the pipeline body using produce/consume
        let source = pipeline.source();
        self.generate_produce(source, pipeline);

        // Run LLVM optimization passes
        let pass_manager = LLVMPassManager::new();
        pass_manager.add_pass(LLVMPass::InstCombine);
        pass_manager.add_pass(LLVMPass::SROA);      // Scalar replacement
        pass_manager.add_pass(LLVMPass::GVN);       // Value numbering
        pass_manager.add_pass(LLVMPass::LoopVectorize);
        pass_manager.add_pass(LLVMPass::SLPVectorize);
        pass_manager.run(func);

        self.jit_compile(func)
    }

    /// Recursive code generation: produce tuples
    fn generate_produce(
        &self,
        node: &PlanNode,
        pipeline: &Pipeline,
    ) {
        match &node.op {
            Op::Scan { table } => {
                // Generate scan loop
                let loop_header = self.context.append_basic_block(
                    self.current_function(), "scan_loop"
                );
                let loop_body = self.context.append_basic_block(
                    self.current_function(), "scan_body"
                );
                let loop_end = self.context.append_basic_block(
                    self.current_function(), "scan_end"
                );

                // for each page in table
                self.builder.build_br(loop_header);
                self.builder.position_at_end(loop_header);
                let page = self.builder.build_call("next_page", &[table]);
                let done = self.builder.build_is_null(page);
                self.builder.build_cond_br(done, loop_end, loop_body);

                // for each tuple in page
                self.builder.position_at_end(loop_body);
                let tuple = self.builder.build_call("get_tuple", &[page]);

                // Call consume on the next operator in the pipeline
                self.generate_consume(
                    pipeline.next_after(node),
                    tuple,
                    pipeline,
                );

                self.builder.build_br(loop_header);
                self.builder.position_at_end(loop_end);
            }
            Op::HashProbe { hash_table } => {
                // Probe is inline: no loop, just lookup
                let key = self.generate_extract_key(node);
                let match_val = self.builder.build_call(
                    "hash_probe", &[hash_table, key]
                );
                let found = self.builder.build_is_not_null(match_val);

                let then_bb = self.context.append_basic_block(
                    self.current_function(), "probe_hit"
                );
                let merge_bb = self.context.append_basic_block(
                    self.current_function(), "probe_cont"
                );
                self.builder.build_cond_br(found, then_bb, merge_bb);

                self.builder.position_at_end(then_bb);
                let joined = self.builder.build_call(
                    "concatenate", &[node.input_tuple(), match_val]
                );
                self.generate_consume(
                    pipeline.next_after(node), joined, pipeline
                );
                self.builder.build_br(merge_bb);

                self.builder.position_at_end(merge_bb);
            }
            _ => {}
        }
    }

    /// Recursive code generation: consume a tuple
    fn generate_consume(
        &self,
        node: &PlanNode,
        tuple: LLVMValue,
        pipeline: &Pipeline,
    ) {
        match &node.op {
            Op::Filter { predicate } => {
                let cond = self.generate_predicate(predicate, tuple);
                let then_bb = self.context.append_basic_block(
                    self.current_function(), "filter_pass"
                );
                let merge_bb = self.context.append_basic_block(
                    self.current_function(), "filter_cont"
                );
                self.builder.build_cond_br(cond, then_bb, merge_bb);

                self.builder.position_at_end(then_bb);
                self.generate_consume(
                    pipeline.next_after(node), tuple, pipeline
                );
                self.builder.build_br(merge_bb);

                self.builder.position_at_end(merge_bb);
            }
            Op::Project { columns } => {
                let projected = self.generate_projection(columns, tuple);
                self.generate_consume(
                    pipeline.next_after(node), projected, pipeline
                );
            }
            Op::Emit => {
                self.builder.build_call(
                    "output_buffer_append", &[tuple]
                );
            }
            _ => {}
        }
    }
}
```

**Restrictions:**
- Compilation latency: 1-100ms per pipeline (unacceptable for OLTP)
- LLVM dependency adds binary size and complexity
- Debugging JIT-compiled code is difficult (no source mapping)
- Complex expressions may not benefit (already optimized by LLVM)
- Adaptive re-compilation needed when statistics change mid-query
- Not all operators can be fused (hash build, sort are pipeline breakers)

## Cost Model

```rust
fn jit_compilation_cost(
    plan_nodes: usize,
    num_pipelines: usize,
    total_rows: u64,
) -> CompilationTradeoff {
    // Compilation cost: ~0.5ms per plan node, ~2ms per pipeline
    let compile_time_ms = plan_nodes as f64 * 0.5
        + num_pipelines as f64 * 2.0;

    // Per-tuple execution cost
    let jit_cost_per_tuple = 5.0;      // cycles (register-to-register)
    let volcano_cost_per_tuple = 50.0;  // cycles (virtual dispatch)

    let jit_total = compile_time_ms * 1e6  // compile cycles
        + total_rows as f64 * jit_cost_per_tuple;
    let volcano_total = total_rows as f64 * volcano_cost_per_tuple;

    // Break-even point
    let break_even_rows = (compile_time_ms * 1e6)
        / (volcano_cost_per_tuple - jit_cost_per_tuple);

    CompilationTradeoff {
        compile_time_ms,
        jit_execution_cycles: total_rows * jit_cost_per_tuple as u64,
        volcano_execution_cycles: total_rows * volcano_cost_per_tuple as u64,
        speedup: volcano_total / jit_total,
        break_even_rows: break_even_rows as u64,
    }
}
```

**Typical performance:**
- Compilation: 5-50ms for typical OLAP queries
- Break-even: ~10K-100K rows (JIT faster above this)
- Steady-state speedup: 10-100x over Volcano for CPU-bound queries
- Memory: compiled code is typically 1-10KB per pipeline

## Test Cases

### Positive: TPC-H Q1 pipeline (scan + filter + aggregate)

```sql
SELECT l_returnflag, l_linestatus,
       SUM(l_quantity), SUM(l_extendedprice)
FROM lineitem
WHERE l_shipdate <= DATE '1998-12-01' - INTERVAL '90' DAY
GROUP BY l_returnflag, l_linestatus;

-- Pipeline 1: Scan lineitem -> Filter -> Hash Aggregate (build)
-- Generated as single tight loop:
--   for tuple in lineitem:
--     if tuple.shipdate <= threshold:
--       key = (tuple.returnflag, tuple.linestatus)
--       ht[key].qty += tuple.quantity
--       ht[key].price += tuple.extendedprice
-- Pipeline 2: Scan hash table -> Project -> Emit
-- 6M rows: JIT ~50ms execution, Volcano ~2s
```

### Positive: Multi-table join pipeline

```sql
SELECT c.name, o.total
FROM customers c
JOIN orders o ON c.id = o.customer_id
WHERE o.total > 1000 AND c.country = 'US';

-- Pipeline 1 (build): Scan customers -> Filter(country='US') -> HashBuild
-- Pipeline 2 (probe): Scan orders -> Filter(total>1000) -> HashProbe -> Project -> Emit
-- Pipeline 2 compiled to:
--   for tuple in orders:
--     if tuple.total > 1000:
--       match = ht.probe(tuple.customer_id)
--       if match:
--         emit(match.name, tuple.total)
-- All filter, probe, project fused into one loop body
```

### Positive: Adaptive compilation (interpret then compile)

```sql
-- Short query: SELECT * FROM config WHERE key = 'timeout';
-- Row count estimate: 50 rows
-- Decision: interpret (compilation cost > execution cost)

-- Long query: SELECT * FROM events WHERE date > '2024-01-01';
-- Row count estimate: 50M rows
-- Decision: compile (execution savings >> compilation cost)
-- Umbra approach: start interpreting, JIT-compile after 10K rows
```

### Negative: Pipeline breaker limits fusion

```sql
SELECT * FROM (
  SELECT customer_id, SUM(total) AS sum_total
  FROM orders
  GROUP BY customer_id
) t
WHERE sum_total > 10000
ORDER BY sum_total DESC;

-- Pipeline 1: Scan -> HashAggregate (breaker: must see all groups)
-- Pipeline 2: Scan HT -> Filter -> Sort (breaker: must see all rows)
-- Pipeline 3: Scan sorted -> Emit
-- Three separate compiled functions, not one fused loop
-- Materialization at each breaker adds memory traffic
```

### Negative: Very short OLTP query

```sql
SELECT balance FROM accounts WHERE id = 12345;
-- Index lookup: 1-3 rows
-- Compilation: ~5ms, execution: ~0.01ms
-- JIT is 500x slower than interpretation for this query
-- Solution: use adaptive threshold or prepared statement caching
```

### Negative: UDF prevents inlining

```sql
SELECT * FROM data WHERE my_udf(col) > threshold;
-- my_udf is an opaque external function
-- Cannot inline into compiled pipeline
-- Must generate function call at UDF boundary
-- Breaks register allocation and branch prediction
-- Still faster than Volcano but limited fusion benefit
```

## References

**Academic papers:**
- Neumann, "Efficiently Compiling Efficient Query Plans for Modern Hardware", VLDB 2011
- Kersten et al., "Everything You Always Wanted to Know About Compiled and Vectorized Queries But Were Afraid to Ask", VLDB 2018
- Klonatos et al., "Building Efficient Query Engines in a High-Level Language", VLDB 2014
- Shaikhha et al., "How to Architect a Query Compiler, Revisited", SIGMOD 2018
- Kohn et al., "Adaptive Execution of Compiled Queries", ICDE 2018

**Implementation:**
- HyPer: Original push-based JIT compilation (Neumann 2011)
- Umbra: Adaptive compilation with interpretation fallback
- mssql: Hekaton in-memory engine with JIT compilation
- CockroachDB: Vectorized execution with optional JIT
- Apache Spark: Whole-stage code generation (Tungsten)
