# CMU 15-721 Lecture 5: Query Execution II - Meta's Velox Engine

**Source:** CMU 15-721 Spring 2024, Lecture 5
**Date:** 2024-02-07
**Topic:** Unified execution engine architecture and optimization
**Key Papers:** "Velox: Meta's Unified Execution Engine" (VLDB 2022)

## Key Points

Velox is Meta's open-source unified execution engine that powers multiple query
engines (Presto, Spark) with a single optimized execution layer. The lecture covers
how a shared execution engine affects optimization decisions.

### Velox Architecture

1. **Connector layer**: Abstract data source access (HDFS, S3, local, in-memory)
2. **Type system**: Arrow-compatible columnar type system
3. **Expression evaluation**: Compiled expression trees with lazy/eager evaluation
4. **Operator library**: Hash join, sort-merge join, aggregate, etc.
5. **Memory management**: Spill-to-disk support, memory arbitration

### Optimization Techniques from Velox

**1. Adaptive Filter Reordering:**
- Track selectivity of each filter predicate at runtime
- Reorder predicates to evaluate most selective first
- Amortize tracking overhead across batches (not per-row)
- Dynamic: adjusts as data distribution changes within a query

**Optimization rule:** adaptive-filter-reordering - maintain running selectivity
statistics per predicate and reorder to minimize evaluation cost.

**2. Lazy Materialization:**
- Don't read/decompress columns until they're actually needed
- Filters evaluated on a few columns first; only qualifying rows
  trigger loading of remaining columns
- Critical for wide tables (100+ columns) where most queries use < 10

**Optimization rule:** lazy-column-loading - defer column access until after
selectivity-reducing operations have been applied.

**3. Dictionary-Aware Processing:**
- Maintain dictionary encoding through operators (not just at scan)
- Filter on dictionary codes instead of materialized values
- Join on dictionary codes when both sides share a dictionary
- Only decode at output or when dictionary must change

**Optimization rules:**
- dictionary-propagation: maintain dictionary encoding through operators
- dictionary-join: join on dictionary codes when compatible
- dictionary-decode-deferral: delay decoding until necessary

**4. Spill-to-Disk Framework:**
- When hash table or sort buffer exceeds memory budget, spill to disk
- Optimizer should generate plans that minimize spill probability
- Cost model should include spill penalty (extra I/O)

**Optimization rule:** memory-aware-plan-selection - prefer plans that fit in
available memory; penalize plans that are likely to spill.

**5. Expression Common Subexpression Elimination (CSE):**
- Identify common subexpressions across the entire query plan
- Evaluate once, reference result multiple times
- Works across different operators (filter and project may share expressions)

**Optimization rule:** cross-operator-cse - CSE that spans operator boundaries.

### Connector-Level Optimizations

Velox pushes operations into connectors (data sources):
1. Column pruning: only read needed columns from file
2. Predicate pushdown: push filters to file format (Parquet row group pruning)
3. Aggregate pushdown: push COUNT/MIN/MAX to metadata
4. Limit pushdown: stop reading after enough rows
5. Dynamic filter pushdown: pass runtime bloom filters to scanner

## Optimization Rules for Ra

### New Rules Identified

1. **adaptive-filter-reordering** - Runtime reordering of conjunctive predicates
   by observed selectivity (most selective first)
2. **lazy-column-materialization** - Defer column loading until after filtering
   reduces the number of rows that need materialization
3. **dictionary-encoding-propagation** - Keep dictionary encoding through filter,
   project, and join operators rather than decoding at scan
4. **cross-operator-cse** - Eliminate common subexpressions that span multiple
   operators (e.g., same expression in WHERE and SELECT)
5. **memory-budget-aware-join-selection** - Prefer hash joins that fit in memory;
   add I/O penalty for joins expected to spill
6. **connector-pushdown-aggregation** - Push COUNT/MIN/MAX to storage connector
   when answerable from metadata (Parquet footer, etc.)
7. **dynamic-filter-to-connector** - Pass runtime bloom filters from hash join
   build to scan connector for early filtering

### Ra Gap Analysis

Ra currently has:
- `rules/logical/expression-simplification/common-subexpression-elimination.rra` - basic CSE
- `rules/logical/predicate-pushdown/` - predicate pushdown rules
- `rules/logical/column-pruning.md` - column pruning
- `rules/logical/aggregate-pushdown/count-star-optimization.rra` - COUNT(*) optimization

**Missing capabilities:**
- Adaptive (runtime) filter reordering
- Dictionary encoding propagation through operators
- Cross-operator CSE (current CSE is likely within-operator)
- Memory-budget-aware join algorithm selection
- Dynamic filter pushdown to connectors
- Lazy materialization boundaries

## Relevance to Ra

**Priority:** High for several items - dictionary-aware processing and adaptive filter
reordering are implemented by every major analytical engine. Memory-aware join
selection prevents costly spill-to-disk scenarios.

**Proposed RFC:** Dictionary-Aware Query Processing - maintain dictionary encoding
through the operator pipeline, evaluate predicates on dictionary codes, and defer
decoding until output or incompatible operation. This optimization alone can provide
2-5x speedup on dictionary-encoded columnar data.
