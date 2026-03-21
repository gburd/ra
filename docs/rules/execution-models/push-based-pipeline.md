# Rule: Push-Based Data-Centric Execution Pipeline

**Category:** execution-models/push-based
**File:** `rules/execution-models/push-based/push-based-pipeline.rra`

## Metadata

- **ID:** `push-based-pipeline`
- **Version:** "1.0.0"
- **Databases:** hyper, umbra, mssql, spark
- **Tags:** execution, push-based, pipeline, data-centric, fusion, compilation
- **Authors:** "RA Contributors"


# Push-Based Data-Centric Execution Pipeline

## Description

Push-based execution inverts the Volcano iterator model: instead of parent
operators pulling tuples from children (demand-driven), source operators
push tuples through the pipeline to the sink. Combined with code generation,
this eliminates virtual function dispatch overhead and enables the compiler
to keep intermediate tuple values in CPU registers throughout the pipeline.

**When to apply**: All query execution in push-based engines. The pipeline
model is the fundamental execution abstraction that determines how operators
interact and how code is generated.

**Why it works**: In the Volcano model, processing one tuple through a
5-operator pipeline requires 5 virtual `next()` calls (push up) and 5
returns (pop down) = 10 function boundary crossings. Each crossing costs
~15-20 cycles (indirect branch, pipeline flush, possible icache miss).
In push-based execution, the entire pipeline compiles to a single function
with direct branches. Intermediate values stay in registers. The CPU's
branch predictor sees a tight, predictable loop.

**Key concepts:**
- **Pipeline**: A sequence of operators from a source to a sink (or pipeline
  breaker). All operators in a pipeline can be fused into one loop.
- **Pipeline breaker**: An operator that needs to see all input before
  producing output (hash build, sort, aggregate finalization). Breakers
  split the query plan into multiple pipelines.
- **Produce/Consume protocol**: Operators implement `produce()` to request
  tuples from children and `consume(tuple)` to process a received tuple.
  Code generation traverses this protocol to emit the fused loop.
- **Full pipeline breaker**: Must materialize entire input (sort, hash build).
- **Partial pipeline breaker**: Can produce partial output while still
  receiving input (e.g., streaming aggregation with pre-sorted input).

## Relational Algebra

```algebra
-- Volcano model (pull-based):
result = Project.next()
  -> Filter.next()
    -> Scan.next()       -- returns one tuple
    <- Filter evaluates predicate
  <- Project extracts columns
-- 3 virtual function calls per tuple, values cross stack frames

-- Push-based model:
Scan.produce():
  for tuple in table:
    Filter.consume(tuple)

Filter.consume(tuple):
  if predicate(tuple):
    Project.consume(tuple)

Project.consume(tuple):
  result = extract_columns(tuple)
  Emit.consume(result)

-- Compiled form (all fused):
fn pipeline() {
  for tuple in table.scan():
    if predicate(tuple):
      let result = extract_columns(tuple)
      output.append(result)
}
-- Zero virtual calls, values in registers
```

## Implementation

