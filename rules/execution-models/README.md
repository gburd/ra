# Execution Model Specific Optimizations

Rules in this directory are specific to particular query execution models.

## Directory Structure

- **volcano/** - Iterator model (tuple-at-a-time)
- **vectorized/** - Batch processing model
- **push-based/** - Compiled/JIT execution
- **morsel-driven/** - NUMA-aware parallelism
- **differential/** - Differential dataflow / streaming (Materialize)
- **column-at-a-time/** - MonetDB X100 model

## Execution Models Overview

### Volcano (Iterator Model)
**Databases:** PostgreSQL, MySQL, SQLite, Oracle (traditional)

Traditional pull-based execution where each operator processes one tuple at a time. Simple but high interpretation overhead.

**Key Optimizations:**
- Minimize tuple flow through predicate pushdown
- Index selection to avoid full scans
- Sort avoidance by exploiting existing order
- Early materialization for complex expressions

### Vectorized (Batch Processing)
**Databases:** DuckDB, ClickHouse, Apache Arrow, Snowflake

Processes batches of tuples (typically 1K-8K) for better CPU cache utilization and SIMD opportunities.

**Key Optimizations:**
- Batch size tuning
- Column pruning (process fewer columns per batch)
- Expression batching
- Late materialization within batches

### Push-Based (Compiled)
**Databases:** HyPer, Umbra, SQL Server (batch mode), Spark Tungsten

Generates compiled code with tight inner loops. Minimal interpretation overhead.

**Key Optimizations:**
- Pipeline fusion (combine operators)
- Hoist invariant expressions
- Specialize for common cases
- Minimize pipeline breakers (sorts, hash joins)

### Morsel-Driven
**Databases:** HyPer, Umbra, MemSQL/SingleStore

Parallel execution with work-stealing and NUMA-awareness. Processes data in cache-sized morsels.

**Key Optimizations:**
- Parallel scan with work stealing
- NUMA-aware data placement
- Local aggregation before global
- Balance morsel size vs overhead

### Differential Dataflow (Streaming)
**Database:** Materialize

Incremental computation using differential dataflow. Maintains materialized views with automatic incrementalization.

**Key Optimizations:**
- Arrangement selection (what to index)
- Join order for minimal intermediate state
- Temporal filter pushdown
- Delta query optimization
- Arrangement sharing

See [Materialize-specific optimizations](../../docs/execution-models.md#5-differential-dataflow--streaming-model).

### Column-at-a-Time (X100)
**Databases:** MonetDB, VectorWise (Actian Vector)

Processes entire columns with late materialization. Works with positions rather than full tuples.

**Key Optimizations:**
- Late materialization (defer tuple reconstruction)
- Column cracking (adaptive indexing)
- Filter ordering by selectivity
- Position-based joins
- Aggressive column pruning

See [MonetDB-specific optimizations](../../docs/execution-models.md#6-column-at-a-time--x100-model).

## When to Use

Choose rules from the appropriate directory based on your target execution model:

```rust
// Example: Apply vectorized-specific rules
let rules = match execution_model {
    ExecutionModel::Volcano => load_rules("execution-models/volcano/"),
    ExecutionModel::Vectorized => load_rules("execution-models/vectorized/"),
    ExecutionModel::PushBased => load_rules("execution-models/push-based/"),
    ExecutionModel::MorselDriven => load_rules("execution-models/morsel-driven/"),
    ExecutionModel::Differential => load_rules("execution-models/differential/"),
    ExecutionModel::ColumnAtATime => load_rules("execution-models/column-at-a-time/"),
};
```

## Hybrid Systems

Modern databases often use multiple execution models:

- **SQL Server**: Row mode (Volcano) + Batch mode (Vectorized)
- **PostgreSQL with JIT**: Volcano + Push-based for hot paths
- **Materialize**: Differential + Vectorized operators

For hybrid systems, load rules from multiple directories and let the optimizer choose.

## References

See [docs/execution-models.md](../../docs/execution-models.md) for detailed information about each execution model, including:
- Architecture and design principles
- Advantages and trade-offs
- Database-specific implementations
- Performance characteristics
- Rule applicability

## Contributing

When adding execution model-specific rules:

1. Place the rule in the appropriate directory
2. Tag it with `execution_models` in frontmatter
3. Document why the rule is model-specific
4. Include performance comparisons if available
5. Reference database implementations