```rust
/// Pipeline represents a fused sequence of operators
pub struct Pipeline {
    /// Operators in execution order (source -> sink)
    operators: Vec<Box<dyn PipelineOperator>>,
    /// Index of the source operator
    source_idx: usize,
    /// Compiled function (if JIT is enabled)
    compiled: Option<CompiledFunction>,
}

/// The produce/consume interface for push-based operators
pub trait PipelineOperator {
    /// Generate code that produces tuples and calls consume on parent
    fn produce(&self, ctx: &mut CodeGenContext);

    /// Generate code that processes one tuple from a child
    fn consume(&self, tuple: &TupleRef, ctx: &mut CodeGenContext);

    /// Is this operator a pipeline breaker?
    fn is_pipeline_breaker(&self) -> bool { false }
}

/// Scan operator: the source of a pipeline
pub struct ScanOperator {
    table: TableRef,
    parent: Box<dyn PipelineOperator>,
}

impl PipelineOperator for ScanOperator {
    fn produce(&self, ctx: &mut CodeGenContext) {
        // Generate: for each tuple in table
        ctx.emit_loop_start(&self.table);
        let tuple = ctx.emit_get_tuple();
        self.parent.consume(&tuple, ctx);
        ctx.emit_loop_end();
    }

    fn consume(&self, _: &TupleRef, _: &mut CodeGenContext) {
        unreachable!("Scan is a source, never consumes");
    }
}

/// Filter operator: inline predicate check
pub struct FilterOperator {
    predicate: Expr,
    parent: Box<dyn PipelineOperator>,
}

impl PipelineOperator for FilterOperator {
    fn produce(&self, ctx: &mut CodeGenContext) {
        // Delegate to child's produce
        // (child will call our consume)
    }

    fn consume(&self, tuple: &TupleRef, ctx: &mut CodeGenContext) {
        let cond = ctx.emit_eval_predicate(&self.predicate, tuple);
        ctx.emit_if(cond, |ctx| {
            self.parent.consume(tuple, ctx);
        });
    }
}

/// Hash Join Probe: inline probe into compiled pipeline
pub struct HashProbeOperator {
    hash_table: HashTableRef,
    join_key: Vec<ColumnRef>,
    parent: Box<dyn PipelineOperator>,
}

impl PipelineOperator for HashProbeOperator {
    fn produce(&self, ctx: &mut CodeGenContext) {
        // Delegate to child's produce
    }

    fn consume(&self, tuple: &TupleRef, ctx: &mut CodeGenContext) {
        let key = ctx.emit_extract_key(tuple, &self.join_key);
        let hash = ctx.emit_hash(key);
        let matched = ctx.emit_hash_probe(&self.hash_table, hash, key);

        ctx.emit_if_not_null(matched, |ctx| {
            let joined = ctx.emit_concatenate(tuple, matched);
            self.parent.consume(&joined, ctx);
        });
    }
}

/// Hash Build: pipeline breaker (materializes input into hash table)
pub struct HashBuildOperator {
    hash_table: HashTableRef,
    build_key: Vec<ColumnRef>,
}

impl PipelineOperator for HashBuildOperator {
    fn produce(&self, _: &mut CodeGenContext) {
        unreachable!("Hash build is a sink, never produces");
    }

    fn consume(&self, tuple: &TupleRef, ctx: &mut CodeGenContext) {
        let key = ctx.emit_extract_key(tuple, &self.build_key);
        let hash = ctx.emit_hash(key);
        ctx.emit_hash_insert(&self.hash_table, hash, key, tuple);
    }

    fn is_pipeline_breaker(&self) -> bool { true }
}

/// Plan segmentation: split query plan into pipelines
pub fn segment_into_pipelines(plan: &QueryPlan) -> Vec<Pipeline> {
    let mut pipelines = Vec::new();
    let mut current_ops = Vec::new();

    plan.visit_post_order(|node| {
        current_ops.push(node.clone());
        if node.is_pipeline_breaker() {
            pipelines.push(Pipeline::from_ops(current_ops.clone()));
            current_ops.clear();
        }
    });

    if !current_ops.is_empty() {
        pipelines.push(Pipeline::from_ops(current_ops));
    }

    pipelines
}
```

**Restrictions:**
- Pipeline breakers force materialization, losing register residence
- Deep plans with many breakers have many short pipelines (less fusion)
- Semi-join and anti-join require careful integration into produce/consume
- Correlated subqueries may introduce additional pipeline breakers
- Parallel execution requires partitioning pipelines across workers

## Cost Model

```rust
fn pipeline_execution_cost(
    num_tuples: u64,
    pipeline_length: usize,   // non-breaking operators in pipeline
    num_breakers: usize,       // pipeline breakers in the full plan
) -> CostComparison {
    // Volcano: per-tuple overhead proportional to pipeline depth
    let volcano_per_tuple = pipeline_length as f64 * 20.0; // cycles
    let volcano_total = num_tuples as f64 * volcano_per_tuple;

    // Push-based: minimal per-tuple overhead (register-to-register)
    let push_per_tuple = 5.0 + pipeline_length as f64 * 1.0; // cycles
    let push_total = num_tuples as f64 * push_per_tuple;

    // Materialization cost at breakers
    let breaker_cost = num_breakers as f64 * num_tuples as f64 * 3.0;

    CostComparison {
        volcano_cycles: volcano_total as u64,
        push_cycles: (push_total + breaker_cost) as u64,
        speedup: volcano_total / (push_total + breaker_cost),
        pipeline_efficiency: push_per_tuple / volcano_per_tuple,
    }
}
```

**Typical performance:**
- Single pipeline (scan-filter-project): 10-50x faster than Volcano
- With 1 breaker (hash join): 5-20x faster
- With 3+ breakers: 3-10x faster (breaker cost dominates)
- Memory traffic reduction: 2-5x (register residence eliminates loads/stores)

## Test Cases

### Positive: Long pipeline without breakers (scan-filter-project)

```sql
SELECT customer_id, order_date
FROM orders
WHERE status = 'completed'
  AND total > 100
  AND region = 'US';

-- Single pipeline: Scan -> Filter1 -> Filter2 -> Filter3 -> Project -> Emit
-- Compiled to one tight loop:
--   for row in orders:
--     if row.status == 'completed' && row.total > 100 && row.region == 'US':
--       emit(row.customer_id, row.order_date)
-- All intermediate values in registers
-- Volcano: 6 virtual calls per tuple
-- Push-based: 0 virtual calls, ~5 cycles per tuple
```

### Positive: Hash join with fused probe pipeline

```sql
SELECT c.name, SUM(o.total)
FROM customers c
JOIN orders o ON c.id = o.customer_id
WHERE c.country = 'US'
GROUP BY c.name;

-- Pipeline 1 (build): Scan customers -> Filter(country='US') -> HashBuild
--   Tight loop: scan + branch + hash insert
-- Pipeline 2 (probe): Scan orders -> HashProbe -> HashAggUpdate
--   Tight loop: scan + hash probe + aggregate update
-- Pipeline 3 (finalize): Scan HT -> Project -> Emit
-- Key: probe + filter + aggregate fused into one function
```

### Positive: Pipelined aggregation (no breaker for streaming case)

```sql
-- Pre-sorted input: aggregation becomes a pipeline operator (not a breaker)
SELECT region, SUM(total)
FROM orders  -- clustered on region
GROUP BY region;

-- Single pipeline: Scan -> StreamingAggregate -> Emit
-- Streaming aggregate detects group boundaries, emits completed groups
-- No materialization needed: entire query is one pipeline
```

### Negative: Many pipeline breakers reduce benefit

```sql
SELECT c.name, p.description, SUM(oi.quantity)
FROM customers c
JOIN orders o ON c.id = o.customer_id
JOIN order_items oi ON o.id = oi.order_id
JOIN products p ON oi.product_id = p.id
WHERE c.country = 'US'
GROUP BY c.name, p.description
ORDER BY SUM(oi.quantity) DESC
LIMIT 10;

-- Pipeline 1: Scan customers -> Filter -> HashBuild1
-- Pipeline 2: Scan orders -> HashProbe1 -> HashBuild2
-- Pipeline 3: Scan order_items -> HashProbe2 -> HashBuild3
-- Pipeline 4: Scan products -> HashProbe3 -> HashAgg
-- Pipeline 5: Scan HT -> Sort -> Limit -> Emit
-- 5 pipelines with 4 breakers: less fusion benefit
-- Each breaker materializes to memory, losing register residence
-- Still faster than Volcano but closer to 3-5x than 50x
```

### Negative: Correlated subquery prevents full fusion

```sql
SELECT e.name,
  (SELECT MAX(salary) FROM employees e2
   WHERE e2.dept_id = e.dept_id) AS max_dept_salary
FROM employees e;

-- Correlated subquery: inner query depends on outer row
-- Cannot fuse into single pipeline (dependency)
-- Each outer row triggers a separate inner pipeline execution
-- Solution: decorrelate first (apply-to-join), then compile
```

## References

**Academic papers:**
- Neumann, "Efficiently Compiling Efficient Query Plans for Modern Hardware", VLDB 2011
- Kersten et al., "Everything You Always Wanted to Know About Compiled and Vectorized Queries But Were Afraid to Ask", VLDB 2018
- Shaikhha et al., "How to Architect a Query Compiler, Revisited", SIGMOD 2018
- Kohn et al., "Adaptive Execution of Compiled Queries", ICDE 2018
- Leis et al., "Morsel-Driven Parallelism", SIGMOD 2014

**Implementation:**
- HyPer: Original produce/consume compilation (Neumann 2011)
- Umbra: Adaptive interpretation/compilation with Babelfish IR
- mssql Hekaton: Native compilation for in-memory tables
- Apache Spark: Whole-stage code generation via Janino
- Photon (Databricks): Vectorized push-based C++ engine
